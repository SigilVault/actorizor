# Actorizor

Actorizor takes standard Rust structs and turns them into tokio actors.

[Credit to Alice Ryhl for the approach](https://ryhl.io/blog/actors-with-tokio/).

## Usage

```rust
struct MyActor {
  some_state: u64,
}

#[actorizor::actorize]
impl MyActor {
  pub fn new() -> Self {
    Self {
      some_state: 0
    }
  }

  pub fn increment(&mut self) -> u64 {
    self.some_state += 1;
    self.some_state
  }
}

#[tokio::main]
async fn main() {
    let handle = MyActorHandle::new();
    let new_val = handle.increment().await.expect("error incrementing");
    println!("New value: {new_val}");
}
```

You only need `actorizor` in your `Cargo.toml`. The generated code resolves
its `tokio` / `tracing` usage through actorizor's own dependency tree (via a
hidden re-export), so you don't have to declare those just to satisfy the
macro, and there's no `thiserror` requirement at all. You'll still depend on
`tokio` yourself in practice — you need a runtime for `#[tokio::main]` — and
because tokio is a single semver-`1.x` crate, Cargo unifies your version
with actorizor's: *you* choose the exact tokio version, actorizor only sets
a permissive floor.

Handles are cheap to clone and cloning is the expected way to have multiple
producers communicate with the actor.

## How it works

Actorizor creates a new `MyActorHandle` type for the `impl` block specified.
This handle is used to interact with the actor, sending messages and
receiving responses. Any public initializer or `&self`/`&mut self` method on
the base actor is proxied onto the handle.

A `run_actor` function takes ownership of the actor and brokers messages
from the handle onto the underlying actor. Your actor is extended with a
`handle_msg` entrypoint that `run_actor` dispatches into.

Queue depth defaults to 10; override with `#[actorizor::actorize(32)]` or
`#[actorizor::actorize(qdepth = 32)]`.

## Lifecycle controls

Every generated handle has, in addition to the proxied methods:

- `abort()` — forceful. Cancels the actor task at its next `.await` even if
  other handle clones exist. In-flight calls return `RecvFromActorError`.
- `shutdown()` — cooperative. The actor finishes the message it's currently
  handling, then the loop exits. Doesn't require dropping handle clones.
- `is_alive()` / `is_finished()` — cheap liveness checks against the task.

The actor task also exits naturally when the last handle clone is dropped.

## Supervision

By default, generated constructors (`MyActorHandle::new()` etc.) spawn the
actor via `tokio::task::spawn` and drop the `JoinHandle` — fire and forget.

To take ownership of how the task is spawned — to hold the `JoinHandle`,
observe panics, emit metrics, or keep a registry of live actors — implement
the `Supervisor` trait and use the generated `launch_with` method:

```rust
use actorizor::{actorize, Supervisor, TokioSpawn};

let h = MyActorHandle::launch_with(MyActor::new(), &TokioSpawn); // no-op supervisor
```

`TokioSpawn` is the always-available trivial supervisor (delegates to
`tokio::task::spawn`). A real supervisor implements:

```rust
pub trait Supervisor: Send + Sync + 'static {
    fn spawn<F>(&self, name: &'static str, fut: F) -> tokio::task::AbortHandle
    where F: std::future::Future<Output = ()> + Send + 'static;
}
```

Supervisors are owned values you construct in `main` (or per test) and pass
by reference into `launch_with` — there is no global state in the library.

### `TrackingSupervisor` (feature = "tracking")

Enabling the `tracking` feature unlocks `TrackingSupervisor`: a name-keyed
registry that watches each actor task, emits a `tracing` event on
exit/abort/panic, and exposes query + control methods —
`alive_count`, `alive_count_by_name`, `is_alive`, `snapshot`,
`abort_by_name`, `abort_by_id`, `abort_all`.

```toml
actorizor = { version = "0.2", features = ["tracking"] }
```

See [`examples/supervisor.rs`](examples/supervisor.rs) for a full
walkthrough — run it with
`cargo run --example supervisor --features tracking`.

## Examples

Runnable, narrated demos live in [`examples/`](examples/):

