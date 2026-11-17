// How both file ports write: each file to a new temporary file beside its target, and each target's old content kept beside it under a second new name, by a hard link or else a copy; then each new file is moved over its target, which is never missing. A move that fails puts back every target already replaced, so a failure leaves each as it was; nothing but the targets is left behind, and a folder made for a write that failed is removed again.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const ATTEMPTS: usize = 16;

static NAMED: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static FIRST: RefCell<VecDeque<PathBuf>> = const { RefCell::new(VecDeque::new()) };
}

struct Staged<'a> {
    target: PathBuf,
    bytes: &'a [u8],
    temporary: Option<PathBuf>,
    aside: Option<PathBuf>,
    // The target could not be put back, and the error names `aside` as where its old content is.
    stuck: bool,
}

pub(crate) fn name_beside(target: &Path, suffix: &str, count: u64, clock: u128) -> PathBuf {
    let mut name = OsString::from(target.as_os_str());
    name.push(format!(
        ".onelaunch-{}-{count}-{clock:x}.{suffix}",
        std::process::id()
    ));
    PathBuf::from(name)
}

// A name no file had a moment ago, from this process, a count and the clock: the file is made with `create_new`, so one that has it already is never opened, and the next name is tried.
fn fresh_name(target: &Path, suffix: &str) -> PathBuf {
    if let Some(first) = FIRST.with_borrow_mut(VecDeque::pop_front) {
        return first;
    }
    let count = NAMED.fetch_add(1, Ordering::Relaxed);
    let clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    name_beside(target, suffix, count, clock)
}

// For the contract: the writes `write` makes on this thread try `names` first, in turn, so a test can have a name taken before the writer picks it. Gives back how many were not tried.
pub(crate) fn trying_first<R>(names: Vec<PathBuf>, write: impl FnOnce() -> R) -> (R, usize) {
    FIRST.set(names.into());
    let written = write();
    (written, FIRST.take().len())
}

fn beside(
    target: &Path,
    suffix: &str,
    mut make: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    for _ in 0..ATTEMPTS {
        let path = fresh_name(target, suffix);
        match make(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("no free name was found beside {}", target.display()),
    ))
}

fn new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

// The folders of `folder` that are not there yet, made from the top down; each one this call made is recorded.
fn make_folders(folder: &Path, made: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut at = Some(folder);
    while let Some(path) = at.filter(|path| !path.as_os_str().is_empty()) {
        match fs::symlink_metadata(path) {
            Ok(found) if found.is_dir() => break,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    format!("{} is not a folder", path.display()),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push(path),
            Err(error) => return Err(error),
        }
        at = path.parent();
    }
    for path in missing.into_iter().rev() {
        match fs::create_dir(path) {
            Ok(()) => made.push(path.to_path_buf()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn keep_aside(target: &Path) -> io::Result<PathBuf> {
    beside(target, "old", |path| match fs::hard_link(target, path) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
            let mut old = File::open(target)?;
            let mut copy = new_file(path)?;
            let copied = io::copy(&mut old, &mut copy).and_then(|_| copy.sync_all());
            if copied.is_err() {
                drop(copy);
                let _ = fs::remove_file(path);
            }
            copied
        }
        linked => linked,
    })
}

fn stage(
    staged: &mut [Staged<'_>],
    made: &mut Vec<PathBuf>,
    observe: &mut dyn FnMut(),
) -> io::Result<()> {
    for file in staged.iter_mut() {
        let had = match fs::symlink_metadata(&file.target) {
            Ok(found) if !found.is_file() => {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    format!("{} is not a file", file.target.display()),
                ));
            }
            // A rename over a read-only file fails, as the Electron app's did: refused before anything is touched.
            Ok(found) if found.permissions().readonly() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} is read-only", file.target.display()),
                ));
            }
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if let Some(folder) = file.target.parent() {
            make_folders(folder, made)?;
        }
        let mut written = None;
        let temporary = beside(&file.target, "tmp", |path| {
            written = Some(new_file(path)?);
            Ok(())
        })?;
        file.temporary = Some(temporary);
        if let Some(mut written) = written {
            written.write_all(file.bytes)?;
            written.sync_all()?;
        }
        observe();
        if had {
            file.aside = Some(keep_aside(&file.target)?);
            observe();
        }
    }
    Ok(())
}

