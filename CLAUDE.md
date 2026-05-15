# actorizor — contributor guide

This workspace contains two crates:

- **`actorizor`** — the user-facing library. Re-exports the `actorize` macro and exposes the `Supervisor` trait, `TokioSpawn`, and (behind the `tracking` feature) `TrackingSupervisor`. **This is what users depend on.** Integration tests live in `actorizor/tests/`; the runnable demo lives in `actorizor/examples/`.
- **`actorizor-macros`** — the proc-macro implementation. Users do not depend on it directly; it's a private peer of `actorizor`. Split out because proc-macro crates cannot export non-macro items, but the macro's generated code needs to reference `actorizor::Supervisor` and `actorizor::TokioSpawn`.

The crates are versioned in lockstep with a `=` pin on `actorizor-macros`.

## What the macro does

`#[actorizor::actorize]` is applied to an `impl` block. It reads the block with `syn`, builds an intermediate representation, and emits five pieces of generated code:

1. **The original `impl` block** — unchanged except `handle_msg` is injected onto it.
2. **`MyActorActorMsg` enum** — one variant per public method (not constructors), each carrying the method's parameters plus a `oneshot::Sender` for the response.
3. **`MyActorHandle` struct** — clone-derived, holds `Sender<MyActorActorMsg>`, `AbortHandle`, and `Arc<Notify>`. Exposes:
   - generated constructors that mirror the actor's pub fns returning `Self` (unsupervised via `TokioSpawn`)
   - `launch_with<S: Supervisor>(actor, &S) -> Self` — the supervised entry point
   - `abort()` / `shutdown()` / `is_alive()` / `is_finished()`
   - async wrappers for every other pub method
4. **`MyActorHandleError` enum** — hand-rolled `Debug`/`Display`/`Error`/`From` impls, covers send/receive failures.
5. **`run_actor` free function** — owns the actor, biased `select!` over `shutdown.notified()` and `receiver.recv()`, dispatches to `handle_msg`, logs per-message errors via `tracing::warn!`.

## Key internal types

- `Root` — top-level IR built from `ItemImpl`. Holds idents for all generated type names and the split between `actor_funcs` (methods) and `actor_constructors`.
- `ActorFunc` — represents one function from the impl block. Knows how to emit its enum variant, its handle wrapper, its match arm, and (for constructors) the handle-side constructor.
- `FuncInput` — a single parameter, knows how to render itself in enum, handle fn, and passthrough positions.

## Constructor detection

A function is treated as a constructor (not a message-dispatched method) if:

- It is `pub`
- Its return type is `Self` or the actor struct's name

Constructors become sync or async `fn` on the handle (matching the original) that wrap `launch_unsupervised`, which is itself sugar for `launch_with(actor, &TokioSpawn)`.

## Queue depth

Defaults to `STD_QUEUE_DEPTH = 10`. Override with `#[actorize(32)]` (positional) or `#[actorize(qdepth = 32)]` (named).

## Attribute parsing

Args go through `AttrArgs` (custom `Parse` impl, top of `actorizor-macros/src/actorizor.rs`). Accepts:

- a bare integer literal (positional `qdepth`)
- `qdepth = N`
- a comma-separated combination

Rejects unknown keys with `syn::Error` at the key's span so users see the bad arg highlighted in their editor.

## Supervision: trait + impls

The runtime side of the macro is the `Supervisor` trait, defined in `actorizor/src/supervisor.rs`:

```rust
pub trait Supervisor: Send + Sync + 'static {
    fn spawn<F>(&self, name: &'static str, fut: F) -> tokio::task::AbortHandle
    where F: Future<Output = ()> + Send + 'static;
}
```

Generated `launch_with` calls `sup.spawn(stringify!(Actor), run_actor(...))` and stashes the returned `AbortHandle` in the `Handle` struct.

Two built-in impls:

- **`TokioSpawn`** — zero-state, delegates to `tokio::task::spawn`, discards the actor name. Always available. The generated bare constructors (`new()`, `new_with(...)`, …) use this under the hood via `launch_unsupervised`.
- **`TrackingSupervisor`** — behind the `tracking` feature. Name-keyed registry, monotonic-u64 identity per spawn, per-spawn watcher task that emits a `tracing` event on exit (`info` for clean/abort, `error` for panic, `warn` for other join errors) and prunes the registry. Methods: `new`, `alive_count`, `alive_count_by_name`, `is_alive`, `snapshot`, `abort_by_name`, `abort_by_id`, `abort_all`. Snapshot type is `ActorSnapshot`.

