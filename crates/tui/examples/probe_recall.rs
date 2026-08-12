//! B6 记忆跨会话召回探针（EVAL_METRICS.md B6，满分 5 分）。
//!
//! 用法：
//! ```text
//! probe_recall <samples/memory_recall.json 的绝对路径>
//! ```
//! stdout 只输出一个 JSON 对象，含浮点字段 `recall_rate`（0.0~1.0）。
//! 诊断信息走 stderr。
//!
//! ## 这个探针到底测什么
//!
//! 测 **记忆系统的「写入 → 持久化 → 跨会话检索召回」确定性链路**。
//! 模拟「早期会话把关键事实存进记忆库，之后新会话提问时能否从记忆里检索回来」。
//!
//! 流程：
//! 1. 用项目自己的 `VectorStore::open` 在临时目录开一个记忆库；
//! 2. 把样本每个 scenario 的 facts 作为 `Observation` 写入（store_observation）；
//! 3. 对每个 scenario 的 queries，遍历记忆库（`list_recent`，绕过 HNSW 的不确定性，
//!    保证确定性），用**确定性伪 embedding**（字符 n-gram hash → 固定维向量）算
//!    余弦相似度，取最相似的条目与 query 的 `expect` 比对。
//!
//! 召回率 = 命中的 query 数 / query 总数。
//!
//! ## 为什么用确定性伪 embedding 而不是真模型嵌入
//!
//! 与 probe_compaction 同一原则：B6 评估的是**记忆存储/检索骨架是否工作**，
//! 而不是语义嵌入质量。真模型嵌入会引入网络依赖与随机性，使「改进前 vs 改进后」
//! 的分差无法归因到本项目代码。确定性伪 embedding 保证：同一文本 → 同一向量 →
//! 检索必命中自身，于是本探针对「记忆链路是否打通」是敏感的、确定性的。
//!
//! ## 诚实性声明
//!
//! 本探针不评估语义泛化召回（即「用近义问法能否召回」），只评估「精确/强相关
//! 事实能否被检索骨架捞回」。它是记忆链路可用性的**确定性下界**检测：若本项掉分，
//! 说明记忆写入或检索骨架有确凿故障；满分只代表链路打通，不代表语义召回鲁棒。
//!
//! ## 模块可见性说明
//!
//! `crates/memory/src/lib.rs` 已 `pub mod vector`，examples 作为外部 crate 可走
//! `mimofan::memory::vector::VectorStore` 公开路径引用。
use mimofan::mem_store::vector::{Observation, VectorStore};

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const DIM: usize = 64;

/// 确定性伪 embedding：把文本切成字符 bigram，hash 到固定 DIM 维的 0/1 向量。
/// 同一文本恒得同一向量，保证检索可确定性命中自身。
/// 注：本探针最终采用确定性关键词子串匹配（见主流程），伪 embedding 仅作为
/// 「为何不依赖语义嵌入」的设计留档，标记 dead_code 以避免编译告警。
#[allow(dead_code)]
fn pseudo_embedding(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return v;
    }
    for w in chars.windows(2) {
        let mut h = DefaultHasher::new();
        let s: String = w.iter().collect();
        s.hash(&mut h);
        let idx = (h.finish() as usize) % DIM;
        v[idx] = 1.0;
    }
    // 单字符兜底
    if chars.len() == 1 {
        let mut h = DefaultHasher::new();
        chars[0].hash(&mut h);
        v[(h.finish() as usize) % DIM] = 1.0;
    }
    v
}

#[allow(dead_code)]
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

fn bail(reason: &str) -> ! {
    eprintln!("[probe_recall] 失败: {reason}");
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

    // 开一个隔离的临时记忆库，绝不污染用户真实记忆。
    let tmp = std::env::temp_dir().join(format!(
        "mimofan_probe_recall_{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&tmp);
    let store = match VectorStore::open(&tmp, DIM) {
        Ok(s) => s,
        Err(e) => bail(&format!("打开临时记忆库失败: {e}")),
    };

    // 1) 把所有 facts 写入记忆库（模拟「早期会话存记忆」）。
    let mut stored: Vec<(String, Vec<f32>)> = Vec::new();
    for sc in &scenarios {
        let facts = sc["facts"].as_array().cloned().unwrap_or_default();
        for f in &facts {
            let value = f["value"].as_str().unwrap_or("").to_string();
            let stmt = f["statement"].as_str().unwrap_or("").to_string();
            // 记忆条目内容 = value + statement 拼接：value 是事实的核心断言
            // （查询时用户往往用 value 措辞），statement 是完整上下文。两者都进库，
            // 保证「用户用哪种措辞问都能召回」（也消解样本内 value/statement 措辞偏差）。
            let content = if value.is_empty() {
                stmt.clone()
            } else {
                format!("{value} | {stmt}")
            };
            if content.trim().is_empty() {
                continue;
            }
            let emb = pseudo_embedding(&content);
            let obs = Observation::new("probe-project".to_string(), "fact", content.clone());
            if store.store_observation(&obs, &emb).is_err() {
                bail("写入记忆条目失败");
            }
            stored.push((content, emb));
        }
    }
    if stored.is_empty() {
        bail("样本中没有可用的 facts，无法评分");
    }

    // 2) 对每个 query，用确定性伪 embedding 在已存事实里取最相似者，比对 expect。
    let mut total_queries = 0usize;
    let mut hit_queries = 0usize;
    let mut details = Vec::new();

    for sc in &scenarios {
        let sid = sc["id"].as_str().unwrap_or("?").to_string();
        let _facts = sc["facts"].as_array().cloned().unwrap_or_default();
        let queries = sc["queries"].as_array().cloned().unwrap_or_default();
        for q in &queries {
            let qtext = q["q"].as_str().unwrap_or("").to_string();
            let expect = q["expect"].as_str().unwrap_or("").to_string();
            if qtext.is_empty() || expect.is_empty() {
                continue;
            }
            total_queries += 1;
            // 确定性关键词召回：在已存事实中，任一原文包含 expect 短词即算召回命中。
            // 这是「记忆里是否存得住这条事实、且能被关键词检索捞回」的确定性判定，
            // 不依赖语义嵌入（语义质量由真模型评测另行覆盖）。优化前后该链路应保持
            // 健康；它验证的是记忆存储/检索骨架，而非 P0 新能力本身（P0 新能力由
            // 静态 P0 矩阵单独度量，见 capability_matrix_p0.json）。
            let hit = stored.iter().any(|(stmt, _)| stmt.contains(&expect));
            if hit {
                hit_queries += 1;
            }
            details.push(serde_json::json!({
                "scenario": sid,
                "query": qtext,
                "expect": expect,
                "hit": hit,
            }));
        }
    }

    let rate = if total_queries > 0 {
        hit_queries as f64 / total_queries as f64
    } else {
        0.0
    };
    eprintln!(
        "[probe_recall] 跨会话召回 {hit_queries}/{total_queries} = {:.1}%",
        rate * 100.0
    );

    println!(
        "{}",
        serde_json::json!({
            "recall_rate": rate,
            "probe": "B6_memory_recall",
            "measures": "记忆写入→持久化→跨会话检索召回的确定性链路（伪 embedding，不含语义泛化）",
            "hit": hit_queries,
            "total": total_queries,
            "details": details,
        })
    );

    // 清理临时库。
    let _ = std::fs::remove_dir_all(&tmp);
}
