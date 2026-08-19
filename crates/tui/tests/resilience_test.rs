//! `resilience` 模块单元测试（TaskBudget / CheckpointStore / SerializableAgentState /
//! EffortEscalationPolicy / ResumeController）。
//!
//! 从 `crates/tui/src/core/engine/resilience.rs` 的内联 `#[cfg(test)] mod tests` 迁出；
//! 相关类型与方法的可见性已为 `pub`，集成测试经 `mimofan::core::engine::resilience` 访问。

use mimofan::core::engine::resilience::{
    CheckpointStore, DEFAULT_MAX_ESCALATIONS, EffortEscalationPolicy, EffortTier,
    ResumeController, SerializableAgentState, TaskBudget, ValidationRetryConfig, ValidationVerdict,
    retry_turn_with_escalation,
};
use mimofan::models::Usage;
use std::path::PathBuf;
use uuid::Uuid;

// ---- #848 TaskBudget -------------------------------------------------
#[test]
fn budget_decrements_and_halts_at_zero() {
    let mut budget = TaskBudget::new(10);
    assert!(!budget.spend(4));
    assert_eq!(budget.remaining, 6);
    assert!(!budget.spend(5));
    assert_eq!(budget.remaining, 1);
    // Spending 1 more exhausts it.
    assert!(budget.spend(1));
    assert_eq!(budget.remaining, 0);
    assert!(budget.is_exhausted());
    // Saturating: never goes negative.
    assert!(budget.spend(100));
    assert_eq!(budget.remaining, 0);
}

#[test]
fn budget_from_config_none_is_unbounded() {
    assert!(TaskBudget::from_config(None).is_none());
    let b = TaskBudget::from_config(Some(50)).unwrap();
    assert_eq!(b.total, 50);
    assert_eq!(b.remaining, 50);
}

#[test]
fn budget_spend_usage_sums_io_tokens() {
    let mut budget = TaskBudget::new(100);
    let usage = Usage {
        input_tokens: 30,
        output_tokens: 20,
        ..Usage::default()
    };
    assert!(!budget.spend_usage(&usage));
    assert_eq!(budget.remaining, 50);
    assert_eq!(budget.consumed, 50);
}

#[test]
fn budget_context_marker_format() {
    let budget = TaskBudget::new(100);
    assert_eq!(
        budget.context_marker(),
        "<!-- budget: 100 remaining / 100 total -->"
    );
}

