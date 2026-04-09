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
/// The default channel depth is 10. Override with a literal:
///
/// ```ignore
/// #[actorizor::actorize(32)]
/// impl MyStruct { ... }
/// ```
///
/// # Dependencies
///
/// Your project must include `tokio` and `thiserror` as direct dependencies.
#[proc_macro_attribute]
pub fn actorize(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    actorizor::actorize(attr, item)
}
