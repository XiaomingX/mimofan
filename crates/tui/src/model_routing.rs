//! Model selection and auto-routing.
//!
//! The CLI, TUI, runtime threads, subagents, and command handlers all need
//! this behavior, so it intentionally lives outside the command tree.

use std::time::Duration;

use anyhow::Result;

use crate::client::DeepSeekClient;
use crate::config::{ApiProvider, Config, normalize_model_name_for_provider};
use crate::llm_client::LlmClient;
use crate::model_inventory::ModelInventory;
use crate::models::{ContentBlock, Message, MessageRequest, MessageResponse, SystemPrompt};
use crate::tui::app::ReasoningEffort;

/// Big/cheap model pair the auto-router may choose between for the active
/// provider (#3018).
///
/// `cheap == None` means the provider has no known cheap tier: heuristics
/// stay on the current model (only thinking effort varies) and the network
/// router is skipped entirely (#1549).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouterCandidates {
    pub(crate) big: String,
    pub(crate) cheap: Option<String>,
}

impl RouterCandidates {
    pub(crate) fn deepseek() -> Self {
        Self {
            big: "deepseek-v4-pro".to_string(),
            cheap: Some("deepseek-v4-flash".to_string()),
        }
    }

    /// The cheap-tier id, falling back to `big` when no cheap tier exists.
    pub(crate) fn cheap_or_big(&self) -> &str {
        self.cheap.as_deref().unwrap_or(&self.big)
    }
}

/// Derive the auto-router's candidate pair for the active provider (#3018).
///
/// DeepSeek providers route between the canonical pro/flash pair. Hosted
/// routes with known wire ids for that pair (NVIDIA NIM, OpenRouter, Novita,
/// SiliconFlow, Wanjie Ark, Volcengine) use their provider
/// spellings. Every other provider has no known cheap tier: `big` is the
/// session model and `cheap` is `None`, so auto mode never fabricates a
/// DeepSeek id for a provider that cannot serve it.
pub(crate) fn provider_router_candidates(
    provider: crate::config::ApiProvider,
    current_model: &str,
) -> RouterCandidates {
    use crate::config::ApiProvider;
    if provider == ApiProvider::XiaomiMimo {
        let normalized = crate::config::normalize_model_name_for_provider(provider, current_model)
            .unwrap_or_else(|| current_model.to_string());
        return RouterCandidates {
            // GLM-5.2 (the default) routes faster/explore children to GLM-5-Turbo,
            // the same-family fast sibling. GLM-5.1 and GLM-5-Turbo itself have no
            // cheaper tier and keep children on the parent model.
            cheap: if normalized == crate::config::ZAI_GLM_5_2_MODEL {
                Some(crate::config::ZAI_GLM_5_TURBO_MODEL.to_string())
            } else {
                None
            },
            big: normalized,
        };
    }

    if provider == ApiProvider::XiaomiMimo
        && let Some(normalized) =
            crate::config::normalize_model_name_for_provider(provider, current_model)
        && matches!(
            normalized.as_str(),
            "z-ai/glm-5.1" | "z-ai/glm-5.2" | "z-ai/glm-5-turbo"
        )
    {
        return RouterCandidates {
            // z-ai/glm-5.2 routes faster children to z-ai/glm-5-turbo; the 5.1
            // and turbo ids have no cheaper tier and keep children on parent.
            cheap: if normalized == "z-ai/glm-5.2" {
                Some("z-ai/glm-5-turbo".to_string())
            } else {
                None
            },
            big: normalized,
        };
    }

    match provider {
        ApiProvider::XiaomiMimo => RouterCandidates::deepseek(),
        _ => RouterCandidates {
            big: current_model.to_string(),
            cheap: None,
        },
    }
}

/// Auto-select a model based on request complexity.
///
/// Short messages (<100 chars) go to the cheap tier. Long messages and
/// requests with complex keywords go to the big tier. The fallback is cheap.
/// This DeepSeek-candidate wrapper keeps legacy callers and tests intact;
/// provider-aware callers use [`auto_model_heuristic_for_candidates`].
pub(crate) fn auto_model_heuristic(input: &str, current_model: &str) -> String {
    auto_model_heuristic_for_candidates(input, current_model, &RouterCandidates::deepseek())
}

