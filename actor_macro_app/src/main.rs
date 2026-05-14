// #![feature(trace_macros)]
// trace_macros!(true);

use actorizor::actorize;

#[cfg(test)]
mod tests {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    struct TestActor {
        value: u64,
    }

    #[actorize]
    impl TestActor {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        pub fn new_with(v: u64) -> Self {
            Self { value: v }
        }

        pub fn get_value(&self) -> u64 {
            self.value
        }

        pub fn set_value(&mut self, v: u64) {
            self.value = v;
        }

        pub fn add(&mut self, a: u64, b: u64) -> u64 {
            self.value += a + b;
            self.value
        }

        pub async fn async_get(&self) -> u64 {
            self.value
        }
    }

    #[tokio::test]
    async fn test_default_constructor() {
        let handle = TestActorHandle::new();
        let val = handle.get_value().await.unwrap();
        assert_eq!(val, 0);
    }

    #[tokio::test]
    async fn test_parameterized_constructor() {
        let handle = TestActorHandle::new_with(42);
        let val = handle.get_value().await.unwrap();
        assert_eq!(val, 42);
    }

    #[tokio::test]
    async fn test_sync_mutation() {
        let handle = TestActorHandle::new();
        handle.set_value(100).await.unwrap();
        let val = handle.get_value().await.unwrap();
        assert_eq!(val, 100);
    }

    #[tokio::test]
    async fn test_multi_arg_method() {
        let handle = TestActorHandle::new();
        let result = handle.add(3, 7).await.unwrap();
        assert_eq!(result, 10);
    }

    #[tokio::test]
    async fn test_async_method() {
        let handle = TestActorHandle::new_with(99);
        let val = handle.async_get().await.unwrap();
        assert_eq!(val, 99);
    }

    #[tokio::test]
    async fn test_cloned_handles_share_state() {
        let handle1 = TestActorHandle::new_with(5);
        let handle2 = handle1.clone();
        handle1.set_value(50).await.unwrap();
        let val = handle2.get_value().await.unwrap();
        assert_eq!(val, 50);
    }
}

// ---------------------------------------------------------------------------
// launch_with + TokioSpawn coverage
// ---------------------------------------------------------------------------

#[cfg(test)]
mod launch_with_tokio_spawn {
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

// ---------------------------------------------------------------------------
// launch_with + custom user-supplied Supervisor
// ---------------------------------------------------------------------------

#[cfg(test)]
mod launch_with_custom_supervisor {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use actorizor::{Supervisor, actorize};
    use tokio::task::AbortHandle;

    /// Captures every spawn through it and records the actor name.
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

        // The actor's handle responds, proving the supervisor really did
        // drive the future.
        assert_eq!(h.ping().await.unwrap(), 0);

        assert!(sup.count.load(Ordering::SeqCst) > before);
        let name = *sup.last_name.lock().await;
        assert_eq!(name, Some("Tracked"));
    }
}

// ---------------------------------------------------------------------------
// abort() forcefully kills the task
// ---------------------------------------------------------------------------

#[cfg(test)]
mod abort_kills_task {
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

        // Give tokio a tick to mark the task as finished.
        for _ in 0..50 {
            if h.is_finished() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(h.is_finished(), "abort() should mark the task finished");
        assert!(!h.is_alive());

        // Subsequent handle methods can't get a response. The exact
        // variant depends on whether send happens before or after the
        // receiver is dropped; both Send and Recv errors are acceptable.
        let r = h.read().await;
        assert!(r.is_err(), "post-abort handle call must fail");
    }
}

// ---------------------------------------------------------------------------
// shutdown() lets the loop exit cooperatively
// ---------------------------------------------------------------------------

#[cfg(test)]
mod shutdown_exits_cleanly {
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

        // Wait for the loop to exit. Notify wakeup + a yield should be
        // enough but allow a few ms for cross-thread coordination.
        for _ in 0..50 {
            if h.is_finished() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(h.is_finished(), "shutdown() should make the loop exit");

        // Sender clones still exist (we hold h and h2) but no one is
        // draining anymore; the next call should fail at recv.
        let r = h2.read().await;
        assert!(r.is_err(), "post-shutdown handle call must fail");
    }
}

// ---------------------------------------------------------------------------
// TrackingSupervisor (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "tracking"))]
mod tracking_supervisor {
    use actorizor::{TrackingSupervisor, actorize};

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

        // Force the handles to communicate so the actor tasks are
        // definitely registered before we look.
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

        // is_alive against a known id holds, against a bogus one fails.
        assert!(sup.is_alive("Worker", snap[0].id));
        assert!(!sup.is_alive("Worker", 9999));

        // abort_by_name kills every Worker instance.
        let killed = sup.abort_by_name("Worker");
        assert_eq!(killed, 2);

        // Watcher tasks need a tick to observe abort and prune.
        for _ in 0..50 {
            if sup.alive_count() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sup.alive_count(), 0, "abort + watcher should clean up");
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

        let ok = sup.abort_by_id("Worker", target_id);
        assert!(ok);

        for _ in 0..50 {
            if sup.alive_count() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
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

        let killed = sup.abort_all();
        assert_eq!(killed, 2);

        for _ in 0..50 {
            if sup.alive_count() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(sup.alive_count(), 0);
    }

}

#[cfg(all(test, feature = "tracking"))]
mod tracking_supervisor_panics {
    use actorizor::{TrackingSupervisor, actorize};

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

        // Trigger the panic. The handle method will return Err because the
        // oneshot Sender held inside the killed Msg gets dropped.
        let _ = h.boom().await;

        for _ in 0..50 {
            if sup.alive_count() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(
            sup.alive_count(),
            0,
            "watcher should remove the panicked actor from the registry"
        );
    }
}

#[derive(Debug, Default)]
#[allow(dead_code)]
struct Bar {
    number: u64,
}

#[actorize(20)]
#[allow(dead_code)]
impl Bar {
    pub fn do_thing(&self, something: u64, otherwise: String) -> u64 {
        println!("do_thing {something} {otherwise}");
        42
    }
    pub async fn other(&self) {
        println!("other")
    }
    fn blah() {}

    pub async fn constr_1(_num: i32) -> Self {
        panic!()
    }

    pub fn constr_2() -> Bar {
        panic!()
    }

    pub fn new() -> Self {
        Self { number: 123 }
    }

    pub fn new_2(a: u64, b: u64) -> Self {
        Self { number: a * b }
    }

    pub async fn new_3(a: u64, b: u64) -> Self {
        Self { number: a * b }
    }

    pub fn new_4(a: u64) -> Self {
        Self { number: a }
    }

    pub fn do_a() -> u64 {
        42
    }

    pub fn do_b(a: u64) -> u64 {
        a
    }
    pub fn do_c(a: u64, b: u64) -> u64 {
        a + b
    }
}

#[tokio::main]
async fn main() {
    let foo_handle = BarHandle::new();
    let r = foo_handle.do_thing(123, "Str".to_owned()).await.unwrap();
    println!("r: {r}");
}
