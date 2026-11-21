use std::thread;
use std::time::{Duration, Instant};

use ol_platform_api::{CancelToken, Clock};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

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

// Seconds east of UTC on this PC at that moment, by the rules of its time zone for that date, summer time included.
pub fn local_offset(unix_seconds: i64) -> Option<i32> {
    let utc = OffsetDateTime::from_unix_timestamp(unix_seconds).ok()?;
    let year = u16::try_from(utc.year()).ok()?;
    let universal = SYSTEMTIME {
        wYear: year,
        wMonth: u16::from(u8::from(utc.month())),
        wDayOfWeek: 0,
        wDay: u16::from(utc.day()),
        wHour: u16::from(utc.hour()),
        wMinute: u16::from(utc.minute()),
        wSecond: u16::from(utc.second()),
        wMilliseconds: 0,
    };
    let mut local = SYSTEMTIME::default();
    unsafe { SystemTimeToTzSpecificLocalTime(None, &universal, &mut local) }.ok()?;
    let local = PrimitiveDateTime::new(
        Date::from_calendar_date(
            i32::from(local.wYear),
            Month::try_from(u8::try_from(local.wMonth).ok()?).ok()?,
            u8::try_from(local.wDay).ok()?,
        )
        .ok()?,
        Time::from_hms(
            u8::try_from(local.wHour).ok()?,
            u8::try_from(local.wMinute).ok()?,
            u8::try_from(local.wSecond).ok()?,
        )
        .ok()?,
    );
    let universal = PrimitiveDateTime::new(utc.date(), utc.time().replace_nanosecond(0).ok()?);
    i32::try_from((local - universal).whole_seconds()).ok()
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

    #[test]
    fn the_local_offset_is_a_zone_s_and_follows_the_date() {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let offset = local_offset(now).expect("this PC has a time zone");
        assert!(offset.abs() <= 14 * 3600, "{offset}");
        assert_eq!(offset % 900, 0, "{offset}");
        let half_a_year = 183 * 24 * 3600;
        let later = local_offset(now + half_a_year).expect("a date half a year on");
        assert!((later - offset).abs() <= 3600, "{offset} {later}");
    }
}