/// Candidate-aware variant of [`auto_model_heuristic`] (#3018).
pub(crate) fn auto_model_heuristic_for_candidates(
    input: &str,
    current_model: &str,
    candidates: &RouterCandidates,
) -> String {
    auto_model_heuristic_selection_with_bias(input, current_model, false, candidates).model
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoModelHeuristicConfidence {
    Decisive,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoModelHeuristicSelection {
    model: String,
    confidence: AutoModelHeuristicConfidence,
}

fn auto_model_heuristic_selection_with_bias(
    input: &str,
    _current_model: &str,
    cost_saving: bool,
    candidates: &RouterCandidates,
) -> AutoModelHeuristicSelection {
    let len = input.chars().count();
    let lower = input.to_lowercase();
    let borderline_pro_keywords: &[&str] = &[
        "implement",
        "analyze",
        "\u{5b9e}\u{73b0}",
        "\u{5206}\u{6790}",
        "\u{5be6}\u{73fe}",
    ];
    let strong_match = COMPLEX_KEYWORDS
        .iter()
        .any(|kw| !borderline_pro_keywords.contains(kw) && lower.contains(kw));
    let borderline_match = borderline_pro_keywords.iter().any(|kw| lower.contains(kw));
    let pro_match = strong_match || (!cost_saving && borderline_match);
    if pro_match {
        return AutoModelHeuristicSelection {
            model: candidates.big.clone(),
            confidence: AutoModelHeuristicConfidence::Decisive,
        };
    }
    if len < 100 {
        return AutoModelHeuristicSelection {
            model: candidates.cheap_or_big().to_string(),
            confidence: AutoModelHeuristicConfidence::Decisive,
        };
    }
    let long_threshold = if cost_saving { 1_000 } else { 500 };
    if len > long_threshold {
        return AutoModelHeuristicSelection {
            model: candidates.big.clone(),
            confidence: AutoModelHeuristicConfidence::Decisive,
        };
    }

    AutoModelHeuristicSelection {
        model: candidates.cheap_or_big().to_string(),
        confidence: AutoModelHeuristicConfidence::Ambiguous,
    }
}

const COMPLEX_KEYWORDS: &[&str] = &[
    "refactor",
    "architecture",
    "design",
    "debug",
    "security",
    "review",
    "audit",
    "migrate",
    "optimize",
    "rewrite",
    "implement",
    "analyze",
    "\u{91cd}\u{6784}",
    "\u{67b6}\u{6784}",
    "\u{8bbe}\u{8ba1}",
    "\u{8c03}\u{8bd5}",
    "\u{5b89}\u{5168}",
    "\u{5ba1}\u{67e5}",
    "\u{5ba1}\u{8ba1}",
    "\u{8fc1}\u{79fb}",
    "\u{4f18}\u{5316}",
    "\u{91cd}\u{5199}",
    "\u{5b9e}\u{73b0}",
    "\u{5206}\u{6790}",
    "\u{91cd}\u{69cb}",
    "\u{67b6}\u{69cb}",
    "\u{8a2d}\u{8a08}",
    "\u{8abf}\u{8a66}",
    "\u{5be9}\u{67e5}",
    "\u{5be9}\u{8a08}",
    "\u{9077}\u{79fb}",
    "\u{512a}\u{5316}",
    "\u{91cd}\u{5beb}",
    "\u{5be6}\u{73fe}",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoRouteRecommendation {
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoRouteSource {
    FlashRouter,
    Heuristic,
}

impl AutoRouteSource {
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            AutoRouteSource::FlashRouter => "flash-router",
            AutoRouteSource::Heuristic => "heuristic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoRouteSelection {
    pub(crate) provider: ApiProvider,
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
    pub(crate) source: AutoRouteSource,
}

/// Render the auto-router system prompt with the actual candidate ids
/// (#3018): the classifier must answer with ids the active provider can
/// serve, not hardcoded DeepSeek spellings.
#[allow(dead_code)] // legacy active-provider flash router tests still exercise this prompt.
pub(crate) fn auto_router_system_prompt(
    candidates: &RouterCandidates,
    cost_saving: bool,
) -> String {
    let cheap = candidates.cheap_or_big();
    let big = &candidates.big;
    let mut prompt = format!(
        include_str!("prompts/router_classifier.md"),
        cheap = cheap,
        big = big,
    );
    if cost_saving {
        prompt.push_str(&format!(
            "\n\nCost-saving mode is ON. Prefer {cheap} for any request that is \
not unmistakably agentic, multi-step, architecture/design, security review, \
debugging, or otherwise clearly out of the cheap tier's capability. Resolve \
ambiguous cases in favour of {cheap}, not {big}."
        ));
    }
    prompt
}

/// DeepSeek-candidate wrapper kept for the legacy parser tests; the
/// network router parses with [`parse_auto_route_recommendation_for_candidates`].

pub(crate) fn parse_auto_route_recommendation_for_candidates(
    raw: &str,
    candidates: &RouterCandidates,
) -> Option<AutoRouteRecommendation> {
    let json = extract_first_json_object(raw)?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let model = value.get("model").and_then(serde_json::Value::as_str)?;
    let model = normalize_auto_route_model(model, candidates)?;
    let reasoning_effort = value
        .get("thinking")
        .or_else(|| value.get("reasoning_effort"))
        .or_else(|| value.get("effort"))
        .and_then(serde_json::Value::as_str)
        .and_then(parse_auto_route_reasoning_effort);

    Some(AutoRouteRecommendation {
        model,
        reasoning_effort,
    })
}

fn extract_first_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end >= start).then_some(&raw[start..=end])
}

fn normalize_auto_route_model(model: &str, candidates: &RouterCandidates) -> Option<String> {
    let normalized = model.trim().to_ascii_lowercase();
    // Exact candidate ids, case-insensitively (#3018).
    if normalized == candidates.big.to_ascii_lowercase() {
        return Some(candidates.big.clone());
    }
    if let Some(cheap) = candidates.cheap.as_deref()
        && normalized == cheap.to_ascii_lowercase()
    {
        return Some(cheap.to_string());
    }
    // Legacy pro/flash shorthand maps onto the big/cheap tiers.
    match normalized.as_str() {
        "deepseek-v4-pro" | "v4-pro" | "pro" => Some(candidates.big.clone()),
        "deepseek-v4-flash" | "v4-flash" | "flash" => Some(candidates.cheap_or_big().to_string()),
        _ => None,
    }
}

fn parse_auto_route_reasoning_effort(effort: &str) -> Option<ReasoningEffort> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "off" | "disabled" | "none" | "false" => Some(ReasoningEffort::Off),
        "low" | "minimal" | "medium" | "mid" => Some(ReasoningEffort::High),
        "high" => Some(ReasoningEffort::High),
        "max" | "maximum" | "xhigh" | "ultracode" => Some(ReasoningEffort::Max),
        _ => None,
    }
}

