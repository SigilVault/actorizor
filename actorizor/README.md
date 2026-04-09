# Actorizor

Actorizor takes standard Rust structs and turns them into actors.

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

Actorizor uses tokio and thiserror - remember to add them to your project.

Handles are cheap to clone and cloning is the expected way to have multiple producers communicate with the actor.

## How it works

Actorizor creates a new ActorHandle type for the `impl` block specified. This handle is used to interact with the actor, allowing you to send messages and receive responses. Any public initializer or function on the base actor is proxied onto the handle.

A `run_actor` function ownership over the actor and brokers messages from the handle onto the underlying actor.

Your actor will be extended with a `handle_msg` function as an entrypoint to `run_actor`.

## Diags

Enabling the `diagout` feature flag dumps the macro output to stderr whenever the macro is processed.

## Using actorizor with Claude Code

Claude does not have built-in knowledge of actorizor. If you use Claude Code in a project that depends on actorizor, add the following to your project's `CLAUDE.md` (create one at your repo root if it doesn't exist). This tells Claude how to use actorizor correctly and prevents it from suggesting `Arc<Mutex<T>>` patterns that actorizor is specifically designed to replace.

````markdown
## actorizor

This project uses the `actorizor` crate. Apply `#[actorizor::actorize]` to an `impl` block to
turn a plain struct into a tokio actor. Never suggest Arc<Mutex<T>> for shared state — clone
the handle instead.

### What the macro generates (example: `MyActor`)

- `MyActorHandle` — the public interface. Cheap to clone; cloning is the intended sharing mechanism.
- `MyActorMsg` — internal message enum. Do not use directly.
- `MyActorHandleError` — error type on all handle methods.

### Rules

- Only `pub` methods become handle methods. Private methods stay on the actor only.
- A `pub fn` returning `Self` or the actor type is a constructor; it migrates to the handle
  (e.g. `pub fn new() -> Self` becomes `MyActorHandle::new()`).
- All handle methods are `async` and return `Result<T, MyActorHandleError>`.
- Queue depth defaults to 10; override with `#[actorizor::actorize(32)]`.
- Actor structs must not use generics or lifetimes (`MyActor<T>` or `MyActor<'a>` will fail).

### Required dependencies

The consuming project must have `tokio` and `thiserror` as direct dependencies.

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
