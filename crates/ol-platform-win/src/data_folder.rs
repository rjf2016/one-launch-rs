use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf, Prefix, absolute};

use thiserror::Error;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

pub const APP_FOLDER: &str = "OneLaunch";
const MOST_LINKS: usize = 32;

#[derive(Debug, Error)]
pub enum FolderError {
    #[error("it is not written as a plain folder path")]
    NotPlain,
    #[error("it runs through too many links")]
    TooManyLinks,
    #[error("Windows could not look it up ({0})")]
    Lookup(io::ErrorKind),
}

impl From<io::Error> for FolderError {
    fn from(error: io::Error) -> Self {
        Self::Lookup(error.kind())
    }
}

// Where the installed app keeps its data: the roaming AppData folder as Windows itself gives it, which is not always what %APPDATA% says.
pub fn real_data_folder() -> Option<PathBuf> {
    let found =
        unsafe { SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None) }.ok()?;
    let roaming = OsString::from_wide(unsafe { found.as_wide() });
    unsafe { CoTaskMemFree(Some(found.as_ptr().cast_const().cast())) };
    (!roaming.is_empty()).then(|| Path::new(&roaming).join(APP_FOLDER))
}

#[derive(Debug, Error)]
pub enum Refusal {
    #[error(
        "--data-dir is the real data folder (%APPDATA%\\OneLaunch) or a folder inside it; this build runs only on a copy of it or on a scratch folder"
    )]
    RealDataFolder,
    #[error("--data-dir cannot be told apart from the real data folder: {0}")]
    Unclear(#[from] FolderError),
    #[error(
        "Windows did not name the roaming AppData folder, so --data-dir cannot be told apart from the real data folder"
    )]
    NoRoamingFolder,
}

pub fn refuse_real_data_folder(data_dir: &Path) -> Result<(), Refusal> {
    refuse_within(real_data_folder().as_deref(), data_dir)
}

// What cannot be decided is refused: a start on the wrong folder is the mistake this is here for.
pub fn refuse_within(real: Option<&Path>, data_dir: &Path) -> Result<(), Refusal> {
    let real = real.ok_or(Refusal::NoRoamingFolder)?;
    if is_within(real, data_dir)? {
        return Err(Refusal::RealDataFolder);
    }
    Ok(())
}

// Two spellings name one folder far more often than they look alike, so each side is held both as written and as the file system resolves it: case, short names, "..", a trailing dot, a junction or a link on the way.
pub fn is_within(root: &Path, folder: &Path) -> Result<bool, FolderError> {
    let roots = [key(&absolute(root)?)?, key(&resolve(root)?)?];
    let folders = [key(&absolute(folder)?)?, key(&resolve(folder)?)?];
    Ok(folders
        .iter()
        .any(|folder| roots.iter().any(|root| folder.starts_with(root))))
}

fn resolve(path: &Path) -> Result<PathBuf, FolderError> {
    let mut path = absolute(path)?;
    let mut below = Vec::new();
    let mut links = 0;
    loop {
        match std::fs::canonicalize(&path) {
            Ok(mut found) => {
                found.extend(below.iter().rev());
                return Ok(found);
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            Err(_) => {}
        }
        // A link whose target is not there yet still says where a folder made through it would land.
        if let Ok(target) = std::fs::read_link(&path) {
            links += 1;
            if links > MOST_LINKS {
                return Err(FolderError::TooManyLinks);
            }
            path = absolute(path.parent().unwrap_or(&path).join(target))?;
            continue;
        }
        match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => {
                below.push(name.to_owned());
                path = parent.to_path_buf();
            }
            _ => {
                path.extend(below.iter().rev());
                return Ok(path);
            }
        }
    }
}

fn key(path: &Path) -> Result<Vec<String>, FolderError> {
    let mut key = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => key.push(match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    format!("{}:", char::from(letter.to_ascii_uppercase()))
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                    r"\\{}\{}",
                    fold(&server.to_string_lossy()),
                    fold(&share.to_string_lossy())
                ),
                Prefix::DeviceNS(name) | Prefix::Verbatim(name) => {
                    format!(r"\\.\{}", fold(&name.to_string_lossy()))
                }
            }),
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => return Err(FolderError::NotPlain),
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                // "name::$INDEX_ALLOCATION" opens the folder "name", and "name:x" a stream of it.
                if name.contains(':') {
                    return Err(FolderError::NotPlain);
                }
                key.push(fold(&name));
            }
        }
    }
    Ok(key)
}