// Puts the first `done` targets back as they were. What cannot be put back is named, with where its old content is.
fn put_back(staged: &mut [Staged<'_>], done: usize, observe: &mut dyn FnMut()) -> Vec<String> {
    let mut stuck = Vec::new();
    for file in staged[..done].iter_mut().rev() {
        let back = match &file.aside {
            Some(aside) => fs::rename(aside, &file.target),
            None => fs::remove_file(&file.target),
        };
        observe();
        file.stuck = back.is_err();
        match (back, &file.aside) {
            (Ok(()), _) => file.aside = None,
            (Err(_), Some(aside)) => stuck.push(format!(
                "{} holds the new content and its old content is in {}",
                file.target.display(),
                aside.display()
            )),
            (Err(_), None) => stuck.push(format!(
                "{} was made and could not be removed",
                file.target.display()
            )),
        }
    }
    stuck
}

fn commit(
    staged: &mut [Staged<'_>],
    observe: &mut dyn FnMut(),
) -> Result<(), (io::Error, Vec<String>)> {
    for index in 0..staged.len() {
        let file = &mut staged[index];
        let Some(temporary) = file.temporary.take() else {
            continue;
        };
        let moved = fs::rename(&temporary, &file.target);
        observe();
        if let Err(error) = moved {
            file.temporary = Some(temporary);
            return Err((error, put_back(staged, index, observe)));
        }
    }
    Ok(())
}

pub fn write_files(files: &[(&Path, &[u8])]) -> io::Result<()> {
    write_observed(files, &mut || {})
}

// `observe` is called after each step that changes the disk.
fn write_observed(files: &[(&Path, &[u8])], observe: &mut dyn FnMut()) -> io::Result<()> {
    let mut staged: Vec<Staged<'_>> = files
        .iter()
        .map(|(target, bytes)| Staged {
            target: target.to_path_buf(),
            bytes,
            temporary: None,
            aside: None,
            stuck: false,
        })
        .collect();
    let mut made = Vec::new();
    let outcome = stage(&mut staged, &mut made, observe).map_err(|error| (error, Vec::new()));
    let outcome = outcome.and_then(|()| commit(&mut staged, observe));
    for file in &staged {
        if let Some(temporary) = &file.temporary {
            let _ = fs::remove_file(temporary);
        }
    }
    for file in staged.iter().filter(|file| !file.stuck) {
        if let Some(aside) = &file.aside {
            let _ = fs::remove_file(aside);
        }
    }
    if outcome.is_err() {
        for folder in made.iter().rev() {
            let _ = fs::remove_dir(folder);
        }
    }
    match outcome {
        Ok(()) => Ok(()),
        Err((error, stuck)) if stuck.is_empty() => Err(error),
        Err((error, stuck)) => Err(io::Error::new(
            error.kind(),
            format!(
                "{error}; the files written before it could not all be put back: {}",
                stuck.join("; ")
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use ol_testkit::Scratch;

    use super::*;

    #[test]
    fn a_target_that_was_there_is_there_after_every_step_of_a_write() {
        let scratch = Scratch::new("atomic-steps");
        let first = scratch.join("a.txt");
        let second = scratch.join("b.txt");
        fs::write(&first, b"old a").unwrap();
        fs::write(&second, b"old b").unwrap();
        let mut steps = 0;
        let mut missing = Vec::new();
        write_observed(&[(&first, b"new a"), (&second, b"new b")], &mut || {
            steps += 1;
            for target in [&first, &second] {
                if fs::read(target).is_err() {
                    missing.push((steps, target.display().to_string()));
                }
            }
        })
        .unwrap();
        assert!(steps > 0);
        assert!(missing.is_empty(), "{missing:?}");
        assert_eq!(fs::read(&first).unwrap(), b"new a");
        assert_eq!(fs::read(&second).unwrap(), b"new b");
    }

    #[test]
    fn a_target_put_back_after_a_later_move_failed_is_there_after_every_step() {
        use std::os::windows::fs::OpenOptionsExt;

        let scratch = Scratch::new("atomic-put-back");
        let first = scratch.join("a.txt");
        let second = scratch.join("b.txt");
        fs::write(&first, b"old a").unwrap();
        fs::write(&second, b"old b").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&second)
            .unwrap();
        let mut missing = Vec::new();
        let written = write_observed(&[(&first, b"new a"), (&second, b"new b")], &mut || {
            if fs::read(&first).is_err() {
                missing.push(first.display().to_string());
            }
        });
        drop(held);
        assert!(written.is_err());
        assert!(missing.is_empty(), "{missing:?}");
        assert_eq!(fs::read(&first).unwrap(), b"old a");
        assert_eq!(fs::read(&second).unwrap(), b"old b");
        let names: Vec<String> = ol_testkit::snapshot(scratch.path()).into_keys().collect();
        assert_eq!(names, ["a.txt", "b.txt"]);
    }
}
