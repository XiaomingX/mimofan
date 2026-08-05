//! 路由钉定：防止工作流静默漂移
//!
//! 路由钉定确保 issue monitor 的工作范围在创建时被明确约束，
//! 后续 turn 不得静默跳到无关 issue / 文件。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::monitor::WakeEvent;

/// 路由约束类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RouteConstraint {
    /// 限定监控的 issue 编号
    Issues(Vec<u64>),
    /// 限定允许修改的文件路径模式（glob）
    AllowedPaths(Vec<String>),
    /// 限定不允许修改的文件路径模式（glob）
    DeniedPaths(Vec<String>),
    /// 限定目标分支
    TargetBranch(String),
    /// 自定义约束描述
    Custom(String),
}

/// 违规详情
#[derive(Debug, Clone)]
pub struct RouteViolation {
    /// 违规类型
    pub violation_type: String,
    /// 违规描述
    pub description: String,
    /// 建议的处理方式
    pub suggestion: String,
}

impl std::fmt::Display for RouteViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} (suggestion: {})",
            self.violation_type, self.description, self.suggestion
        )
    }
}

/// 路由钉定器
pub struct RoutePinning {
    /// monitor ID -> 约束列表
    constraints: Arc<RwLock<HashMap<String, Vec<RouteConstraint>>>>,
    /// monitor ID -> 允许的 issue 集合（预计算）
    allowed_issues: Arc<RwLock<HashMap<String, HashSet<u64>>>>,
    /// monitor ID -> 允许的路径模式
    allowed_paths: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl Default for RoutePinning {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutePinning {
    pub fn new() -> Self {
        Self {
            constraints: Arc::new(RwLock::new(HashMap::new())),
            allowed_issues: Arc::new(RwLock::new(HashMap::new())),
            allowed_paths: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册 monitor 的路由约束
    pub async fn register(&self, monitor_id: &str, constraints: &[RouteConstraint]) -> Result<()> {
        let mut all_constraints = self.constraints.write().await;
        let mut issues = self.allowed_issues.write().await;
        let mut paths = self.allowed_paths.write().await;

        all_constraints.insert(monitor_id.to_string(), constraints.to_vec());

        // 预计算允许的 issue 集合
        let mut allowed = HashSet::new();
        let mut allowed_p = Vec::new();

        for constraint in constraints {
            match constraint {
                RouteConstraint::Issues(nums) => {
                    allowed.extend(nums.iter().cloned());
                }
                RouteConstraint::AllowedPaths(patterns) => {
                    allowed_p.extend(patterns.iter().cloned());
                }
                _ => {}
            }
        }

        issues.insert(monitor_id.to_string(), allowed);
        paths.insert(monitor_id.to_string(), allowed_p);

        Ok(())
    }

    /// 取消注册
    pub async fn unregister(&self, monitor_id: &str) -> Result<()> {
        let mut all_constraints = self.constraints.write().await;
        let mut issues = self.allowed_issues.write().await;
        let mut paths = self.allowed_paths.write().await;

        all_constraints.remove(monitor_id);
        issues.remove(monitor_id);
        paths.remove(monitor_id);

        Ok(())
    }

    /// 检查 wake 事件是否违反路由约束
    pub async fn check_violation(
        &self,
        monitor_id: &str,
        event: &WakeEvent,
    ) -> Result<Option<RouteViolation>> {
        // 检查 issue 约束
        if let Some(issue_num) = event.related_issue() {
            let issues = self.allowed_issues.read().await;
            if let Some(allowed) = issues.get(monitor_id)
                && !allowed.is_empty()
                && !allowed.contains(&issue_num)
            {
                return Ok(Some(RouteViolation {
                    violation_type: "ISSUE_OUT_OF_SCOPE".to_string(),
                    description: format!(
                        "Issue #{} is not in the monitored scope {:?}",
                        issue_num, allowed
                    ),
                    suggestion:
                        "Confirm this issue should be included, or create a separate monitor."
                            .to_string(),
                }));
            }
        }

        // 检查 PR 约束（PR 关联的 issue 是否在范围内）
        if let Some(_pr_num) = event.related_pr() {
            // PR 的检查需要额外的 GitHub API 调用，这里简化处理
            // 实际实现中应该检查 PR 关联的 issues
        }

        Ok(None)
    }

    /// 验证文件修改是否在允许范围内
    pub async fn check_file_violation(
        &self,
        monitor_id: &str,
        file_path: &str,
    ) -> Result<Option<RouteViolation>> {
        let paths = self.allowed_paths.read().await;
        if let Some(allowed) = paths.get(monitor_id)
            && !allowed.is_empty()
        {
            let is_allowed = allowed.iter().any(|pattern| {
                // 简单的 glob 匹配（实际应该用 glob crate）
                if pattern.ends_with("/**") {
                    let prefix = &pattern[..pattern.len() - 3];
                    file_path.starts_with(prefix)
                } else if pattern.contains('*') {
                    // 简化的通配符匹配
                    let parts: Vec<&str> = pattern.split('*').collect();
                    if parts.len() == 2 {
                        file_path.starts_with(parts[0]) && file_path.ends_with(parts[1])
                    } else {
                        false
                    }
                } else {
                    file_path == pattern
                }
            });

            if !is_allowed {
                return Ok(Some(RouteViolation {
                    violation_type: "FILE_OUT_OF_SCOPE".to_string(),
                    description: format!(
                        "File '{}' is not in the allowed paths {:?}",
                        file_path, allowed
                    ),
                    suggestion:
                        "Confirm this file change is necessary, or update the monitor constraints."
                            .to_string(),
                }));
            }
        }

        Ok(None)
    }

    /// 获取 monitor 的所有约束
    pub async fn get_constraints(&self, monitor_id: &str) -> Vec<RouteConstraint> {
        let all_constraints = self.constraints.read().await;
        all_constraints.get(monitor_id).cloned().unwrap_or_default()
    }
}
