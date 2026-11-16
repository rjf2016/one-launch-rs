use std::ffi::c_void;
use std::mem::{offset_of, size_of};

use ol_platform_api::ProcessError;
use windows::Wdk::System::SystemInformation::{
    NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS, SystemProcessInformation,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, HWND, LPARAM,
    STATUS_INFO_LENGTH_MISMATCH, UNICODE_STRING, WAIT_OBJECT_0, WIN32_ERROR, WPARAM,
};
use windows::Win32::Storage::FileSystem::{GetLogicalDrives, QueryDosDeviceW};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, GetCurrentProcess,
    GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, STARTUPINFOW,
    TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    WM_CLOSE,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

use super::{Ended, Entry, Posted, System, TopWindow};
use crate::wide;

const SYSTEM_PROCESS_ID_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(88);
const FIRST_TABLE_BYTES: usize = 1 << 20;
const ATTEMPTS: usize = 8;
const NETWORK: &str = r"\Device\Mup\";

// The head of each row SystemProcessInformation writes, as winternl.h and every reader of the table since Windows XP lay it out, up to the session's id. ToolHelp reads this same table and drops the start time, which is what tells a parent from a process that took its id later.
#[repr(C)]
struct ProcessRow {
    next_entry_offset: u32,
    number_of_threads: u32,
    working_set_private_size: i64,
    hard_fault_count: u32,
    number_of_threads_high_watermark: u32,
    cycle_time: u64,
    create_time: i64,
    user_time: i64,
    kernel_time: i64,
    image_name: UNICODE_STRING,
    base_priority: i32,
    unique_process_id: HANDLE,
    inherited_from_unique_process_id: HANDLE,
    handle_count: u32,
    session_id: u32,
}

#[repr(C)]
struct ProcessIdInformation {
    process_id: HANDLE,
    image_name: UNICODE_STRING,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Win32;

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn win32_code(error: &windows::core::Error) -> u32 {
    WIN32_ERROR::from_error(error).map_or(error.code().0 as u32, |code| code.0)
}

fn listing_failed(why: &str) -> ProcessError {
    ProcessError::Failed(format!("The process list could not be read ({why})"))
}

fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn created(process: HANDLE) -> Option<u64> {
    let mut times = [FILETIME::default(); 4];
    let [created, exited, kernel, user] = &mut times;
    unsafe { GetProcessTimes(process, created, exited, kernel, user) }.ok()?;
    Some(ticks(times[0]))
}

fn read<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

fn read_usize(bytes: &[u8], at: usize) -> Option<usize> {
    read::<{ size_of::<usize>() }>(bytes, at).map(usize::from_ne_bytes)
}

// The table is parsed from its bytes, not cast: a row or a name that would reach outside the buffer ends the read as a failure.
fn rows(table: &[u8]) -> Option<Vec<Entry>> {
    let base = table.as_ptr() as usize;
    let name_at = offset_of!(ProcessRow, image_name);
    let mut entries = Vec::new();
    let mut at = 0_usize;
    loop {
        let row = table.get(at..at.checked_add(size_of::<ProcessRow>())?)?;
        let next = u32::from_ne_bytes(read(row, offset_of!(ProcessRow, next_entry_offset))?);
        let started = i64::from_ne_bytes(read(row, offset_of!(ProcessRow, create_time))?);
        let length = usize::from(u16::from_ne_bytes(read(row, name_at)?));
        let pointer = read_usize(row, name_at + offset_of!(UNICODE_STRING, Buffer))?;
        let pid = read_usize(row, offset_of!(ProcessRow, unique_process_id))?;
        let parent = read_usize(
            row,
            offset_of!(ProcessRow, inherited_from_unique_process_id),
        )?;
        let session = u32::from_ne_bytes(read(row, offset_of!(ProcessRow, session_id))?);
        let name = if pointer == 0 {
            String::new()
        } else {
            let from = pointer.checked_sub(base)?;
            let units: Vec<u16> = table
                .get(from..from.checked_add(length)?)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_ne_bytes(*pair))
                .collect();
            String::from_utf16_lossy(&units)
        };
        entries.push(Entry {
            pid: u32::try_from(pid).ok()?,
            parent: u32::try_from(parent).unwrap_or(0),
            created: u64::try_from(started).ok()?,
            session,
            name,
        });
        if next == 0 {
            return Some(entries);
        }
        at = at.checked_add(next as usize)?;
    }
}

fn own_session(pid: u32) -> Option<u32> {
    let mut session = 0_u32;
    unsafe { ProcessIdToSessionId(pid, &raw mut session) }.ok()?;
    Some(session)
}

