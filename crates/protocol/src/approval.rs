use serde::{Deserialize, Serialize};

/// Policy controlling when the agent must ask the user for approval before acting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AskForApproval {
    /// Ask for approval unless the action is on a trusted path/resource.
    UnlessTrusted,
    /// Only ask after a tool call fails.
    OnFailure,
    /// Ask every time a tool call is requested.
    OnRequest,
    /// Reject the action without asking, with details on which categories are blocked.
    Reject {
        sandbox_approval: bool,
        rules: bool,
        mcp_elicitations: bool,
    },
    /// Never ask; auto-approve all actions.
    Never,
}

/// Action to take for a network policy rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicyRuleAction {
    /// Allow network access to the host.
    Allow,
    /// Deny network access to the host.
    Deny,
}

/// A proposed amendment to the network access policy for a specific host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkPolicyAmendment {
    /// The host to amend the policy for.
    pub host: String,
    /// The action to apply.
    pub action: NetworkPolicyRuleAction,
}

/// A user's decision on an approval request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReviewDecision {
    /// Approve the action.
    Approved,
    /// Approve and also amend the execution policy.
    ApprovedExecpolicyAmendment,
    /// Approve for the remainder of this session only.
    ApprovedForSession,
    /// Approve with a network policy amendment.
    NetworkPolicyAmendment {
        host: String,
        action: NetworkPolicyRuleAction,
    },
    /// Deny the action.
    Denied,
    /// Abort the entire turn.
    Abort,
}

/// Context about a network access request that requires approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkApprovalContext {
    /// The host being accessed.
    pub host: String,
    /// The network protocol (e.g. `"https"`, `"tcp"`).
    pub protocol: String,
}

/// A user's approval decision sent in response to an approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecisionRequest {
    /// The decision identifier (e.g. `"approved"`, `"denied"`).
    pub decision: String,
    /// Whether to remember this decision for future similar requests.
    #[serde(default)]
    pub remember: bool,
}
