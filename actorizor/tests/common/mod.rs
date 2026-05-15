//! Shared test helpers. `tests/common/mod.rs` is treated by cargo as a
//! module (not its own test binary); each test file does `mod common;`.

#![allow(dead_code)] // not every test file uses every helper

use std::time::Duration;

/// Poll `condition` every 5ms until it returns `true` or `timeout`
/// elapses. Returns whether the condition was met. Replaces the
/// hand-rolled `for _ in 0..50 { … sleep(5ms) }` loops so the budget is
/// stated once and uniformly.
pub async fn wait_until<F>(mut condition: F, timeout: Duration) -> bool
where
    F: FnMut() -> bool,
{
    let start = tokio::time::Instant::now();
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    condition()
}

/// The default budget for actor lifecycle transitions in tests. These are
/// in-process operations that normally complete in microseconds; the
/// generous budget only matters under a heavily loaded CI box.
pub const SETTLE: Duration = Duration::from_secs(1);
