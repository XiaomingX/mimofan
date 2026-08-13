//! 研究动作级自治授权边界（#753）：对标 open-discovery 的 Default autonomy 纪律。
//!
//! 与命令级 execpolicy（shell 命令/网络/MCP/plugin 审计门）互补：本模块管的是
//! **研究副作用动作类**——即 agent 在自主研究期间可能产生「外部影响」的动作。
//!
//! 纪律：Auto 默认只做「本地、零成本、非破坏、用户请求隐含允许」的研究；
//! 花钱 / 外部通信 / 发布 / 读凭证 仍需显式授权。

use serde::Deserialize;
use serde::Serialize;

/// 研究副作用动作类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchActionClass {
    /// 本地文件读写、跑测试、本地探索：零成本非破坏。
    LocalSafe,
    /// 创建/推送 GitHub remote（含 #750 --publish）。
    PublishRemote,
    /// 调用付费外部 API（外部算力/模型）。
    ExternalSpend,
    /// 对外发消息/邮件/通信。
    ExternalComm,
    /// 读取私有凭证/密钥。
    CredentialRead,
}

/// 副作用策略（与 execpolicy 三态对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectPolicy {
    Allow,
    AskUser,
    Deny,
}

/// 把动作名归类为研究动作类（纯逻辑，便于单测）。
///
/// 识别规则（前缀/关键字匹配）：
/// - `gh repo create` / `gh push` / `git push` / `publish` → PublishRemote
/// - 含 `paid-api` / `external-model` / 显式花费标记 → ExternalSpend
/// - `send-email` / `notify` / `webhook` / `post-message` → ExternalComm
/// - `read-secret` / `read-credential` / `vault get` → CredentialRead
/// - 其余本地动作 → LocalSafe
pub fn classify_action(action: &str) -> ResearchActionClass {
    let a = action.trim().to_lowercase();
    if a.contains("gh repo create")
        || a.contains("gh push")
        || a.contains("git push")
        || a == "publish"
        || a.contains("--publish")
    {
        ResearchActionClass::PublishRemote
    } else if a.contains("paid-api") || a.contains("external-model") || a.contains("billing") {
        ResearchActionClass::ExternalSpend
    } else if a.contains("send-email")
        || a.contains("notify")
        || a.contains("webhook")
        || a.contains("post-message")
        || a.contains("external-comm")
    {
        ResearchActionClass::ExternalComm
    } else if a.contains("read-secret")
        || a.contains("read-credential")
        || a.contains("vault get")
        || a.contains("get-token")
    {
        ResearchActionClass::CredentialRead
    } else {
        ResearchActionClass::LocalSafe
    }
}

/// 默认策略：LocalSafe 自动放行，其余副作用类默认 AskUser（不自动执行）。
pub fn default_policy(class: ResearchActionClass) -> SideEffectPolicy {
    match class {
        ResearchActionClass::LocalSafe => SideEffectPolicy::Allow,
        _ => SideEffectPolicy::AskUser,
    }
}

/// 评估一个研究动作：返回是否需要在无人确认时中断请求授权。
/// 返回 true 表示「需显式授权（默认不放行）」。
pub fn requires_explicit_authorization(action: &str) -> bool {
    let class = classify_action(action);
    default_policy(class) != SideEffectPolicy::Allow
}

/// 端到端闸门建议：给定动作返回默认策略，以及一句话人类可读建议。
/// 编排层（如 `/artifact --publish`）用它向用户解释为何需要授权。
#[must_use]
pub fn advice_for(action: &str) -> (SideEffectPolicy, String) {
    let class = classify_action(action);
    let policy = default_policy(class);
    let advice = match policy {
        SideEffectPolicy::Allow => {
            format!("动作 {class:?} 默认自动放行（本地、零成本、非破坏）。")
        }
        SideEffectPolicy::AskUser => {
            format!(
                "动作 {class:?} 属于研究副作用类，默认需显式授权（Auto 不自动执行）。\
                 请在交互 / yolo 模式下确认，或去掉该副作用只做本地部分。"
            )
        }
        SideEffectPolicy::Deny => {
            format!("动作 {class:?} 默认拒绝。")
        }
    };
    (policy, advice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_safe_allowed_by_default() {
        assert_eq!(
            classify_action("run pytest"),
            ResearchActionClass::LocalSafe
        );
        assert!(!requires_explicit_authorization("cargo test"));
        assert_eq!(
            default_policy(ResearchActionClass::LocalSafe),
            SideEffectPolicy::Allow
        );
    }

    #[test]
    fn publish_remote_asks_by_default() {
        assert_eq!(
            classify_action("gh repo create"),
            ResearchActionClass::PublishRemote
        );
        assert_eq!(
            classify_action("git push origin"),
            ResearchActionClass::PublishRemote
        );
        assert!(requires_explicit_authorization("--publish"));
        assert_eq!(
            default_policy(ResearchActionClass::PublishRemote),
            SideEffectPolicy::AskUser
        );
    }

    #[test]
    fn external_spend_and_comm_ask() {
        assert_eq!(
            classify_action("call paid-api gpt"),
            ResearchActionClass::ExternalSpend
        );
        assert_eq!(
            classify_action("send-email report"),
            ResearchActionClass::ExternalComm
        );
        assert!(requires_explicit_authorization("webhook post"));
    }

    #[test]
    fn credential_read_asks() {
        assert_eq!(
            classify_action("vault get api_key"),
            ResearchActionClass::CredentialRead
        );
        assert!(requires_explicit_authorization("read-secret"));
    }

    #[test]
    fn advice_for_publish_asks_with_human_text() {
        let (policy, advice) = advice_for("--publish");
        assert_eq!(policy, SideEffectPolicy::AskUser);
        assert!(advice.contains("PublishRemote"));
        assert!(advice.contains("需显式授权"));
    }

    #[test]
    fn advice_for_local_safe_allows() {
        let (policy, advice) = advice_for("cargo test");
        assert_eq!(policy, SideEffectPolicy::Allow);
        assert!(advice.contains("自动放行"));
    }
}
