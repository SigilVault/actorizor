//! Impl-level generics on an actorized type.
//!
//! ```text
//! cargo run --example generic
//! ```
//!
//! Shows:
//! - one generic actor used at two different type instantiations,
//! - a payload type that is **not** `Clone` — the generated `…Handle<T>`
//!   is still `Clone` (a `#[derive(Clone)]` would have wrongly demanded
//!   `T: Clone`); cloning is how you share the actor,
//! - a `where`-clause generic,
//! - lifecycle controls working on a generic handle.
//!
//! Note the one-actor-per-module rule: the macro emits a module-scoped
//! `run_actor`, so each actor lives in its own `mod`. (Limits, by design:
//! impl-level type/const generics only — generic *methods* and lifetime
//! parameters are rejected at compile time with a clear error.)

mod store {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    pub struct Store<T> {
        items: Vec<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Store<T> {
        pub fn new() -> Self {
            Self { items: Vec::new() }
        }

        pub fn push(&mut self, item: T) -> usize {
            self.items.push(item);
            self.items.len()
        }

        pub fn len(&self) -> usize {
            self.items.len()
        }
    }
}

mod scale {
    use actorizor::actorize;

    pub trait Weigh {
        fn grams(&self) -> u64;
    }
    impl Weigh for u64 {
        fn grams(&self) -> u64 {
            *self
        }
    }

    #[derive(Debug, Default)]
    pub struct Scale<T> {
        total: u64,
        _last: Option<T>,
    }

    #[actorize]
    impl<T> Scale<T>
    where
        T: Weigh + Send + 'static,
    {
        pub fn new() -> Self {
            Self {
                total: 0,
                _last: None,
            }
        }

        pub fn add(&mut self, item: T) -> u64 {
            self.total += item.grams();
            self.total
        }
    }
}

/// A deliberately non-`Clone` payload, to prove `StoreHandle<Ticket>` is
/// still `Clone`. (`allow(dead_code)`: the id is never read back; the
/// point is the type's *non-Clone-ness*, not its contents.)
#[derive(Debug)]
#[allow(dead_code)]
struct Ticket(u64);

/// A non-primitive custom struct used as `T`. Not `Clone`, not `Copy` —
/// works fine as a payload, and `StoreHandle<Parcel>` is still `Clone`.
#[derive(Debug)]
#[allow(dead_code)]
struct Parcel {
    id: u64,
    label: String,
}

use std::sync::Arc;

use scale::ScaleHandle;
use store::StoreHandle;

#[tokio::main]
async fn main() {
    // Same actor, two instantiations.
    let strings = StoreHandle::<String>::new();
    let numbers = StoreHandle::<u64>::new();

    println!("Store<String>.push: {}", strings.push("hello".into()).await.unwrap());
    println!("Store<String>.push: {}", strings.push("world".into()).await.unwrap());
    println!("Store<u64>.push   : {}", numbers.push(7).await.unwrap());
    println!(
        "lens — strings={}, numbers={}",
        strings.len().await.unwrap(),
        numbers.len().await.unwrap()
    );

    // Non-Clone payload. The HANDLE is still Clone — share by cloning.
    let tickets = StoreHandle::<Ticket>::new();
    let desk_a = tickets.clone();
    let desk_b = tickets.clone();
    desk_a.push(Ticket(101)).await.unwrap();
    desk_b.push(Ticket(102)).await.unwrap();
    println!(
        "Store<Ticket> shared across two handle clones: len={}",
        tickets.len().await.unwrap()
    );

    // --- T as a custom struct, a reference, a Box, and an Arc ---------
    //
    // The only hard requirement on `T` is `Send + 'static` (the actor task
    // is spawned). That admits owned structs, `&'static` references,
    // `Box<_>`, and `Arc<_>`. It does NOT admit `Rc<_>` — `Rc` is `!Send`,
    // so `StoreHandle::<Rc<Parcel>>::new()` would fail to compile with
    // "`Rc<Parcel>` cannot be sent between threads safely". Use `Arc`.

    // Custom struct, owned.
    let parcels = StoreHandle::<Parcel>::new();
    parcels
        .push(Parcel { id: 1, label: "books".into() })
        .await
        .unwrap();
    println!("Store<Parcel>.len = {}", parcels.len().await.unwrap());

    // &'static str — a reference type (Send + 'static because 'static).
    let refs = StoreHandle::<&'static str>::new();
    refs.push("alpha").await.unwrap();
    refs.push("beta").await.unwrap();
    println!("Store<&'static str>.len = {}", refs.len().await.unwrap());

    // Box<Parcel>.
    let boxed = StoreHandle::<Box<Parcel>>::new();
    boxed
        .push(Box::new(Parcel { id: 2, label: "boxed".into() }))
        .await
        .unwrap();
    println!("Store<Box<Parcel>>.len = {}", boxed.len().await.unwrap());

    // Arc<Parcel> — shared, and the handle is still cheaply cloneable.
    let shared = Arc::new(Parcel { id: 3, label: "shared".into() });
    let arced = StoreHandle::<Arc<Parcel>>::new();
    arced.push(Arc::clone(&shared)).await.unwrap();
    arced.push(shared).await.unwrap();
    println!(
        "Store<Arc<Parcel>>.len = {} (same Parcel pushed twice)",
        arced.len().await.unwrap()
    );

    // where-clause generic.
    let scale = ScaleHandle::<u64>::new();
    println!("Scale<u64>.add(250) -> {}", scale.add(250).await.unwrap());
    println!("Scale<u64>.add(750) -> {}", scale.add(750).await.unwrap());

    // Lifecycle works the same on a generic handle.
    println!("scale.is_alive() = {}", scale.is_alive());
    scale.shutdown();

    println!("done.");
}
