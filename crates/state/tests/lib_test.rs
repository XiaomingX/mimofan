//! Externalized integration tests for `mimofan_state`.
//!
//! Relocated verbatim from `crates/state/src/lib.rs`. Only the
//! `#[cfg(test)] mod tests` wrapper and the `use super::*` import were replaced
//! with the public-API imports below; no test logic or assertion changed.

use mimofan_state::*;
use rusqlite::params;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_state_store(name: &str) -> StateStore {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "mimofan-state-{name}-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp state dir");
    StateStore::open(Some(dir.join("state.db"))).expect("open state store")
}

fn test_thread(id: &str) -> ThreadMetadata {
    ThreadMetadata {
        id: id.to_string(),
        rollout_path: None,
        preview: "test thread".to_string(),
        ephemeral: false,
        model_provider: "deepseek".to_string(),
        created_at: 10,
        updated_at: 10,
        status: ThreadStatus::Running,
        path: None,
        cwd: PathBuf::from("/tmp/mimofan"),
        cli_version: "0.0.0-test".to_string(),
        source: SessionSource::Interactive,
        name: None,
        sandbox_policy: None,
        approval_mode: None,
        archived: false,
        archived_at: None,
        git_sha: None,
        git_branch: None,
        git_origin_url: None,
        memory_mode: None,
        current_leaf_id: None,
    }
}

fn test_goal(thread_id: &str, objective: &str) -> ThreadGoalRecord {
    ThreadGoalRecord {
        thread_id: thread_id.to_string(),
        goal_id: "goal-1".to_string(),
        objective: objective.to_string(),
        status: ThreadGoalStatus::Active,
        token_budget: Some(123),
        tokens_used: 7,
        time_used_seconds: 11,
        continuation_count: 0,
        created_at: 100,
        updated_at: 101,
    }
}

#[test]
fn thread_goal_crud_round_trips_and_replaces() {
    let store = temp_state_store("thread-goal-crud");
    store
        .upsert_thread(&test_thread("thread-1"))
        .expect("upsert thread");

    let goal = test_goal("thread-1", "Ship v0.8.59");
    store.upsert_thread_goal(&goal).expect("upsert goal");
    assert_eq!(
        store
            .get_thread_goal("thread-1")
            .expect("read goal")
            .as_ref(),
        Some(&goal)
    );

    let mut replacement = test_goal("thread-1", "Ship v0.8.59 safely");
    replacement.goal_id = "goal-2".to_string();
    replacement.status = ThreadGoalStatus::BudgetLimited;
    replacement.token_budget = None;
    replacement.updated_at = 202;
    store
        .upsert_thread_goal(&replacement)
        .expect("replace goal");
    assert_eq!(
        store.get_thread_goal("thread-1").expect("read replacement"),
        Some(replacement)
    );

    assert!(store.delete_thread_goal("thread-1").expect("delete goal"));
    assert!(
        store
            .get_thread_goal("thread-1")
            .expect("read empty")
            .is_none()
    );
    assert!(!store.delete_thread_goal("thread-1").expect("delete empty"));
}

#[test]
fn thread_goal_requires_existing_thread() {
    let store = temp_state_store("thread-goal-missing-thread");
    let err = store
        .upsert_thread_goal(&test_goal("missing-thread", "nope"))
        .expect_err("goal without a thread should fail");
    assert!(err.to_string().contains("thread missing-thread not found"));
}

#[test]
fn delete_thread_cascades_child_rows() {
    let store = temp_state_store("thread-delete-cascade");
    store
        .upsert_thread(&test_thread("thread-1"))
        .expect("upsert thread");
    store
        .append_message("thread-1", "user", "hello", None)
        .expect("append message");
    store
        .save_checkpoint("thread-1", "checkpoint-1", &serde_json::json!({"ok": true}))
        .expect("save checkpoint");
    store
        .persist_dynamic_tools(
            "thread-1",
            &[DynamicToolRecord {
                position: 0,
                name: "test_tool".to_string(),
                description: Some("test".to_string()),
                input_schema: serde_json::json!({"type": "object"}),
            }],
        )
        .expect("persist dynamic tools");
    store
        .upsert_thread_goal(&test_goal("thread-1", "Ship v0.8.67"))
        .expect("upsert goal");

    store.delete_thread("thread-1").expect("delete thread");

    let conn = store.conn().expect("conn");
    for table in [
        "messages",
        "checkpoints",
        "thread_dynamic_tools",
        "thread_goals",
    ] {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE thread_id = ?1");
        let count: i64 = conn
            .query_row(&sql, params!["thread-1"], |row| row.get(0))
            .expect("count child rows");
        assert_eq!(count, 0, "{table} row survived thread deletion");
    }
}

