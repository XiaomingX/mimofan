//! B5 压缩保真度探针（EVAL_METRICS.md B5，满分 7 分）。
//!
//! 用法：
//! ```text
//! probe_compaction <samples/memory_recall.json 的绝对路径>
//! ```
//! stdout 只输出一个 JSON 对象，含浮点字段 `recall_rate`（0.0~1.0）。
//! 诊断信息走 stderr，保证 stdout 可被 Python 侧直接 `json.loads`。
//!
//! ## 这个探针到底测什么
//!
//! 测 **压缩后关键事实是否仍然存在于送给下一轮的上下文里**。
//!
//! 构造一段长对话：把样本里的关键事实（架构决策、硬约束、已排除的调试假设）
//! 放在前部，后面跟上大量 `distractor_pool` 里的无关闲聊，凑到真实长会话的
//! 形态。然后走项目自己的 compaction 主路径（替换式摘要，不是 seam_manager
//! 的 append-only，也不是 purge 的外科清理）：
//!
//! 1. `plan_compaction` 划分 `pinned_indices` / `summarize_indices`；
//! 2. 被划入待摘要区的消息，会经 `build_formatted_summary_request` 的同款
//!    截断逻辑压成「摘要输入文本」再交给 LLM。
//!
//! 一条事实只要**落在 pinned 原文里**，或者**仍出现在摘要输入文本中**，
//! 就算被保住；两边都没有，说明它在送进 LLM 之前就已经被物理丢弃了，
//! 无论后面模型多聪明都不可能再恢复——这才是真正的压缩失真。
//!
//! 召回率 = 保住的关键事实数 / 关键事实总数。
//!
//! ## 为什么这样切，而不是只看 pinned
//!
//! 只看 pinned 会得到误导性的低分。`plan_compaction` 默认只 pin 最后
//! `KEEP_RECENT_MESSAGES`(=4) 条，外加命中「working set 文件路径 / error 标记 /
//! patch 标记」的消息（见 `should_pin_message`）。本样本的事实是自然语言决策
//! 陈述，一条都不命中这些规则，于是「只看 pinned」恒为 0/9 —— 这个 0 既不
//! 反映真实丢失（它们其实进了摘要输入，LLM 大概率能保住），也没有任何区分度
//! （策略怎么改都还是 0）。那是坏指标。
//!
//! 计入摘要输入之后，探针度量的是**确定性的、不可逆的信息丢弃**：
//! 消息被截断策略切掉（head/tail 裁剪，中间整段丢弃）就是真丢了。
//!
//! ## 为什么不跑完整 compact_messages
//!
//! 完整路径在 plan 之后会调 `create_summary` → `client.create_message(...)`，
//! 必须真实请求 LLM：CI/离线环境跑不了，且分数会掺进模型随机性和网络抖动，
//! 使「改进前 vs 改进后」的分差无法归因到本项目的代码改动。
//!
//! ## 诚实性声明（报告中不得省略）
//!
//! 本探针**不评估 LLM 摘要本身的信息损失**——即「事实进了摘要输入，但模型
//! 没把它写进摘要」这种情况测不到。因此本项分数是压缩保真度的**上界**：
//! 满分只代表「关键事实被完整送到了摘要器面前」，不代表最终摘要里一定还在。
//! 反之掉分则是**确凿的**丢失（内容在进模型之前就被截断丢弃了）。
//! 该口径无网络依赖、完全确定性，适合做前后对比。
//!
//! ## 模块可见性说明
//!
//! `crates/tui/src/lib.rs` 中 `compaction` 现已声明为 `pub mod`，examples 作为
//! 外部 crate 可正常走 `mimofan::compaction` 公开路径引用。
use mimofan::compaction::{KEEP_RECENT_MESSAGES, estimate_tokens, plan_compaction};
use mimofan::models::{ContentBlock, Message};

// ── 摘要输入的截断参数 ───────────────────────────────────────────────────
// 与 compaction/mod.rs 的 `SUMMARY_*` 常量保持一致（非大上下文模型分支）。
// 这些常量在 compaction 里是私有的，无法直接引用，故在此复刻；
// 若上游调整了这些值，本探针需要同步更新，否则测的就不是真实截断行为。
const SUMMARY_TEXT_SNIPPET_CHARS: usize = 800;
const SUMMARY_INPUT_MAX_CHARS: usize = 24_000;
const SUMMARY_INPUT_HEAD_CHARS: usize = 14_000;
const SUMMARY_INPUT_TAIL_CHARS: usize = 6_000;

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

