//! Proc-macro implementation for the `actorizor` crate.
//!
//! Don't depend on this crate directly — depend on `actorizor`, which
//! re-exports the [`actorize`] macro alongside the runtime types
//! (`Supervisor`, `TokioSpawn`, optional `TrackingSupervisor`) the macro's
//! generated code relies on.

extern crate proc_macro;

mod actorizor;

#[cfg(feature = "diagout")]
mod pretty;

/// Transforms an `impl` block into a tokio actor.
///
/// See the `actorizor` crate's documentation for the full description; this
/// is the proc-macro entry point that lives in a separate crate for the
/// usual proc-macro + runtime-types split.
#[proc_macro_attribute]
pub fn actorize(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    actorizor::actorize(attr, item)
}
