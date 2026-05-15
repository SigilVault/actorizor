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
4. **`MyActorHandleError` enum** — via `thiserror`, covers send/receive failures.
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

## Error logging in `run_actor`

The generated `run_actor` loop calls `::tracing::warn!(actor = stringify!(Actor), error = ?e, "actor message handling failed")` when `handle_msg` returns Err. Users must therefore have `tracing` as a direct dependency.

## diagout feature

Forwarded from `actorizor` → `actorizor-macros`. Emits a `eprintln!` of the pretty-printed macro output at compile time. Debugging aid; not user-facing.

## Known limitations

- Actor structs must not use generic parameters or lifetime parameters. `MyActor<T>` and `MyActor<'a>` will both fail to expand correctly. Known gap; tracked in the `generics` branch.
- `pub(super)` and similar restricted visibility are currently passed through but not semantically restricted in generated code (TODO in `extract_functions_raw`).
- **One actor per module.** The macro emits `run_actor` as a module-scoped free function with a fixed name, so two `#[actorize]` blocks in the same module will collide. Wrap each actor in its own `mod { ... }`. This is the convention the test suite follows.

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

```
# Default features:
cargo test -p actorizor

# Everything including TrackingSupervisor:
cargo test -p actorizor --features tracking

# The runnable supervisor demo (also rendered on docs.rs):
cargo run --example supervisor --features tracking
```

`actorizor/examples/supervisor.rs` is the canonical "this is what your
supervisor wiring should look like" reference. Living in the lib crate, it
shows up on docs.rs.
