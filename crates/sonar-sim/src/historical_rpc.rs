//! Process-wide limits for historical account RPC workflows.

use std::sync::{Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

const REQUEST_INTERVAL: Duration = Duration::from_micros(12_500); // 80 requests/second

pub(crate) fn concurrency_for_cores(cores: usize) -> usize {
    cores.saturating_mul(2).clamp(4, 16)
}

pub(crate) fn concurrency() -> usize {
    static CONCURRENCY: LazyLock<usize> = LazyLock::new(|| {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        concurrency_for_cores(cores)
    });
    *CONCURRENCY
}

struct State {
    active: usize,
    next_start: Instant,
}

struct Limits {
    max_active: usize,
    interval: Duration,
    state: Mutex<State>,
    available: Condvar,
}

impl Limits {
    fn new(max_active: usize, interval: Duration) -> Self {
        Self {
            max_active,
            interval,
            state: Mutex::new(State { active: 0, next_start: Instant::now() }),
            available: Condvar::new(),
        }
    }

    fn acquire(&self) -> Permit<'_> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.active == self.max_active {
                state = self.available.wait(state).unwrap();
                continue;
            }
            let now = Instant::now();
            if now < state.next_start {
                let delay = state.next_start - now;
                state = self.available.wait_timeout(state, delay).unwrap().0;
                continue;
            }
            state.active += 1;
            state.next_start = now + self.interval;
            return Permit { limits: self };
        }
    }
}

pub(crate) struct Permit<'a> {
    limits: &'a Limits,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut state = self.limits.state.lock().unwrap();
        state.active -= 1;
        self.limits.available.notify_all();
    }
}

pub(crate) fn acquire() -> Permit<'static> {
    static LIMITS: LazyLock<Limits> =
        LazyLock::new(|| Limits::new(concurrency(), REQUEST_INTERVAL));
    LIMITS.acquire()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::*;

    #[test]
    fn concurrency_scales_with_logical_cores_and_is_bounded() {
        for (cores, expected) in
            [(0, 4), (1, 4), (2, 4), (4, 8), (8, 16), (64, 16), (usize::MAX, 16)]
        {
            assert_eq!(concurrency_for_cores(cores), expected);
        }
    }

    #[test]
    fn request_starts_are_spaced_without_accumulated_bursts() {
        let limits = Limits::new(4, REQUEST_INTERVAL);
        let started = Instant::now();
        thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let _permit = limits.acquire();
                    assert!(limits.state.lock().unwrap().active <= 4);
                });
            }
        });
        // Seven intervals are needed for eight starts, even with spare permits.
        assert!(started.elapsed() >= REQUEST_INTERVAL * 7);
        thread::sleep(REQUEST_INTERVAL * 2);
        let _first = limits.acquire();
        let started = Instant::now();
        let _second = limits.acquire();
        assert!(started.elapsed() >= REQUEST_INTERVAL / 2);
    }

    #[test]
    fn shared_limit_caps_in_flight_requests_and_releases_permits() {
        let limits = Limits::new(2, Duration::ZERO);
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let _permit = limits.acquire();
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(10));
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert_eq!(peak.load(Ordering::SeqCst), 2);
        assert_eq!(limits.state.lock().unwrap().active, 0);
    }
}
