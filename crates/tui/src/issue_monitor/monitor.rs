//! Issue Monitor 核心类型定义

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::route::RouteConstraint;

/// Monitor 状态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MonitorStatus {
    /// 活跃状态，监听 issue 变化
    Active,
    /// 暂停状态，不响应 wake 事件
    Paused,
    /// 已完成，PR 已合并或关闭
    Completed,
}

/// Issue Monitor 实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueMonitor {
    /// 唯一标识
    pub id: String,
    /// 监控名称
    pub name: String,
    /// 监控的 issue 编号列表
    pub issue_numbers: Vec<u64>,
    /// GitHub 仓库（格式：owner/repo）
    pub repo: String,
    /// 目标分支
    pub target_branch: String,
    /// 当前状态
    pub status: MonitorStatus,
    /// 路由约束
    pub constraints: Vec<RouteConstraint>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    /// 最后唤醒时间
    pub last_wake_at: Option<DateTime<Utc>>,
    /// 关联的 PR 编号
    pub pr_number: Option<u64>,
    /// 唤醒次数
    pub wake_count: u64,
}

/// Wake 事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WakeEventType {
    /// Issue 状态变化（opened, closed, reopened）
    IssueStateChange {
        issue_number: u64,
        old_state: String,
        new_state: String,
    },
    /// Issue 新增评论
    IssueCommentAdded {
        issue_number: u64,
        comment_id: u64,
    },
    /// Issue 标签变化
    IssueLabelChange {
        issue_number: u64,
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// PR 状态变化
    PrStateChange {
        pr_number: u64,
        old_state: String,
        new_state: String,
    },
    /// PR 评论
    PrCommentAdded {
        pr_number: u64,
        comment_id: u64,
    },
    /// PR 合并
    PrMerged {
        pr_number: u64,
    },
    /// 定时唤醒（兜底）
    ScheduledWake,
}

/// Wake 事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeEvent {
    /// 事件 ID
    pub id: String,
    /// 关联的 monitor ID
    pub monitor_id: String,
    /// 事件类型
    pub event_type: WakeEventType,
    /// 事件时间
    pub timestamp: DateTime<Utc>,
    /// 事件来源（webhook, poll, schedule）
    pub source: String,
}

impl WakeEvent {
    /// 创建新的 wake 事件
    pub fn new(monitor_id: String, event_type: WakeEventType, source: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            monitor_id,
            event_type,
            timestamp: Utc::now(),
            source,
        }
    }

    /// 获取关联的 issue 编号
    pub fn related_issue(&self) -> Option<u64> {
        match &self.event_type {
            WakeEventType::IssueStateChange { issue_number, .. }
            | WakeEventType::IssueCommentAdded { issue_number, .. }
            | WakeEventType::IssueLabelChange { issue_number, .. } => Some(*issue_number),
            _ => None,
        }
    }

    /// 获取关联的 PR 编号
    pub fn related_pr(&self) -> Option<u64> {
        match &self.event_type {
            WakeEventType::PrStateChange { pr_number, .. }
            | WakeEventType::PrCommentAdded { pr_number, .. }
            | WakeEventType::PrMerged { pr_number } => Some(*pr_number),
            _ => None,
        }
    }
}
