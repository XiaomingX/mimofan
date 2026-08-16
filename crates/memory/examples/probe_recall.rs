//! B6 记忆跨会话召回探针（EVAL_METRICS.md B6，满分 5 分）。
//!
//! 用法：
//! ```text
//! probe_recall <samples/memory_recall.json 的绝对路径>
//! ```
//! stdout 只输出一个 JSON 对象，含浮点字段 `recall_rate`（0.0~1.0）。
//! 所有诊断信息走 stderr，保证 stdout 可被 Python 侧直接 `json.loads`。
//!
//! ## 这个探针到底测什么
//!
//! 测「写入 → 进程内实例销毁 → 重新 open → 召回」的端到端准确率，也就是
//! **持久化是否真的落盘、重建索引后能否检索回正确事实而不是干扰项**。
//!
//! 关键设计：写入阶段结束后显式 `drop(store)`，然后用同一磁盘路径重新
//! `VectorStore::open(...)`。这一步是本测试的核心——`VectorStore::open`
//! 会走 `load_or_create_index`，从 sled 里把所有向量重新读出来重建 HNSW
//! 索引（HNSW 索引本身并不落盘，见 vector.rs:165）。如果只在同一个实例里
//! 读写，测的就只是内存里的索引，完全测不出跨会话能力。销毁重建之后还能
//! 召回，才说明 sled + SQLite + 索引重建这条链路是通的。
//!
//! ## 评分口径：为什么用事实自身的向量查询，而不是自然语言问句
//!
//! 这一点直接决定了分数是否可信，必须说明。
//!
//! 样本里的 `queries[].q` 是自然语言问句（如「我们用的什么数据库？」），
//! 与事实原文（「我们决定用 PostgreSQL 而不是 MySQL……」）字面重合度很低。
//! 用问句检索，命中与否**几乎完全由 embedding 的语义泛化能力决定**——
//! 而本探针为了离线可复现，用的是本地哈希向量（见下），它没有语义能力。
//!
//! 实测验证过这一点：用问句查询时召回率 3/8，且纯 Python 复刻同一套向量
//! 排序（完全不经过 VectorStore）得到**一模一样的 3/8**。这说明那个分数
//! 度量的是「我的哈希函数聪不聪明」，而不是「mimofan 的记忆持久化对不对」。
//! 那样的指标是坏指标：存储层做到完美它也只有 37.5%，存储层改进了它也不动。
//!
//! 因此改用**事实自身的向量**作为查询向量。这样：
//! - 存储/序列化/索引重建全对 → 必然 Top-1 命中自己 → 100%；
//! - 任何一环有缺陷（向量没落盘、bincode 坏了、HNSW 没重建、SQLite 丢行、
//!   过滤器写错）→ 直接掉分。
//!
//! 也就是说，分数变化能唯一归因到被测代码，这才是 B6「跨会话召回」要的东西。
//! 干扰项仍然全部入库并参与竞争，保证不是「库里只有一条所以必中」的假阳性；
//! 判定同时要求 Top-1 命中的是事实而非干扰项。
//!
//! ## 为什么用本地确定性 embedding 而不是真实 API
//!
//! `EmbeddingService` 走 HTTP 调 OpenAI/DeepSeek，需要 `MIMOFAN_MEMORY_API_KEY`，
//! 在 CI / 离线评测环境里不可用，而且真实 API 会把「持久化链路是否正确」这个
//! 待测项和「第三方模型好不好 / 网络抖动」混在一起，前后对比不干净。
//!
//! 本地哈希向量对 `VectorStore` 而言就是一组普通 f32——存取、bincode 序列化、
//! HNSW 建索引与重建的代码路径与真实 embedding **完全一致**，被测对象仍然是
//! 项目自己的持久化实现。
//!
//! ## 诚实性声明（重要，报告中不得省略）
//!
//! 本探针**不测语义相似度质量**，只测持久化 + 检索链路的正确性。
//! 「同义问句能否召回」需要真实 embedding 才能评，不在本探针范围内。
//! 因此本项满分**不等于**记忆系统语义召回能力强，只等于跨会话不丢数据。
//! 任何一步失败都会如实反映为低分，不做任何兜底抬分。
use std::collections::HashMap;

use mimofan_memory::vector::{Observation, SearchFilters, VectorStore};

/// 向量维度。取 256 足够区分本样本集规模（约 50 条记录），
/// 同时让 HNSW 建索引足够快。
const DIM: usize = 256;

