//! 可机评优化回路（#751）：对标 open-discovery/program-evolution。
//!
//! 核心思想：用户给 baseline + evaluator 脚本 + 目标；**evaluator 拥有正确性**，
//! agent 只提候选。外部程序裁决每个候选是否「正确且更优」，agent 不自己报分。
//!
//! 本模块提供纯逻辑层：
//! - [`EvaluatorOutput`]：evaluator 约定 JSON 的解析与判定。
//! - [`lock_baseline`]：拷贝+哈希+求值，拒绝覆盖、拒绝 evaluator 改写 baseline。
//! - [`CandidateLineage`]：候选血统（parent/patch/evaluator 输出/失败）留痕。
//!
//! 命令层 `/evolve` 复用 subagent 派发 candidate worker，但纯逻辑与编排解耦，
//! 便于单测与在 CI 中验收。

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use anyhow::bail;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

pub const LOCK_FILE_NAME: &str = "lock.json";
pub const CANDIDATES_DIR_NAME: &str = "candidates";
pub const EVOLUTION_DIR_NAME: &str = "evolution";

/// evaluator 输出的目标度量子结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Objective {
    pub name: String,
    pub value: f64,
    pub baseline_value: f64,
    /// "maximize" | "minimize"。
    #[serde(default = "default_direction")]
    pub direction: String,
}

fn default_direction() -> String {
    "maximize".to_string()
}

impl Objective {
    /// 相对 baseline 的改进比例（value / baseline_value，方向已归一）。
    pub fn improvement_ratio(&self) -> f64 {
        if self.baseline_value == 0.0 {
            return 0.0;
        }
        let raw = self.value / self.baseline_value;
        if self.direction == "minimize" {
            2.0 - raw
        } else {
            raw
        }
    }
}

/// evaluator 的标准输出契约：退出成功 + 单一 JSON 到 stdout。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorOutput {
    /// 候选是否通过正确性门（evaluator 判定，非 agent 自述）。
    pub valid: bool,
    /// 在 valid 前提下，是否优于 baseline。
    #[serde(default)]
    pub improved: bool,
    #[serde(default)]
    pub objective: Option<Objective>,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(default)]
    pub failures: Vec<String>,
}

impl EvaluatorOutput {
    pub fn from_stdout(stdout: &str) -> Result<Self> {
        // evaluator 可能打印额外日志，最后一行为 JSON 对象。
        let line = stdout
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with('{'))
            .ok_or_else(|| anyhow::anyhow!("evaluator stdout 无 JSON 对象"))?;
        Ok(serde_json::from_str(line)?)
    }

    /// 是否可作为「有效候选」进入下一步（valid 且 improved）。
    pub fn is_winner(&self) -> bool {
        self.valid && self.improved
    }
}

/// baseline 锁定记录：拷贝后的基线、evaluator、其哈希、求值结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineLock {
    pub baseline_path: PathBuf,
    pub evaluator_path: PathBuf,
    pub baseline_hash: String,
    pub evaluator_hash: String,
    pub result: EvaluatorOutput,
    pub goal: String,
}

/// 锁定 baseline：拷贝到 `<out>/` 隔离目录并求值，写 lock.json。
///
/// 安全约束：
/// - 若 `<out>/lock.json` 已存在，**拒绝覆盖**（BaselineLock 不可变，保证可比性）。
/// - 若 evaluator 在锁定时改写 baseline，后续校验哈希会失败（见 [`verify_baseline_unchanged`]）。
pub fn lock_baseline(
    baseline: &Path,
    evaluator: &Path,
    goal: &str,
    out: &Path,
) -> Result<BaselineLock> {
    let lock_path = out.join(LOCK_FILE_NAME);
    if lock_path.exists() {
        bail!("baseline lock 已存在，拒绝覆盖: {}", lock_path.display());
    }
    std::fs::create_dir_all(out)?;
    let copied_baseline = out.join("baseline_copy");
    // 保留原扩展名，使 run_evaluator 的解释器选择（sh/python3）生效。
    let copied_evaluator = match evaluator.extension().and_then(|e| e.to_str()) {
        Some(ext) => out.join(format!("evaluator_copy.{ext}")),
        None => out.join("evaluator_copy"),
    };
    copy_file(baseline, &copied_baseline)?;
    copy_file(evaluator, &copied_evaluator)?;

    let baseline_hash = hash_file(baseline)?;
    let evaluator_hash = hash_file(evaluator)?;

    let raw = run_evaluator(&copied_evaluator, &copied_baseline)?;
    let result = EvaluatorOutput::from_stdout(&raw)?;

    let lock = BaselineLock {
        baseline_path: copied_baseline,
        evaluator_path: copied_evaluator,
        baseline_hash,
        evaluator_hash,
        result,
        goal: goal.to_string(),
    };
    std::fs::write(&lock_path, serde_json::to_string_pretty(&lock)?)?;
    Ok(lock)
}

/// 校验锁定后 baseline / evaluator 未被改写（哈希对比）。
pub fn verify_baseline_unchanged(lock: &BaselineLock, baseline: &Path, evaluator: &Path) -> Result<()> {
    let cur_baseline = hash_file(baseline)?;
    let cur_evaluator = hash_file(evaluator)?;
    if cur_baseline != lock.baseline_hash {
        bail!("baseline 在锁定后被改写，哈希不匹配");
    }
    if cur_evaluator != lock.evaluator_hash {
        bail!("evaluator 在锁定后被改写，哈希不匹配");
    }
    Ok(())
}

