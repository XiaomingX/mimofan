//! `LoopGuard` 单元测试。
//!
//! 从 `crates/tui/src/loop_guard/mod.rs` 的内联 `#[cfg(test)] mod tests` 迁出，
//! `fingerprint` 已在本模块中 `pub` 暴露，供集成测试直接引用。

use mimofan::compaction::objective::Objective;
use mimofan::core::engine::resilience::{
    EffortEscalationPolicy, EffortTier, ValidationRetryConfig, ValidationVerdict,
    retry_turn_with_escalation,
};
use mimofan::loop_guard::{
    DEFAULT_ALTERNATION_CYCLES, DEFAULT_MAX_NUDGES_PER_PATTERN, DEFAULT_NO_PROGRESS_THRESHOLD,
    DEFAULT_REPEAT_THRESHOLD, DEFAULT_WARMUP_CALLS, LoopGuard, LoopGuardConfig, LoopPattern,
    ToolObservation, fingerprint,
};
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

/// A tool call that genuinely made progress (distinct output, `progress =
/// true`). Used by the periodic-nudge cadence tests: a progressing call
/// must never trip the NoProgress detector, so the scheduled memory/skill
/// reminder is the only thing that can fire on its cadence.
fn progressing<'a>(name: &'a str, args: &'a serde_json::Value) -> ToolObservation<'a> {
    ToolObservation {
        name,
        args,
        success: true,
        output: "applied",
        progress: true,
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
        // Loop detection must be opted in explicitly; the production
        // default is `enabled: false`.
        enabled: true,
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
    let guard = guard().with_objective(objective);
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
    // Calls 1..=4 stay silent; call 5 hits the cadence. Progressing calls
    // (distinct output) keep the NoProgress detector silent so the only
    // thing that can fire on this cadence is the scheduled reminder.
    for index in 1..=4 {
        assert_eq!(
            guard.observe(&progressing("read_file", &json!({ "p": index }))),
            None,
            "call {index} must not trigger the periodic nudge yet"
        );
    }
    let first = guard
        .observe(&progressing("read_file", &json!({ "p": 5 })))
        .expect("call 5 must trigger the periodic nudge");
    assert_eq!(first.pattern, LoopPattern::MemorySkill);
    assert!(first.nudge.contains("memory"));
    // Counter continues; call 10 is the next checkpoint, with Skill copy.
    for index in 6..=9 {
        assert_eq!(
            guard.observe(&progressing("read_file", &json!({ "p": index }))),
            None
        );
    }
    let second = guard
        .observe(&progressing("read_file", &json!({ "p": 10 })))
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
    // 7 progressing calls in the first session (NoProgress stays silent).
    for index in 0..7 {
        let _ = guard.observe(&progressing("read_file", &json!({ "p": index })));
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
    // 2 more calls => turn_counter 8, 9; the 3rd (global call 10) fires.
    for index in 7..=8 {
        let _ = restored.observe(&progressing("read_file", &json!({ "p": index })));
    }
    let nudge = restored
        .observe(&progressing("read_file", &json!({ "p": 9 })))
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
