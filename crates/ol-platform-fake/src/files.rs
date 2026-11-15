use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use ol_platform_api::{FileReader, FileWriter};
use sha2::{Digest, Sha256};

use crate::OpLog;

// Files under the folders it was made with, and nothing else on the disk: a path outside them does not exist to it and cannot be written.
pub struct ScratchFiles {
    roots: Vec<String>,
    log: OpLog,
}

fn key(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

impl ScratchFiles {
    pub fn new(roots: &[&Path], log: OpLog) -> Self {
        Self {
            roots: roots
                .iter()
                .map(|root| {
                    key(&root.to_string_lossy())
                        .trim_end_matches('\\')
                        .to_owned()
                })
                .collect(),
            log,
        }
    }

    fn inside(&self, path: &str) -> io::Result<PathBuf> {
        let wanted = key(path);
        let climbs = wanted.split('\\').any(|part| part == "..");
        let rooted = self.roots.iter().any(|root| {
            wanted
                .strip_prefix(root.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('\\'))
        });
        if climbs || !rooted {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the path is outside the scratch folders",
            ));
        }
        Ok(PathBuf::from(path))
    }
}

impl FileReader for ScratchFiles {
    fn exists(&self, path: &str) -> bool {
        self.inside(path).is_ok_and(|path| path.exists())
    }

    fn size(&self, path: &str) -> io::Result<u64> {
        Ok(fs::metadata(self.inside(path)?)?.len())
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(self.inside(path)?)
    }

    fn read_range(&self, path: &str, from: u64, length: u64) -> io::Result<Vec<u8>> {
        let mut file = fs::File::open(self.inside(path)?)?;
        file.seek(SeekFrom::Start(from))?;
        let mut bytes = Vec::new();
        file.take(length).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn list(&self, folder: &str) -> io::Result<Vec<String>> {
        fs::read_dir(self.inside(folder)?)?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect()
    }

    fn resolved(&self, folder: &str) -> Option<String> {
        let real = fs::canonicalize(self.inside(folder).ok()?).ok()?;
        let real = real.to_string_lossy();
        Some(real.strip_prefix(r"\\?\").unwrap_or(&real).to_owned())
    }
}

impl FileWriter for ScratchFiles {
    fn write_atomic(&self, files: &[(&str, &[u8])]) -> io::Result<()> {
        let mut staged = Vec::new();
        for (path, bytes) in files {
            let target = self.inside(path)?;
            let mut temporary = target.clone().into_os_string();
            temporary.push(format!(".onelaunch-{}.tmp", std::process::id()));
            staged.push((*path, target, PathBuf::from(temporary), *bytes));
        }
        let written = (|| {
            for (_, target, temporary, bytes) in &staged {
                if let Some(folder) = target.parent() {
                    fs::create_dir_all(folder)?;
                }
                fs::write(temporary, bytes)?;
            }
            for (path, target, temporary, bytes) in &staged {
                fs::rename(temporary, target)?;
                let digest: String = Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                self.log.record(
                    "file.write",
                    [("path", (*path).into()), ("sha256", digest.into())],
                );
            }
            Ok(())
        })();
        for (_, _, temporary, _) in &staged {
            let _ = fs::remove_file(temporary);
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use ol_testkit::Scratch;

    use super::*;

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_file_under_a_root_is_read_and_one_beside_it_does_not_exist() {
        let scratch = Scratch::new("fake-files");
        let root = scratch.join("inside");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("a.txt"), b"0123456789").unwrap();
        fs::write(scratch.join("beside.txt"), b"x").unwrap();
        let files = ScratchFiles::new(&[&root], OpLog::new());

        let inside = text(&root.join("sub").join("a.txt"));
        assert!(files.exists(&inside));
        assert!(files.exists(&inside.to_uppercase()));
        assert_eq!(files.size(&inside).unwrap(), 10);
        assert_eq!(files.read(&inside).unwrap(), b"0123456789");
        assert_eq!(files.read_range(&inside, 7, 100).unwrap(), b"789");
        assert_eq!(files.read_range(&inside, 2, 3).unwrap(), b"234");
        assert_eq!(files.list(&text(&root.join("sub"))).unwrap(), ["a.txt"]);
        assert!(files.resolved(&text(&root)).is_some());

        for outside in [
            text(&scratch.join("beside.txt")),
            text(&root.join("..").join("beside.txt")),
            format!("{}-other", text(&root)),
        ] {
            assert!(!files.exists(&outside), "{outside}");
            assert_eq!(
                files.read(&outside).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
            assert!(files.write_atomic(&[(&outside, b"y")]).is_err());
        }
        assert_eq!(fs::read(scratch.join("beside.txt")).unwrap(), b"x");
    }

    #[test]
    fn a_write_replaces_the_file_leaves_no_temporary_file_and_is_recorded() {
        let scratch = Scratch::new("fake-files");
        let log = OpLog::new();
        let files = ScratchFiles::new(&[scratch.path()], log.clone());
        let first = text(&scratch.join("made").join("a.txt"));
        let second = text(&scratch.join("b.txt"));
        fs::write(&second, b"old").unwrap();
        files
            .write_atomic(&[(&first, b"abc"), (&second, b"new")])
            .unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"abc");
        assert_eq!(fs::read(&second).unwrap(), b"new");
        let names: Vec<String> = ol_testkit::snapshot(scratch.path()).into_keys().collect();
        assert_eq!(names, ["b.txt", "made/", "made/a.txt"]);
        let events = log.snapshot();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["op"], "file.write");
        assert_eq!(events[0]["path"], first.as_str());
        assert_eq!(
            events[0]["sha256"],
            ol_testkit::vectors::sha256_hex(b"abc").as_str()
        );
    }

    #[test]
    fn a_write_that_cannot_finish_replaces_nothing() {
        let scratch = Scratch::new("fake-files");
        let files = ScratchFiles::new(&[scratch.path()], OpLog::new());
        let kept = text(&scratch.join("kept.txt"));
        fs::write(&kept, b"old").unwrap();
        fs::write(scratch.join("blocker"), b"a file where a folder is wanted").unwrap();
        let blocked = text(&scratch.join("blocker").join("b.txt"));
        assert!(
            files
                .write_atomic(&[(&kept, b"new"), (&blocked, b"x")])
                .is_err()
        );
        assert_eq!(fs::read(&kept).unwrap(), b"old");
        let names: Vec<String> = ol_testkit::snapshot(scratch.path()).into_keys().collect();
        assert_eq!(names, ["blocker", "kept.txt"]);
    }
}
