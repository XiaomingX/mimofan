//! In-turn loop / repetition / stall detection.
//!
//! A model that gets stuck can burn hundreds of tool calls making the same
//! call over and over, or oscillating between two states, without ever making
//! forward progress. The only backstop in the turn loop is
//! [`TurnContext::at_max_steps`](crate::core::turn::TurnContext::at_max_steps),
//! which defaults to 1000 steps — far too high to be a useful brake.
//!
//! This module is that brake. It observes every executed tool call and looks
//! for three distinct pathologies:
//!
//! 1. [`LoopPattern::RepeatedCall`] — the same `(tool_name, args)` fingerprint
//!    N times in a row.
//! 2. [`LoopPattern::Alternating`] — an A→B→A→B oscillation between two
//!    distinct fingerprints.
//! 3. [`LoopPattern::NoProgress`] — calls keep succeeding but nothing about
//!    the world changes (same tool, same observable output digest).
//!
//! ## Design notes
//!
//! **Retry is not a loop.** The difference between a legitimate retry and a
//! death spiral is whether *state changed*. A model that reads a file, edits
//! it, and reads it again is making progress even though `read_file` repeats —
//! so the observation carries a `progress` signal (see
//! [`ToolObservation::progress`]) and any observation that reports progress
//! resets the relevant counters. Failed calls that produce *different* errors
//! are also treated as progress-ish: the error text feeds the outcome digest,
//! so a retry converging on a new failure mode won't trip `NoProgress`.
//!
//! **Cold start exemption.** The first [`LoopGuardConfig::warmup_calls`] tool
//! calls of a turn are observed but never trip a detector. Early in a turn a
//! model legitimately probes the same paths (e.g. `list_dir` then `read_file`
//! on a handful of candidates), and tripping there would be a false positive
//! on completely healthy behaviour.
//!
//! **Bounded, non-fatal intervention.** Detection never kills the turn.
//! [`LoopGuard::observe`] returns a [`LoopBreak`] carrying a nudge string that
//! the caller injects as a message, giving the model one chance to self-correct.
//! Hard-killing a turn would lose the user's in-flight progress. Each pattern
//! fires at most [`LoopGuardConfig::max_nudges_per_pattern`] times per turn so
//! a stubborn model can't be fed an unbounded stream of identical nudges.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::compaction::objective::Objective;

/// Number of leading tool calls in a turn that are recorded but never trip a
/// detector.
///
/// Three is deliberately small. It covers the common "orient myself" opening
/// of a turn (list, read, grep) without giving a genuine loop room to spin:
/// with the repeat threshold at 3, the earliest possible trip is call 6, which
/// is still cheap compared to the 1000-step backstop.
pub const DEFAULT_WARMUP_CALLS: usize = 3;

/// Consecutive identical `(tool, args)` fingerprints required to declare a
/// repeat loop.
///
/// Three, not two. Two identical calls in a row is a completely normal retry
/// pattern — a transient failure, a file that was mid-write, a race on a
/// freshly created path. By three the model has demonstrated it is not
/// adapting, and the false-positive cost (one advisory message) is far below
/// the cost of letting the spiral run.
pub const DEFAULT_REPEAT_THRESHOLD: usize = 3;

/// Number of full A→B cycles required to declare an alternating loop.
///
/// Two full cycles means the sequence A,B,A,B — four calls. One cycle (A,B,A)
/// is common and legitimate: edit a file, read it back, edit again. Requiring
/// the second complete cycle distinguishes an edit/verify rhythm that is
/// converging from a genuine two-state oscillation that is not.
pub const DEFAULT_ALTERNATION_CYCLES: usize = 2;

/// Consecutive no-progress calls required to declare a stall.
///
/// Four is higher than the repeat threshold on purpose. `NoProgress` is the
/// fuzziest of the three signals — it fires on *different* calls that merely
/// fail to change anything, so it carries the highest false-positive risk. The
/// extra call of slack buys precision where the evidence is weakest.
pub const DEFAULT_NO_PROGRESS_THRESHOLD: usize = 4;

/// Maximum times a single pattern may fire within one turn.
///
/// Two: one nudge to prompt self-correction, a second, firmer one if the model
/// ignores it. Beyond that the nudges are themselves just noise burning
/// context, and `max_steps` remains as the terminal backstop.
pub const DEFAULT_MAX_NUDGES_PER_PATTERN: usize = 2;

/// Minimum number of times a single line/text fragment must repeat *within
/// one tool output* to be flagged as a streaming self-repetition.
///
/// Streaming self-repetition is a distinct pathology from the cross-call
/// detectors: the model emits a single response that is mostly the same
/// sentence copied over and over (a degenerate generation, not a retry loop).
/// Three identical lines inside one output is a clear signal.
pub const DEFAULT_INTRA_REPEAT_LINES: usize = 3;

/// Minimum token-overlap (Jaccard) between two *distinct* consecutive outputs
/// to be flagged as a semantic echo.
///
/// `0.8` means the two outputs share 80% of their tokens: the model is
/// restating essentially the same content rather than advancing. Set below
/// `1.0` so trivial wording changes don't defeat the check.
pub const DEFAULT_SEMANTIC_ECHO_SIMILARITY: f64 = 0.8;

/// How many recent fingerprints to retain for alternation analysis.
const HISTORY_CAPACITY: usize = 16;

/// Tunable thresholds for [`LoopGuard`].
#[derive(Debug, Clone, PartialEq)]
pub struct LoopGuardConfig {
    /// Leading tool calls exempt from detection. See [`DEFAULT_WARMUP_CALLS`].
    pub warmup_calls: usize,
    /// Consecutive identical calls that trip [`LoopPattern::RepeatedCall`].
    pub repeat_threshold: usize,
    /// Full A→B cycles that trip [`LoopPattern::Alternating`].
    pub alternation_cycles: usize,
    /// Consecutive stalled calls that trip [`LoopPattern::NoProgress`].
    pub no_progress_threshold: usize,
    /// Per-pattern nudge cap for a single turn.
    pub max_nudges_per_pattern: usize,
    /// Identical-line count within a single output that trips
    /// [`LoopPattern::StreamingRepetition`].
    pub intra_repeat_lines: usize,
    /// Token-overlap (Jaccard) between distinct consecutive outputs that trips
    /// [`LoopPattern::SemanticEcho`].
    pub semantic_echo_similarity: f64,
    /// Master switch. When `false`, [`LoopGuard::observe`] always returns
    /// `None` — this also disables the periodic memory/skill nudge, so the
    /// guard is fully silent unless explicitly enabled.
    pub enabled: bool,
    /// Cadence (in observed tool calls) at which the guard emits the periodic
    /// "distill this into a memory or a skill" nudge. Only active when
    /// [`LoopGuardConfig::memory_skill_nudge`] is also enabled. `0` disables
    /// the periodic nudge regardless of the master switch.
    pub nudge_every_n: u64,
    /// Separate opt-in for the periodic memory/skill nudge. Defaults to
    /// `false` so users are not interrupted; both `enabled` and this flag must
    /// be set for the reminder to fire.
    pub memory_skill_nudge: bool,
}

