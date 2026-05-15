//! # actorizor — tokio actor framework
//!
//! `#[actorizor::actorize]` transforms an `impl` block into a tokio actor with
//! a cheap-to-clone async handle. See the [`actorize`] macro docs for the full
//! generated surface.
//!
//! In addition to the macro, this crate exposes:
//!
//! - The [`Supervisor`] trait, the contract for code that owns the spawn
//!   decision (where the task runs, whether to watch its `JoinHandle`,
//!   whether to register it somewhere, etc.).
//! - [`TokioSpawn`], a zero-state `Supervisor` that just delegates to
//!   `tokio::task::spawn` and drops the JoinHandle. Always available.
//! - [`TrackingSupervisor`] (behind the `tracking` feature), a name-keyed
//!   registry that watches each actor's task, emits a `tracing` event on
//!   exit or panic, and exposes query / control methods
//!   (`alive_count`, `snapshot`, `abort_by_name`, `abort_by_id`, …).
//!
//! ## Example
//!
//! ```rust
//! use actorizor::{actorize, Supervisor, TokioSpawn};
//!
//! struct Counter { value: u64 }
//!
//! #[actorize]
//! impl Counter {
//!     pub fn new() -> Self { Self { value: 0 } }
//!     pub fn increment(&mut self) -> u64 { self.value += 1; self.value }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // Unsupervised: just calls tokio::task::spawn under the hood.
//!     let h = CounterHandle::new();
//!     let v = h.increment().await.unwrap();
//!
//!     // Supervised: pass any `Supervisor` impl. Here, the no-op one.
//!     let h2 = CounterHandle::launch_with(Counter::new(), &TokioSpawn);
//!     let _ = h2.increment().await.unwrap();
//!
//!     // Lifecycle: abort kills the task; shutdown lets the current message
//!     // finish and exits the loop; is_alive observes either.
//!     assert!(h.is_alive());
//!     h.shutdown();
//! }
//! ```
//!
//! ## Generics
//!
//! Impl-level type and const generics (and `where`-clauses) are supported:
//! `#[actorize] impl<T: Send + 'static> Store<T>`. Construct with the type
//! named at the call site: `StoreHandle::<u64>::new()`.
//!
//! - Every generic param must be `Send + 'static` — the actor task is
//!   spawned. Owned structs, `&'static` refs, `Box<_>`, and `Arc<_>` work;
//!   **`Rc<_>` does not** (it is `!Send` — use `Arc`). No `T: Clone` /
//!   `T: Debug` is required; the generated handle and error type are
//!   `Clone`/`Debug` regardless of `T`.
//! - Generic *methods* (`fn f<U>(…)`) and lifetime parameters
//!   (`impl<'a> …`) are rejected at compile time with a clear error.
//!
//! ## Feature flags
//!
//! - `tracking` — enables [`TrackingSupervisor`] and [`ActorSnapshot`]. Off by
//!   default to keep the bare contract lightweight.
//! - `diagout` — forwarded to `actorizor-macros`; prints the pretty-printed
//!   macro expansion at compile time. Debugging aid.

pub use actorizor_macros::actorize;

mod supervisor;

pub use supervisor::{Supervisor, TokioSpawn};

#[cfg(feature = "tracking")]
pub use supervisor::{ActorSnapshot, TrackingSupervisor};

/// Implementation detail. The `actorize` macro emits paths into this module
/// (`::actorizor::__private::tokio::…`, `…::tracing::…`) so generated code
/// resolves against *actorizor's* dependency tree, not the user's. This
/// means consumers only need `actorizor` in their `Cargo.toml` — they do
/// not have to (and should not need to) add `tokio`/`tracing` solely to
/// satisfy generated code.
///
/// Not part of the public API. No semver guarantees. Do not name these
/// paths in your own code.
#[doc(hidden)]
pub mod __private {
    pub use tokio;
    pub use tracing;
}
