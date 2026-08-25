//! Tower 式 merge gate（合回主干前代码化闸门）。
//!
//! 背景：worktree 合回流程此前仅是文档约定（`CODEBUDDY.md` 的「合并前先验证」+
//! `git merge-base --is-ancestor` 校验），无代码化 gate。本模块在 worktree 合回主干前
//! 强制做一次「scope 越界判定」（改动文件是否超出预期范围）+ 一次 review 校验。
//!
//! 这是一个工具/库模块，不强制接入主 merge 流程（避免破坏既有行为），但可被 CLI/钩子调用。
//! 单向依赖：仅依赖 `reviewer`（纯逻辑 + git diff 调用），不引入新三方 crate。

use std::path::Path;
use std::process::Command;

use crate::reviewer::{ClaimForReview, ReviewVerdict};

/// scope 规范：描述一次 worktree 合回「被允许/被禁止」改动的文件范围。
///
/// 比对基于 `git diff --name-only` 产出的改动文件相对路径。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScopeSpec {
    /// 允许改动的文件 glob 列表（relative path，支持 `*` 通配）。
    /// 若为空，表示「除 deny 外全部允许」。
    pub allow: Vec<String>,
    /// 禁止改动的文件 glob 列表（无论 allow 是否命中，命中即越界）。
    pub deny: Vec<String>,
}

/// 一次 merge gate 的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateVerdict {
    /// scope 未越界（改动文件全部落在 allow 内且未命中 deny）。
    pub scope_ok: bool,
    /// 复用的 review 结论（基于传入的 ClaimForReview）。
    pub review: ReviewVerdict,
    /// 所有导致 gate 阻断的原因（人类可读）。空表示无阻断。
    pub blocking_reasons: Vec<String>,
}

impl GateVerdict {
    /// gate 是否放行：scope 不越界且 review 通过（Accepted）。
    pub fn passed(&self) -> bool {
        self.scope_ok && self.review == ReviewVerdict::Accepted && self.blocking_reasons.is_empty()
    }
}

/// Tower 式 merge gate。
pub struct MergeGate;

impl MergeGate {
    /// 执行合回前闸门：先 `git diff --name-only` 取得改动文件，再比对 scope，
    /// 最后复用 `reviewer::review` 校验传入的 claim。
    ///
    /// - `scope`：允许/禁止的改动范围规范。
    /// - `worktree_path`：worktree 工作目录（作为 git 调用的工作目录）。
    /// - `claim`：用于 review 的描述性 claim（例如「本次合回的变更已通过自动化验证」）。
    pub fn check(scope: &ScopeSpec, worktree_path: &Path, claim: &ClaimForReview) -> GateVerdict {
        let changed = changed_files(worktree_path);
        Self::check_with_files(scope, &changed, claim)
    }

    /// 内部比对逻辑（与「实际取 diff 列表」解耦，便于单元测试注入 mock 的改动文件列表）。
    pub(crate) fn check_with_files(
        scope: &ScopeSpec,
        changed_files: &[String],
        claim: &ClaimForReview,
    ) -> GateVerdict {
        let mut blocking_reasons = Vec::new();

        let scope_ok = evaluate_scope(scope, changed_files, &mut blocking_reasons);

        let review = crate::reviewer::review(claim);
        if review != ReviewVerdict::Accepted {
            blocking_reasons.push(format!("review verdict is {review:?}, expected Accepted"));
        }

        GateVerdict {
            scope_ok,
            review,
            blocking_reasons,
        }
    }
}

