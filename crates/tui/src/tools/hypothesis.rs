//! Hypothesis/Evidence/Verdict tracking tool: `hypothesis`.
//!
//! Realizes issue #803 (Hypothesis / Evidence / Verdict as first-class
//! citizens) and axis B (推理严谨性 / consistency) of the vuln-hunting
//! long-horizon harness. The tool gives the model a durable, structured place
//! to register a *claim* plus its supporting *evidence* before drawing a
//! *conclusion*, and — crucially — enforces "先举证后结论" (evidence before
//! verdict): a hypothesis carrying zero evidence cannot be resolved.
//!
//! One tool, four `action`s (create / add_evidence / resolve / list). State is
//! persisted to `<workspace>/.mimofan/hypotheses.json` so a harness can reload
//! it across turns and measure whether the agent registered claims + evidence
//! before concluding.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const HYPOTHESIS_TOOL_NAME: &str = "hypothesis";

/// Filename (under `<workspace>/.mimofan/`) for the durable hypothesis store.
const HYPOTHESIS_STORE_FILE: &str = "hypotheses.json";

/// A single piece of supporting (or refuting) evidence attached to a
/// [`Hypothesis`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Evidence {
    /// Free-text observation, e.g. "sink reachable from untrusted input at
    /// `parse_request` (call_graph, no auth gate on the path)."
    pub text: String,
    /// Optional provenance — a tool name, file:line, or method that produced
    /// the observation. Missing means "unspecified".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// RFC-3339 / ISO-8601 timestamp of when the evidence was recorded.
    pub added_at: String,
}

/// An independent verifier's verdict on a hypothesis (loop v2 / T1 / G1.3).
///
/// Multi-verdict synthesis replaces the single model-authored verdict: each
/// independent verification stage (taint trace, gadget-chain reachability, PoC
/// realization, manual review) records its own verdict, and a deterministic
/// rule synthesizes them (see [`synthesize_verdict`]) — this is the FP-triage
/// axis the vuln-hunt harness consumes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerdictRecord {
    /// The verifier that produced the verdict, e.g. "taint_trace",
    /// "gadget_chain", "run_poc", "reviewer".
    pub verifier: String,
    /// This verifier's verdict: `confirmed` | `refuted` | `inconclusive`.
    pub verdict: String,
    /// Optional free-text note from the verifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// ISO-8601 timestamp of when the verdict was recorded.
    pub added_at: String,
}

/// A falsifiable claim the agent wants to track during a vuln-hunt, together
/// with the evidence gathered for/against it and the eventual verdict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hypothesis {
    /// Stable identifier (monotonic counter string, workspace-scoped).
    pub id: String,
    /// The claim itself.
    pub statement: String,
    /// Optional classification (e.g. "vulnerability", "taint-flow", "design").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothesis_type: Option<String>,
    /// Lifecycle state: `open` | `confirmed` | `refuted` | `inconclusive`.
    pub status: String,
    /// Accumulated evidence. Empty list is what triggers the consistency gate.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// Independent verifier verdicts feeding [`synthesize_verdict`] (G1.3).
    #[serde(default)]
    pub verdicts: Vec<VerdictRecord>,
    /// Deterministic synthesis of `verdicts`, computed when verdicts are
    /// recorded or at resolve time. `None` when no verdicts exist yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synthesized: Option<String>,
    /// ISO-8601 timestamp of creation.
    pub created_at: String,
    /// The verdict summary recorded at resolution time. `None` while open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_with: Option<String>,
}

