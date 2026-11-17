use std::fs;

use ol_platform_api::{FileWriter, contract};
use ol_platform_win::files::WindowsFiles;
use ol_testkit::Scratch;

fn text(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn the_file_port_contract_holds() {
    let scratch = Scratch::new("win-files-contract");
    contract::files(&WindowsFiles, &text(scratch.path()));
}

#[test]
fn a_read_only_target_is_refused_and_left_as_it_was() {
    let scratch = Scratch::new("win-files-read-only");
    let kept = scratch.join("kept.txt");
    let locked = scratch.join("locked.txt");
    fs::write(&kept, b"old").unwrap();
    fs::write(&locked, b"old").unwrap();
    let mut read_only = fs::metadata(&locked).unwrap().permissions();
    read_only.set_readonly(true);
    fs::set_permissions(&locked, read_only.clone()).unwrap();

    let written = WindowsFiles.write_atomic(&[(&text(&kept), b"new"), (&text(&locked), b"new")]);
    assert!(written.is_err());
    assert_eq!(fs::read(&kept).unwrap(), b"old");
    assert_eq!(fs::read(&locked).unwrap(), b"old");
    let names: Vec<String> = ol_testkit::snapshot(scratch.path()).into_keys().collect();
    assert_eq!(names, ["kept.txt", "locked.txt"]);

    allow_writes(&locked, read_only);
}

#[expect(
    clippy::permissions_set_readonly_false,
    reason = "the test's own file, made writable again so its folder can be removed"
)]
fn allow_writes(file: &std::path::Path, mut permissions: fs::Permissions) {
    permissions.set_readonly(false);
    fs::set_permissions(file, permissions).unwrap();
}