#[must_use]
pub(crate) fn normalize_auto_route_effort(effort: ReasoningEffort) -> ReasoningEffort {
    normalize_auto_route_effort_for_provider(ApiProvider::XiaomiMimo, effort)
}

#[must_use]
pub(crate) fn normalize_auto_route_effort_for_provider(
    provider: ApiProvider,
    effort: ReasoningEffort,
) -> ReasoningEffort {
    if provider == ApiProvider::XiaomiMimo {
        return effort.normalize_for_provider(provider);
    }
    match effort {
        ReasoningEffort::Low | ReasoningEffort::Medium => ReasoningEffort::High,
        other => other,
    }
}

#[allow(dead_code)] // superseded by the route-effective inventory resolver (#3205).
pub(crate) async fn resolve_auto_route_with_flash(
    config: &Config,
    latest_request: &str,
    recent_context: &str,
    selected_model_mode: &str,
    selected_thinking_mode: &str,
) -> AutoRouteSelection {
    let cost_saving = config.auto_cost_saving();
    // #3018: derive the candidate pair from the active provider. The
    // config-resolved default model stands in for the session model — with
    // auto mode on, that is the canonical id the provider serves.
    let candidates = provider_router_candidates(config.api_provider(), &config.default_model());
    let heuristic = auto_model_heuristic_selection_with_bias(
        latest_request,
        selected_model_mode,
        cost_saving,
        &candidates,
    );
    if heuristic.confidence == AutoModelHeuristicConfidence::Decisive {
        return auto_route_from_heuristic(config.api_provider(), latest_request, heuristic);
    }

    // #1549/#3018: no cheap tier → no network round-trip. The heuristic is
    // the only signal and the routed model stays on the session model.
    if candidates.cheap.is_none() {
        return auto_route_from_heuristic(config.api_provider(), latest_request, heuristic);
    }

    match auto_route_flash_recommendation(
        config,
        &candidates,
        latest_request,
        recent_context,
        selected_model_mode,
        selected_thinking_mode,
    )
    .await
    {
        Ok(Some(recommendation)) => AutoRouteSelection {
            provider: config.api_provider(),
            model: recommendation.model,
            reasoning_effort: recommendation.reasoning_effort,
            source: AutoRouteSource::FlashRouter,
        },
        Ok(None) | Err(_) => {
            auto_route_from_heuristic(config.api_provider(), latest_request, heuristic)
        }
    }
}