| Example | Run | Shows |
|---|---|---|
| `basic` | `cargo run --example basic` | construct, call sync/async methods, clone-and-share |
| `constructors` | `cargo run --example constructors` | what becomes a ctor/method, and what is *not* on the handle |
| `lifecycle` | `cargo run --example lifecycle` | natural exit vs `shutdown()` vs `abort()` |
| `custom_supervisor` | `cargo run --example custom_supervisor` | implementing `Supervisor` yourself |
| `supervisor` | `cargo run --example supervisor --features tracking` | the bundled `TrackingSupervisor` |

## Limitations

- **One actor per module.** The macro emits a module-scoped `run_actor`
  free function with a fixed name. Two `#[actorize]` blocks in the same
  module collide with a duplicate-symbol error. Put each actor in its own
  `mod { ... }` (or its own file).
- Actor structs must not use generic parameters or lifetimes (`MyActor<T>`
  or `MyActor<'a>` will fail to expand).
- Associated functions with no `&self` receiver that don't return `Self`
  are neither methods nor constructors — they stay on the original `impl`
  block and are not exposed on the handle.

## Diags

Enabling the `diagout` feature flag dumps the macro output to stderr
whenever the macro is processed.

## Using actorizor with Claude Code

Claude does not have built-in knowledge of actorizor. If you use Claude Code
in a project that depends on actorizor, add the following to your project's
`CLAUDE.md` (create one at your repo root if it doesn't exist). This tells
Claude how to use actorizor correctly and prevents it from suggesting
`Arc<Mutex<T>>` patterns that actorizor is specifically designed to replace.

````markdown
## actorizor

This project uses the `actorizor` crate. Apply `#[actorizor::actorize]` to an `impl` block to
turn a plain struct into a tokio actor. Never suggest Arc<Mutex<T>> for shared state — clone
the handle instead.

### What the macro generates (example: `MyActor`)

- `MyActorHandle` — the public interface. Cheap to clone; cloning is the intended sharing
  mechanism. Carries `abort()`, `shutdown()`, `is_alive()`, `is_finished()`, and
  `launch_with<S: Supervisor>(actor, &S)` in addition to the proxied methods.
- `MyActorActorMsg` — internal message enum. Do not use directly.
- `MyActorHandleError` — error type on all handle methods.

### Rules

- Only `pub` methods become handle methods. Private methods stay on the actor only.
- A `pub fn` returning `Self` or the actor type is a constructor; it migrates to the handle
  (e.g. `pub fn new() -> Self` becomes `MyActorHandle::new()`).
- A `pub fn` with no `&self` receiver that does NOT return `Self` is neither a method nor a
  constructor — it is not exposed on the handle.
- All non-constructor handle methods are `async` and return `Result<T, MyActorHandleError>`.
- Queue depth defaults to 10; override with `#[actorizor::actorize(32)]` or
  `#[actorizor::actorize(qdepth = 32)]`.
- Actor structs must not use generics or lifetimes (`MyActor<T>` or `MyActor<'a>` will fail).
- One actor per module — the macro emits a module-scoped `run_actor`; two actors in one
  module collide. Wrap each in its own `mod`.

### Supervision

- Constructors (`MyActorHandle::new()`) are unsupervised (`tokio::task::spawn`, fire & forget).
- For supervision, implement `actorizor::Supervisor` and call
  `MyActorHandle::launch_with(MyActor::new(), &supervisor)`. Supervisors are owned values, not
  globals. `actorizor::TokioSpawn` is the no-op default; `actorizor::TrackingSupervisor`
  (feature `tracking`) is a registry with abort/snapshot controls.
- Forceful stop: `handle.abort()`. Cooperative stop: `handle.shutdown()`.

### Required dependencies

Only `actorizor` itself. Generated code references `tokio`/`tracing` through
actorizor's hidden re-export, and uses no `thiserror`. You'll have `tokio`
as a direct dependency anyway (you need a runtime), and Cargo unifies that
single semver-1.x tokio with actorizor's — your version wins. Don't rename
the `actorizor` crate in `Cargo.toml` (e.g. `foo = { package = "actorizor" }`):
the macro emits `::actorizor::…` paths and won't resolve under a different name.

### Example

```rust
struct Counter { value: u64 }

#[actorizor::actorize]
impl Counter {
    pub fn new() -> Self { Self { value: 0 } }
    pub fn increment(&mut self) -> u64 { self.value += 1; self.value }
}

// Usage:
let handle = CounterHandle::new();
let v = handle.increment().await.unwrap();
let h2 = handle.clone(); // share by cloning, not Arc
```
````
