use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

const TICKS_PER_SECOND: u64 = 10_000_000;
const SECONDS_FROM_1601_TO_1970: u64 = 11_644_473_600;

#[derive(Clone, Copy, Debug)]
pub struct MemoryUse {
    pub working_set: usize,
    pub private: usize,
}

pub fn memory() -> Option<MemoryUse> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    }
    .ok()?;
    Some(MemoryUse {
        working_set: counters.WorkingSetSize,
        private: counters.PrivateUsage,
    })
}

pub fn cpu_time() -> Option<Duration> {
    let times = times()?;
    Some(from_ticks(times.kernel + times.user))
}

pub fn age() -> Option<Duration> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let now = (now.as_secs() + SECONDS_FROM_1601_TO_1970) * TICKS_PER_SECOND
        + u64::from(now.subsec_nanos()) / 100;
    Some(from_ticks(now.checked_sub(times()?.created)?))
}

struct Times {
    created: u64,
    kernel: u64,
    user: u64,
}

fn times() -> Option<Times> {
    let mut times = [FILETIME::default(); 4];
    let [created, exited, kernel, user] = &mut times;
    unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) }.ok()?;
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Some(Times {
        created: ticks(times[0]),
        kernel: ticks(times[2]),
        user: ticks(times[3]),
    })
}

fn from_ticks(ticks: u64) -> Duration {
    Duration::from_nanos(ticks * 100)
}

#[expect(
    clippy::disallowed_methods,
    reason = "processes are started here, in ol-platform-win"
)]
pub fn start_again(arguments: &[String]) -> bool {
    std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).args(arguments).spawn())
        .is_ok()
}

#[expect(
    clippy::disallowed_methods,
    reason = "processes are started here, in ol-platform-win"
)]
pub fn run_again(arguments: &[String]) -> std::io::Result<(Option<i32>, String)> {
    let exe = std::env::current_exe()?;
    let ran = std::process::Command::new(exe).args(arguments).output()?;
    Ok((
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
    ))
}