#[allow(dead_code)] // retained for the legacy active-provider flash resolver.
fn auto_route_from_heuristic(
    provider: ApiProvider,
    latest_request: &str,
    heuristic: AutoModelHeuristicSelection,
) -> AutoRouteSelection {
    AutoRouteSelection {
        provider,
        model: heuristic.model,
        reasoning_effort: Some(normalize_auto_route_effort_for_provider(
            provider,
            crate::auto_reasoning::select(false, latest_request),
        )),
        source: AutoRouteSource::Heuristic,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InventoryAutoRouteRecommendation {
    provider: ApiProvider,
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
}

pub(crate) async fn resolve_auto_route_with_inventory(
    config: &Config,
    latest_request: &str,
    recent_context: &str,
    selected_model_mode: &str,
    selected_thinking_mode: &str,
) -> Result<AutoRouteSelection> {
    let inventory = ModelInventory::from_config(config);
    if !inventory.router_available {
        // Fall back to heuristic-only auto routing when the flash router
        // is unavailable (e.g. non-DeepSeek providers like wanjie-ark).
        return Ok(auto_route_from_inventory_heuristic(
            config,
            latest_request,
            &inventory,
        ));
    }

    let heuristic = auto_route_from_inventory_heuristic(config, latest_request, &inventory);
    if cfg!(test) {
        return Ok(heuristic);
    }

    match auto_route_inventory_recommendation(
        config,
        &inventory,
        latest_request,
        recent_context,
        selected_model_mode,
        selected_thinking_mode,
    )
    .await
    {
        Ok(Some(recommendation)) => Ok(AutoRouteSelection {
            provider: recommendation.provider,
            model: recommendation.model,
            reasoning_effort: recommendation.reasoning_effort,
            source: AutoRouteSource::FlashRouter,
        }),
        Ok(None) | Err(_) => Ok(heuristic),
    }
}

pub(crate) fn resolve_explicit_route_with_inventory(
    config: &Config,
    requested_model: &str,
) -> Option<AutoRouteSelection> {
    let requested_model = requested_model.trim();
    if requested_model.is_empty() || requested_model.eq_ignore_ascii_case("auto") {
        return None;
    }

    let inventory = ModelInventory::from_config(config);
    let active_provider = config.api_provider();

    if let Some(candidate) = inventory.candidates.iter().find(|candidate| {
        candidate.provider == active_provider
            && explicit_model_matches_candidate(candidate, requested_model)
    }) {
        return Some(AutoRouteSelection {
            provider: candidate.provider,
            model: candidate.model.clone(),
            reasoning_effort: config.reasoning_effort().map(|setting| {
                normalize_auto_route_effort_for_provider(
                    candidate.provider,
                    ReasoningEffort::from_setting(setting),
                )
            }),
            source: AutoRouteSource::Heuristic,
        });
    }

    let mut matches = inventory
        .candidates
        .iter()
        .filter(|candidate| explicit_model_matches_candidate(candidate, requested_model));
    let candidate = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    Some(AutoRouteSelection {
        provider: candidate.provider,
        model: candidate.model.clone(),
        reasoning_effort: config.reasoning_effort().map(|setting| {
            normalize_auto_route_effort_for_provider(
                candidate.provider,
                ReasoningEffort::from_setting(setting),
            )
        }),
        source: AutoRouteSource::Heuristic,
    })
}

pub(crate) fn explicit_route_candidate_providers(
    config: &Config,
    requested_model: &str,
) -> Vec<ApiProvider> {
    let requested_model = requested_model.trim();
    if requested_model.is_empty() || requested_model.eq_ignore_ascii_case("auto") {
        return Vec::new();
    }

    let inventory = ModelInventory::from_config(config);
    let mut providers = Vec::new();
    for candidate in inventory
        .candidates
        .iter()
        .filter(|candidate| explicit_model_matches_candidate(candidate, requested_model))
    {
        if !providers.contains(&candidate.provider) {
            providers.push(candidate.provider);
        }
    }
    providers
}

fn explicit_model_matches_candidate(
    candidate: &crate::model_inventory::ModelRouteCandidate,
    requested_model: &str,
) -> bool {
    candidate.model.eq_ignore_ascii_case(requested_model)
        || normalize_model_name_for_provider(candidate.provider, requested_model)
            .is_some_and(|model| candidate.model.eq_ignore_ascii_case(&model))
}

fn auto_route_from_inventory_heuristic(
    config: &Config,
    latest_request: &str,
    inventory: &ModelInventory,
) -> AutoRouteSelection {
    let Some(active) = inventory.active_default() else {
        return AutoRouteSelection {
            provider: config.api_provider(),
            model: config.default_model(),
            reasoning_effort: Some(crate::auto_reasoning::select(false, latest_request)),
            source: AutoRouteSource::Heuristic,
        };
    };
    // Use the candidates' cheap/big info for complexity-based routing.
    let router_candidates = provider_router_candidates(config.api_provider(), &active.model);
    let chosen = if router_candidates.cheap.is_some() {
        auto_model_heuristic_for_candidates(latest_request, &active.model, &router_candidates)
    } else {
        active.model.clone()
    };
    AutoRouteSelection {
        provider: active.provider,
        model: chosen,
        reasoning_effort: Some(crate::auto_reasoning::select(false, latest_request)),
        source: AutoRouteSource::Heuristic,
    }
}

async fn auto_route_inventory_recommendation(
    config: &Config,
    inventory: &ModelInventory,
    latest_request: &str,
    recent_context: &str,
    selected_model_mode: &str,
    selected_thinking_mode: &str,
) -> Result<Option<InventoryAutoRouteRecommendation>> {
    let mut router_config = config.clone();
    router_config.provider = Some(ApiProvider::XiaomiMimo.as_str().to_string());
    router_config.default_text_model = Some(inventory.router_model.to_string());

    let client = DeepSeekClient::new(&router_config)?;
    let router_system = inventory_auto_router_system_prompt(inventory);
    let request = MessageRequest {
        model: inventory.router_model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: auto_route_prompt(
                    latest_request,
                    recent_context,
                    selected_model_mode,
                    selected_thinking_mode,
                ),
                cache_control: None,
            }],
        }],
        max_tokens: 128,
        system: Some(SystemPrompt::Text(router_system)),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: Some("off".to_string()),
        stream: Some(false),
        temperature: Some(0.0),
        top_p: None,
        response_format: None,
    };

    let response =
        tokio::time::timeout(Duration::from_secs(4), client.create_message(request)).await??;
    Ok(parse_inventory_auto_route_recommendation(
        &message_response_text(&response),
        inventory,
    ))
}