`TrackingSupervisor` is intentionally owned (not static): construct one in `main`, pass `&supervisor` into `launch_with`, drop it when done. Tests construct their own.

## Lifecycle: abort vs shutdown

Both methods exist on every Handle, regardless of which constructor or `launch_with` was used.

- **`abort()`** — calls `AbortHandle::abort()`. Hard kill: the actor task is cancelled mid-`.await`. The oneshot Sender held inside the killed Msg gets dropped, so any in-flight handle method call returns `RecvFromActorError`.
- **`shutdown()`** — calls `notify_waiters()` on the shared `Arc<Notify>`. The biased `select!` in `run_actor` exits on the next poll. The current in-flight message (if any) completes first. Sender clones are not dropped, so post-shutdown calls succeed at send but fail at recv.
- **`is_alive()` / `is_finished()`** — read `AbortHandle::is_finished()`. Suitable for cheap pre-call checks; race conditions can still produce send/recv errors if the task dies between the check and the call.

## Dependency resolution (the `__private` re-export)

Generated code does **not** name `::tokio::…` / `::tracing::…` directly any
more. It emits `::actorizor::__private::tokio::…` and
`::actorizor::__private::tracing::…`, where `__private` is a
`#[doc(hidden)]` module in `actorizor/src/lib.rs` that does
`pub use tokio; pub use tracing;`. Consequences:

- Users only need `actorizor` in their `Cargo.toml`. They no longer have to
  declare `tokio`/`tracing` solely to satisfy generated code (the error
  enum's impls are hand-rolled in `error_enum_stream`, so generated code
  pulls in no derive crates either).
- The user still controls the tokio version: tokio is a single semver-1.x
  crate, Cargo unifies actorizor's `tokio = "1"` floor with the user's own
  (always-present, for the runtime) tokio dependency; the user's constraint
  wins. actorizor only imposes a permissive version floor + an additive
  feature floor (`macros`, `rt`, `sync`).
- `actorizor`'s `[dependencies]` therefore carries `tokio` (with those
  features) and `tracing` — they are real runtime deps now, not just
  user-side expectations.
