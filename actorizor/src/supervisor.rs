//! The `Supervisor` trait + built-in implementations.

use std::future::Future;

use tokio::task::AbortHandle;

/// Owns the "where does this actor task run?" decision. Implementors decide
/// what executor to schedule the future on, whether to watch the resulting
/// `JoinHandle`, and what side-effects to fire (logs, metrics, registry
/// inserts) on each spawn.
///
/// The minimum contract: drive `fut` on a tokio runtime, return promptly,
/// and yield back an `AbortHandle` that the caller can use to terminate the
/// task forcefully. The actor's generated `Handle` stashes the AbortHandle
/// so that `handle.abort()` works regardless of which supervisor was used.
///
/// The trait is intentionally not object-safe (the `spawn` method is
/// generic over the future type). Use `&MySupervisor` directly; if you need
/// to box, wrap it in your own enum or use a concrete supervisor stored by
/// value.
pub trait Supervisor: Send + Sync + 'static {
    /// Schedule `fut` and return its `AbortHandle`. `name` is the actor's
    /// type name (via `stringify!`), useful for logs and metric labels.
    fn spawn<F>(&self, name: &'static str, fut: F) -> AbortHandle
    where
        F: Future<Output = ()> + Send + 'static;
}

/// The trivial `Supervisor`: delegates to `tokio::task::spawn` and discards
/// the actor name. Behaves identically to the implicit spawn used by the
/// generated `Handle::new()` constructors when `launch_with` isn't called.
///
/// Suitable for tests, examples, and any place you want an actor without
/// the bookkeeping that [`TrackingSupervisor`] (behind the `tracking`
/// feature) provides.
///
/// [`TrackingSupervisor`]: crate::TrackingSupervisor
pub struct TokioSpawn;

impl Supervisor for TokioSpawn {
    fn spawn<F>(&self, _name: &'static str, fut: F) -> AbortHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::task::spawn(fut).abort_handle()
    }
}

// ---------------------------------------------------------------------------
// TrackingSupervisor (feature = "tracking")
// ---------------------------------------------------------------------------

#[cfg(feature = "tracking")]
mod tracking {
    use std::collections::HashMap;
    use std::future::Future;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use tokio::task::AbortHandle;

    use super::Supervisor;

    /// A `Supervisor` that holds AbortHandles in a name-keyed registry,
    /// watches each spawned task via a per-spawn watcher future, emits a
    /// `tracing` event when the task ends (clean exit, panic, or
    /// abort-induced cancellation), and removes the entry from the
    /// registry on exit so memory stays bounded.
    ///
    /// Identity is `(actor_name, monotonic_u64_id)`. Multiple instances of
    /// the same actor type get distinct ids; controllers can target one
    /// specific instance via `abort_by_id` or all of them via
    /// `abort_by_name`.
    ///
    /// Owned, not static. Construct one in `main` (or per test) and pass
    /// `&supervisor` into the generated `Handle::launch_with` method.
    pub struct TrackingSupervisor {
        inner: Arc<Inner>,
    }

    struct Inner {
        next_id: AtomicU64,
        actors: Mutex<HashMap<&'static str, Vec<Tracked>>>,
    }

    struct Tracked {
        id: u64,
        abort: AbortHandle,
        spawned_at: Instant,
    }

    /// A read-only view of a tracked actor at one moment in time. Returned
    /// by [`TrackingSupervisor::snapshot`].
    #[derive(Debug, Clone)]
    pub struct ActorSnapshot {
        pub name: &'static str,
        pub id: u64,
        pub spawned_at: Instant,
        /// `true` if the task's `AbortHandle` reports unfinished. Every
        /// query (including `snapshot`) prunes finished entries before
        /// answering, so this is effectively always `true` in fresh
        /// snapshot results — the field is kept for callers that hold a
        /// snapshot over time and want to recheck without going back to the
        /// supervisor.
        pub alive: bool,
    }

    /// Drop every entry whose task has finished, and any name whose list
    /// is now empty. Called under the actors lock before any liveness
    /// query so results never report a finished-but-not-yet-watcher-pruned
    /// task as alive (the per-spawn watcher also prunes, but it may not
    /// have been scheduled yet).
    fn prune_locked(map: &mut HashMap<&'static str, Vec<Tracked>>) {
        map.retain(|_, v| {
            v.retain(|t| !t.abort.is_finished());
            !v.is_empty()
        });
    }

    impl TrackingSupervisor {
        pub fn new() -> Self {
            Self {
                inner: Arc::new(Inner {
                    next_id: AtomicU64::new(0),
                    actors: Mutex::new(HashMap::new()),
                }),
            }
        }