impl Default for LoopGuardConfig {
    fn default() -> Self {
        Self {
            warmup_calls: DEFAULT_WARMUP_CALLS,
            repeat_threshold: DEFAULT_REPEAT_THRESHOLD,
            alternation_cycles: DEFAULT_ALTERNATION_CYCLES,
            no_progress_threshold: DEFAULT_NO_PROGRESS_THRESHOLD,
            max_nudges_per_pattern: DEFAULT_MAX_NUDGES_PER_PATTERN,
            intra_repeat_lines: DEFAULT_INTRA_REPEAT_LINES,
            semantic_echo_similarity: DEFAULT_SEMANTIC_ECHO_SIMILARITY,
            enabled: false,
            nudge_every_n: 20,
            memory_skill_nudge: false,
        }
    }
}

/// Periodic reminder copy. The two variants alternate so the user is nudged
/// toward *either* distilling a durable memory *or* capturing a reusable skill,
/// rather than always the same phrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySkillNudge {
    /// Suggest capturing a durable memory (a fact/conclusion to remember).
    Memory,
    /// Suggest capturing a reusable skill (a repeatable procedure).
    Skill,
}

impl MemorySkillNudge {
    /// Stable identifier for logs and tests.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Skill => "skill",
        }
    }
}

/// Which pathology a [`LoopBreak`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LoopPattern {
    /// The same tool with the same arguments, repeatedly.
    RepeatedCall,
    /// Two distinct calls alternating A→B→A→B.
    Alternating,
    /// Calls keep landing but nothing observable changes.
    NoProgress,
    /// A single tool output repeats the same text fragment many times over
    /// (degenerate generation, not a retry loop).
    StreamingRepetition,
    /// Two consecutive *distinct* outputs are near-duplicates, suggesting the
    /// model is restating the same content rather than advancing.
    SemanticEcho,
    /// A scheduled, non-diagnostic reminder to distil the current work into a
    /// durable memory or a reusable skill. Fires every `nudge_every_n`
    /// observed tool calls when the periodic memory/skill nudge is enabled; it
    /// carries no loop pathology, only advisory copy.
    MemorySkill,
    /// A post-compaction goal self-check reminder. Not a loop pathology — it
    /// asks the model to confirm the original objective is still in view after
    /// context was compressed, guarding long-horizon tasks against goal drift.
    SelfCheck,
}

impl LoopPattern {
    /// Short stable identifier, suitable for logs and metrics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RepeatedCall => "repeated_call",
            Self::Alternating => "alternating",
            Self::NoProgress => "no_progress",
            Self::StreamingRepetition => "streaming_repetition",
            Self::SemanticEcho => "semantic_echo",
            Self::MemorySkill => "memory_skill",
            Self::SelfCheck => "self_check",
        }
    }
}

/// A detected loop, along with the advisory text to feed back to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopBreak {
    /// Which pattern fired.
    pub pattern: LoopPattern,
    /// How many observations formed the evidence.
    pub occurrences: usize,
    /// Tool names involved, in the order they were seen.
    pub tools: Vec<String>,
    /// Advisory message for the model. Bounded in length and phrased as
    /// guidance, never as a termination notice.
    pub nudge: String,
}

/// One executed tool call, as handed to [`LoopGuard::observe`].
#[derive(Debug, Clone)]
pub struct ToolObservation<'a> {
    /// Resolved tool name.
    pub name: &'a str,
    /// Arguments as sent to the tool. Hashed, never retained.
    pub args: &'a serde_json::Value,
    /// Whether the tool reported success.
    pub success: bool,
    /// Observable result of the call — stdout, diff, error text. Hashed into
    /// the outcome digest so a call producing a *different* result is not
    /// treated as a stall.
    pub output: &'a str,
    /// Whether this call demonstrably advanced the world: a file was written,
    /// a command mutated state, a diff was non-empty. When `true`, all stall
    /// and repeat counters reset — real progress is definitionally not a loop,
    /// even if the same tool name recurs.
    pub progress: bool,
}

/// Write a key-order-independent encoding of `value` into `hasher`.
///
/// This crate enables serde_json's `preserve_order` feature, so `Value::Object`
/// is `IndexMap`-backed and both iteration order and `to_string` reflect the
/// order the model happened to emit keys in. Hashing that directly would make
/// `{"a":1,"b":2}` and `{"b":2,"a":1}` — the same call — look like two
/// different ones, silently letting a repeat loop slip past the detector.
/// Object keys are therefore sorted before hashing. Array order is preserved,
/// since for a list the order is genuinely part of the value.
fn hash_canonical(value: &serde_json::Value, hasher: &mut DefaultHasher) {
    use serde_json::Value;
    // Discriminant keeps different shapes from colliding (e.g. the string
    // "1" versus the number 1).
    std::mem::discriminant(value).hash(hasher);
    match value {
        Value::Null => {}
        Value::Bool(b) => b.hash(hasher),
        // `f64` is not `Hash`; the textual form is stable and distinguishes
        // integer from float representations.
        Value::Number(n) => n.to_string().hash(hasher),
        Value::String(s) => s.hash(hasher),
        Value::Array(items) => {
            items.len().hash(hasher);
            for item in items {
                hash_canonical(item, hasher);
            }
        }
        Value::Object(map) => {
            map.len().hash(hasher);
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            for key in keys {
                key.hash(hasher);
                if let Some(entry) = map.get(key) {
                    hash_canonical(entry, hasher);
                }
            }
        }
    }
}

/// Stable 64-bit fingerprint of a `(tool_name, args)` pair.
///
/// Independent of the order the model emitted argument keys in — see
/// [`hash_canonical`].
fn fingerprint(name: &str, args: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    hash_canonical(args, &mut hasher);
    hasher.finish()
}

/// Digest of a call's *observable outcome*, used for stall detection.
fn outcome_digest(observation: &ToolObservation<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    observation.name.hash(&mut hasher);
    observation.success.hash(&mut hasher);
    observation.output.hash(&mut hasher);
    hasher.finish()
}

/// Truncate a tool name for inclusion in a nudge, keeping messages bounded.
fn short_name(name: &str) -> String {
    const MAX: usize = 48;
    if name.chars().count() <= MAX {
        return name.to_string();
    }
    let truncated: String = name.chars().take(MAX).collect();
    format!("{truncated}…")
}

/// Count the longest run of identical non-empty lines within a single output.
///
/// A degenerate generation often copies the same sentence many times; this
/// surfaces that within one tool result rather than across calls.
fn max_identical_line_run(output: &str) -> usize {
    let mut best = 0usize;
    let mut current = 0usize;
    let mut prev: Option<&str> = None;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match prev {
            Some(p) if p == trimmed => current += 1,
            _ => current = 1,
        }
        best = best.max(current);
        prev = Some(trimmed);
    }
    best
}

