//! A complete `TrackingSupervisor` walkthrough.
//!
//! Run with:
//!
//! ```text
//! cargo run --example supervisor --features tracking
//! ```
//!
//! Tracing events are emitted by the supervisor on actor exit/panic — the
//! `tracing-subscriber` setup below routes them to stderr so you can see
//! them.

use std::time::Duration;

use actorizor::{TrackingSupervisor, actorize};

#[derive(Debug, Default)]
struct Worker {
    name: String,
    work_done: u64,
}

#[actorize]
impl Worker {
    pub fn new(name: String) -> Self {
        Self {
            name,
            work_done: 0,
        }
    }

    pub fn do_work(&mut self) -> u64 {
        self.work_done += 1;
        self.work_done
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }
}

#[tokio::main]
async fn main() {
    // Route tracing output to stderr so you can see what the supervisor
    // emits when each actor task exits.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // The supervisor is a local value — no global state, no statics. Drop
    // it when you're done and the registry goes with it.
    let sup = TrackingSupervisor::new();
    println!("supervisor: {} actors alive at start", sup.alive_count());

    // Spawn three workers under the same supervisor. Each gets a unique
    // (name, id) entry in the registry.
    let workers: Vec<_> = (1..=3)
        .map(|i| WorkerHandle::launch_with(Worker::new(format!("w{i}")), &sup))
        .collect();

    // Force at least one message through each so we know the actor tasks
    // are running before we inspect the registry.
    for w in &workers {
        let n = w.do_work().await.expect("send/recv");
        let label = w.name().await.expect("send/recv");
        println!("  worker {label} did work_done={n}");
    }

    println!("\nsupervisor snapshot after warm-up:");
    for snap in sup.snapshot() {
        println!(
            "  - name={} id={} alive={}",
            snap.name, snap.id, snap.alive
        );
    }
    println!(
        "supervisor: total alive = {} (by name `Worker` = {})",
        sup.alive_count(),
        sup.alive_count_by_name("Worker"),
    );

    // Abort one specific instance by (name, id). The remaining two stay
    // alive and remain in the registry.
    let snap = sup.snapshot();
    let target = snap.first().expect("at least one worker").id;
    println!("\naborting worker id={target} via abort_by_id");
    let aborted = sup.abort_by_id("Worker", target);
    assert!(aborted, "abort_by_id should find the target");

    // Wait for the supervisor's per-spawn watcher task to observe the
    // abort and prune the registry.
    for _ in 0..50 {
        if sup.alive_count() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    println!("supervisor: alive after abort_by_id = {}", sup.alive_count());
    assert!(!sup.is_alive("Worker", target));

    // Now kill the rest by name. `abort_by_name` returns the number of
    // tracked instances at the moment of the call.
    println!("\nabort_by_name(\"Worker\") to clean up the rest");
    let killed = sup.abort_by_name("Worker");
    println!("  abort_by_name killed {killed} instance(s)");

    for _ in 0..50 {
        if sup.alive_count() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    println!("supervisor: final alive count = {}", sup.alive_count());
    assert_eq!(sup.alive_count(), 0);

    // The handles still exist but the actor tasks are dead — their
    // is_alive() reports false because the AbortHandle inside each Handle
    // is the same AbortHandle the supervisor used to kill them.
    println!("\nhandle.is_alive() per Handle clone (post-shutdown):");
    for (i, w) in workers.iter().enumerate() {
        println!("  worker {} is_alive() = {}", i + 1, w.is_alive());
    }

    println!("\ndone.");
}