        /// Total number of currently-alive actors across all names.
        /// Finished tasks are pruned before counting.
        pub fn alive_count(&self) -> usize {
            let mut guard = self.inner.actors.lock().unwrap();
            prune_locked(&mut guard);
            guard.values().map(|v| v.len()).sum()
        }

        /// Number of currently-alive actors with the given name.
        /// Finished tasks are pruned before counting.
        pub fn alive_count_by_name(&self, name: &str) -> usize {
            let mut guard = self.inner.actors.lock().unwrap();
            prune_locked(&mut guard);
            guard.get(name).map(|v| v.len()).unwrap_or(0)
        }

        /// Whether an actor with the given (name, id) is still alive.
        /// Finished tasks are pruned first, so a task that has completed
        /// but whose watcher hasn't run yet correctly reports `false`.
        pub fn is_alive(&self, name: &str, id: u64) -> bool {
            let mut guard = self.inner.actors.lock().unwrap();
            prune_locked(&mut guard);
            guard
                .get(name)
                .map(|v| v.iter().any(|t| t.id == id))
                .unwrap_or(false)
        }

        /// Snapshot of every currently-alive actor. Finished tasks are
        /// pruned before the snapshot is built, so every returned entry was
        /// alive at the moment of the call.
        pub fn snapshot(&self) -> Vec<ActorSnapshot> {
            let mut guard = self.inner.actors.lock().unwrap();
            prune_locked(&mut guard);
            let mut out = Vec::new();
            for (name, instances) in guard.iter() {
                for t in instances {
                    out.push(ActorSnapshot {
                        name,
                        id: t.id,
                        spawned_at: t.spawned_at,
                        alive: !t.abort.is_finished(),
                    });
                }
            }
            out
        }

        /// Abort every alive instance of the named actor. Returns the
        /// number of `AbortHandle::abort()` calls made (which is the count
        /// of tracked instances at the moment of the call — tasks may
        /// still be racing to exit).
        pub fn abort_by_name(&self, name: &str) -> usize {
            let guard = self.inner.actors.lock().unwrap();
            match guard.get(name) {
                Some(v) => {
                    for t in v {
                        t.abort.abort();
                    }
                    v.len()
                }
                None => 0,
            }
        }

        /// Abort a single instance by (name, id). Returns `true` if a
        /// matching tracked entry was found, `false` otherwise.
        pub fn abort_by_id(&self, name: &str, id: u64) -> bool {
            let guard = self.inner.actors.lock().unwrap();
            match guard.get(name) {
                Some(v) => match v.iter().find(|t| t.id == id) {
                    Some(t) => {
                        t.abort.abort();
                        true
                    }
                    None => false,
                },
                None => false,
            }
        }

        /// Abort every tracked actor. Useful for emergency shutdown.
        /// Returns the total number of `abort()` calls made.
        pub fn abort_all(&self) -> usize {
            let guard = self.inner.actors.lock().unwrap();
            let mut n = 0;
            for v in guard.values() {
                for t in v {
                    t.abort.abort();
                    n += 1;
                }
            }
            n
        }

    }

    impl Default for TrackingSupervisor {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Supervisor for TrackingSupervisor {
        fn spawn<F>(&self, name: &'static str, fut: F) -> AbortHandle
        where
            F: Future<Output = ()> + Send + 'static,
        {
            let jh = tokio::task::spawn(fut);
            let abort = jh.abort_handle();
            let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
            self.inner
                .actors
                .lock()
                .unwrap()
                .entry(name)
                .or_default()
                .push(Tracked {
                    id,
                    abort: abort.clone(),
                    spawned_at: Instant::now(),
                });

            let inner = self.inner.clone();
            tokio::spawn(async move {
                let result = jh.await;
                match result {
                    Ok(()) => {
                        tracing::info!(actor = name, id, "actor task exited cleanly");
                    }
                    Err(e) if e.is_cancelled() => {
                        tracing::info!(actor = name, id, "actor task aborted");
                    }
                    Err(e) if e.is_panic() => {
                        tracing::error!(actor = name, id, "actor task panicked: {e}");
                    }
                    Err(e) => {
                        tracing::warn!(actor = name, id, "actor task join error: {e}");
                    }
                }
                // Reuse the same prune the queries use — drops this
                // just-finished task (and any other finished ones) and
                // empties dead name buckets.
                let mut guard = inner.actors.lock().unwrap();
                prune_locked(&mut guard);
            });

            abort
        }
    }
}

#[cfg(feature = "tracking")]
pub use tracking::{ActorSnapshot, TrackingSupervisor};
