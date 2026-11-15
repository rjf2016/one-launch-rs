use std::time::Duration;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

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
    let mut times = [FILETIME::default(); 4];
    let [created, exited, kernel, user] = &mut times;
    unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) }.ok()?;
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Some(Duration::from_nanos(
        (ticks(times[2]) + ticks(times[3])) * 100,
    ))
}
