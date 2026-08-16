//! Prompt Audit tool (#846).
//!
//! Given the current system prompt (read from a path or passed in directly),
//! analyze it for redundancy / contradiction / bloat and return a structured
//! report. Pure deterministic text analysis — no LLM calls.
//!
//! The core analysis lives in [`PromptAudit::audit`]; the [`PromptAuditTool`]
//! `ToolSpec` wrapper exposes it to the agent. The tool is ReadOnly: it never
//! mutates files.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::tools::spec::{ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// One detected duplicate section in the prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateSpan {
    /// 1-based line numbers of the first occurrence.
    pub first_line: usize,
    /// 1-based line numbers of the second occurrence.
    pub second_line: usize,
    /// The duplicated text (trimmed).
    pub text: String,
}

/// A suggested cleanup action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub severity: SuggestionSeverity,
    pub message: String,
}

/// Severity tier for a suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionSeverity {
    /// Safe to drop, no behaviour change expected.
    Info,
    /// Likely redundant; review recommended.
    Warning,
    /// Contradictory directives; resolution required.
    Critical,
}

/// Contradiction detected between two directives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contradiction {
    pub line_a: usize,
    pub line_b: usize,
    pub text_a: String,
    pub text_b: String,
}

/// The structured audit report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditReport {
    /// Detected duplicate spans (lines repeated verbatim).
    pub duplicates: Vec<DuplicateSpan>,
    /// Detected contradictory directives.
    pub contradictions: Vec<Contradiction>,
    /// Actionable cleanup suggestions.
    pub suggestions: Vec<Suggestion>,
    /// Estimated tokens that could be removed by applying suggestions.
    pub estimated_token_savings: usize,
}

/// Errors from the audit pass.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuditError {
    /// The prompt input was empty.
    #[error("prompt is empty")]
    EmptyPrompt,
}

/// Heuristic thresholds (kept simple + deterministic).
const MIN_DUPLICATE_LINE_LEN: usize = 12;
/// Lines at/above this length count as "over-long" single instructions.
const OVERLONG_LINE_CHARS: usize = 400;
/// Contradiction keyword pairs (mutually exclusive directives).
const CONTRADICTION_PAIRS: &[(&[&str], &[&str])] = &[(
    &["never", "do not", "don't", "must not", "avoid"],
    &["always", "must", "require", "should"],
)];

/// Pure analysis surface — no tooling, no IO.
pub struct PromptAudit;

impl PromptAudit {
    /// Analyze `prompt` for redundancy, contradiction, and bloat.
    ///
    /// Deterministic: identical input always yields identical output. Token
    /// savings are estimated at ~4 chars/token (the project's client-side
    /// heuristic).
    pub fn audit(prompt: &str) -> Result<AuditReport, AuditError> {
        if prompt.trim().is_empty() {
            return Err(AuditError::EmptyPrompt);
        }

        let lines: Vec<&str> = prompt.lines().collect();
        let duplicates = Self::find_duplicates(&lines);
        let contradictions = Self::find_contradictions(&lines);
        let mut suggestions: Vec<Suggestion> = Vec::new();

        // Duplicate -> warning suggestion; savings = removed duplicate lines.
        let duplicate_line_chars: usize = duplicates.iter().map(|d| d.text.chars().count()).sum();
        if !duplicates.is_empty() {
            suggestions.push(Suggestion {
                severity: SuggestionSeverity::Warning,
                message: format!(
                    "{} duplicate line span(s) detected; removing repeats saves ~{} tokens.",
                    duplicates.len(),
                    Self::estimate_tokens(duplicate_line_chars)
                ),
            });
        }

        // Contradiction -> critical suggestion (no token savings, but must fix).
        if !contradictions.is_empty() {
            suggestions.push(Suggestion {
                severity: SuggestionSeverity::Critical,
                message: format!(
                    "{} contradictory directive pair(s) detected; resolve before shipping the prompt.",
                    contradictions.len()
                ),
            });
        }

        // Over-long instructions -> info suggestion; savings = 50% of the
        // bytes over the threshold, since they can be tightened.
        let mut bloat_chars = 0usize;
        let mut overlong = 0usize;
        for line in &lines {
            let len = line.chars().count();
            if len >= OVERLONG_LINE_CHARS {
                overlong += 1;
                bloat_chars += len.saturating_sub(OVERLONG_LINE_CHARS) / 2;
            }
        }
        if overlong > 0 {
            suggestions.push(Suggestion {
                severity: SuggestionSeverity::Info,
                message: format!(
                    "{overlong} over-long instruction line(s) (>= {OVERLONG_LINE_CHARS} chars); tightening saves ~{} tokens.",
                    Self::estimate_tokens(bloat_chars)
                ),
            });
        }

        let estimated_token_savings =
            Self::estimate_tokens(duplicate_line_chars) + Self::estimate_tokens(bloat_chars);

        Ok(AuditReport {
            duplicates,
            contradictions,
            suggestions,
            estimated_token_savings,
        })
    }

