// #![feature(trace_macros)]
// trace_macros!(true);

extern crate proc_macro;

mod actorizor;

#[cfg(feature = "diagout")]
mod pretty;

/// Transforms an `impl` block into a tokio actor.
///
/// Apply to any `impl MyStruct { ... }` block. The macro generates:
///
/// - `MyStructHandle` — a cheap-to-clone handle that is the public interface to the actor.
///   All communication goes through this type; never use `MyStruct` directly after actorizing.
/// - `MyStructMsg` — internal message enum. Do not use directly.
/// - `MyStructHandleError` — error type returned by every handle method.
///
/// # Usage
///
/// ```rust
/// struct Counter { value: u64 }
///
/// #[actorizor::actorize]
/// impl Counter {
///     pub fn new() -> Self { Self { value: 0 } }
///     pub fn increment(&mut self) -> u64 { self.value += 1; self.value }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let handle = CounterHandle::new();           // constructor migrates to the handle
///     let v = handle.increment().await.unwrap();   // all methods are async on the handle
///     let h2 = handle.clone();                     // clone to share — do not use Arc<Mutex<>>
/// }
/// ```
///
/// # Rules
///
/// - Only `pub` methods appear on the handle. Private methods stay on the actor only.
/// - A `pub fn` returning `Self` or the actor type is treated as a constructor and becomes
///   a (sync or async, matching the original) associated function on the handle that returns
///   the handle type directly — not `Result`.
/// - All non-constructor handle methods are `async` and return `Result<T, MyStructHandleError>`.
/// - Actor structs must not have generic parameters or lifetimes (`MyStruct<T>` will fail).
///
/// # Queue depth
///
/// The default channel depth is 10. Override with a positional literal or
/// the named form:
///
/// ```ignore
/// #[actorizor::actorize(32)]
/// #[actorizor::actorize(qdepth = 32)]
/// impl MyStruct { ... }
/// ```
///
/// # Bring-your-own spawn
///
/// By default the generated code calls `tokio::task::spawn` and drops the
/// `JoinHandle` (fire-and-forget). Supply your own function with
/// `spawn_with = path::to::fn` to take ownership of the spawn — e.g. to
/// track the JoinHandle, observe panics, restart on exit, or emit metrics.
///
/// ```ignore
/// fn my_supervisor<F>(name: &'static str, fut: F)
/// where
///     F: std::future::Future<Output = ()> + Send + 'static,
/// {
///     tracing::info!(actor = name, "spawning");
///     let _ = tokio::task::spawn(fut);
/// }
///
/// #[actorizor::actorize(spawn_with = crate::my_supervisor)]
/// impl MyStruct { ... }
///
/// // Composes with qdepth:
/// #[actorizor::actorize(64, spawn_with = crate::my_supervisor)]
/// #[actorizor::actorize(qdepth = 64, spawn_with = crate::my_supervisor)]
/// ```
///
/// The function's contract:
/// - Drive `fut` on a tokio runtime (it uses `tokio::sync::mpsc` and the
///   actor's methods may be async).
/// - Don't block — `launch_actor` is sync and must return promptly.
/// - The `name` argument is `stringify!(ActorIdent)` — use it for logs and
///   metrics labels.
///
/// # Dependencies
///
/// Your project must include `tokio`, `thiserror`, and `tracing` as direct
/// dependencies. (Per-message error handling in the generated `run_actor`
/// loop uses `tracing::warn!`.)
#[proc_macro_attribute]
pub fn actorize(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    actorizor::actorize(attr, item)
}
