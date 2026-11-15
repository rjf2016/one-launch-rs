use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use ol_platform_api::{CancelToken, Clock};
use time::OffsetDateTime;

type Callback = Box<dyn FnOnce() + Send>;
type Hook = Arc<dyn Fn() + Send + Sync>;

// How long a sleep that follows the system's clock goes without looking at its timers and its token.
const LOOK: Duration = Duration::from_millis(20);

struct Timer {
    due: OffsetDateTime,
    order: u64,
    run: Callback,
}

struct State {
    now: OffsetDateTime,
    follows_the_system: bool,
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
        Self::new(now, false)
    }

    // For a person who watches a fake launcher: the time is the system's and a sleep lasts as long as it says. A timer still runs on the thread that sleeps, once its time has come.
    pub fn following_the_system() -> Self {
        Self::new(OffsetDateTime::now_utc(), true)
    }

    fn new(now: OffsetDateTime, follows_the_system: bool) -> Self {
        Self {
            state: Mutex::new(State {
                now,
                follows_the_system,
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
        let due = state.read() + delay;
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

    fn sleep_with_the_system(&self, duration: Duration, cancel: &CancelToken) {
        let end = Instant::now() + duration;
        loop {
            while let Some(run) = self.take_due(OffsetDateTime::now_utc()) {
                run();
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() || cancel.is_cancelled() {
                return;
            }
            thread::sleep(left.min(LOOK));
        }
    }
}

impl State {
    fn read(&self) -> OffsetDateTime {
        if self.follows_the_system {
            OffsetDateTime::now_utc()
        } else {
            self.now
        }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> OffsetDateTime {
        self.state().read()
    }

    fn sleep(&self, duration: Duration, cancel: &CancelToken) {
        let (hook, follows_the_system) = {
            let state = self.state();
            (state.before_sleep.clone(), state.follows_the_system)
        };
        if let Some(hook) = hook {
            hook();
        }
        if cancel.is_cancelled() {
            return;
        }
        if follows_the_system {
            return self.sleep_with_the_system(duration, cancel);
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
    fn a_clock_that_follows_the_system_sleeps_for_real_and_runs_what_came_due() {
        let clock = FakeClock::following_the_system();
        let ran = Arc::new(Mutex::new(None));
        let (seen, started) = (Arc::clone(&ran), Instant::now());
        clock.schedule(Duration::from_millis(30), move || {
            *seen.lock().unwrap() = Some(started.elapsed());
        });
        let before = clock.now();
        clock.sleep(Duration::from_millis(120), &CancelToken::new());
        assert!(started.elapsed() >= Duration::from_millis(120));
        let ran_after = ran.lock().unwrap().expect("the timer ran during the sleep");
        assert!(ran_after >= Duration::from_millis(30));
        assert!(clock.now() >= before);
    }

    #[test]
    fn a_stop_ends_a_real_sleep_early() {
        let clock = FakeClock::following_the_system();
        let cancel = CancelToken::new();
        let stop = cancel.clone();
        let stopper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            stop.cancel();
        });
        let started = Instant::now();
        clock.sleep(Duration::from_secs(30), &cancel);
        assert!(started.elapsed() < Duration::from_secs(10));
        stopper.join().unwrap();
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