// ---- #851 CheckpointStore -------------------------------------------
#[test]
fn checkpoint_write_reload_last_turn_and_count() {
    let dir = std::env::temp_dir().join(format!("mimofan-cp-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = CheckpointStore::open(&dir);
    store.save_turn_checkpoint(1, "did A", "obj", 10).unwrap();
    store.save_turn_checkpoint(2, "did B", "obj", 25).unwrap();
    store.save_turn_checkpoint(3, "did C", "obj", 40).unwrap();

    // Reload from disk (fresh store) to prove durability.
    let reloaded = CheckpointStore::open(&dir);
    assert_eq!(reloaded.count(), 3);
    let latest = reloaded.load_latest().unwrap();
    assert_eq!(latest.turn, 3);
    assert_eq!(latest.tokens_consumed, 40);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checkpoint_is_idempotent_for_same_tail() {
    let dir = std::env::temp_dir().join(format!("mimofan-cp-idem-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = CheckpointStore::open(&dir);
    store.save_turn_checkpoint(1, "did A", "obj", 10).unwrap();
    // Re-emit the same tail checkpoint (simulating a crash+resend).
    store.save_turn_checkpoint(1, "did A", "obj", 10).unwrap();
    assert_eq!(store.count(), 1, "identical tail must not duplicate");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checkpoint_resolves_duplicate_turn_to_last() {
    let dir = std::env::temp_dir().join(format!("mimofan-cp-dup-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = CheckpointStore::open(&dir);
    store.save_turn_checkpoint(1, "first", "obj", 10).unwrap();
    store
        .save_turn_checkpoint(1, "corrected", "obj", 12)
        .unwrap();
    let reloaded = CheckpointStore::open(&dir);
    assert_eq!(reloaded.count(), 2);
    assert_eq!(reloaded.load_latest().unwrap().summary, "corrected");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- #856 SerializableAgentState ------------------------------------
#[test]
fn state_round_trips_key_fields() {
    let mut state = SerializableAgentState::default();
    state.objective = "ship the feature".to_string();
    state.goal_queue_id = Some("goal-7".to_string());
    state.turn_index = 4;
    state.budget_remaining = Some(123);
    state.budget_total = Some(500);
    state.tokens_consumed = 377;
    state
        .active_subagents
        .insert("agent-1".to_string(), "researcher".to_string());
    state.open_files.push(PathBuf::from("/ws/src/main.rs"));
    state.escalations_applied = 1;
    state.model = "deepseek-chat".to_string();
    state.reasoning_effort = Some("high".to_string());

    let json = state.to_json().unwrap();
    let back = SerializableAgentState::from_json(&json).unwrap();
    assert_eq!(back, state);
    assert_eq!(back.objective, "ship the feature");
    assert_eq!(back.turn_index, 4);
    assert_eq!(back.budget_remaining, Some(123));
    assert_eq!(
        back.active_subagents.get("agent-1").map(String::as_str),
        Some("researcher")
    );
}

#[test]
fn state_rejects_unsupported_schema() {
    let mut state = SerializableAgentState::default();
    state.schema_version = 999;
    let json = serde_json::to_string(&state).unwrap();
    assert!(SerializableAgentState::from_json(&json).is_err());
}

// ---- #845 EffortEscalationPolicy + retry ----------------------------
#[test]
fn effort_tier_steps_up_in_order() {
    let mut t = EffortTier::Off;
    let mut steps = vec![t.clone()];
    while let Some(next) = t.next() {
        steps.push(next.clone());
        t = next;
    }
    assert_eq!(
        steps,
        vec![
            EffortTier::Off,
            EffortTier::Low,
            EffortTier::Medium,
            EffortTier::High,
            EffortTier::Max
        ]
    );
    assert_eq!(EffortTier::Max.next(), None);
}

#[test]
fn policy_escalates_effort_then_model() {
    let policy = EffortEscalationPolicy {
        max_escalations: 3,
        model_upgrade_chain: vec!["model-big".to_string()],
    };
    let step1 = policy.escalate(&EffortTier::Low, "model-small", 0);
    assert!(step1.changed);
    assert_eq!(step1.effort, EffortTier::Medium);
    assert_eq!(step1.model, "model-small");

    // After effort maxes out, the next escalation swaps the model.
    let step2 = policy.escalate(&EffortTier::Max, "model-small", 1);
    assert!(step2.changed);
    assert_eq!(step2.effort, EffortTier::Max);
    assert_eq!(step2.model, "model-big");

    // Beyond the cap, no further change.
    let step3 = policy.escalate(&EffortTier::Max, "model-big", 3);
    assert!(!step3.changed);
}

#[test]
fn retry_escalates_until_pass() {
    let config = ValidationRetryConfig {
        policy: EffortEscalationPolicy {
            max_escalations: 2,
            model_upgrade_chain: vec!["model-big".to_string()],
        },
        objective: Some("make it compile".to_string()),
    };

    // Mock: fails on `low`, passes on `medium` or higher.
    let (escalations, verdict, _effort, _model) = retry_turn_with_escalation(
        &config,
        EffortTier::Low,
        "model-small",
        |effort, _model| {
            if matches!(effort, EffortTier::Low) {
                ValidationVerdict::Fail
            } else {
                ValidationVerdict::Pass
            }
        },
        |v| v.clone(),
    );

    assert_eq!(verdict, ValidationVerdict::Pass);
    assert_eq!(
        escalations, 1,
        "should escalate exactly once (low -> medium)"
    );
}

#[test]
fn retry_gives_up_after_cap() {
    let config = ValidationRetryConfig {
        policy: EffortEscalationPolicy {
            max_escalations: 2,
            model_upgrade_chain: Vec::new(),
        },
        objective: Some("never passes".to_string()),
    };
    let (escalations, verdict, _effort, _model) = retry_turn_with_escalation(
        &config,
        EffortTier::Low,
        "model-small",
        |_effort, _model| ValidationVerdict::Fail,
        |v| v.clone(),
    );
    assert_eq!(verdict, ValidationVerdict::Fail);
    assert_eq!(escalations, 2, "should exhaust the escalation cap");
}

// ---- #858 acceptance — loop/stop must respect the escalation cap -----
#[test]
fn acceptance_858_max_escalations_caps_persistent_failure() {
    // A persistently-failing task (e.g. an infinite-ish retry loop) must
    // STOP rather than spin: the escalation policy caps retries at
    // max_escalations=2 (#845), and retry_turn_with_escalation must honour
    // that cap exactly and never escalate beyond it.
    let config = ValidationRetryConfig {
        policy: EffortEscalationPolicy {
            max_escalations: DEFAULT_MAX_ESCALATIONS, // 2
            model_upgrade_chain: Vec::new(),
        },
        objective: Some("task that never validates".to_string()),
    };

    // Mock turn: always fails validation, simulating a task that the model
    // keeps retrying but never completes.
    let mut attempts = 0u32;
    let (escalations, verdict, effort, model) = retry_turn_with_escalation(
        &config,
        EffortTier::Low,
        "model-small",
        |_effort, _model| {
            attempts += 1;
            ValidationVerdict::Fail
        },
        |v| v.clone(),
    );

    assert_eq!(
        verdict,
        ValidationVerdict::Fail,
        "must give up, not keep spinning"
    );
    assert_eq!(
        escalations, DEFAULT_MAX_ESCALATIONS,
        "escalations must be capped at max_escalations (2)"
    );
    // initial attempt + 2 escalations = 3 attempts total; it must not run
    // away to hundreds of retries.
    assert_eq!(
        attempts,
        DEFAULT_MAX_ESCALATIONS + 1,
        "run_turn must be called exactly cap+1 times"
    );
    // Once the cap is hit, further calls to escalate must be no-ops.
    let step = config.policy.escalate(&effort, &model, escalations);
    assert!(!step.changed, "escalate() must stop changing past the cap");
}

// ---- #861 acceptance — crash recovery resumes the correct turn -------
#[test]
fn acceptance_861_crash_recovery_resumes_turn_three() {
    // #861 — a run interrupted mid-way must resume from where it left off.
    // Simulate 3 turns, each writing a turn checkpoint to a session dir.
    // Then "crash" (drop the handles) and start a fresh engine-like replay
    // from the same session path; it must recover turn index 3 (skip the
    // 3 completed turns) and the budget/objective state.
    let dir = std::env::temp_dir().join(format!("mimofan-accept-861-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    // --- First (crashed) engine: writes 3 turn checkpoints + state ------
    {
        let mut ctrl = ResumeController::open(&dir);
        // Turn 1, 2, 3 all complete and persist their checkpoints.
        ctrl.checkpoints_mut()
            .save_turn_checkpoint(1, "scaffold module", "build the feature", 100)
            .unwrap();
        ctrl.checkpoints_mut()
            .save_turn_checkpoint(2, "implement core", "build the feature", 250)
            .unwrap();
        ctrl.checkpoints_mut()
            .save_turn_checkpoint(3, "wire tests", "build the feature", 400)
            .unwrap();

        // Persist orchestration state: objective + remaining budget.
        let mut state = SerializableAgentState::default();
        state.objective = "build the feature".to_string();
        state.turn_index = 3;
        state.budget_remaining = Some(600);
        state.budget_total = Some(1000);
        state.tokens_consumed = 400;
        ctrl.save_state(&state).unwrap();
        // `ctrl` and `state` drop here = the "crash".
    }

    // --- Fresh engine replays from the same session dir -----------------
    let resumed = ResumeController::open(&dir);
    assert!(resumed.has_resume_point(), "crash left resumable progress");
    // Last completed turn was 3, so the engine must resume at turn 4
    // (already-completed turns are skipped).
    assert_eq!(
        resumed.resume_from_turn(),
        Some(3),
        "must recover turn index 3 as last-completed"
    );
    let recovered = resumed.load_state().expect("state must survive the crash");
    assert_eq!(recovered.turn_index, 3, "objective turn state recovered");
    assert_eq!(
        recovered.objective, "build the feature",
        "objective recovered"
    );
    assert_eq!(
        recovered.budget_remaining,
        Some(600),
        "budget state recovered"
    );
    assert_eq!(recovered.tokens_consumed, 400);
    // Durability: the checkpoint file on disk really holds 3 turns.
    assert_eq!(
        resumed.checkpoints().count(),
        3,
        "three turn checkpoints persisted to disk"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---- #857 ResumeController ------------------------------------------
#[test]
fn resume_skips_already_done_turns() {
    let dir = std::env::temp_dir().join(format!("mimofan-resume-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    // Simulate a run interrupted after 2 turns.
    let mut ctrl = ResumeController::open(&dir);
    ctrl.checkpoints_mut()
        .save_turn_checkpoint(1, "turn 1 done", "obj", 10)
        .unwrap();
    ctrl.checkpoints_mut()
        .save_turn_checkpoint(2, "turn 2 done", "obj", 20)
        .unwrap();

    // New engine, same session dir, restarts.
    let resumed = ResumeController::open(&dir);
    assert!(resumed.has_resume_point());
    // Last completed turn = 2, so the engine should resume at turn 3.
    assert_eq!(resumed.resume_from_turn(), Some(2));

    let _ = std::fs::remove_dir_all(&dir);
}

/// #861 acceptance: a run interrupted mid-way can resume. We simulate a
/// 3-turn run that writes a checkpoint per turn, then "crash" (drop all
/// handles), then start a fresh engine-like replay from the same session
/// path and assert it recovers turn index 3 (already-completed turns
/// skipped) together with the persisted budget/objective state.
#[test]
fn acceptance_861_resume_recovers_turn_index_and_state() {
    let dir = std::env::temp_dir().join(format!("mimofan-resume-acc-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    // ---- Phase 1: original run, 3 turns, then crash. ----
    {
        let mut ctrl = ResumeController::open(&dir);
        let mut tokens = 0usize;
        for turn in 1..=3u64 {
            tokens += 15;
            ctrl.checkpoints_mut()
                .save_turn_checkpoint(
                    turn,
                    &format!("turn {turn} completed"),
                    "ship the feature",
                    tokens,
                )
                .unwrap();
        }
        // Persist orchestration state at the last turn boundary.
        let mut state = SerializableAgentState::default();
        state.objective = "ship the feature".to_string();
        state.turn_index = 3;
        state.budget_remaining = Some(45);
        state.budget_total = Some(100);
        state.tokens_consumed = tokens;
        state.escalations_applied = 0;
        ctrl.save_state(&state).unwrap();
        // <-- handles dropped here = "crash". Disk is the only source of truth.
    }

    // ---- Phase 2: fresh engine replay from the same session dir. ----
    let resumed = ResumeController::open(&dir);
    assert!(
        resumed.has_resume_point(),
        "crash left a resumable checkpoint"
    );

    // Last completed turn is 3, so the engine must resume at turn 4
    // (already-completed turns are skipped).
    assert_eq!(
        resumed.resume_from_turn(),
        Some(3),
        "resume cursor must be the last completed turn"
    );

    // Recovered orchestration state must match what was persisted.
    let state = resumed.load_state().expect("state must survive the crash");
    assert_eq!(state.turn_index, 3, "turn_index must be recovered");
    assert_eq!(
        state.objective, "ship the feature",
        "objective must be recovered"
    );
    assert_eq!(state.budget_remaining, Some(45), "budget must be recovered");
    assert_eq!(state.budget_total, Some(100));
    assert_eq!(state.tokens_consumed, 45, "cumulative tokens recovered");

    // The replay must NOT re-run completed turns: the checkpoint count is
    // still exactly 3 — nothing was lost or duplicated by the crash.
    assert_eq!(resumed.checkpoints().count(), 3);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resume_state_round_trips_through_controller() {
    let dir =
        std::env::temp_dir().join(format!("mimofan-resume-state-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let ctrl = ResumeController::open(&dir);
    let mut state = SerializableAgentState::default();
    state.turn_index = 5;
    state.budget_remaining = Some(42);
    ctrl.save_state(&state).unwrap();

    let reloaded = ResumeController::open(&dir).load_state().unwrap();
    assert_eq!(reloaded.turn_index, 5);
    assert_eq!(reloaded.budget_remaining, Some(42));

    let _ = std::fs::remove_dir_all(&dir);
}
