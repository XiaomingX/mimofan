//! 可复现性纪律（#754）：BRIEF 单一事实源 + provenance 留痕 + env/依赖快照。
//!
//! 对标 open-discovery 的可复现性范式：
//! - `BRIEF.md` 是研究的唯一事实源，多轮后原始意图不漂移。
//! - provenance 记录每条结论/代码的来源（回合、模型、读写文件、父候选）。
//! - env/依赖哈希快照，保证干净环境可复现。
//!
//! 本模块是纯逻辑层，不强制触发；goal_loop / evolve 在启动时调用
//! [`write_brief`]，工具/回合层可调用 [`record_provenance`] 累积证据。

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

pub const BRIEF_FILE_NAME: &str = "BRIEF.md";
pub const PROVENANCE_FILE_NAME: &str = "provenance.jsonl";
pub const ENV_SNAPSHOT_FILE_NAME: &str = "env_snapshot.json";
pub const REPRO_DIR_NAME: &str = "repro";

/// 一个研究 initiative 的原始 brief：唯一事实源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    pub id: String,
    pub text: String,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    /// 来源：goal_loop / evolve / cli。
    #[serde(default)]
    pub source: String,
}

impl Default for Brief {
    fn default() -> Self {
        Brief {
            id: String::new(),
            text: String::new(),
            created_at: Utc::now(),
            source: String::new(),
        }
    }
}

impl Brief {
    pub fn new(text: impl Into<String>, source: impl Into<String>) -> Self {
        Brief {
            id: format!("brief-{}", UlidLike::new()),
            text: text.into(),
            created_at: Utc::now(),
            source: source.into(),
        }
    }

    /// 渲染为 BRIEF.md 文本：第一行标题，空行，原文。
    pub fn to_markdown(&self) -> String {
        format!("# Research Brief\n\n{}\n", self.text)
    }
}

/// 单条 provenance 记录：回合/工具调用级来源元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
    /// 回合计数或标识（如 turn-12）。
    #[serde(default)]
    pub turn_id: String,
    /// 使用的模型标识（如 glm-5 / deepseek-chat）。
    #[serde(default)]
    pub model: String,
    /// 本次读取的文件。
    #[serde(default)]
    pub files_read: Vec<String>,
    /// 本次写入/修改的文件。
    #[serde(default)]
    pub files_written: Vec<String>,
    /// 若来自 evolve 候选，记录父候选 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_candidate: Option<String>,
    /// 自由文本：本次产出了什么结论/代码。
    #[serde(default)]
    pub note: String,
}

impl Default for ProvenanceRecord {
    fn default() -> Self {
        ProvenanceRecord {
            timestamp: Utc::now(),
            turn_id: String::new(),
            model: String::new(),
            files_read: Vec::new(),
            files_written: Vec::new(),
            parent_candidate: None,
            note: String::new(),
        }
    }
}

/// 环境快照：用于复现性声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvSnapshot {
    #[serde(default)]
    pub rust_version: Option<String>,
    #[serde(default)]
    pub python_version: Option<String>,
    /// 关键依赖锁文件（Cargo.lock / package-lock.json）的 sha256 前缀。
    #[serde(default)]
    pub dependency_lock_hashes: Vec<String>,
    #[serde(default = "Utc::now")]
    pub captured_at: DateTime<Utc>,
}

/// 把 brief 写入 `<dir>/repro/BRIEF.md`，返回完整路径。
pub fn write_brief(dir: &Path, brief: &Brief) -> Result<PathBuf> {
    let repro_dir = dir.join(REPRO_DIR_NAME);
    std::fs::create_dir_all(&repro_dir)?;
    let path = repro_dir.join(BRIEF_FILE_NAME);
    std::fs::write(&path, brief.to_markdown())?;
    Ok(path)
}

/// 把一条 provenance 记录追加到 `<dir>/repro/provenance.jsonl`（JSON Lines）。
pub fn record_provenance(dir: &Path, record: &ProvenanceRecord) -> Result<PathBuf> {
    let repro_dir = dir.join(REPRO_DIR_NAME);
    std::fs::create_dir_all(&repro_dir)?;
    let path = repro_dir.join(PROVENANCE_FILE_NAME);
    let line = serde_json::to_string(record)?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    Ok(path)
}