- **Crate-rename caveat (LLM-relevant edge case):** the macro hardcodes the
  path `::actorizor::__private::…`. If a user renames the crate in
  `Cargo.toml` — `foo = { package = "actorizor" }` — generated code won't
  resolve (`::actorizor` isn't a valid path). The robust fix would be the
  `proc-macro-crate` crate (what serde-derive uses); we deliberately did
  *not* pull that in — it's a weird enough case that a human won't hit it,
  but an LLM editing a downstream `Cargo.toml` might. If you're an agent:
  do not rename the `actorizor` dependency.

## Error logging in `run_actor`

The generated `run_actor` loop calls
`::actorizor::__private::tracing::warn!(actor = stringify!(Actor), error = ?e, "actor message handling failed")`
when `handle_msg` returns Err. The `tracing` dependency is satisfied through
the re-export; users don't declare it themselves.

## diagout feature

Forwarded from `actorizor` → `actorizor-macros`. Emits a `eprintln!` of the pretty-printed macro output at compile time. Debugging aid; not user-facing.

## Generics

Impl-level **type** and **const** generics are supported:
`#[actorize] impl<T: Bound> MyActor<T>` and `impl<const N: usize> Buf<N>`,
including `where`-clauses. The generated `MyActorHandle<T>` /
`MyActorActorMsg<T>` / `MyActorHandleError<T>` / `run_actor<T>` all thread
the generics through. Plumbing lives in `ActorGenerics` (one source of
truth vending `decl` / `ty` / `where_` / `spawn` / `turbofish` / `phantom`).

### Constraint on `T` (and the `Rc` trap)

Every generic type param must be `Send + 'static`, because the actor task
is handed to a `Supervisor` and `tokio::spawn`ed. This is not a special
check — the spawn-path augmentation adds the bound, so violations surface
as ordinary trait errors on the user's `#[actorize]`. Consequences for
what `T` (or any message-carried payload) can be:

- ✅ owned custom structs (no `Clone`/`Copy` needed — the *handle* is
  `Clone` regardless), `&'static` references, `Box<_>`, `Arc<_>`.
- ❌ `Rc<_>` — it is `!Send`, so e.g. `MyActorHandle::<Rc<Payload>>::new()`
  fails with `error[E0277]: Rc<..> cannot be sent between threads safely`.
  Use `Arc` instead. (Verified; documented in `examples/generic.rs` and
  the `payload_shapes` module of `tests/generics.rs`.)
- ❌ non-`'static` borrows — see the rejected list below; a lifetime
  *parameter* is the macro-level form of this.

Robustness points the implementation deliberately handles (these are the
traps that made earlier attempts fragile):

- **Self-type vs name.** `extract_base_ident` takes only the leading
  segment ident for *naming* generated types; generic args are handled
  separately by `ActorGenerics`. (Case-converting `MyActor<T>` is what
  produced garbage idents before.)
- **Perfect `Clone`.** `MyActorHandle<T>`'s `Clone` is hand-written, NOT
  `#[derive(Clone)]` — a derive would add a bogus `T: Clone` bound. Same
  for the error enum's `Debug` (hand-written, no `T: Debug`).
- **Spawn bounds.** Type params get `Send + 'static` for the spawn path
  (`run_actor`, inherent handle impl), added *only if absent* (scans both
  param bounds and the where-clause) so there's no "bound defined in more
  than one place" warning on the user's `#[actorize]`.
- **Phantom.** A hidden `__ActorizorPhantom` variant on the message enum
  carries `PhantomData<fn() -> (T, [(); N])>` so a generic param no method
  references still counts as used (handle/error hold `…<MsgEnum<T>>`).
  `handle_msg` gets a matching `unreachable!` arm.

Rejected at expansion with a clear `syn::Error` at the offending span:

- **Lifetime parameters** (`impl<'a> MyActor<'a>`) — the actor task is
  spawned and must be `'static`.
- **Method-level generics** (`pub fn foo<U>(…)`) — an enum variant can't
  carry a generic that isn't a param of the enum; would need per-message
  type erasure.

## Known limitations

- `pub(super)` and similar restricted visibility are currently passed through but not semantically restricted in generated code (TODO in `extract_functions_raw`).
- **One actor per module.** The macro emits `run_actor` as a module-scoped free function with a fixed name, so two `#[actorize]` blocks in the same module will collide. Wrap each actor in its own `mod { ... }`. This is the convention the test/example suite follows.

## Test + example layout

Integration tests are in `actorizor/tests/`, one concern per file. Each
`tests/*.rs` is its own crate root, so the "one actor per module" rule only
bites for multiple actors *within one file* — those go in submodules.

| File | Covers |
|---|---|
| `basic.rs` | constructors, sync/async methods, multi-arg, clone-shares-state |
| `lifecycle.rs` | `abort()`, `shutdown()`, `is_alive()` / `is_finished()` |
| `supervision.rs` | `launch_with` + `TokioSpawn` + a custom `Supervisor` impl |
| `tracking.rs` | `TrackingSupervisor` (whole file `#![cfg(feature = "tracking")]`) |
| `complex_impl.rs` | gnarly impl block: qdepth, many ctors, private fns, non-method assoc fns |
| `generics.rs` | impl-level type/const generics, where-clauses, perfect-`Clone`, phantom path |

```
# Default features:
cargo test -p actorizor

# Everything including TrackingSupervisor:
cargo test -p actorizor --features tracking

# Runnable examples (all in actorizor/examples/, rendered on docs.rs):
cargo run --example basic
cargo run --example constructors
cargo run --example lifecycle
cargo run --example custom_supervisor
cargo run --example generic
cargo run --example supervisor --features tracking
```

Examples mirror the test coverage but are narrated (println-driven, no
asserts) so they double as docs.rs-visible documentation:

| Example | Mirrors test | Shows |
|---|---|---|
| `basic.rs` | `basic.rs` | construct, sync/async methods, multi-arg, clone-and-share |
| `constructors.rs` | `complex_impl.rs` | which fns become ctors/methods and which are NOT on the handle |
| `lifecycle.rs` | `lifecycle.rs` | natural drop-exit vs `shutdown()` vs `abort()`, observed via a `Drop` impl |
| `custom_supervisor.rs` | `supervision.rs` | implementing the `Supervisor` trait by hand (owned, no statics) |
| `generic.rs` | `generics.rs` | generic actor at two instantiations, non-`Clone` payload, where-clause |
| `supervisor.rs` | `tracking.rs` | `TrackingSupervisor` registry/snapshot/abort (needs `--features tracking`) |

`examples/supervisor.rs` + `examples/custom_supervisor.rs` are the canonical
"this is what your supervisor wiring should look like" references.

When adding an example: one actor per file is automatic (each example is
its own crate root). Non-`tracking` examples are auto-discovered; anything
needing a feature gets an explicit `[[example]]` `required-features` stanza
in `actorizor/Cargo.toml` (only `supervisor` does today).