#[test]
fn record_thread_goal_usage_accumulates_tokens_and_time() {
    let store = temp_state_store("thread-goal-usage");
    store
        .upsert_thread(&test_thread("thread-1"))
        .expect("upsert thread");

    // Mirror the runtime, which creates goals with zeroed accounting.
    let mut goal = test_goal("thread-1", "Ship the persistent goal loop");
    goal.tokens_used = 0;
    goal.time_used_seconds = 0;
    goal.updated_at = 100;
    store.upsert_thread_goal(&goal).expect("upsert goal");

    // First accrual lands the deltas and advances updated_at.
    let after_first = store
        .record_thread_goal_usage("thread-1", 250, 12, 150)
        .expect("record usage")
        .expect("goal exists");
    assert_eq!(after_first.tokens_used, 250);
    assert_eq!(after_first.time_used_seconds, 12);
    assert_eq!(after_first.updated_at, 150);
    // Identity fields are preserved across accrual.
    assert_eq!(after_first.goal_id, goal.goal_id);
    assert_eq!(after_first.objective, goal.objective);
    assert_eq!(after_first.status, goal.status);
    assert_eq!(after_first.token_budget, goal.token_budget);
    assert_eq!(after_first.created_at, goal.created_at);
    assert_eq!(after_first.continuation_count, 0);

    // Second accrual adds on top of the first (additive, not replacing).
    let after_second = store
        .record_thread_goal_usage("thread-1", 75, 8, 200)
        .expect("record usage")
        .expect("goal exists");
    assert_eq!(after_second.tokens_used, 325);
    assert_eq!(after_second.time_used_seconds, 20);
    assert_eq!(after_second.updated_at, 200);

    // A stale `now` must not move updated_at backwards.
    let after_stale = store
        .record_thread_goal_usage("thread-1", 5, 1, 1)
        .expect("record usage")
        .expect("goal exists");
    assert_eq!(after_stale.tokens_used, 330);
    assert_eq!(after_stale.time_used_seconds, 21);
    assert_eq!(after_stale.updated_at, 200);

    // Read back through the normal getter to confirm durability.
    let persisted = store
        .get_thread_goal("thread-1")
        .expect("read goal")
        .expect("goal exists");
    assert_eq!(persisted.tokens_used, 330);
    assert_eq!(persisted.time_used_seconds, 21);
}

#[test]
fn record_thread_goal_usage_returns_none_without_goal() {
    let store = temp_state_store("thread-goal-usage-missing");
    store
        .upsert_thread(&test_thread("thread-1"))
        .expect("upsert thread");
    // Thread exists but has no goal row yet: accrual is a no-op, not an error,
    // and must not create a goal.
    let result = store
        .record_thread_goal_usage("thread-1", 100, 5, 999)
        .expect("record usage on goalless thread");
    assert!(result.is_none());
    assert!(
        store
            .get_thread_goal("thread-1")
            .expect("read goal")
            .is_none()
    );
}

#[test]
fn record_thread_goal_continuation_accumulates_durably() {
    let store = temp_state_store("thread-goal-continuation");
    store
        .upsert_thread(&test_thread("thread-1"))
        .expect("upsert thread");

    let mut goal = test_goal("thread-1", "Keep working across turns");
    goal.updated_at = 100;
    store.upsert_thread_goal(&goal).expect("upsert goal");

    let after_first = store
        .record_thread_goal_continuation("thread-1", 120)
        .expect("record continuation")
        .expect("goal exists");
    assert_eq!(after_first.continuation_count, 1);
    assert_eq!(after_first.tokens_used, goal.tokens_used);
    assert_eq!(after_first.time_used_seconds, goal.time_used_seconds);
    assert_eq!(after_first.updated_at, 120);

    let after_second = store
        .record_thread_goal_continuation("thread-1", 110)
        .expect("record second continuation")
        .expect("goal exists");
    assert_eq!(after_second.continuation_count, 2);
    assert_eq!(after_second.updated_at, 120);

    let persisted = store
        .get_thread_goal("thread-1")
        .expect("read goal")
        .expect("goal exists");
    assert_eq!(persisted.continuation_count, 2);
}

// ── $MIMOFAN_HOME override tests ──────────────────────────────
//
// These touch a process-global env var, so they serialize against each
// other (and restore the prior value) to stay hermetic under parallel test
// runs — the same concern AGENTS.md flags for config_command_allow_shell_*.

static MIMOFAN_HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct MimofanHomeGuard {
    prior: Option<std::ffi::OsString>,
}
impl MimofanHomeGuard {
    fn set(value: &str) -> Self {
        let prior = std::env::var_os("MIMOFAN_HOME");
        // SAFETY: serialised by MIMOFAN_HOME_TEST_LOCK.
        unsafe { std::env::set_var("MIMOFAN_HOME", value) };
        Self { prior }
    }
}
impl Drop for MimofanHomeGuard {
    fn drop(&mut self) {
        // SAFETY: serialised by MIMOFAN_HOME_TEST_LOCK.
        unsafe {
            match &self.prior {
                Some(value) => std::env::set_var("MIMOFAN_HOME", value),
                None => std::env::remove_var("MIMOFAN_HOME"),
            }
        }
    }
}

#[test]
fn default_state_db_path_uses_mimofan_home_when_set() {
    let _lock = MIMOFAN_HOME_TEST_LOCK
        .lock()
        .expect("lock MIMOFAN_HOME test mutex");
    let dir = std::env::temp_dir().join(format!(
        "cw-home-state-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is valid since unix epoch")
            .as_nanos()
    ));
    let _g = MimofanHomeGuard::set(dir.to_str().expect("MIMOFAN_HOME dir is valid utf-8"));
    // Hard override: the DB is <MIMOFAN_HOME>/state.db, NOT
    // <MIMOFAN_HOME>/.mimofan/state.db, and the legacy ~/.mimofan
    // fallback is bypassed entirely.
    assert_eq!(default_state_db_path(), dir.join("state.db"));
}