/// 读取并解析 provenance.jsonl 全部记录。空文件/不存在返回空 vec。
pub fn read_provenance(dir: &Path) -> Result<Vec<ProvenanceRecord>> {
    let path = dir.join(REPRO_DIR_NAME).join(PROVENANCE_FILE_NAME);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        out.push(serde_json::from_str(line)?);
    }
    Ok(out)
}

/// 捕获环境快照（纯逻辑：调用 `rustc --version` / `python3 --version`，
/// 并对给定依赖锁文件计算 sha256 前缀）。任何步骤失败则对应字段为 None，
/// 不整体失败（快照是尽力而为的复现性声明）。
pub fn snapshot_env(dependency_lock_paths: &[PathBuf]) -> EnvSnapshot {
    let rust_version = run_version(&["rustc", "--version"]);
    let python_version = run_version(&["python3", "--version"]);
    let mut dependency_lock_hashes = Vec::new();
    for p in dependency_lock_paths {
        if let Ok(hash) = hash_file_prefix(p, 16) {
            dependency_lock_hashes.push(hash);
        }
    }
    EnvSnapshot {
        rust_version,
        python_version,
        dependency_lock_hashes,
        captured_at: Utc::now(),
    }
}

/// 把环境快照写入 `<dir>/repro/env_snapshot.json`。
pub fn write_env_snapshot(dir: &Path, snap: &EnvSnapshot) -> Result<PathBuf> {
    let repro_dir = dir.join(REPRO_DIR_NAME);
    std::fs::create_dir_all(&repro_dir)?;
    let path = repro_dir.join(ENV_SNAPSHOT_FILE_NAME);
    std::fs::write(&path, serde_json::to_string_pretty(snap)?)?;
    Ok(path)
}

fn run_version(cmd: &[&str]) -> Option<String> {
    std::process::Command::new(cmd[0])
        .args(&cmd[1..])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            let s = s.trim();
            if s.is_empty() {
                String::from_utf8_lossy(&o.stderr).trim().to_string()
            } else {
                s.to_string()
            }
            .into()
        })
        .filter(|s: &String| !s.is_empty())
}

fn hash_file_prefix(path: &Path, hex_len: usize) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let full = format!("{:x}", hasher.finalize());
    Ok(full[..hex_len.min(full.len())].to_string())
}

/// 轻量唯一 id（避免引入 ulid crate 依赖；仅需会话内唯一性）。
struct UlidLike;
impl UlidLike {
    fn new() -> String {
        let now = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let rand: u32 = rand_simple();
        format!("{now:x}{rand:x}")
    }
}

fn rand_simple() -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut h = DefaultHasher::new();
    h.write_i64(Utc::now().timestamp_nanos_opt().unwrap_or(0));
    h.finish() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("mimofan-repro-test-{}", UlidLike::new()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn brief_markdown_roundtrip() {
        let b = Brief::new("Reproduce paper X method Y", "goal");
        let md = b.to_markdown();
        assert!(md.starts_with("# Research Brief"));
        assert!(md.contains("Reproduce paper X method Y"));
    }

    #[test]
    fn write_and_read_provenance_appends_jsonl() {
        let d = tmp();
        let r1 = ProvenanceRecord {
            turn_id: "turn-1".into(),
            model: "glm-5".into(),
            files_written: vec!["a.py".into()],
            note: "baseline impl".into(),
            ..Default::default()
        };
        let r2 = ProvenanceRecord {
            turn_id: "turn-2".into(),
            model: "deepseek-chat".into(),
            parent_candidate: Some("cand-1".into()),
            note: "optimized".into(),
            ..Default::default()
        };
        record_provenance(&d, &r1).unwrap();
        record_provenance(&d, &r2).unwrap();
        let all = read_provenance(&d).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].files_written, vec!["a.py".to_string()]);
        assert_eq!(all[1].parent_candidate.as_deref(), Some("cand-1"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn read_provenance_missing_file_is_empty() {
        let d = tmp();
        assert!(read_provenance(&d).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_snapshot_persists() {
        let d = tmp();
        let snap = snapshot_env(&[]);
        let p = write_env_snapshot(&d, &snap).unwrap();
        let back: EnvSnapshot =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(back.rust_version, snap.rust_version);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn brief_write_creates_repro_dir() {
        let d = tmp();
        let b = Brief::new("do thing", "cli");
        let p = write_brief(&d, &b).unwrap();
        assert!(p.ends_with("repro/BRIEF.md"));
        assert!(p.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
