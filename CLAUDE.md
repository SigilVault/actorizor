# actorizor — contributor guide

This repo contains `actorizor`, a Rust proc-macro crate that converts plain structs into tokio-based actors, and `actor_macro_app`, an integration test/example app.

## What the macro does

`#[actorizor::actorize]` is applied to an `impl` block. It reads the block with `syn`, builds an intermediate representation, and emits five pieces of generated code:

1. **The original `impl` block** — unchanged except `handle_msg` is injected onto it
2. **`MyActorMsg` enum** — one variant per public method (not constructors), each carrying the method's parameters plus a `oneshot::Sender` for the response
3. **`MyActorHandle` struct** — wraps an `mpsc::Sender<MyActorMsg>`, is `Clone`-derive'd, exposes async wrappers for every public method and the constructor(s)
4. **`MyActorHandleError` enum** — via `thiserror`, covers send/receive failures
5. **`run_actor` free function** — owns the actor, loops on `receiver.recv()`, dispatches to `handle_msg`

## Key internal types

- `Root` — top-level IR built from `ItemImpl`. Holds idents for all generated type names and the split between `actor_funcs` (methods) and `actor_constructors`.
- `ActorFunc` — represents one function from the impl block. Knows how to emit its enum variant, its handle wrapper, its match arm, and (for constructors) the handle constructor.
- `FuncInput` — a single parameter, knows how to render itself in enum, handle fn, and passthrough positions.

## Constructor detection

A function is treated as a constructor (not a message-dispatched method) if:
- It is `pub`
- Its return type is `Self` or the actor struct's name

Constructors become sync or async `fn` on the handle (matching the original), calling `launch_actor` internally.

## Queue depth

Defaults to `STD_QUEUE_DEPTH = 10`. Override with `#[actorizor::actorize(32)]` — the attribute accepts a single integer literal.

## diagout feature

Enables `eprintln!` of the pretty-printed macro output at compile time. Used for debugging macro expansion. Not relevant to users.

## Known limitations

- Actor structs must not use generics or lifetime parameters. `MyActor<T>` and `MyActor<'a>` will both fail to expand correctly. This is a known gap, not a design decision.
- `pub(super)` and similar restricted visibility are currently passed through but not semantically restricted in generated code (see TODO in `extract_functions_raw`).

## Running tests

```
cargo test -p actor_macro_app
```

The `actor_macro_app` crate is the integration test suite — all meaningful tests live there.
