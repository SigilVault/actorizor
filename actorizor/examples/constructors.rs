//! What the macro does and does NOT put on the generated handle.
//!
//! ```text
//! cargo run --example constructors
//! ```
//!
//! Rules demonstrated here:
//!
//! - A `pub fn` returning `Self` (or the actor type) is a **constructor** —
//!   it migrates to the handle as a sync/async associated fn returning the
//!   handle directly (not a `Result`).
//! - A `pub fn`/`async fn` taking `&self`/`&mut self` is a **method** — it
//!   becomes an `async` handle method returning `Result<_, …HandleError>`.
//! - A `pub fn` with **no `&self` receiver that does not return `Self`** is
//!   neither — it stays on the original `impl` only and is NOT on the
//!   handle. (This is the easy-to-miss one.)
//! - Private (`fn` without `pub`) items never appear on the handle.

use actorizor::actorize;

#[derive(Debug, Default)]
struct Widget {
    id: u64,
}

// `#[actorize(N)]` overrides the mailbox depth (default 10). `qdepth = N`
// is the named form. Shown here just to demonstrate it parses.
#[actorize(32)]
impl Widget {
    // --- constructors: all migrate to WidgetHandle ---

    pub fn new() -> Self {
        Self { id: 0 }
    }

    pub fn with_id(id: u64) -> Self {
        Self { id }
    }

    // Async constructors are supported; the handle fn is async to match.
    pub async fn loaded(id: u64) -> Self {
        // pretend we did async I/O to load it
        Self { id }
    }

    // --- methods: become async handle methods ---

    pub fn id(&self) -> u64 {
        self.id
    }

    pub async fn relabel(&mut self, id: u64) -> u64 {
        self.id = id;
        self.id
    }

    // --- NOT exposed on the handle ---

    // No `&self`, doesn't return `Self` → neither method nor constructor.
    // Callable as `Widget::compute(...)` on the bare struct, but there is
    // NO `WidgetHandle::compute`.
    pub fn compute(a: u64, b: u64) -> u64 {
        a + b
    }

    // Private → actor-internal only. (`allow(dead_code)`: it exists purely
    // to demonstrate that private items never reach the handle.)
    #[allow(dead_code)]
    fn secret(&self) -> u64 {
        self.id * 2
    }
}

#[tokio::main]
async fn main() {
    let a = WidgetHandle::new();
    println!("new().id() = {}", a.id().await.unwrap());

    let b = WidgetHandle::with_id(7);
    println!("with_id(7).id() = {}", b.id().await.unwrap());

    // Async constructor — note the `.await` on construction.
    let c = WidgetHandle::loaded(99).await;
    println!("loaded(99).id() = {}", c.id().await.unwrap());

    let n = b.relabel(42).await.unwrap();
    println!("relabel(42) -> {n}");

    // `compute` is NOT on the handle — call it on the bare type instead.
    println!("Widget::compute(2, 3) = {} (not on the handle)", Widget::compute(2, 3));

    // `secret` is private — only the actor itself can call it; it shows up
    // in neither `Widget`'s public surface nor `WidgetHandle`.

    // Lifecycle controls exist on every handle regardless of constructor.
    println!("a.is_alive() = {}", a.is_alive());
}
