use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ol_platform_api::{CancelToken, Clock};
use time::OffsetDateTime;

type Callback = Box<dyn FnOnce() + Send>;
type Hook = Arc<dyn Fn() + Send + Sync>;

struct Timer {
    due: OffsetDateTime,
    order: u64,
    run: Callback,
}

struct State {
    now: OffsetDateTime,
    next: u64,
    timers: Vec<Timer>,
    before_sleep: Option<Hook>,
}

// Time that moves only when something sleeps: a sleep ends at once, with the clock set forward by its length and every timer due on the way run in order. A switch that would take a minute takes none, and takes the same steps each time.
pub struct FakeClock {
    state: Mutex<State>,
}

impl FakeClock {
    pub fn starting_at(now: OffsetDateTime) -> Self {
        Self {
            state: Mutex::new(State {
                now,
                next: 0,
                timers: Vec::new(),
                before_sleep: None,
            }),
        }
    }

    // Runs `run` once the clock has moved `delay` on from now.
    pub fn schedule(&self, delay: Duration, run: impl FnOnce() + Send + 'static) {
        let mut state = self.state();
        let order = state.next;
        state.next += 1;
        let due = state.now + delay;
        state.timers.push(Timer {
            due,
            order,
            run: Box::new(run),
        });
    }

    // Called at the start of every sleep, on the thread that sleeps: a test holds the app there while it acts.
    pub fn before_sleep(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.state().before_sleep = Some(Arc::new(hook));
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn take_due(&self, until: OffsetDateTime) -> Option<Callback> {
        let mut state = self.state();
        let at = state
            .timers
            .iter()
            .enumerate()
            .filter(|(_, timer)| timer.due <= until)
            .min_by_key(|(_, timer)| (timer.due, timer.order))
            .map(|(index, _)| index)?;
        let timer = state.timers.remove(at);
        state.now = state.now.max(timer.due);
        Some(timer.run)
    }
}

impl Clock for FakeClock {
    fn now(&self) -> OffsetDateTime {
        self.state().now
    }

    fn sleep(&self, duration: Duration, cancel: &CancelToken) {
        let hook = self.state().before_sleep.clone();
        if let Some(hook) = hook {
            hook();
        }
        if cancel.is_cancelled() {
            return;
        }
        let until = self.now() + duration;
        while let Some(run) = self.take_due(until) {
            run();
        }
        let mut state = self.state();
        state.now = state.now.max(until);
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn a_sleep_moves_the_clock_and_runs_what_was_due_in_order() {
        let clock = Arc::new(FakeClock::starting_at(datetime!(2026-01-02 03:04:05 UTC)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let note = |text: &'static str| {
            let (seen, clock) = (Arc::clone(&seen), Arc::clone(&clock));
            move || seen.lock().unwrap().push((text, clock.now().second()))
        };
        clock.schedule(Duration::from_secs(3), note("third"));
        clock.schedule(Duration::from_secs(1), note("first"));
        clock.schedule(Duration::from_secs(1), note("second"));
        clock.schedule(Duration::from_secs(9), note("later"));
        let again = {
            let (clock, note) = (Arc::clone(&clock), note("nested"));
            move || clock.schedule(Duration::from_secs(1), note)
        };
        clock.schedule(Duration::from_secs(2), again);

        clock.sleep(Duration::from_secs(4), &CancelToken::new());
        assert_eq!(
            *seen.lock().unwrap(),
            [("first", 6), ("second", 6), ("third", 8), ("nested", 8)]
        );
        assert_eq!(clock.now(), datetime!(2026-01-02 03:04:09 UTC));
    }

    #[test]
    fn a_cancelled_sleep_moves_nothing() {
        let clock = FakeClock::starting_at(datetime!(2026-01-02 03:04:05 UTC));
        let cancel = CancelToken::new();
        cancel.cancel();
        clock.sleep(Duration::from_secs(4), &cancel);
        assert_eq!(clock.now(), datetime!(2026-01-02 03:04:05 UTC));
    }
}
