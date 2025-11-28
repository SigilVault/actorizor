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
