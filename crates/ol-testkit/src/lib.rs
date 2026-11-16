#![forbid(unsafe_code)]

pub mod vectors;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("ol-testkit sits two folders below the workspace root")
        .to_path_buf()
}

pub fn oracle(relative: &str) -> PathBuf {
    let path = workspace_root().join("oracle").join(relative);
    assert!(path.exists(), "oracle/{relative} is missing");
    path
}

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let folder = format!(
            "{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = workspace_root().join("target").join("tmp").join(folder);
        if path.exists() {
            fs::remove_dir_all(&path).expect("a stale scratch folder could not be removed");
        }
        fs::create_dir_all(&path).expect("the scratch folder could not be made");
        Self(path)
    }

    pub fn with_copy_of(name: &str, source: &Path) -> Self {
        let scratch = Self::new(name);
        copy_tree(source, scratch.path());
        scratch
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the target folder could not be made");
    for entry in fs::read_dir(from).expect("the source folder could not be listed") {
        let entry = entry.expect("a folder entry could not be read");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a file could not be copied");
        }
    }
}

pub fn snapshot(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect(folder, folder, &mut files);
    files
}

fn collect(root: &Path, folder: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    for entry in fs::read_dir(folder).expect("the folder could not be listed") {
        let path = entry.expect("a folder entry could not be read").path();
        let name = path
            .strip_prefix(root)
            .expect("an entry sits under the folder it was listed from")
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            files.insert(format!("{name}/"), Vec::new());
            collect(root, &path, files);
        } else {
            files.insert(name, fs::read(&path).expect("a file could not be read"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_folder_is_empty_and_goes_away() {
        let kept = {
            let scratch = Scratch::new("testkit");
            assert!(snapshot(scratch.path()).is_empty());
            fs::write(scratch.join("a.txt"), b"a").unwrap();
            scratch.path().to_path_buf()
        };
        assert!(!kept.exists());
    }

    #[test]
    fn a_snapshot_sees_a_changed_an_added_and_a_removed_file() {
        let scratch = Scratch::new("testkit");
        fs::create_dir(scratch.join("sub")).unwrap();
        fs::write(scratch.join("sub/a.txt"), b"a").unwrap();
        let copy = Scratch::with_copy_of("testkit", scratch.path());
        let before = snapshot(scratch.path());
        assert_eq!(before, snapshot(copy.path()));
        assert_eq!(before.keys().collect::<Vec<_>>(), ["sub/", "sub/a.txt"]);

        fs::write(copy.join("sub/a.txt"), b"b").unwrap();
        assert_ne!(before, snapshot(copy.path()));
        fs::write(copy.join("sub/a.txt"), b"a").unwrap();
        fs::write(copy.join("sub/a.txt-wal"), b"").unwrap();
        assert_ne!(before, snapshot(copy.path()));
        fs::remove_file(copy.join("sub/a.txt-wal")).unwrap();
        fs::remove_file(copy.join("sub/a.txt")).unwrap();
        assert_ne!(before, snapshot(copy.path()));
    }
}
