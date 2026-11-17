use std::thread;
use std::time::{Duration, Instant};

use ol_platform_api::{CancelToken, Clock};
use time::OffsetDateTime;

// How long a sleep goes without looking at its token.
const LOOK: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }

    fn sleep(&self, duration: Duration, cancel: &CancelToken) {
        let end = Instant::now() + duration;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() || cancel.is_cancelled() {
                return;
            }
            thread::sleep(left.min(LOOK));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sleep_lasts_its_length_and_a_stop_ends_it_early() {
        let started = Instant::now();
        SystemClock.sleep(Duration::from_millis(120), &CancelToken::new());
        assert!(started.elapsed() >= Duration::from_millis(120));

        let cancel = CancelToken::new();
        let stop = cancel.clone();
        let stopper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            stop.cancel();
        });
        let started = Instant::now();
        SystemClock.sleep(Duration::from_secs(30), &cancel);
        assert!(started.elapsed() < Duration::from_secs(10));
        stopper.join().unwrap();

        let age = OffsetDateTime::now_utc() - SystemClock.now();
        assert!(age.abs() < time::Duration::seconds(5));
    }
}