// Folding both ways joins more names than NTFS does, never fewer: a folder may be refused that is not the same one, and none is let through that is.
fn fold(name: &str) -> String {
    name.to_uppercase().to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ol_testkit::Scratch;

    use super::*;

    struct StandIn {
        scratch: Scratch,
        root: PathBuf,
    }

    fn stand_in(made: bool) -> StandIn {
        let scratch = Scratch::new("data-folder");
        let root = scratch.join("Roaming").join(APP_FOLDER);
        fs::create_dir_all(scratch.join("Roaming")).unwrap();
        fs::create_dir_all(scratch.join("elsewhere").join("sub")).unwrap();
        if made {
            fs::create_dir_all(root.join("backups").join("db")).unwrap();
            fs::write(root.join("onelaunch.db"), b"").unwrap();
        }
        StandIn { scratch, root }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "std makes no junction, and a junction is what a test of a junction needs"
    )]
    fn junction(link: &Path, target: &Path) {
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink /J failed");
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    }

    fn within(root: &Path, folder: impl AsRef<Path>) -> bool {
        is_within(root, folder.as_ref()).unwrap()
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn the_folder_itself_and_what_is_inside_it_are_within() {
        for made in [true, false] {
            let StandIn { scratch, root } = stand_in(made);
            for inside in [
                root.clone(),
                root.join("backups"),
                root.join("backups").join("db"),
                root.join("not").join("there").join("yet"),
            ] {
                assert!(within(&root, &inside), "made {made}: {}", inside.display());
            }
            for outside in [
                scratch.join("Roaming"),
                scratch.join("Roaming").join("OneLaunchScratch"),
                scratch.join("Roaming").join("One"),
                scratch.join("Roaming").join("Other").join(APP_FOLDER),
                scratch.join("elsewhere"),
                scratch.join("elsewhere").join(APP_FOLDER),
                scratch.path().to_path_buf(),
                PathBuf::from(r"Q:\no\such\drive"),
            ] {
                assert!(
                    !within(&root, &outside),
                    "made {made}: {}",
                    outside.display()
                );
            }
        }
    }

    #[test]
    fn another_spelling_of_the_folder_is_still_the_folder() {
        for made in [true, false] {
            let StandIn { scratch, root } = stand_in(made);
            let written = text(&root);
            let roaming = text(&scratch.join("Roaming"));
            for spelling in [
                written.to_uppercase(),
                written.to_lowercase(),
                written.replace('\\', "/"),
                format!(r"{written}\"),
                format!(r"{written}\\\"),
                format!("{written}."),
                format!("{written}. . "),
                format!(r"{written}\."),
                format!(r"{written}\backups\.."),
                format!(r"{roaming}\Other\..\onelaunch\BACKUPS"),
                format!(r"{roaming}\.\ONELAUNCH"),
                format!(r"\\?\{written}"),
                format!(r"\\?\{}", written.to_uppercase()),
                format!(r"\\.\{written}\backups"),
            ] {
                assert!(within(&root, &spelling), "made {made}: {spelling}");
            }
            for other in [
                format!(r"{written}\.."),
                format!(r"{written}\..\Other"),
                format!(r"{written}\backups\..\..\..\elsewhere"),
                format!("{written}x"),
                format!("{written} 2"),
            ] {
                assert!(!within(&root, &other), "made {made}: {other}");
            }
        }
    }

    #[test]
    fn a_junction_to_the_folder_or_into_it_is_within() {
        let StandIn { scratch, root } = stand_in(true);
        let link = scratch.join("link");
        junction(&link, &root);
        let inner = scratch.join("elsewhere").join("inner");
        junction(&inner, &root.join("backups"));
        let chain = scratch.join("chain");
        junction(&chain, &link);
        for inside in [
            link.clone(),
            link.join("backups").join("db"),
            link.join("not there yet"),
            inner.clone(),
            inner.join("db"),
            chain.join("backups"),
            scratch.join("LINK"),
        ] {
            assert!(within(&root, &inside), "{}", inside.display());
        }

        let away = scratch.join("away");
        junction(&away, &scratch.join("elsewhere"));
        let out = root.join("out");
        junction(&out, &scratch.join("elsewhere"));
        assert!(!within(&root, &away));
        assert!(!within(&root, away.join("sub")));
        assert!(
            within(&root, &out),
            "a link inside the folder is an entry of the folder"
        );
        for link in [&link, &inner, &chain, &away, &out] {
            fs::remove_dir(link).unwrap();
        }
    }

    #[test]
    fn a_folder_reached_through_a_junction_is_found_by_its_other_name() {
        let StandIn { scratch, root } = stand_in(true);
        let moved = scratch.join("Moved Roaming");
        junction(&moved, &scratch.join("Roaming"));
        let through = moved.join(APP_FOLDER);
        for inside in [root.clone(), root.join("backups"), through.clone()] {
            assert!(within(&through, &inside), "{}", inside.display());
        }
        assert!(!within(&through, scratch.join("elsewhere")));
        fs::remove_dir(&moved).unwrap();
    }

    #[test]
    fn a_junction_to_a_folder_that_is_not_there_yet_counts_as_that_folder() {
        let StandIn { scratch, root } = stand_in(false);
        fs::create_dir(&root).unwrap();
        let link = scratch.join("link");
        junction(&link, &root);
        fs::remove_dir(&root).unwrap();
        assert!(
            fs::canonicalize(&link).is_err(),
            "the link points at nothing"
        );
        assert!(within(&root, &link));
        assert!(within(&root, link.join("sub")));

        let twice = scratch.join("twice");
        fs::create_dir(&root).unwrap();
        junction(&twice, &root.join("backups"));
        fs::remove_dir(&root).unwrap();
        assert!(within(&root, &twice));
        assert!(!within(&scratch.join("elsewhere"), &twice));
        for link in [&link, &twice] {
            fs::remove_dir(link).unwrap();
        }
    }

    #[test]
    fn links_that_go_round_are_refused() {
        let StandIn { scratch, root } = stand_in(true);
        let (one, two) = (scratch.join("one"), scratch.join("two"));
        fs::create_dir(&two).unwrap();
        junction(&one, &two);
        fs::remove_dir(&two).unwrap();
        junction(&two, &one);
        let refused = is_within(&root, &one);
        assert!(
            matches!(
                refused,
                Err(FolderError::TooManyLinks | FolderError::Lookup(_))
            ),
            "{refused:?}"
        );
        for link in [&one, &two] {
            fs::remove_dir(link).unwrap();
        }
    }

    #[test]
    fn a_name_with_a_stream_in_it_is_refused() {
        for made in [true, false] {
            let StandIn { scratch, root } = stand_in(made);
            let written = text(&root);
            for spelling in [
                format!("{written}::$INDEX_ALLOCATION"),
                format!("{written}:$I30:$INDEX_ALLOCATION"),
                format!(r"{written}:stream\sub"),
                format!(r"{}\elsewhere:x", text(scratch.path())),
            ] {
                let refused = is_within(&root, Path::new(&spelling));
                assert!(
                    matches!(refused, Err(FolderError::NotPlain)),
                    "made {made}: {spelling}: {refused:?}"
                );
            }
        }
    }

    #[test]
    fn a_name_windows_cannot_look_up_is_an_error() {
        let StandIn { scratch, root } = stand_in(true);
        let refused = is_within(&root, &scratch.join("a*b"));
        assert!(
            matches!(refused, Err(FolderError::Lookup(_))),
            "{refused:?}"
        );
    }

    #[test]
    fn windows_names_the_roaming_folder() {
        let folder = real_data_folder().unwrap();
        assert!(folder.is_absolute());
        assert_eq!(folder.file_name().unwrap(), APP_FOLDER);
        assert!(folder.parent().unwrap().file_name().is_some());
    }

    #[test]
    fn a_start_on_the_folder_is_refused_in_plain_words() {
        let StandIn { scratch, root } = stand_in(true);
        let refused = |folder: &Path| refuse_within(Some(&root), folder).unwrap_err();

        let real = refused(&root.join("BACKUPS"));
        assert!(matches!(real, Refusal::RealDataFolder));
        assert_eq!(
            real.to_string(),
            "--data-dir is the real data folder (%APPDATA%\\OneLaunch) or a folder inside it; this build runs only on a copy of it or on a scratch folder"
        );
        assert!(!real.to_string().contains(&text(scratch.path())));

        let unclear = refused(&scratch.join("a*b"));
        assert!(matches!(unclear, Refusal::Unclear(FolderError::Lookup(_))));
        let unclear = refused(Path::new(&format!("{}:x", text(&root))));
        assert_eq!(
            unclear.to_string(),
            "--data-dir cannot be told apart from the real data folder: it is not written as a plain folder path"
        );

        let unknown = refuse_within(None, &scratch.join("elsewhere")).unwrap_err();
        assert!(matches!(unknown, Refusal::NoRoamingFolder));

        refuse_within(Some(&root), &scratch.join("elsewhere")).unwrap();
        refuse_within(Some(&root), &scratch.join("not there yet")).unwrap();
    }
}
