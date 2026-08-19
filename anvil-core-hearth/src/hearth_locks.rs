//! Per-hearth write-lock primitive (spec Req 5).
//!
//! Replaces the engine's single process-wide `snapshot_lock` with one lock
//! instance per canonical hearth path, held for the process lifetime. All write
//! paths (begin / snapshot / complete) obtain their guard from a single shared
//! `HearthLocks` instance, keyed by the call's resolved canonical hearth.
//!
//! Concurrency contract:
//! - Operations on the SAME canonical hearth serialize (the per-hearth mutex).
//! - Operations on DIFFERENT canonical hearths never block each other (distinct
//!   per-hearth mutexes).
//!
//! Lock ordering (N2 — non-droppable): the meta-mutex guarding the lazy-init
//! map is acquired ONLY to look up / insert the per-hearth `Arc<Mutex<()>>` and
//! is RELEASED before the per-hearth lock is acquired. `lock_for` clones the
//! `Arc` out under the meta-guard, drops the meta-guard, then awaits the
//! per-hearth lock. There is no nested inversion: a parked holder of hearth X's
//! lock never holds the meta-mutex, so a fresh `lock_for(Y)` proceeds.
//!
//! Re-entrancy: the per-hearth mutex is a non-reentrant `tokio::sync::Mutex`.
//! Callers acquire it ONCE per logical transaction (e.g. `begin` acquires once
//! at the top and holds the owned guard across all event writes). Re-locking
//! the same hearth from within a held critical section self-deadlocks.
//!
//! Exactly one `Arc<Mutex<()>>` exists per canonical path for the process
//! lifetime: the map only ever inserts, never removes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex as TokioMutex, OwnedMutexGuard};

/// A registry of per-hearth write locks, keyed by canonical hearth path.
#[derive(Default)]
pub struct HearthLocks {
    /// Meta-mutex guarding the lazy-init map. A `std::sync::Mutex` is used (not
    /// a tokio mutex) because the critical section is a synchronous map lookup
    /// / insert with no `.await` inside — it is acquired and released entirely
    /// within `lock_for` before any per-hearth `.await`.
    map: StdMutex<HashMap<PathBuf, Arc<TokioMutex<()>>>>,
}

impl HearthLocks {
    pub fn new() -> Self {
        Self {
            map: StdMutex::new(HashMap::new()),
        }
    }

    /// Acquire the write lock for `canonical`, returning an owned guard that may
    /// be held across `.await` points and moved through a multi-write
    /// transaction (e.g. `begin`'s event loop).
    ///
    /// The meta-mutex is held only for the map lookup/insert and is dropped
    /// before the per-hearth lock is awaited (N2 — no nested inversion).
    pub async fn lock_for(&self, canonical: &Path) -> OwnedMutexGuard<()> {
        let per_hearth = self.arc_for(canonical);
        // Meta-guard already dropped (end of `arc_for`); now await the
        // per-hearth lock with no other lock held.
        per_hearth.lock_owned().await
    }

    /// Look up (or lazily insert) the single `Arc<Mutex<()>>` for `canonical`.
    /// The meta-guard is dropped at the end of this function — before the
    /// caller awaits the per-hearth lock.
    fn arc_for(&self, canonical: &Path) -> Arc<TokioMutex<()>> {
        // Poison-resilient: a panic in another thread WHILE this meta-mutex was
        // held must not wedge every subsequent write path. The guarded map
        // (path -> Arc<Mutex>) has no cross-field invariant a partial write could
        // corrupt, so recovering the poisoned guard is safe.
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(canonical.to_path_buf())
            .or_insert_with(|| Arc::new(TokioMutex::new(())))
            .clone()
    }
}
