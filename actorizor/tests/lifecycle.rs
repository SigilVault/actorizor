//! `abort()` (forceful) and `shutdown()` (cooperative) + the
//! `is_alive()` / `is_finished()` liveness queries.
//!
//! One actor per submodule — the macro emits a module-scoped `run_actor`,
//! so two `#[actorize]` blocks in the same module would collide.

mod abort {
    use actorizor::actorize;

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

        for _ in 0..50 {
            if h.is_finished() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(h.is_finished(), "abort() should mark the task finished");
        assert!(!h.is_alive());

        // Post-abort the handle can't get a response. Send-vs-recv error
        // depends on timing; both are acceptable.
        assert!(h.read().await.is_err(), "post-abort call must fail");
    }
}

mod shutdown {
    use actorizor::actorize;

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

        for _ in 0..50 {
            if h.is_finished() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(h.is_finished(), "shutdown() should make the loop exit");

        // Sender clones still exist (h, h2) but no one drains; next call
        // fails at recv.
        assert!(h2.read().await.is_err(), "post-shutdown call must fail");
    }
}
