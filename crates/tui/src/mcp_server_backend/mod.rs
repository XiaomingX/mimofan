//! MCP 服务端的依赖反转接缝（端口）。
//!
//! `mcp_server` 作为 stdio 传输适配层，只依赖本文件的抽象端口，
//! 不再直接耦合 `crate::client` / `crate::session_manager` / `crate::config`
//! 等平级实现模块。具体实现由 crate 内的组合根（`run_mcp_server`）注入，
//! 符合 DDD 分层：传输适配层依赖抽象，实现细节下沉到基础设施层。

use std::future::Future;
use std::pin::Pin;

use anyhow::{Context, Result};

use crate::client::ApiClient;
use crate::config::Config;
use crate::llm_client::LlmClient;
use crate::models::{MessageRequest, MessageResponse};
use crate::session_manager::{SessionManager, SessionMetadata};

/// MCP 服务端运行期所需的外部能力端口。
///
/// 只抽象「确实被使用」的两项能力，不造多余抽象（奥卡姆剃刀）。
///
/// 用 `Box<dyn Future>` 而非 `async fn`，是为了保持 trait 的对象安全性
/// （`McpServer.backend` 以 `Box<dyn McpBackend>` 持有）。
pub trait McpBackend {
    /// 发送一次聊天补全请求。
    ///
    /// 内部完成「加载配置 + 创建客户端 + 调用 `LlmClient::create_message`」，
    /// 使 `mcp_server` 不再直接依赖 `ApiClient` / `Config` / `LlmClient`。
    ///
    /// 返回 `Pin<Box<dyn Future>>`：`Pin<Box<_>>` 本身即 `Unpin`，可被
    /// 组合根的 `runtime.block_on` 直接驱动；同时维持 trait 的对象安全性。
    fn send_message(
        &self,
        request: MessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<MessageResponse>> + Send + '_>>;

    /// 列出历史会话（替代直接 `SessionManager::default_location` + `list_sessions`）。
    fn list_sessions(&self) -> Vec<SessionSummary>;
}

/// 会话摘要：端口层与传输层之间的契约，避免向 `mcp_server` 暴露 `SessionMetadata` 实现细节。
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub message_count: usize,
}

/// 默认实现：直接对接 crate 内既有实现，由组合根注入。
pub struct RealMcpBackend;

impl McpBackend for RealMcpBackend {
    fn send_message(
        &self,
        request: MessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<MessageResponse>> + Send + '_>> {
        Box::pin(async move {
            // `create_message` 是定义在 `LlmClient` trait 上的方法，`ApiClient` 实现了它；
            // 在组合根这里引入 trait 作用域属合理（实现细节下沉点），传输层无需感知。
            let config = Config::load(None, None).context("Failed to load config")?;
            let client = ApiClient::new_detached(&config).context("Failed to create API client")?;
            client
                .create_message(request)
                .await
                .context("API call failed")
        })
    }

    fn list_sessions(&self) -> Vec<SessionSummary> {
        match SessionManager::default_location().and_then(|m| m.list_sessions()) {
            Ok(sessions) => sessions
                .into_iter()
                .map(|s: SessionMetadata| SessionSummary {
                    id: s.id,
                    title: s.title,
                    message_count: s.message_count,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}
