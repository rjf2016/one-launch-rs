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
    // A file that grew since its size was read is still read no further than one byte past the limit.
    let bytes = files
        .read_range(path, 0, max_bytes.saturating_add(1))
        .map_err(|error| AdapterError::io(&error))?;
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
    read_log_tail(files, path, mark, max_bytes).unwrap_or_default()
}

// As `read_log_since`, for a caller that has to tell a log that cannot be read from one with nothing new in it.
pub fn read_log_tail(
    files: &dyn FileReader,
    path: &str,
    mark: u64,
    max_bytes: u64,
) -> Result<String, AdapterError> {
    let io = |error: std::io::Error| AdapterError::io(&error);
    let size = files.size(path).map_err(io)?;
    let from = if size < mark { 0 } else { mark };
    let start = from.max(size.saturating_sub(max_bytes));
    if size <= start {
        return Ok(String::new());
    }
    if start == from {
        let bytes = files.read_range(path, start, size - start).map_err(io)?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    // A clipped read keeps its first line only when the byte before it ends a line: otherwise that line is cut, maybe inside a character.
    let bytes = files
        .read_range(path, start - 1, size - start + 1)
        .map_err(io)?;
    Ok(match bytes.split_first() {
        Some((b'\n', rest)) => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(&bytes)
            .split_once('\n')
            .map_or_else(String::new, |(_, rest)| rest.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ol_platform_fake::{OpLog, ScratchFiles};
    use ol_testkit::Scratch;

    use super::*;

    // A file that grew after its size was read: it says it is small and is not.
    struct Growing;

    impl FileReader for Growing {
        fn exists(&self, _: &str) -> bool {
            true
        }
        fn size(&self, _: &str) -> std::io::Result<u64> {
            Ok(4)
        }
        fn modified(&self, _: &str) -> std::io::Result<time::OffsetDateTime> {
            Ok(time::OffsetDateTime::UNIX_EPOCH)
        }
        fn read(&self, _: &str) -> std::io::Result<Vec<u8>> {
            panic!("the whole file was read, however large it had grown")
        }
        fn read_range(&self, _: &str, from: u64, length: u64) -> std::io::Result<Vec<u8>> {
            let grown = 64_u64;
            let end = grown.min(from.saturating_add(length));
            Ok(vec![
                b'x';
                usize::try_from(end.saturating_sub(from)).unwrap()
            ])
        }
        fn list(&self, _: &str) -> std::io::Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn resolved(&self, _: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn a_file_that_grew_past_the_limit_is_never_read_whole() {
        assert!(read_launcher_bytes(&Growing, "grown", 8).is_err());
        assert_eq!(
            read_launcher_bytes(&Growing, "grown", 64).unwrap().len(),
            64
        );
    }

    #[test]
    fn a_clipped_read_that_starts_on_a_line_keeps_that_line() {
        let scratch = Scratch::new("log-tail");
        let path = scratch.join("log.txt");
        fs::write(&path, "old\nSuccess\n").unwrap();
        let files = ScratchFiles::new(&[scratch.path()], OpLog::new());
        let path = path.to_str().unwrap();
        assert_eq!(read_log_since(&files, path, 0, 8), "Success\n");
        assert_eq!(read_log_since(&files, path, 0, 6), "");
        assert_eq!(read_log_since(&files, path, 0, 9), "Success\n");
    }
}