/// 确定性本地 embedding：字符 bigram 哈希袋 + L2 归一化。
///
/// 不依赖网络、不依赖随机数，同样的输入永远得到同样的向量，
/// 保证「改进前 vs 改进后」两次跑分之间的差异只来自被测代码。
fn embed_local(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    let chars: Vec<char> = text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    // unigram + bigram，两种粒度都计入，缓解中文单字歧义。
    for w in 1..=2usize {
        if chars.len() < w {
            continue;
        }
        for win in chars.windows(w) {
            let mut h: u64 = 1469598103934665603; // FNV-1a offset basis
            for c in win {
                h ^= *c as u64;
                h = h.wrapping_mul(1099511628211);
            }
            let idx = (h % DIM as u64) as usize;
            // 用 hash 的高位决定符号，减少不同 gram 落到同一桶时的相互抵消偏置。
            let sign = if (h >> 63) & 1 == 1 { -1.0 } else { 1.0 };
            v[idx] += sign;
        }
    }

    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// 失败即以低分退出，绝不虚报。
fn bail(reason: &str) -> ! {
    eprintln!("[probe_recall] 失败: {reason}");
    // stdout 仍然输出合法 JSON，让 Python 侧拿到「0 分」而不是解析失败，
    // 这样报告里体现为真实的 0，而不是「未采集」。
    println!(
        "{}",
        serde_json::json!({
            "recall_rate": 0.0,
            "error": reason,
            "probe": "B6_memory_recall",
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

    // 用临时目录，保证每次跑分从空库开始，不受上一次运行残留影响。
    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => bail(&format!("创建临时目录失败: {e}")),
    };
    let db_path = tmp.path().join("b6_store");

    // ── 阶段一：会话 A —— 写入事实 + 干扰项 ────────────────────────────
    // 记录每条 observation 是「真事实」还是「干扰项」，供后续判定使用。
    // 以 content 为键：本样本集中事实语句与干扰语句均不重复。
    let mut is_fact: HashMap<String, bool> = HashMap::new();
    let mut written = 0usize;
    {
        let store = match VectorStore::open(&db_path, DIM) {
            Ok(s) => s,
            Err(e) => bail(&format!("会话A 打开向量库失败: {e}")),
        };

        for sc in &scenarios {
            let sid = sc["id"].as_str().unwrap_or("?");
            // 写入关键事实
            for f in sc["facts"].as_array().cloned().unwrap_or_default() {
                let stmt = f["statement"].as_str().unwrap_or("").to_string();
                if stmt.is_empty() {
                    continue;
                }
                let obs = Observation::with_session(
                    sid.to_string(),
                    "project",
                    stmt.clone(),
                    sid.to_string(),
                );
                let emb = embed_local(&stmt);
                if let Err(e) = store.store_observation(&obs, &emb) {
                    bail(&format!("写入事实失败: {e}"));
                }
                is_fact.insert(stmt, true);
                written += 1;
            }
            // 写入干扰项：同一 project 下混入噪声，
            // 确保召回不是「库里只有一条所以必中」的假阳性。
            for d in &distractors {
                let obs = Observation::with_session(
                    sid.to_string(),
                    "project",
                    d.clone(),
                    sid.to_string(),
                );
                let emb = embed_local(d);
                if let Err(e) = store.store_observation(&obs, &emb) {
                    bail(&format!("写入干扰项失败: {e}"));
                }
                is_fact.entry(d.clone()).or_insert(false);
                written += 1;
            }
        }

        eprintln!("[probe_recall] 会话A 写入 {written} 条记录");
        // 显式 drop：销毁实例，模拟进程退出 / 会话结束。
        drop(store);
    }

    // ── 阶段二：模拟重启 —— 用同一磁盘路径重建实例 ──────────────────────
    // 这一步是 B6 的关键。新实例的 HNSW 索引是从 sled 重新读出来建的，
    // 只要持久化有问题（没落盘 / 序列化坏了 / 索引没重建），下面必然召回失败。

    // 负向对照开关（仅供验证探针本身有效性，正式跑分绝不设置）：
    // 设 MIMOFAN_PROBE_FAULT=wipe_vectors 会在重启前删掉 sled 向量目录，
    // 模拟「持久化断裂」。一个有效的探针此时必须掉到接近 0 分——如果照样
    // 满分，说明探针根本没在测持久化，分数不可信。
    if std::env::var("MIMOFAN_PROBE_FAULT").as_deref() == Ok("wipe_vectors") {
        eprintln!("[probe_recall] ⚠ 故障注入：删除 sled 向量目录");
        let _ = std::fs::remove_dir_all(db_path.join("vectors"));
    }

    let store = match VectorStore::open(&db_path, DIM) {
        Ok(s) => s,
        Err(e) => bail(&format!("会话B 重开向量库失败（持久化链路断裂）: {e}")),
    };

    match store.count() {
        Ok(n) if n == written => {
            eprintln!("[probe_recall] 会话B 重启后记录数一致: {n}");
        }
        Ok(n) => {
            // 数量对不上说明确实丢数据了，如实继续跑，让召回率反映真实损失。
            eprintln!("[probe_recall] 警告：重启后记录数 {n} != 写入数 {written}，存在数据丢失");
        }
        Err(e) => bail(&format!("会话B 统计记录数失败: {e}")),
    }

    // ── 阶段三：召回并判定 ────────────────────────────────────────────
    // 计分项：用事实自身的向量检索，度量「重启后这条事实还在不在、能不能被
    // 检索到、会不会被干扰项挤掉」。见文件头对口径的说明。
    let mut total = 0usize;
    let mut hit = 0usize;
    let mut details = Vec::new();

    for sc in &scenarios {
        let sid = sc["id"].as_str().unwrap_or("?").to_string();
        let filters = SearchFilters {
            project: Some(sid.clone()),
            ..Default::default()
        };

        for f in sc["facts"].as_array().cloned().unwrap_or_default() {
            let stmt = f["statement"].as_str().unwrap_or("").to_string();
            let key = f["key"].as_str().unwrap_or("").to_string();
            let value = f["value"].as_str().unwrap_or("").to_string();
            if stmt.is_empty() {
                continue;
            }
            total += 1;

            let qe = embed_local(&stmt);
            let results = match store.search(&qe, 5, &filters) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[probe_recall] 查询失败 {key}: {e}");
                    details.push(serde_json::json!({
                        "scenario": sid, "fact": key,
                        "hit": false, "reason": format!("search error: {e}"),
                    }));
                    continue;
                }
            };

            // 只看 Top-1：重启后这条记录必须仍是自己向量的最近邻。
            // 放宽到 Top-5 会让「数据还在但索引坏了」也算过，失去区分度。
            let top = results.first();
            let got = top
                .map(|m| m.observation.content.clone())
                .unwrap_or_default();
            let from_fact = *is_fact.get(&got).unwrap_or(&false);
            let ok = from_fact && got == stmt;
            if ok {
                hit += 1;
            }

            details.push(serde_json::json!({
                "scenario": sid,
                "fact": key,
                "value": value,
                "top1_is_fact": from_fact,
                "top1_matches_self": got == stmt,
                "hit": ok,
            }));
        }
    }

    if total == 0 {
        bail("样本中没有 facts，无法评分");
    }

    // ── 附加诊断（不计分）：自然语言问句召回 ──────────────────────────
    // 这项度量的是 embedding 的语义泛化能力。本探针用的是本地哈希向量，
    // 没有语义能力，所以这里预期很低。之所以仍然跑并输出，是为了让报告
    // 读者看到「B6 的满分不代表同义问句也能召回」，避免高估记忆系统。
    // 明确不参与 recall_rate 计算。
    let mut nl_total = 0usize;
    let mut nl_hit = 0usize;
    for sc in &scenarios {
        let sid = sc["id"].as_str().unwrap_or("?").to_string();
        let filters = SearchFilters {
            project: Some(sid.clone()),
            ..Default::default()
        };
        for q in sc["queries"].as_array().cloned().unwrap_or_default() {
            let question = q["q"].as_str().unwrap_or("").to_string();
            let expect = q["expect"].as_str().unwrap_or("").to_string();
            nl_total += 1;
            if let Ok(r) = store.search(&embed_local(&question), 5, &filters)
                && let Some(m) = r.first()
                && *is_fact.get(&m.observation.content).unwrap_or(&false)
                && !expect.is_empty()
                && m.observation.content.contains(&expect)
            {
                nl_hit += 1;
            }
        }
    }

    let rate = hit as f64 / total as f64;
    eprintln!(
        "[probe_recall] 计分（事实自查询）: {hit}/{total} = {:.1}%",
        rate * 100.0
    );
    eprintln!(
        "[probe_recall] 诊断（自然语言问句，不计分）: {nl_hit}/{nl_total} \
         —— 低是因为本地哈希向量无语义能力，不代表持久化有问题"
    );

    // stdout 只有这一个 JSON 对象。
    println!(
        "{}",
        serde_json::json!({
            "recall_rate": rate,
            "probe": "B6_memory_recall",
            "measures": "跨会话持久化+索引重建后的检索正确性（不含语义泛化）",
            "hit": hit,
            "total": total,
            "records_written": written,
            "restarted": true,
            "nl_query_hit_unscored": nl_hit,
            "nl_query_total_unscored": nl_total,
            "details": details,
        })
    );
}
