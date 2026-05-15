//! `launch_with` against the built-in `TokioSpawn` and against a
//! user-supplied `Supervisor` impl.

mod with_tokio_spawn {
    use actorizor::{TokioSpawn, actorize};

    #[derive(Debug, Default)]
    struct Counter {
        value: u64,
    }

    #[actorize]
    impl Counter {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        pub fn bump(&mut self) -> u64 {
            self.value += 1;
            self.value
        }
    }

    #[tokio::test]
    async fn launches_under_tokio_spawn_supervisor() {
        let h = CounterHandle::launch_with(Counter::new(), &TokioSpawn);
        assert_eq!(h.bump().await.unwrap(), 1);
        assert_eq!(h.bump().await.unwrap(), 2);
        assert!(h.is_alive());
    }
}

mod with_custom_supervisor {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use actorizor::{Supervisor, actorize};
    use tokio::task::AbortHandle;

    /// Records every spawn that goes through it + the actor's name.
    struct CountingSupervisor {
        count: Arc<AtomicUsize>,
        last_name: Arc<tokio::sync::Mutex<Option<&'static str>>>,
    }

    impl CountingSupervisor {
        fn new() -> Self {
            Self {
                count: Arc::new(AtomicUsize::new(0)),
                last_name: Arc::new(tokio::sync::Mutex::new(None)),
            }
        }
    }

    impl Supervisor for CountingSupervisor {
        fn spawn<F>(&self, name: &'static str, fut: F) -> AbortHandle
        where
            F: Future<Output = ()> + Send + 'static,
        {
            self.count.fetch_add(1, Ordering::SeqCst);
            let last_name = self.last_name.clone();
            let jh = tokio::task::spawn(async move {
                *last_name.lock().await = Some(name);
                fut.await;
            });
            jh.abort_handle()
        }
    }

    #[derive(Debug, Default)]
    struct Tracked {
        n: u64,
    }

    #[actorize]
    impl Tracked {
        pub fn new() -> Self {
            Self { n: 0 }
        }

        pub fn ping(&self) -> u64 {
            self.n
        }
    }

    #[tokio::test]
    async fn supervisor_sees_actor_name_and_drives_future() {
        let sup = CountingSupervisor::new();
        let before = sup.count.load(Ordering::SeqCst);

        let h = TrackedHandle::launch_with(Tracked::new(), &sup);

        // Handle responds → the supervisor really did drive the future.
        assert_eq!(h.ping().await.unwrap(), 0);

        assert!(sup.count.load(Ordering::SeqCst) > before);
        let name = *sup.last_name.lock().await;
        assert_eq!(name, Some("Tracked"));
    }
}
