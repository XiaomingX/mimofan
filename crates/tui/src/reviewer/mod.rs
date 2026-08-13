//! 独立评审者角色（#752）：对标 open-discovery 的 Scientific Reviewer。
//!
//! 核心思想：执行者与评审者职责分离，评审者依据**可核验证据**（不是 agent 自述）
//! 下结论。证据强度分级：
//! - strong：有外部 evaluator 通过 / 测试通过（如 #751 的 EvaluatorOutput.is_winner）
//! - medium：有复现步骤但未自动验证
//! - weak：仅自述
//!
//! 只有 Accepted 的 claim 才进入 #750 的 artifact 公开章节。

use serde::Deserialize;
use serde::Serialize;

/// 证据强度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStrength {
    Strong,
    Medium,
    Weak,
}

impl EvidenceStrength {
    pub fn rank(&self) -> u8 {
        match self {
            EvidenceStrength::Strong => 3,
            EvidenceStrength::Medium => 2,
            EvidenceStrength::Weak => 1,
        }
    }
}

/// 评审结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    /// 证据足够，可进入公开产物。
    Accepted,
    /// 证据不足或被反驳，不进入公开产物。
    Rejected,
    /// 介于两者之间：记录但不进入公开章节。
    Weak,
}

/// 一条待评审 claim 的评审输入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimForReview {
    pub title: String,
    pub strength: EvidenceStrength,
    /// 是否有可复现步骤（setup/脚本）。
    #[serde(default)]
    pub has_repro_steps: bool,
    /// 是否被独立 evaluator / 测试反驳。
    #[serde(default)]
    pub contradicted: bool,
}

/// 评审判定（纯逻辑）。
///
/// 规则：
/// - 若被反驳（contradicted），无论强度直接 Rejected。
/// - Strong 且未被反驳 → Accepted。
/// - Medium 且有复现步骤且未被反驳 → Accepted；Medium 无复现步骤 → Weak。
/// - Weak → Weak（不进入公开章节）。
pub fn review(claim: &ClaimForReview) -> ReviewVerdict {
    if claim.contradicted {
        return ReviewVerdict::Rejected;
    }
    match claim.strength {
        EvidenceStrength::Strong => ReviewVerdict::Accepted,
        EvidenceStrength::Medium => {
            if claim.has_repro_steps {
                ReviewVerdict::Accepted
            } else {
                ReviewVerdict::Weak
            }
        }
        EvidenceStrength::Weak => ReviewVerdict::Weak,
    }
}

/// 过滤：只保留可进入公开产物的 claim（Accepted）。
pub fn accepted_only(claims: &[ClaimForReview]) -> Vec<&ClaimForReview> {
    claims
        .iter()
        .filter(|c| review(c) == ReviewVerdict::Accepted)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_uncontradicted_accepted() {
        let c = ClaimForReview {
            title: "x".into(),
            strength: EvidenceStrength::Strong,
            has_repro_steps: false,
            contradicted: false,
        };
        assert_eq!(review(&c), ReviewVerdict::Accepted);
    }

    #[test]
    fn contradicted_strong_rejected() {
        let c = ClaimForReview {
            title: "x".into(),
            strength: EvidenceStrength::Strong,
            has_repro_steps: true,
            contradicted: true,
        };
        assert_eq!(review(&c), ReviewVerdict::Rejected);
    }

    #[test]
    fn medium_with_repro_accepted_else_weak() {
        let with = ClaimForReview {
            title: "x".into(),
            strength: EvidenceStrength::Medium,
            has_repro_steps: true,
            contradicted: false,
        };
        let without = ClaimForReview {
            title: "y".into(),
            strength: EvidenceStrength::Medium,
            has_repro_steps: false,
            contradicted: false,
        };
        assert_eq!(review(&with), ReviewVerdict::Accepted);
        assert_eq!(review(&without), ReviewVerdict::Weak);
    }

    #[test]
    fn weak_is_weak() {
        let c = ClaimForReview {
            title: "x".into(),
            strength: EvidenceStrength::Weak,
            has_repro_steps: true,
            contradicted: false,
        };
        assert_eq!(review(&c), ReviewVerdict::Weak);
    }

    #[test]
    fn accepted_only_filters() {
        let claims = vec![
            ClaimForReview {
                title: "a".into(),
                strength: EvidenceStrength::Strong,
                has_repro_steps: false,
                contradicted: false,
            },
            ClaimForReview {
                title: "b".into(),
                strength: EvidenceStrength::Weak,
                has_repro_steps: false,
                contradicted: false,
            },
            ClaimForReview {
                title: "c".into(),
                strength: EvidenceStrength::Medium,
                has_repro_steps: false,
                contradicted: false,
            },
        ];
        let accepted = accepted_only(&claims);
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].title, "a");
    }
}