/// 候选血统：用于可追溯与终选重建。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateLineage {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// candidate 文件路径（相对 evolution 目录）。
    pub path: PathBuf,
    pub evaluator_output: EvaluatorOutput,
    #[serde(default)]
    pub patch_summary: String,
}

/// 把候选血统追加到 `<evolution_dir>/candidates/<id>.jsonl`。
pub fn record_candidate(evolution_dir: &Path, cand: &CandidateLineage) -> Result<PathBuf> {
    let dir = evolution_dir.join(CANDIDATES_DIR_NAME);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", cand.id));
    std::fs::write(&path, serde_json::to_string_pretty(cand)?)?;
    Ok(path)
}

/// 运行 evaluator：把 candidate 路径作为唯一位置参数，捕获 stdout。
pub fn run_evaluator_on(evaluator: &Path, candidate: &Path) -> Result<EvaluatorOutput> {
    let raw = run_evaluator(evaluator, candidate)?;
    EvaluatorOutput::from_stdout(&raw)
}

fn run_evaluator(evaluator: &Path, target: &Path) -> Result<String> {
    // 按扩展名选择解释器，跨平台可靠执行（macOS 直接 exec .sh 需 +x 位且受
    // SIP/APFS 限制易失败；统一经解释器调用更稳）。
    let mut cmd = std::process::Command::new(interpreter_for(evaluator));
    if !is_direct_executable(evaluator) {
        cmd.arg(evaluator);
    }
    let out = cmd.arg(target).output()?;
    if !out.status.success() {
        bail!(
            "evaluator 退出非零: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 返回执行 evaluator 的命令程序：`.sh`→sh，`.py`→python3，否则直接 exec 文件本身。
fn interpreter_for(evaluator: &Path) -> String {
    match evaluator.extension().and_then(|e| e.to_str()) {
        Some("sh") => "sh".to_string(),
        Some("py") => "python3".to_string(),
        _ => evaluator.to_string_lossy().to_string(),
    }
}

/// 是否应直接 exec（无解释器前缀）：仅当扩展名非 sh/py 时。
fn is_direct_executable(evaluator: &Path) -> bool {
    !matches!(
        evaluator.extension().and_then(|e| e.to_str()),
        Some("sh") | Some("py")
    )
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        use std::sync::atomic::AtomicU64;
        use std::sync::atomic::Ordering;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!(
            "mimofan-evolve-test-{}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or(""),
            n
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_script(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
    }

    #[test]
    fn evaluator_output_parses_from_last_json_line() {
        let out = "loading...\nlog line\n{\"valid\":true,\"improved\":true,\"objective\":{\"name\":\"speedup\",\"value\":1.24,\"baseline_value\":1.0,\"direction\":\"maximize\"}}";
        let e = EvaluatorOutput::from_stdout(out).unwrap();
        assert!(e.is_winner());
        let obj = e.objective.unwrap();
        assert!((obj.improvement_ratio() - 1.24).abs() < 1e-9);
    }

    #[test]
    fn invalid_candidate_not_winner() {
        let out = "{\"valid\":false,\"improved\":false,\"failures\":[\"wrong output\"]}";
        let e = EvaluatorOutput::from_stdout(out).unwrap();
        assert!(!e.is_winner());
        assert_eq!(e.failures.len(), 1);
    }

    #[test]
    fn lock_baseline_refuses_overwrite() {
        let d = tmp();
        let baseline = d.join("prog.py");
        let eval = d.join("eval.sh");
        std::fs::write(&baseline, "print(1)").unwrap();
        write_script(&eval, "#!/bin/sh\necho '{\"valid\":true,\"improved\":false}'\n");
        let out = d.join("lockout");
        let _lock = lock_baseline(&baseline, &eval, "goal", &out).unwrap();
        // 第二次锁定同目录必须拒绝
        let err = lock_baseline(&baseline, &eval, "goal", &out);
        assert!(err.is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lock_then_verify_detects_baseline_mutataion() {
        let d = tmp();
        let baseline = d.join("prog.py");
        let eval = d.join("eval.sh");
        std::fs::write(&baseline, "print(1)").unwrap();
        write_script(&eval, "#!/bin/sh\necho '{\"valid\":true,\"improved\":false}'\n");
        let out = d.join("lockout");
        let lock = lock_baseline(&baseline, &eval, "goal", &out).unwrap();
        // 改写 baseline
        std::fs::write(&baseline, "print(2)").unwrap();
        assert!(verify_baseline_unchanged(&lock, &baseline, &eval).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn candidate_lineage_records() {
        let d = tmp();
        let cand = CandidateLineage {
            id: "cand-1".into(),
            parent_id: None,
            path: PathBuf::from("candidates/cand-1/prog.py"),
            evaluator_output: EvaluatorOutput::from_stdout("{\"valid\":true,\"improved\":true}").unwrap(),
            patch_summary: "use two-token lookup".into(),
        };
        let p = record_candidate(&d, &cand).unwrap();
        assert!(p.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
