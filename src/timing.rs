//! Phase timings for latency work.
//!
//! `VOX_TIMINGS=1 vox "..."` prints, on stderr, how long after launch each
//! step of an utterance finished and how long the step itself took. Off, a
//! mark costs one atomic load.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct Clock {
    launched: Instant,
    last: Mutex<Instant>,
}

static CLOCK: OnceLock<Option<Clock>> = OnceLock::new();

fn clock() -> Option<&'static Clock> {
    CLOCK
        .get_or_init(|| {
            std::env::var_os("VOX_TIMINGS")
                .filter(|v| !v.is_empty() && v != "0")
                .map(|_| {
                    let now = Instant::now();
                    Clock {
                        launched: now,
                        last: Mutex::new(now),
                    }
                })
        })
        .as_ref()
}

/// Start the clock. Called first thing in `main`, so the times count from
/// launch rather than from the first mark.
pub fn start() {
    clock();
}

/// Report that a step just finished.
pub fn mark(step: &str) {
    let Some(clock) = clock() else { return };
    let now = Instant::now();
    let mut last = clock.last.lock().unwrap_or_else(|e| e.into_inner());
    eprintln!(
        "[timing] {:>7.1} ms  +{:>7.1} ms  {step}",
        now.duration_since(clock.launched).as_secs_f64() * 1000.0,
        now.duration_since(*last).as_secs_f64() * 1000.0,
    );
    *last = now;
}
