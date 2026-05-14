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
// spawn_with — bring-your-own spawn function (0.2.0+)
// ---------------------------------------------------------------------------
//
// Each test lives in its own submodule because the macro generates a
// module-scoped `run_actor` (and now `__actorizor_default_spawn`) — see
// `actorizor/src/actorizor.rs`. Putting multiple actors in one module
// triggers duplicate-symbol errors. Predates this PR; documented as a known
// limitation in CLAUDE.md.

#[cfg(test)]
mod spawn_with_shared {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::sync::OnceCell;

    /// Captures which actor names get spawned through this supervisor. Tests
    /// inspect it to confirm the macro routed through `spawn_with` rather
    /// than the default shim.
    pub(crate) static SPAWN_NAMES: OnceCell<Arc<tokio::sync::Mutex<Vec<&'static str>>>> =
        OnceCell::const_new();
    pub(crate) static SPAWN_COUNT: AtomicUsize = AtomicUsize::new(0);

    pub(crate) async fn spawn_log() -> Arc<tokio::sync::Mutex<Vec<&'static str>>> {
        SPAWN_NAMES
            .get_or_init(|| async { Arc::new(tokio::sync::Mutex::new(Vec::new())) })
            .await
            .clone()
    }

    /// Test supervisor: records the actor name, increments the spawn count,
    /// and otherwise behaves like `tokio::task::spawn`.
    pub fn test_supervisor<F>(name: &'static str, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        SPAWN_COUNT.fetch_add(1, Ordering::SeqCst);
        let log = SPAWN_NAMES.get().expect("spawn_log initialised").clone();
        tokio::spawn(async move {
            log.lock().await.push(name);
            fut.await;
        });
    }
}

#[cfg(test)]
mod spawn_with_basic {
    use actorizor::actorize;
    use std::sync::atomic::Ordering;

    use crate::spawn_with_shared::{SPAWN_COUNT, spawn_log};

    #[derive(Debug, Default)]
    struct CountActor {
        value: u64,
    }

    // Path passed to `spawn_with` is resolved by the generated code at call
    // site — no `use` of the function is needed in this module.
    #[actorize(spawn_with = crate::spawn_with_shared::test_supervisor)]
    impl CountActor {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        pub fn bump(&mut self) -> u64 {
            self.value += 1;
            self.value
        }
    }

    #[tokio::test]
    async fn spawn_with_routes_through_user_fn_and_carries_actor_name() {
        let log = spawn_log().await;
        let before = SPAWN_COUNT.load(Ordering::SeqCst);

        let handle = CountActorHandle::new();

        // Give the spawn wrapper a chance to record the name.
        tokio::task::yield_now().await;

        assert!(
            SPAWN_COUNT.load(Ordering::SeqCst) > before,
            "test_supervisor should have been invoked at least once",
        );
        let names = log.lock().await.clone();
        assert!(
            names.contains(&"CountActor"),
            "expected CountActor to appear in {names:?}",
        );

        // And the actor itself still works — the supervisor really did drive
        // the future, not just record-and-drop.
        assert_eq!(handle.bump().await.unwrap(), 1);
        assert_eq!(handle.bump().await.unwrap(), 2);
    }
}

#[cfg(test)]
mod spawn_with_positional_qdepth {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    struct DepthActor {
        value: u64,
    }

    #[actorize(64, spawn_with = crate::spawn_with_shared::test_supervisor)]
    impl DepthActor {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        pub fn bump(&mut self) -> u64 {
            self.value += 1;
            self.value
        }
    }

    #[tokio::test]
    async fn spawn_with_composes_with_positional_qdepth() {
        let _ = crate::spawn_with_shared::spawn_log().await; // init
        let handle = DepthActorHandle::new();
        assert_eq!(handle.bump().await.unwrap(), 1);
    }
}

#[cfg(test)]
mod spawn_with_named_qdepth {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    struct NamedDepthActor {
        value: u64,
    }

    #[actorize(qdepth = 8, spawn_with = crate::spawn_with_shared::test_supervisor)]
    impl NamedDepthActor {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        pub fn bump(&mut self) -> u64 {
            self.value += 1;
            self.value
        }
    }

    #[tokio::test]
    async fn spawn_with_composes_with_named_qdepth() {
        let _ = crate::spawn_with_shared::spawn_log().await;
        let handle = NamedDepthActorHandle::new();
        assert_eq!(handle.bump().await.unwrap(), 1);
    }
}

#[cfg(test)]
mod spawn_with_panic_observation {
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, Ordering};

    use actorizor::actorize;

    // The supervisor wraps the actor's future inside its own `tokio::spawn`
    // and awaits the JoinHandle, proving the override gets its hands on the
    // future (not a copy) and can react to panics.
    pub(crate) static PANIC_OBSERVED: AtomicBool = AtomicBool::new(false);

    pub fn panic_observing_supervisor<F>(_name: &'static str, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            let jh = tokio::spawn(fut);
            if let Err(e) = jh.await {
                if e.is_panic() {
                    PANIC_OBSERVED.store(true, Ordering::SeqCst);
                }
            }
        });
    }

    #[derive(Debug, Default)]
    struct PanicActor;

    #[actorize(spawn_with = crate::spawn_with_panic_observation::panic_observing_supervisor)]
    impl PanicActor {
        pub fn new() -> Self {
            Self
        }

        pub fn boom(&self) {
            panic!("intentional");
        }
    }

    #[tokio::test]
    async fn supervisor_can_observe_actor_panic() {
        PANIC_OBSERVED.store(false, Ordering::SeqCst);
        let handle = PanicActorHandle::new();

        // Fire the panicking call. The oneshot is dropped because the actor
        // task panics before responding, so the handle method returns a
        // RecvFromActorError — fine; we only care the supervisor saw it.
        let _ = handle.boom().await;

        // Give the supervisor's awaiter task a chance to record the panic.
        for _ in 0..50 {
            if PANIC_OBSERVED.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            PANIC_OBSERVED.load(Ordering::SeqCst),
            "supervisor should have caught the actor task panic",
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
