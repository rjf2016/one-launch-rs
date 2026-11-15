use ol_platform_api::FileReader;

use crate::AdapterError;

// A real launcher file is kilobytes. One of many megabytes would stall the app, so it is refused before it is read.
pub const MAX_LAUNCHER_FILE_BYTES: u64 = 4 * 1024 * 1024;
// Steam's files are parsed whole, so they get a tighter limit.
pub const MAX_VDF_BYTES: u64 = 1024 * 1024;
// Steam moves its sign-in log aside at about this size, so one read holds a whole file.
pub const MAX_TAIL_BYTES: u64 = 512 * 1024;

fn too_large(path: &str, size: u64) -> AdapterError {
    AdapterError::LauncherFile {
        path: path.to_owned(),
        message: format!("Too large to be a launcher file ({size} bytes)"),
    }
}

pub fn read_launcher_bytes(
    files: &dyn FileReader,
    path: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, AdapterError> {
    let size = files.size(path).map_err(|error| AdapterError::io(&error))?;
    if size > max_bytes {
        return Err(too_large(path, size));
    }
    let bytes = files.read(path).map_err(|error| AdapterError::io(&error))?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read > max_bytes {
        return Err(too_large(path, read));
    }
    Ok(bytes)
}

// As the Electron app decodes a launcher file: a byte order mark stays, and a byte that is not UTF-8 becomes U+FFFD.
pub fn read_launcher_text(
    files: &dyn FileReader,
    path: &str,
    max_bytes: u64,
) -> Result<String, AdapterError> {
    Ok(String::from_utf8_lossy(&read_launcher_bytes(files, path, max_bytes)?).into_owned())
}

// The size of a log now, so that only what the launcher adds after this point is read. 0 when there is no log.
pub fn log_mark(files: &dyn FileReader, path: &str) -> u64 {
    if !files.exists(path) {
        return 0;
    }
    files.size(path).unwrap_or(0)
}

// The text added since `mark`, at most its last `max_bytes`: the newest lines decide a sign-in, never the oldest. A log smaller than its mark was started afresh and is read from its start; one that cannot be read reads as empty.
pub fn read_log_since(files: &dyn FileReader, path: &str, mark: u64, max_bytes: u64) -> String {
    if !files.exists(path) {
        return String::new();
    }
    let Ok(size) = files.size(path) else {
        return String::new();
    };
    let from = if size < mark { 0 } else { mark };
    let start = from.max(size.saturating_sub(max_bytes));
    if size <= start {
        return String::new();
    }
    let Ok(bytes) = files.read_range(path, start, size - start) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    if start == from {
        return text.into_owned();
    }
    // A clipped read starts in the middle of a line, so its first line goes.
    text.split_once('\n')
        .map_or_else(String::new, |(_, rest)| rest.to_owned())
}
