//! LongMemEval 记忆接入探针（#777 评测支撑二进制）。
//!
//! 把一条 LongMemEval 样本的多轮历史会话灌入 mimofan 的 `VectorStore`
//! （真实写盘 → drop → 重建 → 检索），模拟「跨会话记忆」链路，输出被召回
//! 的会话文本，供 Python harness（`longmemeval_harness.py`）作为 system 上下文
//! 触发真模型回答。
//!
//! 用法：
//! ```text
//! echo '<longmemeval_sample_json>' | longmemeval_ingest --project mimofan --top-k 5
//! ```
//! stdin 是 LongMemEval 单条样本（含 `haystack_sessions` / `haystack_dates` / `question`）。
//! stdout 只输出一个 JSON：{ "recalled": [session_text, ...], "project", "error?" }。
//! 诊断走 stderr。
//!
//! ## 为什么用本地哈希 embedding
//! 与 `probe_recall.rs` 同理由：离线可复现、不依赖第三方 embedding API，
//! 且对 `VectorStore` 而言就是一组普通 f32，被测的是项目自己的持久化+检索链路。
//! 诚实声明：本地哈希向量**无语义能力**，所以「question 能否语义召回相关 session」
//! 取决于字面/词袋重叠，这是本接入口径的已知偏置——报告中必须说明。要真测语义
//! 召回需换真实 embedding（设 `MIMOFAN_MEMORY_API_KEY`），属后续增强。
use std::io::Read;

use mimofan_memory::vector::{Observation, SearchFilters, VectorStore};

const DIM: usize = 256;

/// 确定性本地 embedding：字符 bigram 哈希袋 + L2 归一化（同 probe_recall.rs）。
fn embed_local(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    let chars: Vec<char> = text.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let project = args
        .iter()
        .position(|a| a == "--project")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "mimofan".to_string());
    let top_k: usize = args
        .iter()
        .position(|a| a == "--top-k")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        emit_error("读取 stdin 失败");
        return;
    }
    let data: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            emit_error(&format!("解析样本 JSON 失败: {e}"));
            return;
        }
    };

    let sessions = match data["haystack_sessions"].as_array() {
        Some(s) if !s.is_empty() => s,
        _ => {
            emit_error("样本缺少 haystack_sessions 或为空");
            return;
        }
    };
    let dates = data["haystack_dates"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let question = data["question"].as_str().unwrap_or("").to_string();
    if question.is_empty() {
        emit_error("样本缺少 question");
        return;
    }

    // 把每条 session 拼成带时间戳 header 的文本块。
    // 时间戳在 haystack_dates（按 session 索引），无则省略。
    let mut session_texts: Vec<String> = Vec::new();
    for (i, sess) in sessions.iter().enumerate() {
        let header = dates
            .get(i)
            .map(|d| format!("[session @ {d}]\n"))
            .unwrap_or_else(|| format!("[session #{i}]\n"));
        let mut body = String::new();
        if let Some(turns) = sess.as_array() {
            for turn in turns {
                let role = turn["role"].as_str().unwrap_or("?");
                let content = turn["content"].as_str().unwrap_or("");
                body.push_str(&format!("{role}: {content}\n"));
            }
        }
        session_texts.push(format!("{header}{body}"));
    }

    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => {
            emit_error(&format!("创建临时目录失败: {e}"));
            return;
        }
    };
    let db_path = tmp.path().join("lme_store");

    // 写入阶段：每条 session 一个 observation。
    let written = session_texts.len();
    {
        let store = match VectorStore::open(&db_path, DIM) {
            Ok(s) => s,
            Err(e) => {
                emit_error(&format!("打开向量库失败: {e}"));
                return;
            }
        };
        for text in &session_texts {
            let obs = Observation::new(project.clone(), "project", text.clone());
            let emb = embed_local(text);
            if let Err(e) = store.store_observation(&obs, &emb) {
                emit_error(&format!("写入 observation 失败: {e}"));
                return;
            }
        }
        eprintln!("[longmemeval_ingest] 写入 {written} 个 session 作为 observation");
        drop(store);
    }

    // 重建阶段：模拟跨会话重启。
    let store = match VectorStore::open(&db_path, DIM) {
        Ok(s) => s,
        Err(e) => {
            emit_error(&format!("重开向量库失败（持久化链路断裂）: {e}"));
            return;
        }
    };

    // 检索阶段：用 question 的向量召回 Top-K session。
    let filters = SearchFilters {
        project: Some(project.clone()),
        ..Default::default()
    };
    let results = match store.search(&embed_local(&question), top_k, &filters) {
        Ok(r) => r,
        Err(e) => {
            emit_error(&format!("检索失败: {e}"));
            return;
        }
    };
    let recalled: Vec<String> = results
        .into_iter()
        .map(|m| m.observation.content)
        .collect();

    eprintln!(
        "[longmemeval_ingest] 召回 {}/{} 个 session 供 question='{}'",
        recalled.len(),
        written,
        question.chars().take(40).collect::<String>()
    );

    println!(
        "{}",
        serde_json::json!({
            "project": project,
            "recalled": recalled,
            "written": written,
            "error": null,
        })
    );
}

fn emit_error(reason: &str) {
    eprintln!("[longmemeval_ingest] 失败: {reason}");
    println!(
        "{}",
        serde_json::json!({
            "project": "mimofan",
            "recalled": [],
            "written": 0,
            "error": reason,
        })
    );
}
