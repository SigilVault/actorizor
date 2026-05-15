//! Core macro behavior: constructors, sync/async methods, multi-arg
//! dispatch, and shared state across cloned handles.

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
async fn default_constructor() {
    let handle = TestActorHandle::new();
    assert_eq!(handle.get_value().await.unwrap(), 0);
}

#[tokio::test]
async fn parameterized_constructor() {
    let handle = TestActorHandle::new_with(42);
    assert_eq!(handle.get_value().await.unwrap(), 42);
}

#[tokio::test]
async fn sync_mutation() {
    let handle = TestActorHandle::new();
    handle.set_value(100).await.unwrap();
    assert_eq!(handle.get_value().await.unwrap(), 100);
}

#[tokio::test]
async fn multi_arg_method() {
    let handle = TestActorHandle::new();
    assert_eq!(handle.add(3, 7).await.unwrap(), 10);
}

#[tokio::test]
async fn async_method() {
    let handle = TestActorHandle::new_with(99);
    assert_eq!(handle.async_get().await.unwrap(), 99);
}

#[tokio::test]
async fn cloned_handles_share_state() {
    let handle1 = TestActorHandle::new_with(5);
    let handle2 = handle1.clone();
    handle1.set_value(50).await.unwrap();
    assert_eq!(handle2.get_value().await.unwrap(), 50);
}
