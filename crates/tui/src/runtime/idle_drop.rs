//! Idle-based lazy unload guard.
//!
//! Wraps a handle `T` and records the last time it was accessed. A periodic
//! [`IdleGuarded::maybe_unload`] call drops the inner handle once it has been
//! idle longer than a caller-supplied threshold, freeing the underlying
//! resource (an LSP server process, an embedding session, …). The next
//! [`IdleGuarded::get`] transparently rebuilds the handle via a user-supplied
//! reconfiguration closure, so callers always observe a live handle.
//!
//! # Safety wrt. in-flight requests
//!
//! `IdleGuarded` only owns the *stored* handle. Callers that clone the handle
//! out before driving I/O (e.g. `Arc::clone` the transport, then release the
//! guard's lock) keep the resource alive for as long as the in-flight request
//! needs it — `maybe_unload` only drops the *stored* copy, never a borrowed
//! one. This is exactly how [`crate::lsp::LspManager`] uses it: the transport
//! is `Arc<dyn LspTransport>` and cloned into the request before unlocking.
//!
//! # Why a rebuild closure instead of `T: Default`
//!
//! Real handles (LSP transport, API embedder) do not implement `Default` and
//! need configuration/network setup. The rebuild entry point is therefore a
//! closure `FnMut() -> T` supplied at unload-time, which keeps this module free
//! of any domain knowledge and free of new dependencies (only `std::time`).

use std::time::{Duration, Instant};

/// A handle guarded by idle-time tracking.
///
/// The handle is `Some` while it is considered live (recently used or just
/// built) and `None` only transiently: `get` always rebuilds it on demand, so
/// external observers should treat `None` as "not currently cached, will be
/// rebuilt on next access".
pub struct IdleGuarded<T> {
    inner: Option<T>,
    last_used: Instant,
    /// Total number of times the inner handle was rebuilt after an unload.
    rebuilds: u64,
    /// Total number of times the inner handle was dropped for idleness.
    unloads: u64,
}

impl<T, E> IdleGuarded<T> {
    /// Wrap an already-live handle. Marks it as used right now.
    #[must_use]
    pub fn new(handle: T) -> Self {
        Self {
            inner: Some(handle),
            last_used: Instant::now(),
            rebuilds: 0,
            unloads: 0,
        }
    }

    /// Time elapsed since the handle was last accessed.
    #[must_use]
    pub fn idle_for(&self) -> Duration {
        self.last_used.elapsed()
    }

    /// Record an access *without* rebuilding. Used by async callers that need
    /// to update `last_used` but cannot supply a synchronous rebuild closure
    /// (e.g. when re-spawning the handle itself requires `.await`). No-op when
    /// the handle is currently unloaded — the caller is expected to rebuild it
    /// out-of-band and re-insert via [`IdleGuarded::new`].
    pub fn touch(&mut self) {
        if self.inner.is_some() {
            self.last_used = Instant::now();
        }
    }

    /// Clone the live inner handle (requires `T: Clone`), recording access.
    /// Returns `None` when the handle is currently unloaded. Prefer this over
    /// [`IdleGuarded::get`] when the rebuild step is asynchronous and cannot be
    /// expressed as a sync closure.
    pub fn clone_inner(&mut self) -> Option<T>
    where
        T: Clone,
    {
        if self.inner.is_some() {
            self.last_used = Instant::now();
            self.inner.clone()
        } else {
            None
        }
    }

    /// Whether the inner handle is currently cached (live).
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.inner.is_some()
    }

    /// Total rebuilds performed after idle unloads.
    #[must_use]
    pub fn rebuild_count(&self) -> u64 {
        self.rebuilds
    }

    /// Total idle unloads performed.
    #[must_use]
    pub fn unload_count(&self) -> u64 {
        self.unloads
    }

    /// Access the live handle, recording use now.
    ///
    /// If the handle was previously unloaded, `rebuild` is invoked to recreate
    /// it before returning a mutable reference. Panics if `rebuild` returns
    /// `None` — a handle that cannot be rebuilt is a programming error in the
    /// caller's wiring, not a recoverable runtime condition.
    pub fn get(&mut self, rebuild: impl FnOnce() -> T) -> &mut T {
        if self.inner.is_none() {
            self.inner = Some(rebuild());
            self.rebuilds += 1;
        }
        self.last_used = Instant::now();
        self.inner.as_mut().expect("handle was just rebuilt")
    }

    /// Same as [`IdleGuarded::get`] but with a fallible rebuild.
    ///
    /// Returns `Err` (with the original error) when the handle is unloaded and
    /// `rebuild` fails, so callers can degrade gracefully instead of panicking.
    pub fn get_or_try(
        &mut self,
        rebuild: impl FnOnce() -> Result<T, E>,
    ) -> Result<&mut T, E> {
        if self.inner.is_none() {
            self.inner = Some(rebuild()?);
            self.rebuilds += 1;
        }
        self.last_used = Instant::now();
        Ok(self.inner.as_mut().expect("handle was just rebuilt"))
    }

    /// Drop the inner handle if it has been idle for longer than `idle_timeout`.
    ///
    /// Does nothing while the handle is still fresh. Returns `true` when an
    /// unload actually happened. In-flight clones (held outside this guard)
    /// keep their own copy alive; only the cached copy is released.
    pub fn maybe_unload(&mut self, idle_timeout: Duration) -> bool {
        if self.inner.is_some() && self.last_used.elapsed() >= idle_timeout {
            self.inner = None;
            self.unloads += 1;
            true
        } else {
            false
        }
    }

    /// Manually drop the inner handle regardless of idle time. Mostly useful
    /// for tests and explicit teardown. Returns `true` if something was dropped.
    pub fn force_unload(&mut self) -> bool {
        if self.inner.is_some() {
            self.inner = None;
            self.unloads += 1;
            true
        } else {
            false
        }
    }
}

