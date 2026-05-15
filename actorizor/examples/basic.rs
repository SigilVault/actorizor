//! The "hello world" of actorizor: turn a struct into an actor, call its
//! methods through the generated handle, and share it by cloning.
//!
//! ```text
//! cargo run --example basic
//! ```

use actorizor::actorize;

#[derive(Debug, Default)]
struct Counter {
    value: u64,
}

#[actorize]
impl Counter {
    // A `pub fn` returning `Self` becomes `CounterHandle::new()`.
    pub fn new() -> Self {
        Self { value: 0 }
    }

    // `&self` / `&mut self` methods become async methods on the handle,
    // returning `Result<T, CounterHandleError>`.
    pub fn get(&self) -> u64 {
        self.value
    }

    pub fn set(&mut self, v: u64) {
        self.value = v;
    }

    pub fn add(&mut self, a: u64, b: u64) -> u64 {
        self.value += a + b;
        self.value
    }

    pub async fn get_async(&self) -> u64 {
        self.value
    }
}

#[tokio::main]
async fn main() {
    let counter = CounterHandle::new();
    println!("fresh counter: {}", counter.get().await.unwrap());

    counter.set(10).await.unwrap();
    println!("after set(10): {}", counter.get().await.unwrap());

    let total = counter.add(3, 4).await.unwrap();
    println!("after add(3, 4): {total}");

    // Async methods work the same way.
    println!("get_async(): {}", counter.get_async().await.unwrap());

    // The handle is cheap to clone. Cloning — NOT Arc<Mutex<_>> — is the
    // intended way to share an actor between tasks.
    let clone_a = counter.clone();
    let clone_b = counter.clone();

    let ta = tokio::spawn(async move { clone_a.add(1, 1).await.unwrap() });
    let tb = tokio::spawn(async move { clone_b.add(2, 2).await.unwrap() });
    let (_, _) = (ta.await.unwrap(), tb.await.unwrap());

    // Both clones drove the same underlying actor; the final value
    // reflects every mutation regardless of which clone issued it.
    println!("after concurrent adds via clones: {}", counter.get().await.unwrap());
}