/// Jaccard similarity between two token sets, treating whitespace-separated
/// words as tokens. Used as a cheap proxy for "semantic" near-duplication
/// without invoking an embedding model.
fn token_jaccard(a: &str, b: &str) -> f64 {
    let set_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let set_b: std::collections::HashSet<&str> = b.split_whitespace().collect();
    if set_a.is_empty() && set_b.is_empty() {
        return 0.0;
    }
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

/// Detects repetition, oscillation and stalling across a single turn.
///
/// Construct one per turn (state is turn-scoped by design — a fresh user
/// message is a fresh intent and should not inherit the previous turn's
/// suspicion), feed it every executed tool call via [`LoopGuard::observe`],
/// and inject the returned nudge if one comes back.
#[derive(Debug)]
pub struct LoopGuard {
    config: LoopGuardConfig,
    /// Total observations this turn, including warmup.
    observed: usize,
    /// Recent `(fingerprint, tool_name)` pairs, newest last.
    history: Vec<(u64, String)>,
    /// Length of the current run of identical fingerprints.
    repeat_run: usize,
    /// Fingerprint the current repeat run is counting.
    repeat_fingerprint: Option<u64>,
    /// Length of the current run of identical outcome digests.
    stall_run: usize,
    /// Digest the current stall run is counting.
    stall_digest: Option<u64>,
    /// Nudges already emitted, per pattern.
    fired: HashMap<LoopPattern, usize>,
    /// Most recent *distinct* output text, for semantic-echo detection.
    last_distinct_output: Option<String>,
    /// The user's task objective, carried across compaction windows (W1). When
    /// set, NoProgress reports echo it so a stalled turn is re-anchored to the
    /// original goal rather than drifting toward whatever the summary implied.
    objective: Option<Objective>,
    /// Monotonic count of observed tool calls across the whole session, used to
    /// drive the periodic memory/skill nudge. Persisted so the cadence survives
    /// across turns and process restarts.
    turn_counter: u64,
    /// Cadence (in observed tool calls) for the periodic memory/skill nudge.
    /// Mirrors [`LoopGuardConfig::nudge_every_n`]; 0 disables the periodic
    /// nudge.
    nudge_every_n: u64,
    /// Which periodic reminder fires next; alternates Memory ↔ Skill.
    next_nudge: MemorySkillNudge,
}

impl LoopGuard {
    /// Create a guard with the given thresholds.
    pub fn new(config: LoopGuardConfig) -> Self {
        let nudge_every_n = config.nudge_every_n;
        Self {
            config,
            observed: 0,
            history: Vec::new(),
            repeat_run: 0,
            repeat_fingerprint: None,
            stall_run: 0,
            stall_digest: None,
            fired: HashMap::new(),
            last_distinct_output: None,
            objective: None,
            turn_counter: 0,
            nudge_every_n,
            next_nudge: MemorySkillNudge::Memory,
        }
    }

    /// Total tool calls observed this turn.
    pub fn observed_calls(&self) -> usize {
        self.observed
    }

    /// Attach the user's task objective so NoProgress reports can re-anchor the
    /// model to it (W1). Returns `self` for chaining after [`LoopGuard::new`].
    #[must_use]
    pub fn with_objective(mut self, objective: Objective) -> Self {
        self.objective = Some(objective);
        self
    }

    /// Current task objective, if one was attached.
    pub fn objective(&self) -> Option<&Objective> {
        self.objective.as_ref()
    }

    /// Whether the cold-start exemption still applies.
    pub fn in_warmup(&self) -> bool {
        self.observed <= self.config.warmup_calls
    }

    /// Record one executed tool call and report a loop if one is detected.
    ///
    /// Returns at most one [`LoopBreak`] per call; when several patterns would
    /// fire simultaneously the most specific one wins (repeat, then
    /// alternation, then stall), because the most specific diagnosis produces
    /// the most actionable advice.
    pub fn observe(&mut self, observation: &ToolObservation<'_>) -> Option<LoopBreak> {
        if !self.config.enabled {
            return None;
        }

        self.observed += 1;
        // Durable, session-wide cadence counter for the periodic memory/skill
        // nudge. Incremented on every observed call so the reminder fires every
        // `nudge_every_n` calls regardless of which turn they belong to.
        self.turn_counter += 1;
        let fingerprint = fingerprint(observation.name, observation.args);

        // Update the identical-call run.
        if self.repeat_fingerprint == Some(fingerprint) {
            self.repeat_run += 1;
        } else {
            self.repeat_fingerprint = Some(fingerprint);
            self.repeat_run = 1;
        }

        // Update the stall run. Genuine progress clears it outright: a call
        // that changed the world is evidence the model is working, not stuck.
        let digest = outcome_digest(observation);
        if observation.progress {
            self.stall_run = 0;
            self.stall_digest = None;
            // Progress also invalidates a repeat run: repeatedly calling a
            // tool that keeps changing something is a legitimate loop over
            // work items, not a death spiral.
            self.repeat_run = 1;
        } else if self.stall_digest == Some(digest) {
            self.stall_run += 1;
        } else {
            self.stall_digest = Some(digest);
            self.stall_run = 1;
        }

        self.history
            .push((fingerprint, observation.name.to_string()));
        if self.history.len() > HISTORY_CAPACITY {
            let overflow = self.history.len() - HISTORY_CAPACITY;
            self.history.drain(..overflow);
        }

        // Cold start: observe, but never accuse.
        if self.in_warmup() {
            return None;
        }

        if let Some(loop_break) = self.check_repeat(observation) {
            return Some(loop_break);
        }
        if let Some(loop_break) = self.check_alternating() {
            return Some(loop_break);
        }
        if let Some(loop_break) = self.check_streaming_repetition(observation) {
            return Some(loop_break);
        }
        if let Some(loop_break) = self.check_semantic_echo(observation) {
            return Some(loop_break);
        }
        let result = self.check_no_progress(observation);
        // The periodic memory/skill reminder is non-diagnostic: it fires on its
        // own cadence and never competes with a loop nudge fired the same turn.
        // When the loop detectors stayed silent, consider the scheduled nudge.
        if result.is_none() {
            if let Some(nudge) = self.periodic_memory_skill_nudge() {
                return Some(nudge);
            }
        }
        result
    }

    /// Emit the periodic "distil this into a memory or skill" reminder when the
    /// cadence is hit, or `None` otherwise. The Memory/Skill copy alternates so
    /// consecutive reminders vary. Only active when both the master switch and
    /// `memory_skill_nudge` are on and `nudge_every_n > 0`.
    fn periodic_memory_skill_nudge(&mut self) -> Option<LoopBreak> {
        if !self.config.memory_skill_nudge || self.nudge_every_n == 0 {
            return None;
        }
        if self.turn_counter % self.nudge_every_n != 0 {
            return None;
        }
        let kind = self.next_nudge;
        // Alternate for next time so the reminder varies between memory/skill.
        self.next_nudge = match self.next_nudge {
            MemorySkillNudge::Memory => MemorySkillNudge::Skill,
            MemorySkillNudge::Skill => MemorySkillNudge::Memory,
        };
        let nudge = match kind {
            MemorySkillNudge::Memory => "[Loop guard] You have reached a periodic checkpoint. \
                 Consider distilling the key conclusions or decisions from this session into a \
                 durable memory so they survive future turns and restarts."
                .to_string(),
            MemorySkillNudge::Skill => "[Loop guard] You have reached a periodic checkpoint. \
                 Consider capturing any reusable procedure or workflow you just performed as a \
                 skill, so it can be applied again without re-deriving the steps."
                .to_string(),
        };
        Some(LoopBreak {
            pattern: LoopPattern::MemorySkill,
            occurrences: 1,
            tools: Vec::new(),
            nudge,
        })
    }

    /// Current session-wide cadence counter (for tests/inspection).
    pub fn turn_counter(&self) -> u64 {
        self.turn_counter
    }

    /// Whether `pattern` may still fire, and if so consume one of its budget.
    fn claim_budget(&mut self, pattern: LoopPattern) -> bool {
        let fired = self.fired.entry(pattern).or_insert(0);
        if *fired >= self.config.max_nudges_per_pattern {
            return false;
        }
        *fired += 1;
        true
    }

    fn check_repeat(&mut self, observation: &ToolObservation<'_>) -> Option<LoopBreak> {
        if self.config.repeat_threshold == 0 || self.repeat_run < self.config.repeat_threshold {
            return None;
        }
        if !self.claim_budget(LoopPattern::RepeatedCall) {
            return None;
        }
        let occurrences = self.repeat_run;
        let name = short_name(observation.name);
        // Reset so the next nudge needs a fresh run of evidence rather than
        // firing again on the very next identical call.
        self.repeat_run = 0;
        self.repeat_fingerprint = None;
        Some(LoopBreak {
            pattern: LoopPattern::RepeatedCall,
            occurrences,
            tools: vec![observation.name.to_string()],
            nudge: format!(
                "[Loop guard] You have called `{name}` {occurrences} times in a row with \
                 identical arguments, and the result has not changed. Repeating it again will \
                 not produce a different answer. Stop and re-plan: state what you were trying \
                 to learn or change, then either (a) use different arguments or a different \
                 tool, (b) act on the result you already have, or (c) tell the user what is \
                 blocking you and ask how to proceed."
            ),
        })
    }

    fn check_alternating(&mut self) -> Option<LoopBreak> {
        let cycles = self.config.alternation_cycles;
        if cycles == 0 {
            return None;
        }
        // A→B repeated `cycles` times needs 2*cycles observations.
        let window = cycles * 2;
        if self.history.len() < window {
            return None;
        }
        let tail = &self.history[self.history.len() - window..];
        let first = tail[0].0;
        let second = tail[1].0;
        if first == second {
            // A degenerate "alternation" between a value and itself is just a
            // repeat; let `check_repeat` own that diagnosis.
            return None;
        }
        let alternates = tail
            .iter()
            .enumerate()
            .all(|(index, (fp, _))| *fp == if index % 2 == 0 { first } else { second });
        if !alternates {
            return None;
        }
        // Detach the names from `self.history` before taking a mutable borrow
        // for the budget check and the window reset below.
        let name_a = tail[0].1.clone();
        let name_b = tail[1].1.clone();
        if !self.claim_budget(LoopPattern::Alternating) {
            return None;
        }
        let tool_a = short_name(&name_a);
        let tool_b = short_name(&name_b);
        // Clear the window so the next nudge requires fresh evidence.
        self.history.clear();
        Some(LoopBreak {
            pattern: LoopPattern::Alternating,
            occurrences: window,
            tools: vec![name_a, name_b],
            nudge: format!(
                "[Loop guard] You are alternating between `{tool_a}` and `{tool_b}` \
                 ({window} calls, A→B→A→B) without converging. This usually means two \
                 changes are undoing each other, or you are re-reading state you have \
                 already seen. Stop and re-plan: write down what differs between the two \
                 states, decide which one is correct, and take a single decisive action — \
                 or ask the user to break the tie."
            ),
        })
    }

    fn check_no_progress(&mut self, observation: &ToolObservation<'_>) -> Option<LoopBreak> {
        if self.config.no_progress_threshold == 0
            || self.stall_run < self.config.no_progress_threshold
        {
            return None;
        }
        if !self.claim_budget(LoopPattern::NoProgress) {
            return None;
        }
        let occurrences = self.stall_run;
        let name = short_name(observation.name);
        self.stall_run = 0;
        self.stall_digest = None;
        Some(LoopBreak {
            pattern: LoopPattern::NoProgress,
            occurrences,
            tools: vec![observation.name.to_string()],
            nudge: {
                let mut nudge = format!(
                    "[Loop guard] The last {occurrences} tool calls (most recently `{name}`) \
                     completed but changed nothing observable — same output, no edits applied, \
                     no new information. You are not making progress on the task. Stop and \
                     re-plan: restate the goal, identify the specific thing you still do not \
                     know, and pick an action that would actually change the state or reveal \
                     something new. If nothing would, report what you found and ask the user \
                     for direction."
                );
                if let Some(obj) = &self.objective {
                    if !obj.text.is_empty() {
                        nudge.push_str(&format!(
                            "\n\nYour original task objective is still: {}",
                            obj.text
                        ));
                    }
                }
                nudge
            },
        })
    }

    /// Detect a single output that repeats the same text fragment many times
    /// over — a degenerate generation rather than a retry loop.
    fn check_streaming_repetition(
        &mut self,
        observation: &ToolObservation<'_>,
    ) -> Option<LoopBreak> {
        if self.config.intra_repeat_lines == 0 {
            return None;
        }
        let run = max_identical_line_run(observation.output);
        if run < self.config.intra_repeat_lines {
            return None;
        }
        if !self.claim_budget(LoopPattern::StreamingRepetition) {
            return None;
        }
        let name = short_name(observation.name);
        // Reset so the next nudge needs a fresh degenerate output.
        self.last_distinct_output = None;
        Some(LoopBreak {
            pattern: LoopPattern::StreamingRepetition,
            occurrences: run,
            tools: vec![observation.name.to_string()],
            nudge: format!(
                "[Loop guard] The output of `{name}` repeats the same line {run} times in a \
                 single response. This is a degenerate generation, not useful work: the model \
                 is stuck echoing itself. Stop and re-plan: produce a response that actually \
                 changes state, or if the task is genuinely complete, summarise the result \
                 concisely instead of repeating it. If repetition is unavoidable, tell the user \
                 what is blocking progress."
            ),
        })
    }

    /// Detect two consecutive *distinct* outputs that are near-duplicates,
    /// suggesting the model is restating the same content rather than advancing.
    fn check_semantic_echo(&mut self, observation: &ToolObservation<'_>) -> Option<LoopBreak> {
        if self.config.semantic_echo_similarity <= 0.0 {
            return None;
        }
        // Record the distinct output, comparing against the previous one.
        let previous = self.last_distinct_output.clone();
        self.last_distinct_output = Some(observation.output.to_string());

        let Some(prev) = previous else {
            return None;
        };
        // An exact repeat is owned by the NoProgress/RepeatedCall detectors;
        // only near-duplicates (high but not total overlap) trip here.
        if prev == observation.output {
            return None;
        }
        let similarity = token_jaccard(&prev, observation.output);
        if similarity < self.config.semantic_echo_similarity {
            return None;
        }
        if !self.claim_budget(LoopPattern::SemanticEcho) {
            return None;
        }
        let name = short_name(observation.name);
        self.last_distinct_output = None;
        Some(LoopBreak {
            pattern: LoopPattern::SemanticEcho,
            occurrences: 2,
            tools: vec![observation.name.to_string()],
            nudge: format!(
                "[Loop guard] Your last two `{name}` outputs are {:.0}% the same (near-duplicate \
                 content). You are echoing yourself rather than making progress. Stop and \
                 re-plan: identify what specifically still needs to change, take a distinct \
                 action, or report what you have and ask the user how to proceed.",
                similarity * 100.0
            ),
        })
    }
}

impl Default for LoopGuard {
    fn default() -> Self {
        Self::new(LoopGuardConfig::default())
    }
}

/// Serializable snapshot of a [`LoopGuard`]'s accumulated suspicion, persisted
/// across agent turns so that a model that was looping at the end of one turn
/// does not get a clean slate when the next turn begins. Only the fields that
/// carry cross-turn signal are kept; the per-call `history` window is
/// intentionally dropped (it is turn-local evidence, not durable suspicion).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LoopGuardState {
    /// Total observations carried over from earlier turns.
    pub observed: usize,
    /// Length of the carry-over identical-call run.
    pub repeat_run: usize,
    /// Fingerprint the carry-over repeat run is counting (`None` = no run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_fingerprint: Option<u64>,
    /// Length of the carry-over stall run.
    pub stall_run: usize,
    /// Digest the carry-over stall run is counting (`None` = no run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall_digest: Option<u64>,
    /// Per-pattern nudge budgets already spent across turns.
    #[serde(default)]
    pub fired: std::collections::HashMap<LoopPattern, usize>,
    /// The task objective, persisted so it survives compaction windows and
    /// process restarts (W1). Absent when none was ever attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<Objective>,
    /// Session-wide cadence counter for the periodic memory/skill nudge.
    /// Persisted so the reminder cadence continues across turns and process
    /// restarts instead of resetting every session.
    #[serde(default)]
    pub turn_counter: u64,
}

