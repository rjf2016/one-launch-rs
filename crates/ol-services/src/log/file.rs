// The log file and its rotation, as the Electron app keeps them: onelaunch.log, and onelaunch.1.log to onelaunch.5.log behind it.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub const LOG_NAME: &str = "onelaunch";
pub const MAX_BYTES: u64 = 1024 * 1024;
pub const KEEP: u32 = 5;

pub struct FileSink {
    dir: PathBuf,
    max_bytes: u64,
    keep: u32,
    // Read from disk once, then counted here; forgotten after a failure, so the next write reads it again.
    size: Option<u64>,
}

fn gone_is_fine(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

impl FileSink {
    pub fn new(dir: &Path, max_bytes: u64, keep: u32) -> Self {
        Self {
            dir: dir.to_path_buf(),
            max_bytes,
            keep,
            size: None,
        }
    }

    pub fn current_path(&self) -> PathBuf {
        self.dir.join(format!("{LOG_NAME}.log"))
    }

    fn rotated_path(&self, n: u32) -> PathBuf {
        self.dir.join(format!("{LOG_NAME}.{n}.log"))
    }

    // Before a line goes in, a file that is not empty and that the line would take past the limit is moved behind the others.
    pub fn write_lines(&mut self, lines: &[String]) -> io::Result<()> {
        let written = self.try_write(lines);
        if written.is_err() {
            self.size = None;
        }
        written
    }

    fn try_write(&mut self, lines: &[String]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let mut size = match self.size {
            Some(size) => size,
            None => match fs::metadata(self.current_path()) {
                Ok(meta) => meta.len(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                Err(error) => return Err(error),
            },
        };
        let mut pending = String::new();
        for line in lines {
            let bytes = line.len() as u64;
            if size > 0 && size + bytes > self.max_bytes {
                self.append(&pending)?;
                pending.clear();
                self.rotate()?;
                size = 0;
            }
            pending.push_str(line);
            size += bytes;
        }
        self.append(&pending)?;
        self.size = Some(size);
        Ok(())
    }

    fn append(&self, text: &str) -> io::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.current_path())?
            .write_all(text.as_bytes())
    }

    fn rotate(&mut self) -> io::Result<()> {
        if self.keep == 0 {
            gone_is_fine(fs::remove_file(self.current_path()))?;
            return Ok(());
        }
        gone_is_fine(fs::remove_file(self.rotated_path(self.keep)))?;
        for n in (1..self.keep).rev() {
            gone_is_fine(fs::rename(self.rotated_path(n), self.rotated_path(n + 1)))?;
        }
        gone_is_fine(fs::rename(self.current_path(), self.rotated_path(1)))
    }
}