// A table read with the wrong layout must not choose what is ended: it has to name this very process, under its own file name, started when Windows says it started, in the session Windows says it runs in.
fn agrees_on_this_process(entries: &[Entry]) -> bool {
    let own = unsafe { GetCurrentProcessId() };
    let started = created(unsafe { GetCurrentProcess() });
    let session = own_session(own);
    let name = std::env::current_exe().ok().and_then(|exe| {
        exe.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    entries.iter().any(|entry| {
        entry.pid == own
            && Some(entry.created) == started
            && Some(entry.session) == session
            && name
                .as_ref()
                .is_none_or(|name| entry.name.to_lowercase() == *name)
    })
}

fn snapshot() -> Result<Vec<Entry>, ProcessError> {
    let mut size = FIRST_TABLE_BYTES;
    for _ in 0..ATTEMPTS {
        let mut buffer = vec![0_u64; size.div_ceil(8)];
        let length = buffer.len() * 8;
        let given = u32::try_from(length).map_err(|_| listing_failed("too large"))?;
        let mut needed = 0_u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessInformation,
                buffer.as_mut_ptr().cast::<c_void>(),
                given,
                &raw mut needed,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            size = size.max(needed as usize) + (1 << 16);
            continue;
        }
        if status.is_err() {
            return Err(listing_failed(&format!("status {:#x}", status.0)));
        }
        // The names in the table point into this buffer, so it is read where Windows wrote it.
        let table = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length) };
        let entries = rows(table).ok_or_else(|| listing_failed("a row out of place"))?;
        if !agrees_on_this_process(&entries) {
            return Err(listing_failed("the table does not name this process"));
        }
        return Ok(entries);
    }
    Err(listing_failed("it kept growing"))
}

// Where each drive letter leads in the kernel's names, such as \Device\HarddiskVolume3\ for C:\.
fn volumes() -> Vec<(String, String)> {
    let drives = unsafe { GetLogicalDrives() };
    let mut found = Vec::new();
    for (index, letter) in (b'A'..=b'Z').enumerate() {
        if drives & (1 << index) == 0 {
            continue;
        }
        let drive = format!("{}:", char::from(letter));
        let name = wide(&drive);
        let mut target = vec![0_u16; 1024];
        let written = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(&mut target)) };
        let first: Vec<u16> = target[..(written as usize).min(target.len())]
            .iter()
            .copied()
            .take_while(|unit| *unit != 0)
            .collect();
        if !first.is_empty() {
            found.push((
                format!("{}\\", String::from_utf16_lossy(&first)),
                format!("{drive}\\"),
            ));
        }
    }
    found
}

fn starts_with_ignoring_case(text: &str, start: &str) -> bool {
    text.get(..start.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(start))
}

// A kernel name as a path on a drive letter or a share, or nothing when it is neither.
fn dos_path(native: &str, volumes: &[(String, String)]) -> Option<String> {
    for (device, drive) in volumes {
        if starts_with_ignoring_case(native, device) {
            return Some(format!("{drive}{}", &native[device.len()..]));
        }
    }
    if !starts_with_ignoring_case(native, NETWORK) {
        return None;
    }
    let mut rest = &native[NETWORK.len()..];
    while rest.starts_with(';') {
        rest = &rest[rest.find('\\')? + 1..];
    }
    Some(format!(r"\\{rest}"))
}

// The file of a process, from the kernel's table of ids and images: the process is not opened.
fn native_path(pid: u32) -> Option<String> {
    let mut units = 1024_usize;
    for _ in 0..2 {
        let mut name = vec![0_u16; units];
        let mut info = ProcessIdInformation {
            process_id: HANDLE(pid as usize as *mut c_void),
            image_name: UNICODE_STRING {
                Length: 0,
                MaximumLength: u16::try_from(units * 2).ok()?,
                Buffer: PWSTR(name.as_mut_ptr()),
            },
        };
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESS_ID_INFORMATION,
                (&raw mut info).cast::<c_void>(),
                size_of::<ProcessIdInformation>() as u32,
                std::ptr::null_mut(),
            )
        };
        if status.is_ok() {
            let length = (usize::from(info.image_name.Length) / 2).min(name.len());
            return Some(String::from_utf16_lossy(&name[..length]));
        }
        let wanted = usize::from(info.image_name.MaximumLength) / 2;
        if status != STATUS_INFO_LENGTH_MISMATCH || wanted <= units {
            return None;
        }
        units = wanted.min(usize::from(u16::MAX) / 2);
    }
    None
}

unsafe extern "system" fn keep_window(hwnd: HWND, found: LPARAM) -> BOOL {
    let mut pid = 0_u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    let owned = unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid());
    let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
    let windows = unsafe { &mut *(found.0 as *mut Vec<TopWindow>) };
    windows.push(TopWindow {
        handle: hwnd.0 as isize,
        pid,
        owned,
        visible,
    });
    true.into()
}

// Opened only to end it, with no more rights than that takes and a look at when it started.
fn open(pid: u32) -> Result<Handle, Ended> {
    let rights = PROCESS_TERMINATE | PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION;
    match unsafe { OpenProcess(rights, false, pid) } {
        Ok(handle) => Ok(Handle(handle)),
        Err(error) => Err(match WIN32_ERROR::from_error(&error) {
            Some(ERROR_ACCESS_DENIED) => Ended::Refused,
            Some(ERROR_INVALID_PARAMETER) => Ended::Gone,
            _ => Ended::Failed(win32_code(&error)),
        }),
    }
}

