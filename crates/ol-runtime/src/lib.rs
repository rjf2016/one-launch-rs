#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::time::Duration;

use ol_platform_api::{CancelToken, Clock};

// A wait ended because the switch was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aborted;

// Looks every `poll` until `cond` holds (true) or `timeout` has passed (false). The order is the Electron app's: stopped, condition, deadline, sleep.
pub fn wait_until<E: From<Aborted>>(
    clock: &dyn Clock,
    cancel: &CancelToken,
    timeout: Duration,
    poll: Duration,
    mut cond: impl FnMut() -> Result<bool, E>,
) -> Result<bool, E> {
    let deadline = clock.now() + timeout;
    loop {
        if cancel.is_cancelled() {
            return Err(Aborted.into());
        }
        if cond()? {
            return Ok(true);
        }
        if clock.now() >= deadline {
            return Ok(false);
        }
        clock.sleep(poll, cancel);
    }
}

// Waits an adapter is told to use in place of its own, by name and in milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimingOverrides(BTreeMap<String, u64>);

impl TimingOverrides {
    pub fn set(&mut self, name: &str, milliseconds: u64) {
        self.0.insert(name.to_owned(), milliseconds);
    }

    pub fn get(&self, name: &str) -> Option<Duration> {
        self.0.get(name).copied().map(Duration::from_millis)
    }

    // A finer poll is never slower than the coarse one: shortening pollMs alone brings a poll that was not named down to it.
    pub fn finer_poll(&self, name: &str, default: Duration) -> Duration {
        match (self.get(name), self.get("pollMs")) {
            (Some(named), _) => named,
            (None, Some(coarse)) => default.min(coarse),
            (None, None) => default,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use time::OffsetDateTime;
    use time::macros::datetime;

    use super::*;

    struct Stepping(Mutex<OffsetDateTime>);

    impl Clock for Stepping {
        fn now(&self) -> OffsetDateTime {
            *self.0.lock().unwrap()
        }

        fn sleep(&self, duration: Duration, cancel: &CancelToken) {
            if !cancel.is_cancelled() {
                *self.0.lock().unwrap() += duration;
            }
        }
    }

    fn clock() -> Stepping {
        Stepping(Mutex::new(datetime!(2026-01-02 03:04:05 UTC)))
    }

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn it_ends_when_the_condition_holds_or_the_time_is_up() {
        let clock = clock();
        let never = CancelToken::new();
        let mut looks = 0;
        let held: Result<bool, Aborted> = wait_until(&clock, &never, 5 * SECOND, SECOND, || {
            looks += 1;
            Ok(looks == 3)
        });
        assert_eq!((held, looks), (Ok(true), 3));

        let mut looks = 0;
        let held: Result<bool, Aborted> = wait_until(&clock, &never, 2 * SECOND, SECOND, || {
            looks += 1;
            Ok(false)
        });
        assert_eq!((held, looks), (Ok(false), 3));
    }

    #[test]
    fn a_stopped_wait_ends_before_it_looks_again() {
        let clock = clock();
        let cancel = CancelToken::new();
        let mut looks = 0;
        let held: Result<bool, Aborted> = wait_until(&clock, &cancel, 5 * SECOND, SECOND, || {
            looks += 1;
            cancel.cancel();
            Ok(false)
        });
        assert_eq!((held, looks), (Err(Aborted), 1));
    }

    #[test]
    fn a_shorter_coarse_poll_shortens_a_finer_one_that_was_not_named() {
        let quarter = Duration::from_millis(250);
        let mut overrides = TimingOverrides::default();
        assert_eq!(overrides.finer_poll("logPollMs", quarter), quarter);
        overrides.set("pollMs", 100);
        assert_eq!(
            overrides.finer_poll("logPollMs", quarter),
            Duration::from_millis(100)
        );
        overrides.set("pollMs", 900);
        assert_eq!(overrides.finer_poll("logPollMs", quarter), quarter);
        overrides.set("logPollMs", 400);
        assert_eq!(
            overrides.finer_poll("logPollMs", quarter),
            Duration::from_millis(400)
        );
    }
}
