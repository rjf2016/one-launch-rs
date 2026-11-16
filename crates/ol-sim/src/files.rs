use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use ol_platform_api::{FileReader, FileWriter};
use ol_platform_fake::{OpLog, ScratchFiles};
use time::OffsetDateTime;

// The scratch folders' file port, with each path judged by where it really leads: a junction or a link under a root that points out of the roots leads outside them.
pub struct ContainedFiles {
    roots: Vec<String>,
    files: ScratchFiles,
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "the path leads outside the scratch folders",
    )
}

// The deepest part of the path that is there, with every link in it followed, and the rest as written. Something that is there and still does not resolve is a link to nowhere, and it is refused.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(refused());
    }
    let mut there = path.to_path_buf();
    let mut below = Vec::new();
    loop {
        match fs::canonicalize(&there) {
            Ok(mut found) => {
                found.extend(below.iter().rev());
                return Ok(found);
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Err(_) => {}
        }
        if fs::symlink_metadata(&there).is_ok() {
            return Err(refused());
        }
        match (there.parent(), there.file_name()) {
            (Some(parent), Some(name)) => {
                below.push(name.to_owned());
                there = parent.to_path_buf();
            }
            _ => return Err(refused()),
        }
    }
}

fn key(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .to_lowercase()
        .trim_end_matches('\\')
        .to_owned()
}

impl ContainedFiles {
    pub fn new(roots: &[&Path], log: OpLog) -> Self {
        Self {
            roots: roots
                .iter()
                .filter_map(|root| resolve(root).ok())
                .map(|root| key(&root))
                .collect(),
            files: ScratchFiles::new(roots, log),
        }
    }

    fn inside(&self, path: &str) -> io::Result<()> {
        let real = key(&resolve(Path::new(path))?);
        let rooted = self.roots.iter().any(|root| {
            real.strip_prefix(root.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('\\'))
        });
        if rooted { Ok(()) } else { Err(refused()) }
    }
}

impl FileReader for ContainedFiles {
    fn exists(&self, path: &str) -> bool {
        self.inside(path).is_ok() && self.files.exists(path)
    }

    fn size(&self, path: &str) -> io::Result<u64> {
        self.inside(path)?;
        self.files.size(path)
    }

    fn modified(&self, path: &str) -> io::Result<OffsetDateTime> {
        self.inside(path)?;
        self.files.modified(path)
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.inside(path)?;
        self.files.read(path)
    }

    fn read_range(&self, path: &str, from: u64, length: u64) -> io::Result<Vec<u8>> {
        self.inside(path)?;
        self.files.read_range(path, from, length)
    }

    fn list(&self, folder: &str) -> io::Result<Vec<String>> {
        self.inside(folder)?;
        self.files.list(folder)
    }

    fn resolved(&self, folder: &str) -> Option<String> {
        self.inside(folder).ok()?;
        self.files.resolved(folder)
    }
}

impl FileWriter for ContainedFiles {
    fn write_atomic(&self, files: &[(&str, &[u8])]) -> io::Result<()> {
        for (path, _) in files {
            self.inside(path)?;
        }
        self.files.write_atomic(files)
    }
}

#[cfg(test)]
mod tests {
    use ol_testkit::Scratch;

    use super::*;

    struct Link(PathBuf);

    impl Drop for Link {
        fn drop(&mut self) {
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "std makes no junction, and a junction is what a test of a junction needs"
    )]
    fn junction(link: &Path, target: &Path) -> Link {
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink /J failed");
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
        Link(link.to_path_buf())
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn assert_sealed(files: &ContainedFiles, link: &Path, outside: &Path) {
        let kept = text(&link.join("kept.txt"));
        let made = text(&link.join("made").join("new.txt"));
        assert!(!files.exists(&kept), "{kept}");
        for refusal in [
            files.read(&kept).unwrap_err(),
            files.read_range(&kept, 0, 4).unwrap_err(),
            files.size(&kept).unwrap_err(),
            files.list(&text(link)).unwrap_err(),
            files.write_atomic(&[(&kept, b"new")]).unwrap_err(),
            files.write_atomic(&[(&made, b"new")]).unwrap_err(),
        ] {
            assert_eq!(refusal.kind(), io::ErrorKind::PermissionDenied);
        }
        assert!(files.resolved(&text(link)).is_none());
        assert_eq!(fs::read(outside.join("kept.txt")).unwrap(), b"old");
        assert!(!outside.join("made").exists());
    }

    #[test]
    fn a_junction_under_a_root_that_leads_out_of_the_roots_is_outside_them() {
        let scratch = Scratch::new("sim-files-junction");
        let (root, outside) = (scratch.join("root"), scratch.join("outside"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("kept.txt"), b"old").unwrap();
        fs::write(root.join("own.txt"), b"mine").unwrap();
        let link = junction(&root.join("steam"), &outside);
        let files = ContainedFiles::new(&[&root], OpLog::new());

        assert_sealed(&files, &link.0, &outside);
        assert_eq!(files.read(&text(&root.join("own.txt"))).unwrap(), b"mine");
        let fresh = text(&root.join("made").join("new.txt"));
        files.write_atomic(&[(&fresh, b"new")]).unwrap();
        assert_eq!(fs::read(&fresh).unwrap(), b"new");
    }

    #[test]
    fn a_symbolic_link_under_a_root_that_leads_out_of_the_roots_is_outside_them() {
        let scratch = Scratch::new("sim-files-symlink");
        let (root, outside) = (scratch.join("root"), scratch.join("outside"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("kept.txt"), b"old").unwrap();
        let link = root.join("steam");
        if std::os::windows::fs::symlink_dir(&outside, &link).is_err() {
            // Without developer mode or an elevated prompt Windows lets nobody make a symbolic link.
            eprintln!("not run: this user may not make a symbolic link");
            return;
        }
        let link = Link(link);
        let files = ContainedFiles::new(&[&root], OpLog::new());

        assert_sealed(&files, &link.0, &outside);
    }

    #[test]
    fn a_link_to_nowhere_under_a_root_is_refused() {
        let scratch = Scratch::new("sim-files-dead");
        let (root, gone) = (scratch.join("root"), scratch.join("gone"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&gone).unwrap();
        let link = junction(&root.join("steam"), &gone);
        fs::remove_dir(&gone).unwrap();
        let files = ContainedFiles::new(&[&root], OpLog::new());

        let through = text(&link.0.join("new.txt"));
        assert_eq!(
            files
                .write_atomic(&[(&through, b"new")])
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(!gone.exists());
    }
}
