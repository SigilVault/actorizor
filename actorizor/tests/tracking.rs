//! `TrackingSupervisor` — registry, snapshot, abort-by-name/id/all, and
//! panic cleanup. The whole file is gated on the `tracking` feature.
//!
//! Run with: `cargo test -p actorizor --features tracking`

#![cfg(feature = "tracking")]

mod common;

mod registry {
    use actorizor::{TrackingSupervisor, actorize};

    use crate::common::{SETTLE, wait_until};

    #[derive(Debug, Default)]
    struct Worker {
        n: u64,
    }

    #[actorize]
    impl Worker {
        pub fn new() -> Self {
            Self { n: 0 }
        }

        pub fn ping(&self) -> u64 {
            self.n
        }
    }

    #[tokio::test]
    async fn snapshot_and_abort_by_name_track_lifecycle() {
        let sup = TrackingSupervisor::new();
        assert_eq!(sup.alive_count(), 0);

        let h1 = WorkerHandle::launch_with(Worker::new(), &sup);
        let h2 = WorkerHandle::launch_with(Worker::new(), &sup);
        assert_eq!(h1.ping().await.unwrap(), 0);
        assert_eq!(h2.ping().await.unwrap(), 0);

        assert_eq!(sup.alive_count(), 2);
        assert_eq!(sup.alive_count_by_name("Worker"), 2);

        let snap = sup.snapshot();
        assert_eq!(snap.len(), 2);
        assert!(snap.iter().all(|s| s.name == "Worker"));
        assert!(snap.iter().all(|s| s.alive));
        let mut ids: Vec<u64> = snap.iter().map(|s| s.id).collect();
        ids.sort();
        assert_eq!(ids[1] - ids[0], 1, "ids are monotonic per supervisor");

        assert!(sup.is_alive("Worker", snap[0].id));
        assert!(!sup.is_alive("Worker", 9999));

        let killed = sup.abort_by_name("Worker");
        assert_eq!(killed, 2);

        assert!(
            wait_until(|| sup.alive_count() == 0, SETTLE).await,
            "abort + watcher should clean up"
        );
        assert_eq!(sup.alive_count(), 0);
    }

    #[tokio::test]
    async fn abort_by_id_targets_one_instance() {
        let sup = TrackingSupervisor::new();
        let h1 = WorkerHandle::launch_with(Worker::new(), &sup);
        let h2 = WorkerHandle::launch_with(Worker::new(), &sup);
        let _ = h1.ping().await;
        let _ = h2.ping().await;

        let snap = sup.snapshot();
        assert_eq!(snap.len(), 2);
        let target_id = snap[0].id;

        assert!(sup.abort_by_id("Worker", target_id));

        assert!(
            wait_until(|| sup.alive_count() == 1, SETTLE).await,
            "expected 1 alive"
        );
        assert_eq!(sup.alive_count(), 1);
        assert!(!sup.is_alive("Worker", target_id));
    }

    #[tokio::test]
    async fn abort_all_kills_everything() {
        let sup = TrackingSupervisor::new();
        let h1 = WorkerHandle::launch_with(Worker::new(), &sup);
        let h2 = WorkerHandle::launch_with(Worker::new(), &sup);
        let _ = h1.ping().await;
        let _ = h2.ping().await;
        assert_eq!(sup.alive_count(), 2);

        assert_eq!(sup.abort_all(), 2);

        assert!(
            wait_until(|| sup.alive_count() == 0, SETTLE).await,
            "expected 0 alive"
        );
        assert_eq!(sup.alive_count(), 0);
    }
}

mod panic_cleanup {
    use actorizor::{TrackingSupervisor, actorize};

    use crate::common::{SETTLE, wait_until};

    #[derive(Debug, Default)]
    struct Panics;

    #[actorize]
    impl Panics {
        pub fn new() -> Self {
            Self
        }

        pub fn boom(&self) {
            panic!("intentional");
        }
    }

    #[tokio::test]
    async fn panic_in_actor_is_cleaned_up_from_registry() {
        let sup = TrackingSupervisor::new();
        let h = PanicsHandle::launch_with(Panics::new(), &sup);

        // Triggers the panic; the handle call errors (oneshot Sender
        // dropped) — expected, discard it.
        let _ = h.boom().await;

        assert!(
            wait_until(|| sup.alive_count() == 0, SETTLE).await,
            "watcher should remove the panicked actor from the registry"
        );
        assert_eq!(sup.alive_count(), 0);
    }
}
