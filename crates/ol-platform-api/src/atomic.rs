// How both file ports write: each file to a temporary file beside its target, then every target is moved aside and its new file moved in. A move that fails puts back every target already replaced, so a failure leaves each as it was, and nothing but the targets is left behind.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

struct Staged<'a> {
    target: PathBuf,
    temporary: PathBuf,
    aside: PathBuf,
    bytes: &'a [u8],
    had: bool,
}

fn beside(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(format!(".onelaunch-{}.{suffix}", std::process::id()));
    PathBuf::from(name)
}

fn stage(staged: &mut [Staged<'_>]) -> io::Result<()> {
    for file in staged.iter_mut() {
        match fs::symlink_metadata(&file.target) {
            Ok(found) if !found.is_file() => {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    format!("{} is not a file", file.target.display()),
                ));
            }
            // A rename over a read-only file fails, as the Electron app's did: moving it aside first must not get round that.
            Ok(found) if found.permissions().readonly() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} is read-only", file.target.display()),
                ));
            }
            Ok(_) => file.had = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => file.had = false,
            Err(error) => return Err(error),
        }
        if let Some(folder) = file.target.parent() {
            fs::create_dir_all(folder)?;
        }
        let mut written = File::create(&file.temporary)?;
        written.write_all(file.bytes)?;
        written.sync_all()?;
    }
    Ok(())
}

// Puts the first `done` targets back as they were, and the one that was moved aside when the next move failed. What cannot be put back is named, with where its old content is.
fn put_back(staged: &[Staged<'_>], done: usize, aside_only: Option<usize>) -> Vec<String> {
    let mut stuck = Vec::new();
    if let Some(index) = aside_only
        && staged[index].had
        && fs::rename(&staged[index].aside, &staged[index].target).is_err()
    {
        stuck.push(format!(
            "{} is missing and its old content is in {}",
            staged[index].target.display(),
            staged[index].aside.display()
        ));
    }
    let mut restore = |file: &Staged<'_>| {
        let back = if file.had {
            fs::rename(&file.aside, &file.target)
        } else {
            fs::remove_file(&file.target)
        };
        if back.is_err() {
            stuck.push(if file.had {
                format!(
                    "{} holds the new content and its old content is in {}",
                    file.target.display(),
                    file.aside.display()
                )
            } else {
                format!(
                    "{} was made and could not be removed",
                    file.target.display()
                )
            });
        }
    };
    for file in staged[..done].iter().rev() {
        restore(file);
    }
    stuck
}

fn commit(staged: &[Staged<'_>]) -> Result<(), (io::Error, Vec<String>)> {
    for (index, file) in staged.iter().enumerate() {
        if file.had
            && let Err(error) = fs::rename(&file.target, &file.aside)
        {
            return Err((error, put_back(staged, index, None)));
        }
        if let Err(error) = fs::rename(&file.temporary, &file.target) {
            return Err((error, put_back(staged, index, Some(index))));
        }
    }
    Ok(())
}

pub fn write_files(files: &[(&Path, &[u8])]) -> io::Result<()> {
    let mut staged: Vec<Staged<'_>> = files
        .iter()
        .map(|(target, bytes)| Staged {
            target: target.to_path_buf(),
            temporary: beside(target, "tmp"),
            aside: beside(target, "old"),
            bytes,
            had: false,
        })
        .collect();
    let outcome = stage(&mut staged).map_err(|error| (error, Vec::new()));
    let outcome = outcome.and_then(|()| commit(&staged));
    for file in &staged {
        let _ = fs::remove_file(&file.temporary);
    }
    match outcome {
        Ok(()) => {
            for file in staged.iter().filter(|file| file.had) {
                let _ = fs::remove_file(&file.aside);
            }
            Ok(())
        }
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
