//! Issue Monitor: 一等公民的「Issue → 分组 PR」受监控工作流
//!
//! 本模块实现 GitHub Issue 的持久化监控，支持：
//! - 创建对单个/一组 issue 的监控任务并持久化
//! - issue 状态变化时自动唤醒并产出/更新分组 PR
//! - 工作流路由显式钉定，越界改动需确认（防静默漂移）

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::task_manager::SharedTaskManager;

mod monitor;
mod persistence;
mod route;

pub use monitor::{IssueMonitor, MonitorStatus, WakeEvent};
pub use persistence::MonitorStore;
pub use route::{RouteConstraint, RoutePinning};

/// Issue Monitor 管理器
pub struct IssueMonitorManager {
    /// 活跃的 monitors
    monitors: Arc<RwLock<HashMap<String, IssueMonitor>>>,
    /// 持久化存储
    store: MonitorStore,
    /// 路由钉定器
    route_pinner: RoutePinning,
    /// 任务管理器
    task_manager: SharedTaskManager,
}

impl IssueMonitorManager {
    /// 创建新的管理器
    pub fn new(task_manager: SharedTaskManager, data_dir: PathBuf) -> Self {
        let store = MonitorStore::new(data_dir);
        Self {
            monitors: Arc::new(RwLock::new(HashMap::new())),
            store,
            route_pinner: RoutePinning::new(),
            task_manager,
        }
    }

    /// 从持久化存储加载所有 monitors
    pub async fn load(&self) -> Result<()> {
        let loaded = self.store.load_all().await?;
        let mut monitors = self.monitors.write().await;
        for monitor in loaded {
            monitors.insert(monitor.id.clone(), monitor);
        }
        Ok(())
    }

    /// 创建新的 issue monitor
    pub async fn create_monitor(
        &self,
        name: String,
        issue_numbers: Vec<u64>,
        repo: String,
        target_branch: String,
        constraints: Vec<RouteConstraint>,
    ) -> Result<IssueMonitor> {
        let monitor = IssueMonitor {
            id: Uuid::new_v4().to_string(),
            name,
            issue_numbers: issue_numbers.clone(),
            repo,
            target_branch,
            status: MonitorStatus::Active,
            constraints,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_wake_at: None,
            pr_number: None,
            wake_count: 0,
        };

        // 持久化
        self.store.save(&monitor).await?;

        // 添加到内存
        let mut monitors = self.monitors.write().await;
        monitors.insert(monitor.id.clone(), monitor.clone());

        // 注册路由约束
        self.route_pinner
            .register(&monitor.id, &monitor.constraints)
            .await?;

        Ok(monitor)
    }

    /// 获取 monitor
    pub async fn get_monitor(&self, id: &str) -> Option<IssueMonitor> {
        let monitors = self.monitors.read().await;
        monitors.get(id).cloned()
    }

    /// 列出所有活跃的 monitors
    pub async fn list_active(&self) -> Vec<IssueMonitor> {
        let monitors = self.monitors.read().await;
        monitors
            .values()
            .filter(|m| m.status == MonitorStatus::Active)
            .cloned()
            .collect()
    }

    /// 处理 wake 事件
    pub async fn handle_wake_event(&self, event: WakeEvent) -> Result<()> {
        let monitors = self.monitors.read().await;
        let monitor = monitors
            .get(&event.monitor_id)
            .context("Monitor not found")?;

        if monitor.status != MonitorStatus::Active {
            return Ok(());
        }

        // 检查路由约束
        if let Some(violation) = self
            .route_pinner
            .check_violation(&event.monitor_id, &event)
            .await?
        {
            bail!(
                "Route violation: {}. Changes outside scope require explicit confirmation.",
                violation
            );
        }

        // 更新 wake 计数
        drop(monitors);
        let mut monitors = self.monitors.write().await;
        if let Some(m) = monitors.get_mut(&event.monitor_id) {
            m.last_wake_at = Some(Utc::now());
            m.wake_count += 1;
            self.store.save(m).await?;
        }

        Ok(())
    }

    /// 暂停 monitor
    pub async fn pause(&self, id: &str) -> Result<()> {
        let mut monitors = self.monitors.write().await;
        let monitor = monitors.get_mut(id).context("Monitor not found")?;
        monitor.status = MonitorStatus::Paused;
        monitor.updated_at = Utc::now();
        self.store.save(monitor).await?;
        Ok(())
    }

    /// 恢复 monitor
    pub async fn resume(&self, id: &str) -> Result<()> {
        let mut monitors = self.monitors.write().await;
        let monitor = monitors.get_mut(id).context("Monitor not found")?;
        monitor.status = MonitorStatus::Active;
        monitor.updated_at = Utc::now();
        self.store.save(monitor).await?;
        Ok(())
    }

    /// 删除 monitor
    pub async fn delete(&self, id: &str) -> Result<()> {
        let mut monitors = self.monitors.write().await;
        monitors.remove(id);
        self.store.delete(id).await?;
        self.route_pinner.unregister(id).await?;
        Ok(())
    }

    /// 获取 monitor 关联的 issue 列表
    pub async fn get_monitored_issues(&self) -> HashSet<u64> {
        let monitors = self.monitors.read().await;
        monitors
            .values()
            .filter(|m| m.status == MonitorStatus::Active)
            .flat_map(|m| m.issue_numbers.clone())
            .collect()
    }

    /// 检查某个 issue 是否被监控
    pub async fn is_issue_monitored(&self, issue_number: u64) -> bool {
        let monitored = self.get_monitored_issues().await;
        monitored.contains(&issue_number)
    }
}