    fn estimate_tokens(chars: usize) -> usize {
        // ~4 chars/token, mirroring the engine's client-side reasoning-replay
        // estimate. Saturating so empty inputs yield 0.
        chars.div_ceil(4)
    }

    /// Find verbatim duplicate non-trivial lines.
    fn find_duplicates(lines: &[&str]) -> Vec<DuplicateSpan> {
        let mut spans = Vec::new();
        // Index by trimmed non-empty line text.
        for (i, &a) in lines.iter().enumerate() {
            let ta = a.trim();
            if ta.is_empty() || ta.chars().count() < MIN_DUPLICATE_LINE_LEN {
                continue;
            }
            // Only report the *first* duplicate pair for each distinct text to
            // keep the report deterministic and compact.
            if spans.iter().any(|s: &DuplicateSpan| s.text == ta) {
                continue;
            }
            for (j, &b) in lines.iter().enumerate().skip(i + 1) {
                let tb = b.trim();
                if tb == ta {
                    spans.push(DuplicateSpan {
                        first_line: i + 1,
                        second_line: j + 1,
                        text: ta.to_string(),
                    });
                    break;
                }
            }
        }
        spans
    }

    /// Find contradictory directive pairs across the prompt.
    ///
    /// Detects both orderings: a negation in an earlier line paired with an
    /// obligation in a later line, and the reverse. Each unordered pair is
    /// reported once.
    fn find_contradictions(lines: &[&str]) -> Vec<Contradiction> {
        let lowered: Vec<String> = lines.iter().map(|l| l.to_lowercase()).collect();
        let mut out = Vec::new();
        let mut seen_pairs: std::collections::HashSet<(usize, usize)> =
            std::collections::HashSet::new();
        for (i, la) in lowered.iter().enumerate() {
            let i_neg = CONTRADICTION_PAIRS
                .iter()
                .any(|(neg, _)| neg.iter().any(|k| la.contains(*k)));
            let i_pos = CONTRADICTION_PAIRS
                .iter()
                .any(|(_, pos)| pos.iter().any(|k| la.contains(*k)));
            if !i_neg && !i_pos {
                continue;
            }
            for (j, lb) in lowered.iter().enumerate().skip(i + 1) {
                let j_neg = CONTRADICTION_PAIRS
                    .iter()
                    .any(|(neg, _)| neg.iter().any(|k| lb.contains(*k)));
                let j_pos = CONTRADICTION_PAIRS
                    .iter()
                    .any(|(_, pos)| pos.iter().any(|k| lb.contains(*k)));
                // One line asserts an obligation, the other denies it.
                let contradicts = (i_pos && j_neg) || (i_neg && j_pos);
                if contradicts && !seen_pairs.contains(&(i, j)) {
                    seen_pairs.insert((i, j));
                    out.push(Contradiction {
                        line_a: i + 1,
                        line_b: j + 1,
                        text_a: lines[i].trim().to_string(),
                        text_b: lines[j].trim().to_string(),
                    });
                }
            }
        }
        out
    }
}

/// Tool wrapper exposing the audit to the agent. ReadOnly.
pub struct PromptAuditTool;

#[async_trait]
impl ToolSpec for PromptAuditTool {
    fn name(&self) -> &str {
        "prompt_audit"
    }