fn inventory_auto_router_system_prompt(inventory: &ModelInventory) -> String {
    format!(
        include_str!("prompts/inventory_router_classifier.md"),
        inventory = inventory.router_context_json()
    )
}

fn parse_inventory_auto_route_recommendation(
    raw: &str,
    inventory: &ModelInventory,
) -> Option<InventoryAutoRouteRecommendation> {
    let json = extract_first_json_object(raw)?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let provider = value
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .and_then(ApiProvider::parse)?;
    let model = value.get("model").and_then(serde_json::Value::as_str)?;
    let candidate = inventory.candidate(provider, model)?;
    let reasoning_effort = value
        .get("thinking")
        .or_else(|| value.get("reasoning_effort"))
        .or_else(|| value.get("effort"))
        .and_then(serde_json::Value::as_str)
        .and_then(parse_auto_route_reasoning_effort)
        .map(|effort| normalize_auto_route_effort_for_provider(provider, effort));

    Some(InventoryAutoRouteRecommendation {
        provider,
        model: candidate.model.clone(),
        reasoning_effort,
    })
}

#[allow(dead_code)] // retained for the legacy active-provider flash resolver.
async fn auto_route_flash_recommendation(
    config: &Config,
    candidates: &RouterCandidates,
    latest_request: &str,
    recent_context: &str,
    selected_model_mode: &str,
    selected_thinking_mode: &str,
) -> Result<Option<AutoRouteRecommendation>> {
    if cfg!(test) {
        return Ok(None);
    }
    let Some(cheap_model) = candidates.cheap.clone() else {
        // Callers skip the router when there is no cheap tier; this is a
        // defensive second gate so a future caller cannot 400 the provider.
        return Ok(None);
    };

    let client = DeepSeekClient::new(config)?;
    let router_system = auto_router_system_prompt(candidates, config.auto_cost_saving());
    let request = MessageRequest {
        model: cheap_model,
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: auto_route_prompt(
                    latest_request,
                    recent_context,
                    selected_model_mode,
                    selected_thinking_mode,
                ),
                cache_control: None,
            }],
        }],
        max_tokens: 96,
        system: Some(SystemPrompt::Text(router_system)),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: Some("off".to_string()),
        stream: Some(false),
        temperature: Some(0.0),
        top_p: None,
        response_format: None,
    };

    let response =
        tokio::time::timeout(Duration::from_secs(4), client.create_message(request)).await??;
    Ok(parse_auto_route_recommendation_for_candidates(
        &message_response_text(&response),
        candidates,
    ))
}

fn auto_route_prompt(
    latest_request: &str,
    recent_context: &str,
    selected_model_mode: &str,
    selected_thinking_mode: &str,
) -> String {
    format!(
        "Session mode: agent\nSelected model mode: {}\nSelected thinking mode: {}\n\nRecent context:\n{}\n\nLatest user request:\n{}\n\nReturn JSON only.",
        selected_model_mode,
        selected_thinking_mode,
        if recent_context.trim().is_empty() {
            "No prior context."
        } else {
            recent_context
        },
        truncate_for_auto_router(latest_request, 4_000)
    )
}

fn message_response_text(response: &MessageResponse) -> String {
    let mut out = String::new();
    for block in &response.content {
        match block {
            ContentBlock::Text { text, .. } | ContentBlock::ToolResult { content: text, .. } => {
                append_router_text(&mut out, text);
            }
            ContentBlock::Thinking { thinking, .. } => {
                append_router_text(&mut out, thinking);
            }
            ContentBlock::ToolUse { name, .. } => {
                append_router_text(&mut out, &format!("[tool call: {name}]"));
            }
            _ => {}
        }
    }
    out
}

fn append_router_text(out: &mut String, text: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(text);
}

fn truncate_for_auto_router(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {}
