//! `abort()` (forceful) and `shutdown()` (cooperative) + the
//! `is_alive()` / `is_finished()` liveness queries.
//!
//! One actor per submodule — the macro emits a module-scoped `run_actor`,
//! so two `#[actorize]` blocks in the same module would collide.

mod common;

mod abort {
    use actorizor::actorize;

    use crate::common::{SETTLE, wait_until};

    #[derive(Debug, Default)]
    struct AbortMe {
        n: u64,
    }

    #[actorize]
    impl AbortMe {
        pub fn new() -> Self {
            Self { n: 0 }
        }

        pub fn read(&self) -> u64 {
            self.n
        }
    }

    #[tokio::test]
    async fn abort_marks_handle_finished_and_blocks_further_calls() {
        let h = AbortMeHandle::new();
        assert!(h.is_alive());
        assert_eq!(h.read().await.unwrap(), 0);

        h.abort();

        assert!(
            wait_until(|| h.is_finished(), SETTLE).await,
            "abort() should mark the task finished"
        );
        assert!(!h.is_alive());

        // Post-abort the handle can't get a response. Send-vs-recv error
        // depends on timing; both are acceptable.
        assert!(h.read().await.is_err(), "post-abort call must fail");
    }
}

mod shutdown {
    use actorizor::actorize;

    use crate::common::{SETTLE, wait_until};

    #[derive(Debug, Default)]
    struct ShutdownMe {
        n: u64,
    }

    #[actorize]
    impl ShutdownMe {
        pub fn new() -> Self {
            Self { n: 0 }
        }

        pub fn read(&self) -> u64 {
            self.n
        }
    }

    #[tokio::test]
    async fn shutdown_terminates_loop_without_dropping_senders() {
        let h = ShutdownMeHandle::new();
        let h2 = h.clone();
        assert!(h.is_alive());
        assert_eq!(h2.read().await.unwrap(), 0);

        h.shutdown();

        assert!(
            wait_until(|| h.is_finished(), SETTLE).await,
            "shutdown() should make the loop exit"
        );

        // Sender clones still exist (h, h2) but no one drains; next call
        // fails at recv.
        assert!(h2.read().await.is_err(), "post-shutdown call must fail");
    }
}

/// Regression guard for the `notify_one()` (vs `notify_waiters()`) fix:
/// a `shutdown()` issued while the actor is busy inside `handle_msg` (i.e.
/// NOT currently awaiting `notified()`) must still terminate the actor.
/// With `notify_waiters()` no permit is stored, so a signal sent with no
/// waiter registered is lost; `notify_one()` stores a sticky permit the
/// next `notified()` consumes.
mod shutdown_race {
    use std::time::Duration;

    use actorizor::actorize;

    use crate::common::{SETTLE, wait_until};

    #[derive(Debug, Default)]
    struct Slow;

    #[actorize]
    impl Slow {
        pub fn new() -> Self {
            Self
        }

        // Holds the actor inside handle_msg for a beat, so a shutdown()
        // racing in lands while no `notified()` waiter is registered.
        pub async fn slow(&self) {
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    }

    #[tokio::test]
    async fn shutdown_during_handle_msg_is_not_lost() {
        let h = SlowHandle::new();
        let worker = h.clone();

        // Kick off a slow message WITHOUT awaiting it — the actor is now
        // parked inside `handle_msg`, not on the select!'s notified() arm.
        let call = tokio::spawn(async move { worker.slow().await });

        // Give the actor a moment to actually enter `slow()`.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Signal shutdown while it's busy. notify_one()'s permit must
        // survive until the loop comes back around to `notified()`.
        h.shutdown();

        let _ = call.await;

        assert!(
            wait_until(|| h.is_finished(), SETTLE).await,
            "shutdown() issued mid-handle_msg must still terminate the actor"
        );
    }
}
