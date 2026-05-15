//! One custom `Supervisor` driving two **differently-typed generic
//! actors** at once.
//!
//! ```text
//! cargo run --example generic_supervisor
//! ```
//!
//! The point: an actor's generic parameter does NOT leak into the
//! supervisor. `Supervisor::spawn` is generic over the *future* type
//! (`F`), never over the actor's `T`, and returns a concrete
//! `AbortHandle`. So a single, concrete `Tracker` value — not
//! `Tracker<Secret>`, not `Tracker<u64>` — drives both `Vault<Secret>`
//! and `Meter<u64>`. If generics "coloured" the surrounding code, this
//! file would not compile (the supervisor would need a `T`, or two
//! different `T`s simultaneously).
//!
//! Two actors ⇒ two modules (the macro emits a module-scoped `run_actor`).

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use actorizor::Supervisor;
use tokio::task::AbortHandle;

/// Concrete, non-generic supervisor. No `<T>` on the type, fields, or
/// impl — only on `spawn`'s actor-agnostic future param `F`.
struct Tracker {
    spawns: Arc<AtomicUsize>,
}

impl Tracker {
    fn new() -> Self {
        Self {
            spawns: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn spawned(&self) -> usize {
        self.spawns.load(Ordering::SeqCst)
    }
}

impl Supervisor for Tracker {
    fn spawn<F>(&self, name: &'static str, fut: F) -> AbortHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let n = self.spawns.fetch_add(1, Ordering::SeqCst) + 1;
        println!("  [tracker] spawn #{n}: {name}");
        tokio::task::spawn(fut).abort_handle()
    }
}

/// A non-primitive custom struct used as one actor's `T`.
#[derive(Debug)]
#[allow(dead_code)]
struct Secret {
    bytes: Vec<u8>,
}

mod vault {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    pub struct Vault<T> {
        held: Vec<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Vault<T> {
        pub fn new() -> Self {
            Self { held: Vec::new() }
        }
        pub fn store(&mut self, v: T) -> usize {
            self.held.push(v);
            self.held.len()
        }
        pub fn count(&self) -> usize {
            self.held.len()
        }
    }
}

mod meter {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    pub struct Meter<T> {
        samples: Vec<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Meter<T> {
        pub fn new() -> Self {
            Self {
                samples: Vec::new(),
            }
        }
        pub fn record(&mut self, v: T) -> usize {
            self.samples.push(v);
            self.samples.len()
        }
        pub fn total(&self) -> usize {
            self.samples.len()
        }
    }
}

#[tokio::main]
async fn main() {
    // ONE supervisor value, reused for two differently-instantiated
    // generic actors.
    let tracker = Tracker::new();

    let vault =
        vault::VaultHandle::<Secret>::launch_with(vault::Vault::new(), &tracker);
    let meter =
        meter::MeterHandle::<u64>::launch_with(meter::Meter::new(), &tracker);

    let n = vault
        .store(Secret {
            bytes: vec![0xDE, 0xAD],
        })
        .await
        .unwrap();
    println!("Vault<Secret>.store -> len {n}");

    println!("Meter<u64>.record(7)  -> len {}", meter.record(7).await.unwrap());
    println!("Meter<u64>.record(8)  -> len {}", meter.record(8).await.unwrap());

    println!(
        "vault.count={}, meter.total={}",
        vault.count().await.unwrap(),
        meter.total().await.unwrap()
    );
    println!(
        "tracker drove {} actor task(s) — one concrete supervisor, two T's",
        tracker.spawned()
    );

    // Per-actor lifecycle still works through the shared supervisor.
    vault.shutdown();
    meter.abort();
    println!("done.");
}
