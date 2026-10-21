//! Wall-clock timestamps and interruptible worker waits.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Returns Unix epoch seconds, or zero if the system clock predates the epoch.
pub fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or(0.0)
}

/// Sleeps in short slices so a stop request is acted on quickly.
pub fn sleep_until(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(100);
    let mut left = total;
    while left > Duration::ZERO {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let slice = step.min(left);
        std::thread::sleep(slice);
        left -= slice;
    }
}
