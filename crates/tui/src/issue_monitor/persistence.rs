//! Monitor 持久化存储
//!
//! 将 issue monitor 信息持久化到本地文件系统，
//! 复用 mimofan 的数据目录结构。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::fs;

use super::monitor::IssueMonitor;

/// Monitor 存储
pub struct MonitorStore {
    /// 数据目录
    data_dir: PathBuf,
}

impl MonitorStore {
    /// 创建新的存储
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    /// 获取 monitor 文件路径
    fn monitor_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(format!("{}.json", id))
    }

    /// 确保数据目录存在
    async fn ensure_dir(&self) -> Result<()> {
        if !self.data_dir.exists() {
            fs::create_dir_all(&self.data_dir)
                .await
                .context("Failed to create monitor data directory")?;
        }
        Ok(())
    }

    /// 保存 monitor
    pub async fn save(&self, monitor: &IssueMonitor) -> Result<()> {
        self.ensure_dir().await?;

        let json = serde_json::to_string_pretty(monitor)
            .context("Failed to serialize monitor")?;

        let path = self.monitor_path(&monitor.id);
        fs::write(&path, json)
            .await
            .context("Failed to write monitor file")?;

        Ok(())
    }

    /// 加载单个 monitor
    pub async fn load(&self, id: &str) -> Result<IssueMonitor> {
        let path = self.monitor_path(id);
        let json = fs::read_to_string(&path)
            .await
            .context("Failed to read monitor file")?;

        let monitor: IssueMonitor =
            serde_json::from_str(&json).context("Failed to deserialize monitor")?;

        Ok(monitor)
    }

    /// 加载所有 monitors
    pub async fn load_all(&self) -> Result<Vec<IssueMonitor>> {
        self.ensure_dir().await?;

        let mut monitors = Vec::new();
        let mut entries = fs::read_dir(&self.data_dir)
            .await
            .context("Failed to read monitor directory")?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .context("Failed to read directory entry")?
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    match self.load(id).await {
                        Ok(monitor) => monitors.push(monitor),
                        Err(e) => {
                            // 跳过损坏的文件，继续加载其他
                            eprintln!("Warning: Failed to load monitor {}: {}", id, e);
                        }
                    }
                }
            }
        }

        Ok(monitors)
    }

    /// 删除 monitor
    pub async fn delete(&self, id: &str) -> Result<()> {
        let path = self.monitor_path(id);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .context("Failed to delete monitor file")?;
        }
        Ok(())
    }

    /// 检查 monitor 是否存在
    pub async fn exists(&self, id: &str) -> bool {
        self.monitor_path(id).exists()
    }

    /// 获取所有 monitor ID
    pub async fn list_ids(&self) -> Result<Vec<String>> {
        self.ensure_dir().await?;

        let mut ids = Vec::new();
        let mut entries = fs::read_dir(&self.data_dir)
            .await
            .context("Failed to read monitor directory")?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .context("Failed to read directory entry")?
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(id.to_string());
                }
            }
        }

        Ok(ids)
    }
}

/// Wake 事件日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeEventLog {
    /// 事件 ID
    pub id: String,
    /// Monitor ID
    pub monitor_id: String,
    /// 事件类型（序列化字符串）
    pub event_type: String,
    /// 事件时间
    pub timestamp: chrono::DateTime<Utc>,
    /// 事件来源
    pub source: String,
    /// 处理结果
    pub result: Option<String>,
}

/// 事件日志存储
pub struct WakeEventStore {
    data_dir: PathBuf,
}

impl WakeEventStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    /// 保存事件日志
    pub async fn save(&self, log: &WakeEventLog) -> Result<()> {
        if !self.data_dir.exists() {
            fs::create_dir_all(&self.data_dir)
                .await
                .context("Failed to create event log directory")?;
        }

        let filename = format!("{}_{}.json", log.monitor_id, log.id);
        let path = self.data_dir.join(&filename);
        let json = serde_json::to_string_pretty(log)
            .context("Failed to serialize event log")?;

        fs::write(&path, json)
            .await
            .context("Failed to write event log")?;

        Ok(())
    }

    /// 加载 monitor 的所有事件日志
    pub async fn load_for_monitor(&self, monitor_id: &str) -> Result<Vec<WakeEventLog>> {
        if !self.data_dir.exists() {
            return Ok(Vec::new());
        }

        let mut logs = Vec::new();
        let mut entries = fs::read_dir(&self.data_dir)
            .await
            .context("Failed to read event log directory")?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .context("Failed to read directory entry")?
        {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                if filename.starts_with(&format!("{}_", monitor_id))
                    && filename.ends_with(".json")
                {
                    let json = fs::read_to_string(&path)
                        .await
                        .context("Failed to read event log")?;
                    if let Ok(log) = serde_json::from_str::<WakeEventLog>(&json) {
                        logs.push(log);
                    }
                }
            }
        }

        // 按时间排序
        logs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        Ok(logs)
    }
}
