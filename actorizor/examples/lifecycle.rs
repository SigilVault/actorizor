//! The three ways an actor task ends, and the liveness queries.
//!
//! ```text
//! cargo run --example lifecycle
//! ```
//!
//! 1. **Natural** — drop every handle clone; `recv()` yields `None`, the
//!    loop exits.
//! 2. **Cooperative** — `handle.shutdown()`; the current message finishes,
//!    then the biased `select!` exits. Handle clones can stay alive.
//! 3. **Forceful** — `handle.abort()`; the task is cancelled at its next
//!    `.await`, mid-message.
//!
//! `is_alive()` / `is_finished()` observe the task state in all cases.

use std::time::Duration;

use actorizor::actorize;

#[derive(Debug, Default)]
struct Service {
    hits: u64,
}

// A Drop impl makes the actor's end observable. Note it fires for ALL
// three exit mechanisms — natural drop, cooperative shutdown, and forceful
// abort — because however the task ends, `run_actor` returns and the owned
// `Service` value is dropped. (You can't observe natural exit *through* a
// handle: holding one to query liveness would itself keep the actor alive,
// which is exactly why the Drop impl is the honest way to show it.)
impl Drop for Service {
    fn drop(&mut self) {
        println!("  [drop] Service value dropped (hits={}) — its actor task has ended", self.hits);
    }
}

#[actorize]
impl Service {
    pub fn new() -> Self {
        Self { hits: 0 }
    }

    pub fn hit(&mut self) -> u64 {
        self.hits += 1;
        self.hits
    }
}

async fn settle(label: &str, h: &ServiceHandle) {
    for _ in 0..50 {
        if h.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    println!(
        "  [{label}] is_alive={} is_finished={}",
        h.is_alive(),
        h.is_finished()
    );
}

#[tokio::main]
async fn main() {
    // 1. Natural exit when the last clone drops.
    {
        println!("natural: spawn, hit twice, then drop every clone");
        let h = ServiceHandle::new();
        let h2 = h.clone();
        h.hit().await.unwrap();
        h2.hit().await.unwrap();
        drop(h);
        // Still one clone (h2) alive → actor still running, no Drop yet.
        drop(h2);
        // Last clone gone: recv() yields None, loop exits, Service drops.
        // Yield so the actor task gets scheduled to observe the closed
        // channel and run its Drop.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // 2. Cooperative shutdown — clones stay in scope, task still exits.
    {
        let h = ServiceHandle::new();
        let keep = h.clone();
        println!("\ncooperative: hit -> {}", h.hit().await.unwrap());
        h.shutdown();
        settle("cooperative", &h).await;
        // `keep` still exists but the loop is gone; calls now error.
        println!(
            "  post-shutdown call errors as expected: {}",
            keep.hit().await.is_err()
        );
    }

    // 3. Forceful abort — kills the task immediately.
    {
        let h = ServiceHandle::new();
        println!("\nforceful: hit -> {}", h.hit().await.unwrap());
        h.abort();
        settle("forceful", &h).await;
        println!(
            "  post-abort call errors as expected: {}",
            h.hit().await.is_err()
        );
    }
}
