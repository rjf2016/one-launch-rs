use std::thread;
use std::time::{Duration, Instant};

use ol_platform_api::{CancelToken, Clock};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_STIMEFORMAT};
use windows::Win32::System::Time::{
    DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation,
    SystemTimeToTzSpecificLocalTimeEx, TIME_ZONE_ID_INVALID,
};
use windows::core::PCWSTR;

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

pub fn local_offset(unix_seconds: i64) -> Option<i32> {
    offset_in(None, unix_seconds)
}

// The regional settings' time format: a capital H is the 24-hour clock in Windows' format pictures.
pub fn uses_24_hour_clock() -> bool {
    let mut picture = [0_u16; 80];
    let written =
        unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_STIMEFORMAT, Some(&mut picture)) };
    let Some(written) = usize::try_from(written).ok().filter(|written| *written > 0) else {
        return false;
    };
    String::from_utf16_lossy(&picture[..written - 1]).contains('H')
}

// `zone` None is this PC's own. A zone named by its key keeps the daylight-saving rules each year had, as a stored date needs.
fn offset_in(zone: Option<&DYNAMIC_TIME_ZONE_INFORMATION>, unix_seconds: i64) -> Option<i32> {
    let mut own = DYNAMIC_TIME_ZONE_INFORMATION::default();
    let zone = match zone {
        Some(zone) => zone,
        None => {
            if unsafe { GetDynamicTimeZoneInformation(&mut own) } == TIME_ZONE_ID_INVALID {
                return None;
            }
            &own
        }
    };
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
    unsafe {
        SystemTimeToTzSpecificLocalTimeEx(Some(std::ptr::from_ref(zone)), &universal, &mut local)
    }
    .ok()?;
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

    // A rule of Windows' own: the `day`th Sunday of `month`, 5 being its last.
    fn sunday(month: u16, day: u16, hour: u16) -> SYSTEMTIME {
        SYSTEMTIME {
            wMonth: month,
            wDayOfWeek: 0,
            wDay: day,
            wHour: hour,
            ..SYSTEMTIME::default()
        }
    }

    fn zone(
        key: &str,
        bias: i32,
        standard: SYSTEMTIME,
        daylight: SYSTEMTIME,
    ) -> DYNAMIC_TIME_ZONE_INFORMATION {
        let mut name = [0_u16; 128];
        for (slot, unit) in name.iter_mut().zip(key.encode_utf16()) {
            *slot = unit;
        }
        DYNAMIC_TIME_ZONE_INFORMATION {
            Bias: bias,
            StandardDate: standard,
            DaylightDate: daylight,
            DaylightBias: -60,
            TimeZoneKeyName: name,
            ..DYNAMIC_TIME_ZONE_INFORMATION::default()
        }
    }

    fn at(text: &str) -> i64 {
        OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
            .unwrap()
            .unix_timestamp()
    }

    #[test]
    fn the_offset_follows_the_zone_s_summer_time_for_the_date_given() {
        let zone = zone(
            "W. Europe Standard Time",
            -60,
            sunday(10, 5, 3),
            sunday(3, 5, 2),
        );
        assert_eq!(
            offset_in(Some(&zone), at("2026-01-15T12:00:00Z")),
            Some(3600)
        );
        assert_eq!(
            offset_in(Some(&zone), at("2026-07-15T12:00:00Z")),
            Some(7200)
        );
        assert_eq!(
            offset_in(Some(&zone), at("2026-03-29T00:59:00Z")),
            Some(3600)
        );
        assert_eq!(
            offset_in(Some(&zone), at("2026-03-29T01:00:00Z")),
            Some(7200)
        );
    }

    // Eastern Time with today's rules: in 2006 summer time began on 2 April, where today's rules would start it on 12 March.
    #[test]
    fn a_zone_named_by_its_key_keeps_the_rules_of_the_year() {
        let zone = zone(
            "Eastern Standard Time",
            300,
            sunday(11, 1, 2),
            sunday(3, 2, 2),
        );
        assert_eq!(
            offset_in(Some(&zone), at("2006-03-20T12:00:00Z")),
            Some(-5 * 3600)
        );
        assert_eq!(
            offset_in(Some(&zone), at("2026-03-20T12:00:00Z")),
            Some(-4 * 3600)
        );
    }

    #[test]
    fn this_pc_s_offset_is_one_a_zone_can_have() {
        let offset = local_offset(OffsetDateTime::now_utc().unix_timestamp())
            .expect("this PC has a time zone");
        assert!(offset.abs() <= 14 * 3600, "{offset}");
        assert_eq!(offset % 900, 0, "{offset}");
    }
}