/// 调用 `git diff --name-only` 取得工作区相对改动文件列表（相对路径）。
///
/// 使用 std::process::Command，工作目录设为 `worktree_path`。失败（如非 git 仓库）
/// 时返回空列表，由调用方基于 scope 规则判定。
fn changed_files(worktree_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("diff")
        .arg("--name-only")
        .current_dir(worktree_path)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// 比对改动文件与 scope 规范，向 `blocking_reasons` 追加越界原因，返回 scope 是否 ok。
fn evaluate_scope(
    scope: &ScopeSpec,
    changed: &[String],
    blocking_reasons: &mut Vec<String>,
) -> bool {
    // deny 优先：命中任何 deny 即越界。
    for file in changed {
        if scope.deny.iter().any(|g| matches_glob(file, g)) {
            blocking_reasons.push(format!("file '{file}' is in deny list"));
        }
    }

    // allow 约束：若 allow 非空，任何未命中 allow 的文件即越界。
    if !scope.allow.is_empty() {
        for file in changed {
            let allowed = scope.allow.iter().any(|g| matches_glob(file, g));
            if !allowed {
                blocking_reasons.push(format!("file '{file}' is outside allow list"));
            }
        }
    }

    blocking_reasons.is_empty()
}

/// 简易 glob 匹配：支持 `*` 作为「任意字符序列」通配符（可多次出现）。
///
/// 覆盖 `src/**`、`*.rs`、`crates/tui/*`、`crates/*/src/*.rs` 等常见需求，
/// 不引入 glob crate。匹配规则：将 pattern 按 `*` 拆成非空的锚点段，
/// 文件必须按顺序包含这些锚点段（段之间允许任意字符）。
fn matches_glob(file: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let anchors: Vec<&str> = pattern.split('*').filter(|s| !s.is_empty()).collect();
    // 无锚点（纯 `*` 或空段组合）=> 通配一切。
    if anchors.is_empty() {
        return true;
    }
    let mut pos = 0usize;
    for (i, anchor) in anchors.iter().enumerate() {
        match file[pos..].find(anchor) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    // 首锚点必须出现在文件开头（除非 pattern 以 `*` 起始）。
                    if !pattern.starts_with('*') {
                        return false;
                    }
                }
                pos += idx + anchor.len();
            }
            None => return false,
        }
    }
    // 末锚点必须落在文件结尾（除非 pattern 以 `*` 结束）。
    if !pattern.ends_with('*') && pos != file.len() {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reviewer::EvidenceStrength;

    fn accepted_claim() -> ClaimForReview {
        ClaimForReview {
            title: "merge scope verified".into(),
            strength: EvidenceStrength::Strong,
            has_repro_steps: false,
            contradicted: false,
        }
    }

    fn rejected_claim() -> ClaimForReview {
        ClaimForReview {
            title: "merge scope NOT verified".into(),
            strength: EvidenceStrength::Strong,
            has_repro_steps: false,
            contradicted: true,
        }
    }

    #[test]
    fn in_scope_with_accepted_review_passes() {
        let scope = ScopeSpec {
            allow: vec!["crates/tui/src/worktree_gate.rs".into()],
            deny: vec![],
        };
        let changed = vec!["crates/tui/src/worktree_gate.rs".to_string()];
        let v = MergeGate::check_with_files(&scope, &changed, &accepted_claim());
        assert!(v.scope_ok);
        assert_eq!(v.review, ReviewVerdict::Accepted);
        assert!(v.blocking_reasons.is_empty());
        assert!(v.passed());
    }

    #[test]
    fn out_of_allow_scope_blocks() {
        let scope = ScopeSpec {
            allow: vec!["crates/tui/src/worktree_gate.rs".into()],
            deny: vec![],
        };
        // 改了范围外的文件
        let changed = vec![
            "crates/tui/src/worktree_gate.rs".to_string(),
            "crates/tui/src/lib.rs".to_string(),
        ];
        let v = MergeGate::check_with_files(&scope, &changed, &accepted_claim());
        assert!(!v.scope_ok);
        assert!(
            v.blocking_reasons
                .iter()
                .any(|r| r.contains("lib.rs") && r.contains("outside allow"))
        );
        assert!(!v.passed());
    }

    #[test]
    fn deny_list_blocks_even_if_allowed() {
        let scope = ScopeSpec {
            allow: vec!["crates/tui/src/*.rs".into()],
            deny: vec!["crates/tui/src/secret.rs".into()],
        };
        let changed = vec!["crates/tui/src/secret.rs".to_string()];
        let v = MergeGate::check_with_files(&scope, &changed, &accepted_claim());
        assert!(!v.scope_ok);
        assert!(
            v.blocking_reasons
                .iter()
                .any(|r| r.contains("secret.rs") && r.contains("deny"))
        );
        assert!(!v.passed());
    }

    #[test]
    fn empty_allow_allows_anything_except_deny() {
        let scope = ScopeSpec {
            allow: vec![],
            deny: vec!["Cargo.lock".into()],
        };
        let changed = vec!["crates/tui/src/lib.rs".to_string()];
        let v = MergeGate::check_with_files(&scope, &changed, &accepted_claim());
        assert!(v.scope_ok);
        assert!(v.passed());
    }

    #[test]
    fn review_failure_blocks_even_if_in_scope() {
        let scope = ScopeSpec {
            allow: vec!["crates/tui/src/worktree_gate.rs".into()],
            deny: vec![],
        };
        let changed = vec!["crates/tui/src/worktree_gate.rs".to_string()];
        let v = MergeGate::check_with_files(&scope, &changed, &rejected_claim());
        // scope 本身 ok，但 review 失败 => 整体阻断
        assert!(v.scope_ok);
        assert_eq!(v.review, ReviewVerdict::Rejected);
        assert!(!v.blocking_reasons.is_empty());
        assert!(!v.passed());
    }

    #[test]
    fn glob_prefix_and_suffix_match() {
        let scope = ScopeSpec {
            allow: vec!["crates/*/src/*.rs".into()],
            deny: vec![],
        };
        let changed = vec![
            "crates/tui/src/worktree_gate.rs".to_string(),
            "crates/core/src/foo.rs".to_string(),
        ];
        let v = MergeGate::check_with_files(&scope, &changed, &accepted_claim());
        assert!(v.scope_ok);
        assert!(v.passed());
    }

    #[test]
    fn changed_files_mock_empty_scope_ok() {
        // 空 scope + 无改动 => 通过
        let v = MergeGate::check_with_files(&ScopeSpec::default(), &[], &accepted_claim());
        assert!(v.scope_ok);
        assert!(v.passed());
    }
}
