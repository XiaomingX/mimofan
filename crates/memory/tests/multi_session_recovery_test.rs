//! #860 — Multi-session memory recovery acceptance sample.
//!
//! Proves that memories written in one session survive a full store restart and
//! are byte/semantically equal when re-opened from the same persistence backend
//! (sqlite + sled + hnsw), which is the real backend the `memory` crate uses.
//!
//! Steps:
//! 1. Open a `VectorStore` at a unique temp dir (session 1).
//! 2. Write several observations tagged with a session id.
//! 3. Simulate a restart: drop the handle, re-open from the same path (session 2).
//! 4. Assert every session-1 memory is present and equal after the restart.

use mimofan_memory::vector::VectorStore;
use mimofan_memory::Observation;
use tempfile::TempDir;

const DIM: usize = 8;

/// Build a deterministic observation with the given content and session id.
fn session_observation(n: usize, session_id: &str) -> Observation {
    Observation::with_session(
        "mimofan".to_string(),
        "project",
        format!("session-memory-{n}: the build target lives at /tmp/mimofan-out"),
        session_id.to_string(),
    )
}

#[test]
fn test_memory_survives_session_restart() {
    let temp = TempDir::new().expect("create temp dir");
    let path = temp.path().to_path_buf();
    let session_id = "session-1";

    // --- Session 1: write memories ---
    let store1 = VectorStore::open(&path, DIM).expect("open store (session 1)");
    let mut written_ids = Vec::new();
    for n in 0..4 {
        let obs = session_observation(n, session_id);
        let embedding = vec![n as f32; DIM];
        let id = store1
            .store_observation(&obs, &embedding)
            .expect("store observation in session 1");
        assert!(id > 0, "stored observation must get a positive id");
        written_ids.push((id, n));
    }
    assert_eq!(
        store1.count().expect("count after session 1"),
        written_ids.len()
    );

    // Drop the handle to simulate a full process/session restart.
    drop(store1);

    // --- Session 2: re-open from the SAME backend ---
    let store2 = VectorStore::open(&path, DIM).expect("re-open store (session 2)");
    assert_eq!(
        store2.count().expect("count after restart"),
        written_ids.len(),
        "all session-1 memories must persist across the restart"
    );

    for (id, n) in &written_ids {
        let loaded = store2
            .load_observation(*id)
            .expect("load observation in session 2")
            .unwrap_or_else(|| panic!("memory {id} missing after restart"));
        // Byte/semantic equality of the durable fields.
        assert_eq!(loaded.content, session_observation(*n, session_id).content);
        assert_eq!(loaded.kind, "project");
        assert_eq!(loaded.project.as_deref(), Some("mimofan"));
        assert_eq!(loaded.session_id, session_id, "session tag must survive restart");
        assert!(loaded.id > 0);
    }
}

#[test]
fn test_memory_recovery_is_idempotent_across_multiple_restarts() {
    let temp = TempDir::new().expect("create temp dir");
    let path = temp.path().to_path_buf();

    // Session 1: write.
    let store1 = VectorStore::open(&path, DIM).expect("open store 1");
    let obs = session_observation(0, "alpha");
    let id = store1
        .store_observation(&obs, &vec![1.0; DIM])
        .expect("store observation");
    drop(store1);

    // Session 2: re-open, read, drop.
    {
        let store2 = VectorStore::open(&path, DIM).expect("open store 2");
        assert_eq!(store2.count().expect("count 2"), 1);
        let loaded = store2
            .load_observation(id)
            .expect("load 2")
            .expect("present 2");
        assert_eq!(loaded.content, obs.content);
        assert_eq!(loaded.session_id, "alpha");
    }

    // Session 3: re-open again — recovery must be stable, not accumulate dupes.
    let store3 = VectorStore::open(&path, DIM).expect("open store 3");
    assert_eq!(store3.count().expect("count 3"), 1, "no duplicate on reopen");
    let loaded = store3
        .load_observation(id)
        .expect("load 3")
        .expect("present 3");
    assert_eq!(loaded.content, obs.content);
}
