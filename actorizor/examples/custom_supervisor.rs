//! Implementing the `Supervisor` trait yourself.
//!
//! ```text
//! cargo run --example custom_supervisor
//! ```
//!
//! `TokioSpawn` (the default, no-op) and `TrackingSupervisor` (behind the
//! `tracking` feature) cover most needs, but the trait is small and you can
//! implement it for whatever bookkeeping you want. This example builds a
//! supervisor that:
//!
//! - assigns a sequential id to every spawned actor,
//! - logs spawn + exit (clean / panic / abort) to stdout,
//! - keeps a count of currently-live actors.
//!
//! The key point: the supervisor is an **owned value**. You construct it in
//! `main` (or per test), pass `&supervisor` into `launch_with`, and it drops
//! when it goes out of scope. No globals, no statics.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use actorizor::{Supervisor, actorize};
use tokio::task::AbortHandle;

struct LoggingSupervisor {
    next_id: AtomicU64,
    live: Arc<AtomicU64>,
}

impl LoggingSupervisor {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(0),
            live: Arc::new(AtomicU64::new(0)),
        }
    }

    fn live_count(&self) -> u64 {
        self.live.load(Ordering::SeqCst)
    }
}

impl Supervisor for LoggingSupervisor {
    fn spawn<F>(&self, name: &'static str, fut: F) -> AbortHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let live = self.live.clone();
        live.fetch_add(1, Ordering::SeqCst);
        println!("  [sup] spawn  {name}#{id}  (live now: {})", live.load(Ordering::SeqCst));

        // Spawn the actor task and ALSO spawn a watcher that awaits its
        // JoinHandle so we learn how it ended. Returning the AbortHandle is
        // the trait's only hard requirement — actorizor stashes it in the
        // generated handle so `handle.abort()` works.
        let jh = tokio::task::spawn(fut);
        let abort = jh.abort_handle();
        tokio::spawn(async move {
            let how = match jh.await {
                Ok(()) => "clean",
                Err(e) if e.is_cancelled() => "aborted",
                Err(e) if e.is_panic() => "panicked",
                Err(_) => "join-error",
            };
            let remaining = live.fetch_sub(1, Ordering::SeqCst) - 1;
            println!("  [sup] exit   {name}#{id}  ({how}; live now: {remaining})");
        });
        abort
    }
}

#[derive(Debug, Default)]
struct Echo {
    last: u64,
}

#[actorize]
impl Echo {
    pub fn new() -> Self {
        Self { last: 0 }
    }

    pub fn echo(&mut self, v: u64) -> u64 {
        self.last = v;
        v
    }
}

#[tokio::main]
async fn main() {
    let sup = LoggingSupervisor::new();

    println!("launch two Echo actors under the custom supervisor:");
    let a = EchoHandle::launch_with(Echo::new(), &sup);
    let b = EchoHandle::launch_with(Echo::new(), &sup);

    println!("a.echo(11) -> {}", a.echo(11).await.unwrap());
    println!("b.echo(22) -> {}", b.echo(22).await.unwrap());
    println!("supervisor live_count = {}", sup.live_count());

    println!("\nabort a, cooperatively shut down b:");
    a.abort();
    b.shutdown();

    // Give the watcher tasks a moment to observe the exits and log them.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    println!("supervisor live_count = {}", sup.live_count());
}