fn terminate(process: &Handle) -> Ended {
    match unsafe { TerminateProcess(process.0, 1) } {
        Ok(()) => Ended::Yes,
        Err(_) if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 => Ended::Gone,
        Err(error) if WIN32_ERROR::from_error(&error) == Some(ERROR_ACCESS_DENIED) => {
            Ended::Refused
        }
        Err(error) => Ended::Failed(win32_code(&error)),
    }
}

impl System for Win32 {
    fn own_pid(&self) -> u32 {
        unsafe { GetCurrentProcessId() }
    }

    fn snapshot(&self) -> Result<Vec<Entry>, ProcessError> {
        snapshot()
    }

    fn top_windows(&self) -> Vec<TopWindow> {
        let mut windows: Vec<TopWindow> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(keep_window), LPARAM(&raw mut windows as isize));
        }
        windows
    }

    fn post_close(&self, window: isize) -> Posted {
        let hwnd = HWND(window as *mut c_void);
        match unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
            Ok(()) => Posted::Yes,
            Err(error) if WIN32_ERROR::from_error(&error) == Some(ERROR_ACCESS_DENIED) => {
                Posted::Refused
            }
            Err(_) => Posted::Failed,
        }
    }

    // All are opened first, which holds their ids, and each is ended only when the process behind the id started when the snapshot says.
    fn end(&self, targets: &[Entry]) -> Vec<Ended> {
        let opened: Vec<Result<Handle, Ended>> =
            targets.iter().map(|target| open(target.pid)).collect();
        opened
            .iter()
            .zip(targets)
            .map(|(opened, target)| match opened {
                Err(ended) => *ended,
                Ok(process) if created(process.0) != Some(target.created) => Ended::Gone,
                Ok(process) => terminate(process),
            })
            .collect()
    }

    fn image_paths(&self, pids: &[u32]) -> Vec<Option<String>> {
        let volumes = volumes();
        pids.iter()
            .map(|pid| dos_path(&native_path(*pid)?, &volumes))
            .collect()
    }

    fn create(&self, exe: &str, command_line: &mut [u16], cwd: &str) -> Result<(), u32> {
        let program = wide(exe);
        let folder = (!cwd.is_empty()).then(|| wide(cwd));
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut started = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                PCWSTR(program.as_ptr()),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                false,
                DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
                None,
                folder
                    .as_ref()
                    .map_or(PCWSTR::null(), |folder| PCWSTR(folder.as_ptr())),
                &raw const startup,
                &raw mut started,
            )
        }
        .map_err(|error| win32_code(&error))?;
        drop((Handle(started.hProcess), Handle(started.hThread)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_head_lies_where_the_kernel_writes_it() {
        assert_eq!(offset_of!(ProcessRow, create_time), 32);
        assert_eq!(offset_of!(ProcessRow, image_name), 56);
        assert_eq!(offset_of!(ProcessRow, unique_process_id), 80);
        assert_eq!(offset_of!(ProcessRow, inherited_from_unique_process_id), 88);
        assert_eq!(offset_of!(ProcessRow, session_id), 100);
    }

    #[test]
    fn a_kernel_name_becomes_a_drive_path_or_a_share() {
        let volumes = [
            (r"\Device\HarddiskVolume3\".to_owned(), r"C:\".to_owned()),
            (r"\Device\HarddiskVolume12\".to_owned(), r"D:\".to_owned()),
        ];
        assert_eq!(
            dos_path(r"\Device\HarddiskVolume3\Program Files\a.exe", &volumes).as_deref(),
            Some(r"C:\Program Files\a.exe")
        );
        assert_eq!(
            dos_path(r"\device\harddiskvolume12\Games\b.exe", &volumes).as_deref(),
            Some(r"D:\Games\b.exe")
        );
        assert_eq!(
            dos_path(
                r"\Device\Mup\;LanmanRedirector\;Z:0001\server\share\c.exe",
                &volumes
            )
            .as_deref(),
            Some(r"\\server\share\c.exe")
        );
        assert_eq!(
            dos_path(r"\Device\Mup\server\share\c.exe", &volumes).as_deref(),
            Some(r"\\server\share\c.exe")
        );
        assert_eq!(dos_path(r"\Device\HarddiskVolume1\x.exe", &volumes), None);
        assert_eq!(dos_path(r"\Device\HarddiskVolume3", &volumes), None);
    }

    #[test]
    fn the_table_names_this_process_and_its_parent() {
        let entries = snapshot().unwrap();
        let own = unsafe { GetCurrentProcessId() };
        let mine = entries.iter().find(|entry| entry.pid == own).unwrap();
        assert!(entries.iter().any(|entry| entry.pid == mine.parent));
        assert_eq!(Some(mine.session), own_session(own));
        let paths = Win32.image_paths(&[own]);
        let exe = std::env::current_exe().unwrap();
        assert!(
            paths[0]
                .as_deref()
                .is_some_and(|path| path.eq_ignore_ascii_case(&exe.to_string_lossy())),
            "the path of this test program was not found"
        );
    }
}