    fn description(&self) -> &str {
        "Audit the current system prompt for redundant, contradictory, or bloated instructions. \
         Returns a structured report with duplicate spans, contradictions, cleanup suggestions, \
         and estimated token savings. Pure text analysis — does not call the model."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The system prompt text to audit (use this when the prompt is passed inline)."
                },
                "path": {
                    "type": "string",
                    "description": "Path to a file containing the prompt to audit (alternative to `prompt`)."
                }
            },
            "anyOf": [
                { "required": ["prompt"] },
                { "required": ["path"] }
            ]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        use mimofan_tools::{optional_str, required_str};

        // Prefer inline prompt; fall back to reading a file from workspace.
        let prompt = match required_str(&input, "prompt") {
            Ok(p) => p.to_string(),
            Err(_) => {
                let path = optional_str(&input, "path")
                    .ok_or_else(|| ToolError::missing_field("prompt or path"))?;
                let resolved = context
                    .resolve_path(path)
                    .map_err(|e| ToolError::invalid_input(format!("cannot resolve {path}: {e}")))?;
                std::fs::read_to_string(&resolved).map_err(|e| {
                    ToolError::execution_failed(format!(
                        "failed to read {}: {e}",
                        resolved.display()
                    ))
                })?
            }
        };

        let report = PromptAudit::audit(&prompt).map_err(|e| match e {
            AuditError::EmptyPrompt => ToolError::invalid_input("prompt is empty"),
        })?;

        let json = serde_json::to_value(&report)
            .map_err(|e| ToolError::execution_failed(format!("failed to serialize report: {e}")))?;

        Ok(ToolResult::success(json.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_duplicate_line() {
        let prompt = "\
You are a helpful coding assistant.
Always respond in the user's language.
Always respond in the user's language.
Keep answers concise.";
        let report = PromptAudit::audit(prompt).expect("audit");
        assert_eq!(report.duplicates.len(), 1, "expected exactly one duplicate");
        assert_eq!(report.duplicates[0].first_line, 2);
        assert_eq!(report.duplicates[0].second_line, 3);
        assert_eq!(
            report.duplicates[0].text,
            "Always respond in the user's language."
        );
        // Duplicate line ~41 chars -> ~10 tokens saved.
        assert!(report.estimated_token_savings >= 10);
        assert!(
            report
                .suggestions
                .iter()
                .any(|s| s.severity == SuggestionSeverity::Warning)
        );
    }

    #[test]
    fn no_false_positive_on_distinct_lines() {
        // These lines share no duplicate text and assert no opposing
        // obligations (no negation/obligation keyword pair), so the report
        // should be clean.
        let prompt = "\
Write tests for new code.
Keep the changelog updated.
Summarize the diff before committing.";
        let report = PromptAudit::audit(prompt).expect("audit");
        assert!(report.duplicates.is_empty(), "no duplicates expected");
        assert!(
            report.contradictions.is_empty(),
            "no contradictions expected"
        );
    }

    #[test]
    fn detects_contradiction() {
        // Obligation in the first line, negation in the second.
        let prompt = "\
You must always verify before committing.
Do not run the test suite automatically.";
        let report = PromptAudit::audit(prompt).expect("audit");
        assert!(
            !report.contradictions.is_empty(),
            "expected a contradiction"
        );
        assert!(
            report
                .suggestions
                .iter()
                .any(|s| s.severity == SuggestionSeverity::Critical)
        );
    }

    #[test]
    fn detects_contradiction_reverse_order() {
        // Negation first, obligation second — must also be caught.
        let prompt = "\
Do not skip the type checker.
You should always run the full build.";
        let report = PromptAudit::audit(prompt).expect("audit");
        assert!(
            !report.contradictions.is_empty(),
            "expected reverse-order contradiction"
        );
    }

    #[test]
    fn empty_prompt_errors() {
        assert_eq!(PromptAudit::audit("   "), Err(AuditError::EmptyPrompt));
    }

    #[test]
    fn tool_executes_on_inline_prompt() {
        let tokio_rt = tokio::runtime::Runtime::new().unwrap();
        tokio_rt.block_on(async {
            let ctx = ToolContext::new(std::env::temp_dir());
            let input = json!({ "prompt": "Repeat this line.\nRepeat this line.\n" });
            let res = PromptAuditTool.execute(input, &ctx).await.unwrap();
            assert!(res.success, "tool should succeed");
            assert!(
                res.content.contains("duplicates"),
                "report should mention duplicates: {}",
                res.content
            );
        });
    }
}