/// Synthesize independent verifier verdicts into one conclusion.
///
/// Deterministic and deliberately **FP-conservative** (the rule may only
/// *suppress* false positives relative to the old single-`confirmed` verdict —
/// it never auto-confirms on one static opinion):
///
/// - A confirmed PoC/`run_poc` realization is ground truth: `confirmed`.
/// - Refutations only win (`refuted`) when there is **zero** confirmation.
/// - `confirmed` requires **at least two independent confirming verifiers**
///   outnumbering refutations.
/// - Everything else (single static confirm, tie, mixed) is `inconclusive`.
///
/// Returns `None` when there are no verdict records.
pub fn synthesize_verdict(records: &[VerdictRecord]) -> Option<&'static str> {
    if records.is_empty() {
        return None;
    }
    // PoC realization is the strongest signal available.
    for r in records {
        if r.verifier.to_ascii_lowercase().contains("poc")
            && matches!(r.verdict.as_str(), "confirmed")
        {
            return Some("confirmed");
        }
    }
    // One effective verdict per verifier (last record wins), so repeated
    // verdicts from the same tool cannot stack.
    let mut by_verifier: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for r in records {
        by_verifier.insert(r.verifier.as_str(), r.verdict.as_str());
    }
    let mut confirms = 0usize;
    let mut refutes = 0usize;
    for v in by_verifier.values() {
        match *v {
            "confirmed" => confirms += 1,
            "refuted" => refutes += 1,
            _ => {}
        }
    }
    if confirms == 0 && refutes > 0 {
        return Some("refuted");
    }
    if confirms >= 2 && confirms > refutes {
        return Some("confirmed");
    }
    Some("inconclusive")
}

/// Durable, crash-free store of hypotheses. Serialized as a JSON array to
/// `<workspace>/.mimofan/hypotheses.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HypothesisStore {
    /// Monotonic id counter (next id to hand out = `next_id`).
    #[serde(default)]
    next_id: u64,
    /// All hypotheses, in creation order.
    #[serde(default)]
    hypotheses: Vec<Hypothesis>,
}

/// Errors that can occur while loading/saving the hypothesis store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The store directory could not be created or resolved.
    #[error("failed to prepare hypothesis store directory: {0}")]
    Dir(std::io::Error),
    /// The on-disk JSON could not be read or parsed.
    #[error("failed to read hypothesis store: {0}")]
    Read(std::io::Error),
    /// The on-disk JSON was present but malformed.
    #[error("hypothesis store is corrupt (not valid JSON): {0}")]
    Parse(serde_json::Error),
    /// The store could not be written back to disk.
    #[error("failed to write hypothesis store: {0}")]
    Write(std::io::Error),
}

impl From<StoreError> for ToolError {
    fn from(other: StoreError) -> Self {
        ToolError::execution_failed(other.to_string())
    }
}

impl HypothesisStore {
    /// Resolve the store path: `<workspace>/.mimofan/hypotheses.json`.
    ///
    /// Falls back to the current working directory when `context` is `None`
    /// (e.g. in some test contexts), per the issue brief.
    fn store_path(workspace: &Path) -> PathBuf {
        workspace.join(".mimofan").join(HYPOTHESIS_STORE_FILE)
    }

    /// Load the store from disk. Missing file → empty store (no error).
    /// Corrupt file → surfaced as [`StoreError::Parse`].
    fn load(workspace: &Path) -> Result<Self, StoreError> {
        let path = Self::store_path(workspace);
        match std::fs::read_to_string(&path) {
            Ok(contents) if !contents.trim().is_empty() => {
                serde_json::from_str(&contents).map_err(StoreError::Parse)
            }
            // File absent or empty → fresh store.
            _ => Ok(Self::default()),
        }
    }

    /// Persist the store, creating `<workspace>/.mimofan/` if needed.
    ///
    /// Writes to a `.tmp` file in the same directory then atomically renames it
    /// onto the final path. `rename` is atomic on a single filesystem, so a
    /// concurrent reader never observes a partial file and a crash mid-write
    /// leaves the previous store intact.
    fn save(&self, workspace: &Path) -> Result<(), StoreError> {
        let dir = workspace.join(".mimofan");
        std::fs::create_dir_all(&dir).map_err(StoreError::Dir)?;
        let path = dir.join(HYPOTHESIS_STORE_FILE);
        let tmp = dir.join(format!("{HYPOTHESIS_STORE_FILE}.tmp"));
        let bytes = serde_json::to_vec_pretty(self).map_err(StoreError::Parse)?;
        std::fs::write(&tmp, bytes).map_err(StoreError::Write)?;
        std::fs::rename(&tmp, &path).map_err(StoreError::Write)
    }