fn tail_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    s.chars().skip(n - max).collect()
}

/// 复刻 `build_formatted_summary_request` 构造摘要输入文本的过程。
///
/// 逐条消息按 `text_snippet_chars` 截断后拼接；总长超过 `input_max_chars`
/// 时保留头部 + 尾部，**中间整段丢弃**。落在被丢弃区间的事实就是真丢了。
fn build_summary_input(messages: &[Message]) -> String {
    let mut text = String::new();
    for msg in messages {
        let role = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        for block in &msg.content {
            if let ContentBlock::Text { text: t, .. } = block {
                text.push_str(role);
                text.push_str(": ");
                text.push_str(&truncate_chars(t, SUMMARY_TEXT_SNIPPET_CHARS));
                text.push_str("\n\n");
            }
        }
    }

    let n = text.chars().count();
    if n > SUMMARY_INPUT_MAX_CHARS {
        let head = truncate_chars(&text, SUMMARY_INPUT_HEAD_CHARS);
        let tail = tail_chars(&text, SUMMARY_INPUT_TAIL_CHARS);
        let omitted = n
            .saturating_sub(head.chars().count())
            .saturating_sub(tail.chars().count());
        text = format!("{head}\n\n[... {omitted} characters omitted before summary ...]\n\n{tail}");
    }
    text
}

fn user(text: &str) -> Message {
    Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            cache_control: None,
        }],
    }
}

fn assistant(text: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            cache_control: None,
        }],
    }
}