impl LoopGuard {
    /// Snapshot the durable parts of this guard for cross-turn persistence.
    #[must_use]
    pub fn snapshot_state(&self) -> LoopGuardState {
        LoopGuardState {
            observed: self.observed,
            repeat_run: self.repeat_run,
            repeat_fingerprint: self.repeat_fingerprint,
            stall_run: self.stall_run,
            stall_digest: self.stall_digest,
            fired: self.fired.clone(),
            objective: self.objective.clone(),
            turn_counter: self.turn_counter,
        }
    }

    /// Restore durable state carried over from a previous turn. Per-call
    /// evidence (`history`, `last_distinct_output`) starts fresh for the new
    /// turn; the suspicion counters, nudge budgets, and the task objective
    /// continue.
    pub fn restore_state(&mut self, state: &LoopGuardState) {
        self.observed = state.observed;
        self.repeat_run = state.repeat_run;
        self.repeat_fingerprint = state.repeat_fingerprint;
        self.stall_run = state.stall_run;
        self.stall_digest = state.stall_digest;
        // Continue the cadence from where the previous run left off so the
        // periodic memory/skill nudge keeps its rhythm across sessions.
        self.turn_counter = state.turn_counter;
        for (pattern, count) in &state.fired {
            let entry = self.fired.entry(*pattern).or_insert(0);
            *entry = (*entry).max(*count);
        }
        // The objective is the user's original goal — once known it stays the
        // source of truth across compaction, so always carry the latest.
        if state.objective.is_some() {
            self.objective = state.objective.clone();
        }
    }
}

