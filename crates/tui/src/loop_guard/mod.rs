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

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

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

/// How many recent fingerprints to retain for alternation analysis.
const HISTORY_CAPACITY: usize = 16;

/// Tunable thresholds for [`LoopGuard`].
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Master switch. When `false`, [`LoopGuard::observe`] always returns
    /// `None`.
    pub enabled: bool,
}

impl Default for LoopGuardConfig {
    fn default() -> Self {
        Self {
            warmup_calls: DEFAULT_WARMUP_CALLS,
            repeat_threshold: DEFAULT_REPEAT_THRESHOLD,
            alternation_cycles: DEFAULT_ALTERNATION_CYCLES,
            no_progress_threshold: DEFAULT_NO_PROGRESS_THRESHOLD,
            max_nudges_per_pattern: DEFAULT_MAX_NUDGES_PER_PATTERN,
            enabled: true,
        }
    }
}

/// Which pathology a [`LoopBreak`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoopPattern {
    /// The same tool with the same arguments, repeatedly.
    RepeatedCall,
    /// Two distinct calls alternating A→B→A→B.
    Alternating,
    /// Calls keep landing but nothing observable changes.
    NoProgress,
}

impl LoopPattern {
    /// Short stable identifier, suitable for logs and metrics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RepeatedCall => "repeated_call",
            Self::Alternating => "alternating",
            Self::NoProgress => "no_progress",
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
}

impl LoopGuard {
    /// Create a guard with the given thresholds.
    pub fn new(config: LoopGuardConfig) -> Self {
        Self {
            config,
            observed: 0,
            history: Vec::new(),
            repeat_run: 0,
            repeat_fingerprint: None,
            stall_run: 0,
            stall_digest: None,
            fired: HashMap::new(),
        }
    }

    /// Total tool calls observed this turn.
    pub fn observed_calls(&self) -> usize {
        self.observed
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

        self.history.push((fingerprint, observation.name.to_string()));
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
        self.check_no_progress(observation)
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
            nudge: format!(
                "[Loop guard] The last {occurrences} tool calls (most recently `{name}`) \
                 completed but changed nothing observable — same output, no edits applied, \
                 no new information. You are not making progress on the task. Stop and \
                 re-plan: restate the goal, identify the specific thing you still do not \
                 know, and pick an action that would actually change the state or reveal \
                 something new. If nothing would, report what you found and ask the user \
                 for direction."
            ),
        })
    }
}

impl Default for LoopGuard {
    fn default() -> Self {
        Self::new(LoopGuardConfig::default())
    }
}

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
        LoopGuard::default()
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
        assert_ne!(fingerprint("read_file", &args), fingerprint("edit_file", &args));
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
        assert!(guard.in_warmup(), "6 calls with warmup_calls=6 is still warmup");
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
    }
}