    /// Insert a new hypothesis and return its id.
    fn create(&mut self, statement: String, hypothesis_type: Option<String>) -> String {
        let id = self.next_id.to_string();
        self.next_id = self.next_id.saturating_add(1);
        let now = now_iso();
        self.hypotheses.push(Hypothesis {
            id: id.clone(),
            statement,
            hypothesis_type,
            status: "open".to_string(),
            evidence: Vec::new(),
            verdicts: Vec::new(),
            synthesized: None,
            created_at: now,
            resolved_with: None,
        });
        id
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut Hypothesis> {
        self.hypotheses.iter_mut().find(|h| h.id == id)
    }
}

/// Current wall-clock time as an ISO-8601 timestamp. Best-effort and
/// monotonic-ish: if the clock is somehow unavailable we fall back to the
/// Unix epoch rather than failing the whole operation.
fn now_iso() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| {
            // Seconds since epoch is enough granularity for a harness report.
            format!("{}", d.as_secs())
        })
        .unwrap_or_else(|_| "0".to_string())
}

/// One independent verifier verdict supplied to `resolve` (G1.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VerifierInput {
    verifier: String,
    verdict: String,
    #[serde(default)]
    note: Option<String>,
}

/// Discriminated `action` field shared by the sub-operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum HypothesisInput {
    Create {
        statement: String,
        #[serde(default)]
        hypothesis_type: Option<String>,
    },
    AddEvidence {
        id: String,
        evidence: String,
        #[serde(default)]
        source: Option<String>,
    },
    /// Record one independent verifier's verdict (G1.3 multi-verdict
    /// synthesis). The synthesized conclusion is recomputed on every record.
    AddVerdict {
        id: String,
        verifier: String,
        verdict: String,
        #[serde(default)]
        note: Option<String>,
    },
    Resolve {
        id: String,
        /// Explicit model-authored verdict. Optional when `verifiers` is
        /// supplied — then the deterministic synthesis is used.
        #[serde(default)]
        verdict: Option<String>,
        /// Independent verifier verdicts to record before resolving.
        #[serde(default)]
        verifiers: Option<Vec<VerifierInput>>,
        #[serde(default)]
        summary: Option<String>,
    },
    List {
        #[serde(default)]
        status_filter: Option<String>,
    },
}

/// A compact summary row returned by `list`, so the harness can measure
/// evidence coverage without re-parsing full hypotheses.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HypothesisSummary {
    id: String,
    statement: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hypothesis_type: Option<String>,
    status: String,
    evidence_count: usize,
    /// Number of independent verifier verdicts recorded (G1.3).
    verdict_count: usize,
    /// Deterministic synthesis of the recorded verifier verdicts.
    #[serde(skip_serializing_if = "Option::is_none")]
    synthesized: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_with: Option<String>,
}

/// The `hypothesis` tool. Stateless across calls; all state lives in the JSON
/// store keyed by the `ToolContext` workspace root.
pub struct HypothesisTool;