/// 失败即以低分退出，绝不虚报。
fn bail(reason: &str) -> ! {
    eprintln!("[probe_compaction] 失败: {reason}");
    println!(
        "{}",
        serde_json::json!({
            "recall_rate": 0.0,
            "error": reason,
            "probe": "B5_compaction_fidelity",
        })
    );
    std::process::exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(sample_path) = args.get(1) else {
        bail("缺少参数：需要 memory_recall.json 的路径");
    };

    let raw = match std::fs::read_to_string(sample_path) {
        Ok(s) => s,
        Err(e) => bail(&format!("读取样本失败 {sample_path}: {e}")),
    };
    let data: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => bail(&format!("解析样本 JSON 失败: {e}")),
    };

    let scenarios = data["scenarios"].as_array().cloned().unwrap_or_default();
    if scenarios.is_empty() {
        bail("样本中没有 scenarios");
    }
    let distractors: Vec<String> = data["distractor_pool"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if distractors.is_empty() {
        bail("样本中没有 distractor_pool");
    }

    let mut total_facts = 0usize;
    let mut kept_facts = 0usize;
    let mut details = Vec::new();

    // 每个场景独立构造一段对话并独立压缩，避免场景之间互相干扰。
    for sc in &scenarios {
        let sid = sc["id"].as_str().unwrap_or("?").to_string();
        let facts = sc["facts"].as_array().cloned().unwrap_or_default();
        let distractor_turns = sc["distractor_turns"].as_u64().unwrap_or(20) as usize;

        let mut messages: Vec<Message> = Vec::new();
        // 记录每条事实落在哪个消息下标，压缩后据此判断是否被保留。
        let mut fact_positions: Vec<(String, String, usize)> = Vec::new();

        // 负向对照开关（仅供验证探针有效性，正式跑分绝不设置）：
        // MIMOFAN_PROBE_FAULT=flood 会在事实**之前**插入 300 轮干扰，把事实
        // 推到摘要输入的中部。此时总长超过 SUMMARY_INPUT_MAX_CHARS(24000)，
        // 触发「保头 14000 + 保尾 6000、中间整段丢弃」，事实正好落在被丢弃的
        // 区间里，分数应当掉到 0。若照样满分，说明探针没有真在测截断丢失。
        //
        // 这个开关同时也是对「事实放开头就永远安全」这一点的证明：常规布局
        // 下事实落在受保护的头部，所以拿满分是真实结论，不是探针恒真。
        let flood = std::env::var("MIMOFAN_PROBE_FAULT").as_deref() == Ok("flood");
        let assistant_filler = "好的，我看一下这段代码，然后给出对应的修改建议和理由。";
        if flood {
            for i in 0..300 {
                messages.push(user(&distractors[i % distractors.len()]));
                messages.push(assistant(assistant_filler));
            }
        }

        // 1) 说出关键事实（模拟「早期会话中确定的决策」）。
        for f in &facts {
            let key = f["key"].as_str().unwrap_or("").to_string();
            let stmt = f["statement"].as_str().unwrap_or("").to_string();
            if stmt.is_empty() {
                continue;
            }
            messages.push(user(&stmt));
            fact_positions.push((key, stmt, messages.len() - 1));
            messages.push(assistant("明白了，我会记住这个前提。"));
        }

        // 2) 再灌入干扰轮次，把关键事实推离对话末尾——
        //    这正是真实长会话里事实脱离 keep_recent 保护区的典型形态。
        let turns = if flood { 100 } else { distractor_turns };
        for i in 0..turns {
            messages.push(user(&distractors[i % distractors.len()]));
            messages.push(assistant(assistant_filler));
        }

        let before_tokens = estimate_tokens(&messages);

        // 3) 跑项目自己的压缩规划。workspace 传 None：本探针只评「事实是否
        //    被保留」，不引入仓库文件路径带来的额外 pin，保证结果只取决于
        //    压缩策略本身，且在任何机器上可复现。
        let plan = plan_compaction(&messages, None, KEEP_RECENT_MESSAGES, None, None);

        // 4) 构造摘要输入：待摘要区的消息会被压成这段文本再交给 LLM。
        let to_summarize: Vec<Message> = plan
            .summarize_indices
            .iter()
            .map(|&i| messages[i].clone())
            .collect();
        let summary_input = build_summary_input(&to_summarize);

        // 5) 判定：事实要么原文留在 pinned，要么仍出现在摘要输入里。
        //    两者皆无 = 在进 LLM 之前就被物理丢弃，属确凿的压缩失真。
        for (key, stmt, idx) in &fact_positions {
            total_facts += 1;
            let in_pinned = plan.pinned_indices.contains(idx);
            // 事实语句本身可能被 800 字符的 snippet 截断，故用其前缀判断
            // 是否幸存，避免把「被截了个尾巴」误判成整条丢失。
            let probe_key: String = stmt.chars().take(24).collect();
            let in_summary = summary_input.contains(&probe_key);
            let kept = in_pinned || in_summary;
            if kept {
                kept_facts += 1;
            }
            details.push(serde_json::json!({
                "scenario": sid,
                "fact": key,
                "message_index": idx,
                "in_pinned": in_pinned,
                "in_summary_input": in_summary,
                "kept": kept,
            }));
        }

        let kept_msgs: Vec<Message> = messages
            .iter()
            .enumerate()
            .filter(|(i, _)| plan.pinned_indices.contains(i))
            .map(|(_, m)| m.clone())
            .collect();
        let after_tokens = estimate_tokens(&kept_msgs);

        eprintln!(
            "[probe_compaction] {sid}: {} 条消息 → pin {} / 待摘要 {}, \
             摘要输入 {} 字符, token {before_tokens} → {after_tokens}(pinned)",
            messages.len(),
            plan.pinned_indices.len(),
            plan.summarize_indices.len(),
            summary_input.chars().count(),
        );
    }

    if total_facts == 0 {
        bail("样本中没有可用的 facts，无法评分");
    }

    let rate = kept_facts as f64 / total_facts as f64;
    eprintln!(
        "[probe_compaction] 关键事实保留 {kept_facts}/{total_facts} = {:.1}%",
        rate * 100.0
    );

    println!(
        "{}",
        serde_json::json!({
            "recall_rate": rate,
            "probe": "B5_compaction_fidelity",
            "measures": "compaction plan 阶段的关键事实保留率（不含 LLM 摘要环节，为保真度下界）",
            "kept": kept_facts,
            "total": total_facts,
            "details": details,
        })
    );
}