/// Shared, cross-turn loop-guard handle. The engine holds one per session and
/// keeps feeding it every tool call; because the same `Arc<Mutex<LoopGuard>>`
/// survives across turns, accumulated loop suspicion is continuous rather than
/// reset on each new user message. The durable portion is also persisted to
/// disk (see `crate::core::engine::turn_loop`) so it survives process restarts.
pub type SharedLoopGuard = std::sync::Arc<tokio::sync::Mutex<LoopGuard>>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build an observation that reports no progress (the interesting case for
    /// most detectors).
    fn stalled<'a>(name: &'a str, args: &'a serde_json::Value) -> ToolObservation<'a> {
        ToolObservation {
            name,
            args,
            success: true,
            output: "same output",
            progress: false,
        }
    }

    fn guard() -> LoopGuard {
        // Loop-detection tests need the guard enabled. The *production* default
        // is `enabled: false` (see `LoopGuardConfig::default`); these tests
        // exercise detection, so they opt in explicitly.
        LoopGuard::new(LoopGuardConfig {
            enabled: true,
            ..LoopGuardConfig::default()
        })
    }

    /// Drive the guard past the cold-start window with calls that are all
    /// distinct, so warmup itself never contributes evidence to a detector.
    fn finish_warmup(guard: &mut LoopGuard) {
        for index in 0..=DEFAULT_WARMUP_CALLS {
            let args = json!({ "warmup": index });
            let observation = ToolObservation {
                name: "list_dir",
                args: &args,
                success: true,
                output: "warmup output",
                progress: true,
            };
            assert!(
                guard.observe(&observation).is_none(),
                "warmup call {index} must not trip a detector"
            );
        }
    }

    #[test]
    fn healthy_varied_sequence_never_fires() {
        let mut guard = guard();
        let calls = [
            ("list_dir", json!({ "path": "src" })),
            ("read_file", json!({ "path": "src/main.rs" })),
            ("grep", json!({ "pattern": "fn main" })),
            ("read_file", json!({ "path": "src/lib.rs" })),
            ("edit_file", json!({ "path": "src/lib.rs" })),
            ("exec_shell", json!({ "cmd": "cargo build" })),
            ("read_file", json!({ "path": "src/main.rs" })),
            ("edit_file", json!({ "path": "src/main.rs" })),
            ("exec_shell", json!({ "cmd": "cargo test" })),
        ];
        for (index, (name, args)) in calls.iter().enumerate() {
            let observation = ToolObservation {
                name,
                args,
                success: true,
                // Distinct output per call: real work produces varying results.
                output: &format!("output {index}"),
                progress: index % 2 == 0,
            };
            assert_eq!(
                guard.observe(&observation),
                None,
                "healthy call {index} ({name}) must not trip a detector"
            );
        }
    }

    #[test]
    fn identical_calls_trip_repeat_detector() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        let args = json!({ "path": "src/main.rs" });

        // repeat_threshold - 1 identical calls are still tolerated.
        for _ in 0..DEFAULT_REPEAT_THRESHOLD - 1 {
            assert_eq!(guard.observe(&stalled("read_file", &args)), None);
        }
        let loop_break = guard
            .observe(&stalled("read_file", &args))
            .expect("identical calls at threshold must trip the guard");
        assert_eq!(loop_break.pattern, LoopPattern::RepeatedCall);
        assert_eq!(loop_break.occurrences, DEFAULT_REPEAT_THRESHOLD);
        assert_eq!(loop_break.tools, vec!["read_file".to_string()]);
        assert!(loop_break.nudge.contains("read_file"));
        // Advisory, not terminal.
        assert!(!loop_break.nudge.to_lowercase().contains("aborting"));
    }

    #[test]
    fn argument_key_order_does_not_defeat_the_fingerprint() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        // Same logical arguments, written in a different order each time.
        // This crate enables serde_json's `preserve_order`, so these two
        // values serialise to *different* strings; the fingerprint must
        // canonicalise key order or a repeat loop would slip through.
        let a = json!({ "path": "x.rs", "limit": 10 });
        let b = json!({ "limit": 10, "path": "x.rs" });
        assert_ne!(
            a.to_string(),
            b.to_string(),
            "precondition: preserve_order makes these serialise differently"
        );
        assert_eq!(guard.observe(&stalled("read_file", &a)), None);
        assert_eq!(guard.observe(&stalled("read_file", &b)), None);
        let loop_break = guard
            .observe(&stalled("read_file", &a))
            .expect("key order must not change the fingerprint");
        assert_eq!(loop_break.pattern, LoopPattern::RepeatedCall);
    }

    #[test]
    fn nested_key_order_does_not_defeat_the_fingerprint() {
        // Canonicalisation has to recurse: a reordered *nested* object is
        // still the same call.
        let a = json!({ "opts": { "deep": { "x": 1, "y": 2 }, "z": [1, 2] } });
        let b = json!({ "opts": { "deep": { "y": 2, "x": 1 }, "z": [1, 2] } });
        assert_eq!(fingerprint("t", &a), fingerprint("t", &b));
    }

    #[test]
    fn array_order_is_significant() {
        // Order within a list is part of the value, not incidental syntax.
        let a = json!({ "paths": ["a.rs", "b.rs"] });
        let b = json!({ "paths": ["b.rs", "a.rs"] });
        assert_ne!(fingerprint("t", &a), fingerprint("t", &b));
    }

    #[test]
    fn distinct_shapes_do_not_collide() {
        // The string "1" and the number 1 are different arguments.
        assert_ne!(
            fingerprint("t", &json!({ "v": "1" })),
            fingerprint("t", &json!({ "v": 1 }))
        );
        // A key present with a null value differs from an absent key.
        assert_ne!(
            fingerprint("t", &json!({ "v": null })),
            fingerprint("t", &json!({}))
        );
        // Tool name participates in the fingerprint.
        let args = json!({ "path": "x.rs" });
        assert_ne!(
            fingerprint("read_file", &args),
            fingerprint("edit_file", &args)
        );
    }

    #[test]
    fn abab_alternation_trips_alternating_detector() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        let a = json!({ "path": "a.rs" });
        let b = json!({ "path": "b.rs" });

        // A, B, A — one full cycle plus one, not yet enough.
        assert_eq!(guard.observe(&stalled("read_file", &a)), None);
        assert_eq!(guard.observe(&stalled("edit_file", &b)), None);
        assert_eq!(guard.observe(&stalled("read_file", &a)), None);
        // The fourth call completes the second A→B cycle.
        let loop_break = guard
            .observe(&stalled("edit_file", &b))
            .expect("A,B,A,B must trip the alternation detector");
        assert_eq!(loop_break.pattern, LoopPattern::Alternating);
        assert_eq!(loop_break.occurrences, DEFAULT_ALTERNATION_CYCLES * 2);
        assert_eq!(
            loop_break.tools,
            vec!["read_file".to_string(), "edit_file".to_string()]
        );
        assert!(loop_break.nudge.contains("read_file"));
        assert!(loop_break.nudge.contains("edit_file"));
    }

    #[test]
    fn cold_start_window_suppresses_detection() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            // Warmup deliberately longer than the repeat threshold so a loop
            // that would otherwise fire is provably suppressed.
            warmup_calls: 6,
            ..LoopGuardConfig::default()
        });
        let args = json!({ "path": "src/main.rs" });
        for index in 0..6 {
            assert_eq!(
                guard.observe(&stalled("read_file", &args)),
                None,
                "call {index} is inside the cold-start window"
            );
        }
        assert!(
            guard.in_warmup(),
            "6 calls with warmup_calls=6 is still warmup"
        );
        // The 7th call is the first eligible one, and the run is long enough.
        let loop_break = guard
            .observe(&stalled("read_file", &args))
            .expect("detection resumes once the cold-start window closes");
        assert_eq!(loop_break.pattern, LoopPattern::RepeatedCall);
    }

    #[test]
    fn progress_resets_counters_so_retries_are_not_loops() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        let args = json!({ "path": "src/main.rs" });

        // A long run of the same tool+args, but each call changes something.
        // This is a legitimate loop over work items, not a death spiral.
        for index in 0..12 {
            let observation = ToolObservation {
                name: "edit_file",
                args: &args,
                success: true,
                output: "applied",
                progress: true,
            };
            assert_eq!(
                guard.observe(&observation),
                None,
                "call {index} made progress and must not be flagged"
            );
        }
    }

    #[test]
    fn differing_errors_are_not_a_stall() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        // Same tool, same args would trip the *repeat* detector, so vary args
        // and confirm the stall detector alone stays quiet while the error
        // text keeps changing — the model is converging on the problem.
        for index in 0..8 {
            let args = json!({ "cmd": format!("cargo build --bin b{index}") });
            let observation = ToolObservation {
                name: "exec_shell",
                args: &args,
                success: false,
                output: &format!("error variant {index}"),
                progress: false,
            };
            assert_eq!(
                guard.observe(&observation),
                None,
                "call {index} produced a new error and is not a stall"
            );
        }
    }

    #[test]
    fn repeated_identical_failures_trip_no_progress() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        // Vary args so the repeat detector stays out of the way, but keep the
        // outcome byte-identical: nothing is changing.
        for index in 0..DEFAULT_NO_PROGRESS_THRESHOLD - 1 {
            let args = json!({ "path": format!("candidate{index}.rs") });
            let observation = ToolObservation {
                name: "read_file",
                args: &args,
                success: true,
                output: "",
                progress: false,
            };
            assert_eq!(guard.observe(&observation), None);
        }
        let args = json!({ "path": "final.rs" });
        let observation = ToolObservation {
            name: "read_file",
            args: &args,
            success: true,
            output: "",
            progress: false,
        };
        let loop_break = guard
            .observe(&observation)
            .expect("identical outcomes must trip the stall detector");
        assert_eq!(loop_break.pattern, LoopPattern::NoProgress);
        assert_eq!(loop_break.occurrences, DEFAULT_NO_PROGRESS_THRESHOLD);
    }

    #[test]
    fn nudges_are_capped_per_pattern() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        let args = json!({ "path": "src/main.rs" });
        let mut fired = 0;
        // Far more identical calls than the cap allows nudges for.
        for _ in 0..60 {
            if let Some(loop_break) = guard.observe(&stalled("read_file", &args))
                && loop_break.pattern == LoopPattern::RepeatedCall
            {
                fired += 1;
            }
        }
        assert_eq!(
            fired, DEFAULT_MAX_NUDGES_PER_PATTERN,
            "repeat nudges must be capped at {DEFAULT_MAX_NUDGES_PER_PATTERN} per turn"
        );
    }

    #[test]
    fn disabled_guard_never_fires() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            enabled: false,
            ..LoopGuardConfig::default()
        });
        let args = json!({ "path": "src/main.rs" });
        for _ in 0..50 {
            assert_eq!(guard.observe(&stalled("read_file", &args)), None);
        }
        assert_eq!(guard.observed_calls(), 0, "disabled guard records nothing");
    }

    #[test]
    fn pattern_labels_are_stable() {
        assert_eq!(LoopPattern::RepeatedCall.as_str(), "repeated_call");
        assert_eq!(LoopPattern::Alternating.as_str(), "alternating");
        assert_eq!(LoopPattern::NoProgress.as_str(), "no_progress");
        assert_eq!(
            LoopPattern::StreamingRepetition.as_str(),
            "streaming_repetition"
        );
        assert_eq!(LoopPattern::SemanticEcho.as_str(), "semantic_echo");
    }

    #[test]
    fn streaming_repetition_trips_on_repeated_lines() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        // A single output that copies the same line 4 times.
        let repeated = "step done\nstep done\nstep done\nstep done";
        let observation = ToolObservation {
            name: "exec_shell",
            args: &json!({ "cmd": "run" }),
            success: true,
            output: repeated,
            progress: false,
        };
        let loop_break = guard
            .observe(&observation)
            .expect("intra-output line repetition must trip the guard");
        assert_eq!(loop_break.pattern, LoopPattern::StreamingRepetition);
        assert_eq!(loop_break.occurrences, 4);
    }

    #[test]
    fn streaming_repetition_ignores_varied_output() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        let varied = "line one\nline two\nline three";
        let observation = ToolObservation {
            name: "exec_shell",
            args: &json!({ "cmd": "run" }),
            success: true,
            output: varied,
            progress: false,
        };
        assert_eq!(guard.observe(&observation), None);
    }

    #[test]
    fn semantic_echo_trips_on_near_duplicate_outputs() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        let first = ToolObservation {
            name: "read_file",
            args: &json!({ "path": "a" }),
            success: true,
            output: "the build failed because the module is missing and the import is broken",
            progress: false,
        };
        assert_eq!(
            guard.observe(&first),
            None,
            "first output recorded, no echo yet"
        );
        let second = ToolObservation {
            name: "read_file",
            args: &json!({ "path": "b" }),
            success: true,
            output: "the build failed because the module is missing and the import is broken now",
            progress: false,
        };
        let loop_break = guard
            .observe(&second)
            .expect("near-duplicate consecutive outputs must trip the echo detector");
        assert_eq!(loop_break.pattern, LoopPattern::SemanticEcho);
    }

    #[test]
    fn semantic_echo_ignores_distinct_outputs() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        let first = ToolObservation {
            name: "read_file",
            args: &json!({ "path": "a" }),
            success: true,
            output: "the config file lists three servers in the eu region",
            progress: false,
        };
        assert_eq!(guard.observe(&first), None);
        let second = ToolObservation {
            name: "read_file",
            args: &json!({ "path": "b" }),
            success: true,
            output: "the test suite reported two passing cases and one failure in parsing",
            progress: false,
        };
        assert_eq!(
            guard.observe(&second),
            None,
            "distinct outputs are not an echo"
        );
    }

    /// #858 acceptance: a turn that makes no observable progress for
    /// `no_progress_threshold` consecutive calls MUST be detected and halted
    /// (the guard trips `NoProgress`). This is the concrete backstop that
    /// proves the agent cannot loop forever on a stale world state.
    #[test]
    fn acceptance_858_no_progress_trips_and_halts() {
        let mut guard = guard();
        finish_warmup(&mut guard);

        // Same tool, identical outcome, zero progress — for N+1 calls.
        let mut tripped_at: Option<usize> = None;
        for index in 0..DEFAULT_NO_PROGRESS_THRESHOLD + 2 {
            let args = json!({ "path": format!("candidate{index}") });
            let observation = ToolObservation {
                name: "read_file",
                args: &args,
                success: true,
                output: "",
                progress: false,
            };
            if let Some(loop_break) = guard.observe(&observation)
                && loop_break.pattern == LoopPattern::NoProgress
                && tripped_at.is_none()
            {
                tripped_at = Some(index);
                // The loop is halted: a LoopBreak is returned carrying the
                // advisory to stop and re-plan, so the caller can break the
                // turn instead of continuing the death spiral.
                assert!(
                    loop_break
                        .nudge
                        .to_lowercase()
                        .contains("not making progress")
                );
                break;
            }
        }
        assert!(
            tripped_at.is_some(),
            "NoProgress must trip within the threshold window"
        );
        assert_eq!(tripped_at.unwrap(), DEFAULT_NO_PROGRESS_THRESHOLD - 1);
    }

    /// #858 acceptance (companion): the #845 escalation path caps retries at
    /// `max_escalations = 2`, so a persistently-failing task is abandoned
    /// rather than spun forever. We use the same pure retry core the engine
    /// uses; a mock that always fails must stop after exactly 2 escalations.
    #[test]
    fn acceptance_858_escalation_caps_retries() {
        use crate::core::engine::resilience::{
            EffortEscalationPolicy, EffortTier, ValidationRetryConfig, ValidationVerdict,
            retry_turn_with_escalation,
        };
        let config = ValidationRetryConfig {
            policy: EffortEscalationPolicy {
                max_escalations: 2,
                model_upgrade_chain: Vec::new(),
            },
            objective: Some("must not spin".to_string()),
        };
        let (escalations, verdict, _effort, _model) = retry_turn_with_escalation(
            &config,
            EffortTier::Low,
            "model-small",
            |_effort, _model| ValidationVerdict::Fail,
            |v| v.clone(),
        );
        assert_eq!(verdict, ValidationVerdict::Fail);
        assert_eq!(escalations, 2, "cap must stop the spin after 2 escalations");
    }

    #[test]
    fn acceptance_858_loop_guard_halts_on_no_progress() {
        // #858 — the agent MUST NOT loop forever. A turn that makes no
        // observable progress for N consecutive iterations must be detected and
        // the loop halted (LoopGuard trips with NoProgress).
        let mut guard = guard();
        finish_warmup(&mut guard);

        // Drive a chain of calls whose *outcome* never changes: the same
        // digest repeats, so the stall detector accumulates a run. Each call
        // uses a distinct name+args (so the repeat/alternating detectors stay
        // out of the way) — this isolates the "no forward progress" pathology,
        // which is exactly what an infinite no-op loop looks like.
        let mut halted_at: Option<usize> = None;
        for index in 0..12 {
            let args = json!({ "probe": format!("candidate{index}") });
            let observation = ToolObservation {
                name: "read_file",
                args: &args,
                success: true,
                // Identical observable output every single turn => no progress.
                output: "file unchanged",
                progress: false,
            };
            if let Some(loop_break) = guard.observe(&observation)
                && loop_break.pattern == LoopPattern::NoProgress
            {
                halted_at = Some(index);
                break;
            }
        }

        let at = halted_at.expect("no-progress loop must be halted by the guard");
        assert!(
            at < DEFAULT_REPEAT_THRESHOLD + DEFAULT_NO_PROGRESS_THRESHOLD + DEFAULT_WARMUP_CALLS,
            "halt must happen early, not after grinding for hundreds of calls (tripped at {at})"
        );
    }

    #[test]
    fn semantic_echo_requires_two_distinct_observations() {
        let mut guard = guard();
        finish_warmup(&mut guard);
        // Only one observation so far: no previous to compare against.
        let solo = ToolObservation {
            name: "read_file",
            args: &json!({ "path": "a" }),
            success: true,
            output: "identical text repeated many times over and over again",
            progress: false,
        };
        assert_eq!(guard.observe(&solo), None);
    }

    #[test]
    fn no_progress_report_carries_objective_text() {
        let objective = Objective {
            text: "Migrate the billing service to Stripe".to_string(),
            key_points: vec!["webhook signatures".to_string()],
        };
        let mut guard = guard().with_objective(objective);
        finish_warmup(&mut guard);
        for index in 0..DEFAULT_NO_PROGRESS_THRESHOLD - 1 {
            let args = json!({ "path": format!("candidate{index}.rs") });
            let observation = ToolObservation {
                name: "read_file",
                args: &args,
                success: true,
                output: "",
                progress: false,
            };
            assert_eq!(guard.observe(&observation), None);
        }
        let args = json!({ "path": "final.rs" });
        let observation = ToolObservation {
            name: "read_file",
            args: &args,
            success: true,
            output: "",
            progress: false,
        };
        let loop_break = guard
            .observe(&observation)
            .expect("stall must trip NoProgress");
        assert_eq!(loop_break.pattern, LoopPattern::NoProgress);
        assert!(
            loop_break
                .nudge
                .contains("Migrate the billing service to Stripe"),
            "NoProgress nudge must re-anchor the model to the original objective"
        );
    }

    #[test]
    fn objective_is_persisted_across_turns_via_state() {
        let objective = Objective {
            text: "Refactor the auth module to use JWT".to_string(),
            key_points: Vec::new(),
        };
        let mut guard = guard().with_objective(objective);
        // Snapshot and restore into a fresh guard (simulates a compaction
        // window or process restart).
        let state = guard.snapshot_state();
        let mut restored = LoopGuard::default();
        restored.restore_state(&state);
        assert_eq!(
            restored.objective().map(|o| o.text.as_str()),
            Some("Refactor the auth module to use JWT")
        );
    }

    /// Periodic memory/skill nudge fires exactly when `turn_counter` reaches
    /// `nudge_every_n`, and the Memory/Skill copy alternates between hits.
    #[test]
    fn periodic_memory_skill_nudge_fires_every_n() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            enabled: true,
            memory_skill_nudge: true,
            nudge_every_n: 5,
            ..LoopGuardConfig::default()
        });
        // Calls 1..=4 stay silent; call 5 hits the cadence.
        for index in 1..=4 {
            assert_eq!(
                guard.observe(&stalled("read_file", &json!({ "p": index }))),
                None,
                "call {index} must not trigger the periodic nudge yet"
            );
        }
        let first = guard
            .observe(&stalled("read_file", &json!({ "p": 5 })))
            .expect("call 5 must trigger the periodic nudge");
        assert_eq!(first.pattern, LoopPattern::MemorySkill);
        assert!(first.nudge.contains("memory"));
        // Counter continues; call 10 is the next checkpoint, with Skill copy.
        for index in 6..=9 {
            assert_eq!(
                guard.observe(&stalled("read_file", &json!({ "p": index }))),
                None
            );
        }
        let second = guard
            .observe(&stalled("read_file", &json!({ "p": 10 })))
            .expect("call 10 must trigger the next periodic nudge");
        assert_eq!(second.pattern, LoopPattern::MemorySkill);
        assert!(second.nudge.contains("skill"));
        assert_eq!(guard.turn_counter(), 10);
    }

    /// The cadence counter survives a snapshot/restore round-trip, so the
    /// reminder keeps its rhythm across sessions (no reset to zero).
    #[test]
    fn turn_counter_continues_across_water() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            enabled: true,
            memory_skill_nudge: true,
            nudge_every_n: 10,
            ..LoopGuardConfig::default()
        });
        // 7 calls in the first session.
        for index in 0..7 {
            let _ = guard.observe(&stalled("read_file", &json!({ "p": index })));
        }
        assert_eq!(guard.turn_counter(), 7);
        // Persist and restore into a fresh guard (process restart).
        let state = guard.snapshot_state();
        let mut restored = LoopGuard::new(LoopGuardConfig {
            enabled: true,
            memory_skill_nudge: true,
            nudge_every_n: 10,
            ..LoopGuardConfig::default()
        });
        assert_eq!(restored.turn_counter(), 0, "fresh guard starts at 0");
        restored.restore_state(&state);
        assert_eq!(restored.turn_counter(), 7, "counter must carry over");
        // 3 more calls => 10th overall => nudge fires, not at 10th of new guard.
        for index in 7..=9 {
            let _ = restored.observe(&stalled("read_file", &json!({ "p": index })));
        }
        let nudge = restored
            .observe(&stalled("read_file", &json!({ "p": 10 })))
            .expect("nudge must fire at global call 10, continuing the counter");
        assert_eq!(nudge.pattern, LoopPattern::MemorySkill);
    }

    /// When `enabled` is false (the default), neither loop detection nor the
    /// periodic nudge ever fires — full silence until the user opts in.
    #[test]
    fn disabled_guard_suppresses_periodic_nudge() {
        // Note: default config now has `enabled: false`.
        let mut guard = LoopGuard::default();
        for index in 1..=100 {
            assert_eq!(
                guard.observe(&stalled("read_file", &json!({ "p": index }))),
                None,
                "disabled guard must stay silent (call {index})"
            );
        }
        assert_eq!(guard.turn_counter(), 0, "disabled guard counts nothing");
    }

    /// `enabled` true but `memory_skill_nudge` false must never emit the
    /// periodic reminder, while loop detection still works.
    #[test]
    fn loop_only_enabled_suppresses_periodic_nudge() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            enabled: true,
            memory_skill_nudge: false,
            nudge_every_n: 3,
            ..LoopGuardConfig::default()
        });
        // Helpfully past warmup, then drive distinct calls up to a multiple of
        // nudge_every_n; no periodic nudge should ever appear.
        finish_warmup(&mut guard);
        for index in 0..30 {
            let result = guard.observe(&stalled(
                "read_file",
                &json!({ "p": format!("distinct-{index}") }),
            ));
            assert_ne!(
                result.map(|b| b.pattern),
                Some(LoopPattern::MemorySkill),
                "periodic nudge must not fire when memory_skill_nudge is false"
            );
        }
    }
}