#[async_trait]
impl ToolSpec for HypothesisTool {
    fn name(&self) -> &'static str {
        HYPOTHESIS_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Track falsifiable claims as first-class Hypothesis/Evidence/Verdict records for a \
         vuln-hunt. Actions: create (register a claim), add_evidence (attach supporting/refuting \
         observation), add_verdict (record one independent verifier's confirmed|refuted|\
         inconclusive verdict, e.g. verifier='taint_trace'|'gadget_chain'|'run_poc'; synthesis \
         rule: PoC realization or >=2 independent confirming verifiers => confirmed; refutations \
         with zero confirmation => refuted; else inconclusive), resolve (set confirmed|refuted|\
         inconclusive explicitly or via 'verifiers' which are synthesized; refused while evidence \
         is empty, enforcing 'evidence before verdict'), list (return claims with evidence and \
         verdict counts plus the synthesized verdict). \
         State persists to <workspace>/.mimofan/hypotheses.json."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "add_evidence", "add_verdict", "resolve", "list"],
                    "description": "Sub-operation to perform."
                },
                "statement": {
                    "type": "string",
                    "description": "create: the falsifiable claim to register."
                },
                "hypothesis_type": {
                    "type": "string",
                    "description": "create: optional classification, e.g. vulnerability | taint-flow | design."
                },
                "id": {
                    "type": "string",
                    "description": "add_evidence / resolve: id of the hypothesis to modify."
                },
                "evidence": {
                    "type": "string",
                    "description": "add_evidence: the supporting/refuting observation text."
                },
                "source": {
                    "type": "string",
                    "description": "add_evidence: optional provenance (tool name, file:line, method)."
                },
                "verifier": {
                    "type": "string",
                    "description":
                        "add_verdict: the independent verifier, e.g. 'taint_trace', \
        'gadget_chain', 'run_poc', 'reviewer'."
                },
                "note": {
                    "type": "string",
                    "description": "add_verdict: optional note from the verifier."
                },
                "verdict": {
                    "type": "string",
                    "enum": ["confirmed", "refuted", "inconclusive"],
                    "description":
                        "add_verdict: this verifier's verdict. resolve: the final conclusion \
        to record (when omitted and 'verifiers' are given, the deterministic synthesis is used)."
                },
                "verifiers": {
                    "type": "array",
                    "description":
                        "resolve: independent verifier verdicts to record and synthesize \
        (items: {verifier, verdict: confirmed|refuted|inconclusive, note?}).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "verifier": {"type": "string"},
                            "verdict": {"type": "string", "enum": ["confirmed", "refuted", "inconclusive"]},
                            "note": {"type": "string"}
                        },
                        "required": ["verifier", "verdict"]
                    }
                },
                "summary": {
                    "type": "string",
                    "description": "resolve: optional free-text rationale for the verdict."
                },
                "status_filter": {
                    "type": "string",
                    "description": "list: optional status to filter by (open|confirmed|refuted|inconclusive)."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Writes to the workspace-local `.mimofan/` state dir, never executes
        // code or makes network calls.
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        // WritesFiles → the trait default would suggest Suggest, but this tool
        // only mutates the agent's own reasoning ledger under `.mimofan/`, so
        // auto-approve keeps the harness non-interactive.
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let parsed: HypothesisInput = serde_json::from_value(input)
            .map_err(|e| ToolError::invalid_input(format!("invalid hypothesis input: {e}")))?;

        let workspace = &context.workspace;
        let mut store = HypothesisStore::load(workspace)?;

        match parsed {
            HypothesisInput::Create {
                statement,
                hypothesis_type,
            } => {
                if statement.trim().is_empty() {
                    return Err(ToolError::invalid_input(
                        "hypothesis create requires a non-empty 'statement'",
                    ));
                }
                let id = store.create(statement, hypothesis_type);
                store.save(workspace)?;
                tracing::debug!(%id, "hypothesis created");
                ToolResult::json(&json!({
                    "id": id,
                    "status": "open",
                }))
                .map_err(|e| ToolError::execution_failed(e.to_string()))
            }

            HypothesisInput::AddEvidence {
                id,
                evidence,
                source,
            } => {
                if id.trim().is_empty() {
                    return Err(ToolError::invalid_input(
                        "hypothesis add_evidence requires 'id'",
                    ));
                }
                if evidence.trim().is_empty() {
                    return Err(ToolError::invalid_input(
                        "hypothesis add_evidence requires non-empty 'evidence'",
                    ));
                }
                let hy = store.find_mut(&id).ok_or_else(|| {
                    ToolError::invalid_input(format!("no hypothesis with id '{id}'"))
                })?;
                hy.evidence.push(Evidence {
                    text: evidence,
                    source,
                    added_at: now_iso(),
                });
                let evidence_count = hy.evidence.len();
                store.save(workspace)?;
                tracing::debug!(%id, count = evidence_count, "hypothesis evidence added");
                ToolResult::json(&json!({
                    "id": id,
                    "evidence_count": evidence_count,
                }))
                .map_err(|e| ToolError::execution_failed(e.to_string()))
            }

            HypothesisInput::AddVerdict {
                id,
                verifier,
                verdict,
                note,
            } => {
                if id.trim().is_empty() {
                    return Err(ToolError::invalid_input(
                        "hypothesis add_verdict requires 'id'",
                    ));
                }
                if verifier.trim().is_empty() {
                    return Err(ToolError::invalid_input(
                        "hypothesis add_verdict requires a non-empty 'verifier'",
                    ));
                }
                if !matches!(verdict.as_str(), "confirmed" | "refuted" | "inconclusive") {
                    return Err(ToolError::invalid_input(format!(
                        "hypothesis verdict must be one of confirmed|refuted|inconclusive, got '{verdict}'"
                    )));
                }
                let hy = store.find_mut(&id).ok_or_else(|| {
                    ToolError::invalid_input(format!("no hypothesis with id '{id}'"))
                })?;
                hy.verdicts.push(VerdictRecord {
                    verifier: verifier.clone(),
                    verdict: verdict.clone(),
                    note,
                    added_at: now_iso(),
                });
                hy.synthesized = synthesize_verdict(&hy.verdicts).map(str::to_string);
                let verdict_count = hy.verdicts.len();
                let synthesized = hy.synthesized.clone();
                store.save(workspace)?;
                tracing::debug!(%id, %verifier, %verdict, ?synthesized, "verdict recorded");
                ToolResult::json(&json!({
                    "id": id,
                    "verdict_count": verdict_count,
                    "synthesized": synthesized,
                }))
                .map_err(|e| ToolError::execution_failed(e.to_string()))
            }

            HypothesisInput::Resolve {
                id,
                verdict,
                verifiers,
                summary,
            } => {
                if id.trim().is_empty() {
                    return Err(ToolError::invalid_input("hypothesis resolve requires 'id'"));
                }
                // Record any independent verifier verdicts first, then derive
                // the synthesized conclusion.
                if let Some(verifiers) = &verifiers {
                    for v in verifiers {
                        if v.verifier.trim().is_empty() {
                            return Err(ToolError::invalid_input(
                                "hypothesis resolve verifiers require a non-empty 'verifier'",
                            ));
                        }
                        if !matches!(v.verdict.as_str(), "confirmed" | "refuted" | "inconclusive") {
                            return Err(ToolError::invalid_input(format!(
                                "verifier '{}' verdict must be confirmed|refuted|inconclusive, got '{}'",
                                v.verifier, v.verdict
                            )));
                        }
                    }
                }
                let hy = store.find_mut(&id).ok_or_else(|| {
                    ToolError::invalid_input(format!("no hypothesis with id '{id}'"))
                })?;
                if let Some(verifiers) = verifiers {
                    for v in verifiers {
                        hy.verdicts.push(VerdictRecord {
                            verifier: v.verifier,
                            verdict: v.verdict,
                            note: v.note,
                            added_at: now_iso(),
                        });
                    }
                    hy.synthesized = synthesize_verdict(&hy.verdicts).map(str::to_string);
                }

                // Final status: explicit model verdict wins; otherwise fall
                // back to the deterministic multi-verdict synthesis.
                let final_verdict = match (&verdict, hy.synthesized.as_deref()) {
                    (Some(v), _)
                        if matches!(v.as_str(), "confirmed" | "refuted" | "inconclusive") =>
                    {
                        v.clone()
                    }
                    (None, Some(s)) => s.to_string(),
                    _ => {
                        return Err(ToolError::invalid_input(
                            "hypothesis resolve requires either a 'verdict' or 'verifiers' \
                             (no verdict supplied and no synthesized conclusion available)",
                        ));
                    }
                };

                // Consistency gate: "先举证后结论". A verdict without a single
                // piece of evidence is exactly the reasoning failure axis B
                // measures — refuse it.
                if hy.evidence.is_empty() {
                    return Err(ToolError::invalid_input(format!(
                        "refusing to resolve hypothesis '{id}': it has zero evidence. \
                         Attach evidence via add_evidence before drawing a verdict \
                         (consistency gate: evidence-before-verdict)."
                    )));
                }

                hy.status = final_verdict.clone();
                hy.resolved_with = Some(summary.unwrap_or_else(|| final_verdict.clone()));
                let status = hy.status.clone();
                let resolved_with = hy.resolved_with.clone();
                let synthesized = hy.synthesized.clone();
                store.save(workspace)?;
                tracing::debug!(%id, %final_verdict, ?synthesized, "hypothesis resolved");
                ToolResult::json(&json!({
                    "id": id,
                    "status": status,
                    "resolved_with": resolved_with,
                    "synthesized": synthesized,
                }))
                .map_err(|e| ToolError::execution_failed(e.to_string()))
            }

            HypothesisInput::List { status_filter } => {
                let rows: Vec<HypothesisSummary> = store
                    .hypotheses
                    .iter()
                    .filter(|h| match &status_filter {
                        Some(f) => &h.status == f,
                        None => true,
                    })
                    .map(|h| HypothesisSummary {
                        id: h.id.clone(),
                        statement: h.statement.clone(),
                        hypothesis_type: h.hypothesis_type.clone(),
                        status: h.status.clone(),
                        evidence_count: h.evidence.len(),
                        verdict_count: h.verdicts.len(),
                        synthesized: h.synthesized.clone(),
                        resolved_with: h.resolved_with.clone(),
                    })
                    .collect();
                ToolResult::json(&json!({
                    "count": rows.len(),
                    "hypotheses": rows,
                }))
                .map_err(|e| ToolError::execution_failed(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::spec::ToolContext;

    fn tmp_ctx() -> (tempfile::TempDir, ToolContext) {
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());
        (dir, ctx)
    }

    fn run(
        tool: &HypothesisTool,
        ctx: &ToolContext,
        input: Value,
    ) -> Result<ToolResult, ToolError> {
        // The tool is async; in unit tests we drive it on a throwaway
        // single-threaded runtime. We must NOT use `Handle::current` because
        // tests run on the default test executor, not a tokio runtime.
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build test runtime");
        rt.block_on(tool.execute(input, ctx))
    }

    #[test]
    fn round_trip_create_evidence_resolve() {
        let (_dir, ctx) = tmp_ctx();
        let tool = HypothesisTool;

        // create
        let created = run(
            &tool,
            &ctx,
            json!({"action": "create", "statement": "fn parse() trusts user input", "hypothesis_type": "vulnerability"}),
        )
        .expect("create succeeds");
        let parsed: Value = serde_json::from_str(&created.content).unwrap();
        let id: String = parsed.get("id").unwrap().as_str().unwrap().to_string();
        assert!(!id.is_empty());

        // add_evidence x2
        for (i, ev) in ["reachable from http handler", "no auth gate on path"]
            .iter()
            .enumerate()
        {
            let res = run(
                &tool,
                &ctx,
                json!({"action": "add_evidence", "id": id, "evidence": ev, "source": format!("probe-{i}")}),
            )
            .expect("add_evidence succeeds");
            let parsed: Value = serde_json::from_str(&res.content).unwrap();
            assert_eq!(parsed["evidence_count"], i as u64 + 1);
        }

        // list shows 2 evidence
        let listed = run(&tool, &ctx, json!({"action": "list"})).expect("list succeeds");
        let parsed: Value = serde_json::from_str(&listed.content).unwrap();
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["hypotheses"][0]["evidence_count"], 2);
        assert_eq!(parsed["hypotheses"][0]["status"], "open");

        // resolve confirmed
        let resolved = run(
            &tool,
            &ctx,
            json!({"action": "resolve", "id": id, "verdict": "confirmed", "summary": "sink reachable, unauthenticated"}),
        )
        .expect("resolve succeeds");
        let parsed: Value = serde_json::from_str(&resolved.content).unwrap();
        assert_eq!(parsed["status"], "confirmed");
        assert_eq!(parsed["resolved_with"], "sink reachable, unauthenticated");

        // status is now confirmed in list
        let listed = run(&tool, &ctx, json!({"action": "list"})).expect("list succeeds");
        let parsed: Value = serde_json::from_str(&listed.content).unwrap();
        assert_eq!(parsed["hypotheses"][0]["status"], "confirmed");
    }

    #[test]
    fn consistency_gate_refuses_resolve_without_evidence() {
        let (_dir, ctx) = tmp_ctx();
        let tool = HypothesisTool;

        let created = run(
            &tool,
            &ctx,
            json!({"action": "create", "statement": "claim with no evidence yet"}),
        )
        .expect("create succeeds");
        let parsed: Value = serde_json::from_str(&created.content).unwrap();
        let id: String = parsed.get("id").unwrap().as_str().unwrap().to_string();

        // Immediate resolve must fail — zero evidence.
        let err = run(
            &tool,
            &ctx,
            json!({"action": "resolve", "id": id, "verdict": "confirmed"}),
        )
        .expect_err("resolve without evidence must be refused");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("evidence"),
            "consistency gate error must mention 'evidence', got: {msg}"
        );
    }

    #[test]
    fn persistence_survives_reload() {
        let (dir, ctx) = tmp_ctx();
        let tool = HypothesisTool;

        let created = run(
            &tool,
            &ctx,
            json!({"action": "create", "statement": "persisted claim"}),
        )
        .expect("create succeeds");
        let parsed: Value = serde_json::from_str(&created.content).unwrap();
        let id: String = parsed.get("id").unwrap().as_str().unwrap().to_string();

        run(
            &tool,
            &ctx,
            json!({"action": "add_evidence", "id": id, "evidence": "supporting fact A"}),
        )
        .expect("add_evidence succeeds");
        run(
            &tool,
            &ctx,
            json!({"action": "resolve", "id": id, "verdict": "refuted", "summary": "false alarm"}),
        )
        .expect("resolve succeeds");

        // Reload from disk via a *fresh* store load (simulating a new turn).
        let reloaded = HypothesisStore::load(dir.path()).expect("reload store");
        let hy = reloaded
            .hypotheses
            .iter()
            .find(|h| h.id == id)
            .expect("hypothesis present after reload");
        assert_eq!(hy.status, "refuted");
        assert_eq!(hy.resolved_with.as_deref(), Some("false alarm"));
        assert_eq!(hy.evidence.len(), 1);
        assert_eq!(hy.evidence[0].text, "supporting fact A");
    }

    fn vr(verifier: &str, verdict: &str) -> VerdictRecord {
        VerdictRecord {
            verifier: verifier.to_string(),
            verdict: verdict.to_string(),
            note: None,
            added_at: "0".to_string(),
        }
    }

    #[test]
    fn synthesis_empty_is_none() {
        assert_eq!(synthesize_verdict(&[]), None);
    }

    #[test]
    fn synthesis_poc_realization_confirms_even_alone() {
        // A realized PoC is ground-truth confirmation regardless of other
        // (or absent) verdicts.
        let recs = vec![vr("run_poc", "confirmed")];
        assert_eq!(synthesize_verdict(&recs), Some("confirmed"));
    }

    #[test]
    fn synthesis_single_static_opinion_is_inconclusive_not_confirmed() {
        // FP-triage: one static verifier alone must NOT auto-confirm.
        let recs = vec![vr("taint_trace", "confirmed")];
        assert_eq!(synthesize_verdict(&recs), Some("inconclusive"));
    }

    #[test]
    fn synthesis_two_independent_confirms_outweigh_one_refute() {
        let recs = vec![
            vr("taint_trace", "confirmed"),
            vr("gadget_chain", "confirmed"),
            vr("reviewer", "refuted"),
        ];
        assert_eq!(synthesize_verdict(&recs), Some("confirmed"));
    }

    #[test]
    fn synthesis_refutations_only_without_confirm_refute() {
        let recs = vec![
            vr("taint_trace", "refuted"),
            vr("gadget_chain", "inconclusive"),
        ];
        assert_eq!(synthesize_verdict(&recs), Some("refuted"));
    }

    #[test]
    fn synthesis_tie_is_inconclusive() {
        let recs = vec![vr("taint_trace", "confirmed"), vr("reviewer", "refuted")];
        assert_eq!(synthesize_verdict(&recs), Some("inconclusive"));
    }

    #[test]
    fn synthesis_dedupes_same_verifier_repeated_verdicts() {
        // Same verifier confirming twice must not stack into 2 confirms.
        let recs = vec![
            vr("taint_trace", "confirmed"),
            vr("taint_trace", "confirmed"),
        ];
        assert_eq!(synthesize_verdict(&recs), Some("inconclusive"));
    }

    #[test]
    fn add_verdict_then_resolve_uses_synthesis() {
        let (_dir, ctx) = tmp_ctx();
        let tool = HypothesisTool;

        let created = run(
            &tool,
            &ctx,
            json!({"action": "create", "statement": "sink reachable from untrusted source"}),
        )
        .expect("create succeeds");
        let id: String = serde_json::from_str::<Value>(&created.content).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        run(
            &tool,
            &ctx,
            json!({"action": "add_evidence", "id": id, "evidence": "flow fact A"}),
        )
        .expect("add_evidence succeeds");

        // Two independent confirming verifiers -> synthesized "confirmed".
        run(
            &tool,
            &ctx,
            json!({"action": "add_verdict", "id": id, "verifier": "taint_trace", "verdict": "confirmed"}),
        )
        .expect("verdict 1 succeeds");
        let v2 = run(
            &tool,
            &ctx,
            json!({"action": "add_verdict", "id": id, "verifier": "gadget_chain", "verdict": "confirmed"}),
        )
        .expect("verdict 2 succeeds");
        let v2_parsed: Value = serde_json::from_str(&v2.content).unwrap();
        assert_eq!(v2_parsed["synthesized"], "confirmed");

        // resolve without an explicit verdict uses the synthesis.
        let resolved = run(&tool, &ctx, json!({"action": "resolve", "id": id}))
            .expect("resolve via synthesis succeeds");
        let r: Value = serde_json::from_str(&resolved.content).unwrap();
        assert_eq!(r["status"], "confirmed");
        assert_eq!(r["synthesized"], "confirmed");
    }

    #[test]
    fn resolve_with_verifiers_array_synthesizes() {
        let (dir, ctx) = tmp_ctx();
        let tool = HypothesisTool;

        let created = run(&tool, &ctx, json!({"action": "create", "statement": "h2"}))
            .expect("create succeeds");
        let id: String = serde_json::from_str::<Value>(&created.content).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        run(
            &tool,
            &ctx,
            json!({"action": "add_evidence", "id": id, "evidence": "only a refuting observation"}),
        )
        .expect("add_evidence succeeds");

        // One refuting verifier (no confirms) -> synthesized "refuted".
        let resolved = run(
            &tool,
            &ctx,
            json!({
                "action": "resolve",
                "id": id,
                "verifiers": [{"verifier": "reviewer", "verdict": "refuted"}]
            }),
        )
        .expect("resolve with verifiers succeeds");
        let r: Value = serde_json::from_str(&resolved.content).unwrap();
        assert_eq!(r["status"], "refuted");
        assert_eq!(r["synthesized"], "refuted");

        // Persisted store carries the verdict records for harness consumption.
        let reloaded = HypothesisStore::load(dir.path()).expect("reload");
        let hy = reloaded.hypotheses.iter().find(|h| h.id == id).unwrap();
        assert_eq!(hy.verdicts.len(), 1);
        assert_eq!(hy.synthesized.as_deref(), Some("refuted"));
    }

    #[test]
    fn list_exposes_verdict_count_and_synthesis() {
        let (_dir, ctx) = tmp_ctx();
        let tool = HypothesisTool;
        let created = run(&tool, &ctx, json!({"action": "create", "statement": "h3"}))
            .expect("create succeeds");
        let id: String = serde_json::from_str::<Value>(&created.content).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        run(
            &tool,
            &ctx,
            json!({"action": "add_verdict", "id": id, "verifier": "run_poc", "verdict": "confirmed"}),
        )
        .expect("poc verdict succeeds");

        let listed = run(&tool, &ctx, json!({"action": "list"})).expect("list succeeds");
        let l: Value = serde_json::from_str(&listed.content).unwrap();
        let row = &l["hypotheses"][0];
        assert_eq!(row["verdict_count"], 1);
        assert_eq!(row["synthesized"], "confirmed");
    }
}