impl<T> Default for IdleGuarded<T> {
    /// An empty (unloaded) guard. The first `get` will rebuild the handle.
    fn default() -> Self {
        Self {
            inner: None,
            last_used: Instant::now(),
            rebuilds: 0,
            unloads: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A mock handle that counts how many times it is dropped, so tests can
    /// prove the idle-unload path actually releases the resource.
    struct MockHandle {
        id: u64,
        drops: Arc<AtomicU64>,
    }

    impl MockHandle {
        fn new(drops: Arc<AtomicU64>) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            Self { id, drops }
        }
    }

    impl Drop for MockHandle {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn unload_after_idle_timeout_drops_handle() {
        let drops = Arc::new(AtomicU64::new(0));
        let mut guarded: IdleGuarded<MockHandle> = IdleGuarded::new(MockHandle::new(drops.clone()));

        // Mark it as used long ago so it is now idle.
        guarded.last_used = Instant::now() - Duration::from_secs(100);

        let unloaded = guarded.maybe_unload(Duration::from_secs(10));
        assert!(unloaded, "handle should have been unloaded");
        assert!(!guarded.is_loaded());
        assert_eq!(drops.load(Ordering::Relaxed), 1, "dropped exactly once");
        assert_eq!(guarded.unload_count(), 1);
    }

    #[test]
    fn fresh_handle_is_not_unloaded() {
        let drops = Arc::new(AtomicU64::new(0));
        let mut guarded: IdleGuarded<MockHandle> = IdleGuarded::new(MockHandle::new(drops.clone()));

        // Just touched; well within the timeout.
        let unloaded = guarded.maybe_unload(Duration::from_secs(10));
        assert!(!unloaded, "freshly used handle must stay loaded");
        assert!(guarded.is_loaded());
        assert_eq!(drops.load(Ordering::Relaxed), 0, "nothing dropped");
    }

    #[test]
    fn active_access_resets_idle_timer() {
        let drops = Arc::new(AtomicU64::new(0));
        let mut guarded: IdleGuarded<MockHandle> = IdleGuarded::new(MockHandle::new(drops.clone()));

        // Simulate idleness.
        guarded.last_used = Instant::now() - Duration::from_secs(100);
        // A get() touches it, resetting the timer.
        let _h = guarded.get(MockHandle::new);
        assert!(guarded.idle_for() < Duration::from_secs(1));

        // Now an unload attempt within a long timeout must NOT drop.
        let unloaded = guarded.maybe_unload(Duration::from_secs(50));
        assert!(!unloaded, "recent access resets the idle timer");
        assert_eq!(drops.load(Ordering::Relaxed), 0, "not dropped after reset");
    }

    #[test]
    fn get_lazily_rebuilds_after_unload() {
        let drops = Arc::new(AtomicU64::new(0));
        let mut guarded: IdleGuarded<MockHandle> = IdleGuarded::new(MockHandle::new(drops.clone()));

        // Force unload, then access.
        assert!(guarded.force_unload());
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert!(!guarded.is_loaded());

        // First get() rebuilds.
        let h1 = guarded.get(MockHandle::new);
        assert!(guarded.is_loaded());
        assert_eq!(guarded.rebuild_count(), 1);
        let _id1 = h1.id;

        // Subsequent get() reuses the rebuilt handle (no extra drop, no extra rebuild).
        let h2 = guarded.get(MockHandle::new);
        assert_eq!(h1.id, h2.id, "same cached handle returned");
        assert_eq!(guarded.rebuild_count(), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 1, "old handle not re-dropped");
    }

    #[test]
    fn timeout_boundary_is_inclusive() {
        let drops = Arc::new(AtomicU64::new(0));
        let mut guarded: IdleGuarded<MockHandle> = IdleGuarded::new(MockHandle::new(drops.clone()));
        // Exactly at the threshold: elapsed() >= timeout should unload.
        guarded.last_used = Instant::now() - Duration::from_millis(10);
        let unloaded = guarded.maybe_unload(Duration::from_millis(10));
        assert!(unloaded, "boundary (elapsed == timeout) must unload");
    }

    #[test]
    fn default_guard_is_unloaded_and_rebuilds_on_first_get() {
        let drops = Arc::new(AtomicU64::new(0));
        let mut guarded: IdleGuarded<MockHandle> = IdleGuarded::default();
        assert!(!guarded.is_loaded());
        let _h = guarded.get(MockHandle::new);
        assert!(guarded.is_loaded());
        assert_eq!(guarded.rebuild_count(), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0, "no drop yet");
    }
}
