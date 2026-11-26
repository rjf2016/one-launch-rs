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
    size: Option<u64>,
}

fn gone_is_fine(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

// A link or a junction would carry the writes out of the data folder.
fn refuse_link(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the log does not write through a link",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
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

    fn moving_path(&self) -> PathBuf {
        self.dir.join(format!("{LOG_NAME}.rotating.log"))
    }

    pub fn write_lines(&mut self, lines: &[String]) -> io::Result<()> {
        let written = self.try_write(lines);
        if written.is_err() {
            self.size = None;
        }
        written
    }

    fn try_write(&mut self, lines: &[String]) -> io::Result<()> {
        refuse_link(&self.dir)?;
        fs::create_dir_all(&self.dir)?;
        refuse_link(&self.dir)?;
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
                if self.rotate().is_ok() {
                    size = 0;
                }
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
        let path = self.current_path();
        refuse_link(&path)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(text.as_bytes())
    }

    // The live file is moved aside first: one that cannot be moved, because another program holds it, leaves every older file where it is.
    fn rotate(&self) -> io::Result<()> {
        let live = self.current_path();
        let moving = self.moving_path();
        for path in [&live, &moving] {
            refuse_link(path)?;
        }
        for n in 1..=self.keep {
            refuse_link(&self.rotated_path(n))?;
        }
        // A rotation that failed after the live file was moved aside is finished first, or every later one would find its file in the way.
        if fs::symlink_metadata(&moving).is_ok() {
            self.shift_in(&moving)?;
        }
        fs::rename(&live, &moving)?;
        self.shift_in(&moving)
    }

    fn shift_in(&self, moving: &Path) -> io::Result<()> {
        if self.keep == 0 {
            return gone_is_fine(fs::remove_file(moving));
        }
        gone_is_fine(fs::remove_file(self.rotated_path(self.keep)))?;
        for n in (1..self.keep).rev() {
            gone_is_fine(fs::rename(self.rotated_path(n), self.rotated_path(n + 1)))?;
        }
        fs::rename(moving, self.rotated_path(1))
    }
}
