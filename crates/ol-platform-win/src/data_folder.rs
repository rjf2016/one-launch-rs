use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf, Prefix, absolute};

use thiserror::Error;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Globalization::{LCMAP_UPPERCASE, LCMapStringEx, LOCALE_NAME_INVARIANT};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

pub const APP_FOLDER: &str = "OneLaunch";
const MOST_LINKS: usize = 32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FolderError {
    #[error("it is not written as a plain path on a lettered drive, such as D:\\folder")]
    NotPlain,
    #[error("it is not on a local drive")]
    NotLocal,
    #[error("it runs through a link that leads nowhere")]
    DeadLink,
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

// Each names the argument it is about: a path the app writes to can come in by more than one.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Refusal {
    #[error(
        "{0} is the real data folder (%APPDATA%\\OneLaunch) or inside it; this build runs only on a copy of it or on a scratch folder"
    )]
    RealDataFolder(&'static str),
    #[error("{0} cannot be told apart from the real data folder: {1}")]
    Unclear(&'static str, #[source] FolderError),
    #[error(
        "Windows did not name the roaming AppData folder, so {0} cannot be told apart from the real data folder"
    )]
    NoRoamingFolder(&'static str),
}

pub fn refuse_real_data_folder(argument: &'static str, path: &Path) -> Result<(), Refusal> {
    refuse_within(real_data_folder().as_deref(), argument, path)
}

// What cannot be decided is refused: a start on the wrong folder is the mistake this is here for.
pub fn refuse_within(
    real: Option<&Path>,
    argument: &'static str,
    path: &Path,
) -> Result<(), Refusal> {
    let real = real.ok_or(Refusal::NoRoamingFolder(argument))?;
    match is_within(real, path) {
        Ok(false) => Ok(()),
        Ok(true) => Err(Refusal::RealDataFolder(argument)),
        Err(unclear) => Err(Refusal::Unclear(argument, unclear)),
    }
}

// Two spellings name one folder far more often than they look alike, so each side is held both as written and as the file system resolves it: case, short names, "..", a trailing dot, a junction or a link on the way.
fn is_within(root: &Path, path: &Path) -> Result<bool, FolderError> {
    let written = absolute(path)?;
    // Only "D:\folder" means one thing to every reader: std takes a \\?\ path letter for letter where SQLite tidies it first, and a share can be this PC under another name.
    if !on_a_lettered_drive(&written, false) {
        return Err(FolderError::NotPlain);
    }
    let as_written = key(&written)?;
    let resolved = resolve(&written)?;
    if !on_a_lettered_drive(&resolved, true) {
        return Err(FolderError::NotLocal);
    }
    let roots = [key(&absolute(root)?)?, key(&resolve(root)?)?];
    let paths = [as_written, key(&resolved)?];
    Ok(paths
        .iter()
        .any(|path| roots.iter().any(|root| path.starts_with(root))))
}

fn on_a_lettered_drive(path: &Path, resolved: bool) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => true,
            Prefix::VerbatimDisk(_) => resolved,
            _ => false,
        },
        _ => false,
    }
}

pub fn identity(path: &Path) -> Result<String, FolderError> {
    Ok(key_by(&resolve(path)?, upcase)?.join("\\"))
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
        // What is there and still does not resolve is a link to nowhere, and it says where a folder made through it would land. A junction says so in full; a relative target counts from where the link really is, and that, like a link that cannot be read, is refused rather than worked out.
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                let target = std::fs::read_link(&path).map_err(|_| FolderError::DeadLink)?;
                links += 1;
                if links > MOST_LINKS {
                    return Err(FolderError::TooManyLinks);
                }
                if !target.is_absolute() {
                    return Err(FolderError::DeadLink);
                }
                path = absolute(target)?;
                continue;
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            Err(_) => {}
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
    key_by(path, |name| Ok(fold(name)))
}

fn key_by(
    path: &Path,
    fold: impl Fn(&str) -> Result<String, FolderError>,
) -> Result<Vec<String>, FolderError> {
    let mut key = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => key.push(match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    format!("{}:", char::from(letter.to_ascii_uppercase()))
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                    r"\\{}\{}",
                    fold(&server.to_string_lossy())?,
                    fold(&share.to_string_lossy())?
                ),
                Prefix::DeviceNS(name) | Prefix::Verbatim(name) => {
                    format!(r"\\.\{}", fold(&name.to_string_lossy())?)
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
                key.push(fold(&name)?);
            }
        }
    }
    Ok(key)
}

// Folding both ways joins more names than NTFS does, never fewer: a folder may be refused that is not the same one, and none is let through that is.
fn fold(name: &str) -> String {
    name.to_uppercase().to_lowercase()
}

// The file system's own casing, one unit for one, so "straße" and "strasse" stay two folders as they are on disk.
fn upcase(name: &str) -> Result<String, FolderError> {
    let wide: Vec<u16> = name.encode_utf16().collect();
    if wide.is_empty() {
        return Ok(String::new());
    }
    let mut upper = vec![0_u16; wide.len()];
    let written = unsafe {
        LCMapStringEx(
            LOCALE_NAME_INVARIANT,
            LCMAP_UPPERCASE,
            &wide,
            Some(&mut upper),
            None,
            None,
            LPARAM(0),
        )
    };
    if written <= 0 {
        return Err(io::Error::last_os_error().into());
    }
    upper.truncate(written as usize);
    Ok(String::from_utf16_lossy(&upper))
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

    #[derive(Debug, PartialEq)]
    enum Verdict {
        Elsewhere,
        Real,
        Unclear(FolderError),
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

    fn verdict(root: &Path, path: impl AsRef<Path>) -> Verdict {
        match refuse_within(Some(root), "--data-dir", path.as_ref()) {
            Ok(()) => Verdict::Elsewhere,
            Err(Refusal::RealDataFolder("--data-dir")) => Verdict::Real,
            Err(Refusal::Unclear("--data-dir", unclear)) => Verdict::Unclear(unclear),
            Err(other) => panic!("{other:?}"),
        }
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn the_folder_itself_and_what_is_inside_it_are_the_real_one() {
        for made in [true, false] {
            let StandIn { scratch, root } = stand_in(made);
            for inside in [
                root.clone(),
                root.join("backups"),
                root.join("backups").join("db"),
                root.join("not").join("there").join("yet"),
                root.join("shot.png"),
            ] {
                assert_eq!(
                    verdict(&root, &inside),
                    Verdict::Real,
                    "made {made}: {}",
                    inside.display()
                );
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
                assert_eq!(
                    verdict(&root, &outside),
                    Verdict::Elsewhere,
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
                format!(r"{roaming}.\OneLaunch"),
            ] {
                assert_eq!(
                    verdict(&root, &spelling),
                    Verdict::Real,
                    "made {made}: {spelling}"
                );
            }
            for other in [
                format!(r"{written}\.."),
                format!(r"{written}\..\Other"),
                format!(r"{written}\backups\..\..\..\elsewhere"),
                format!("{written}x"),
                format!("{written} 2"),
            ] {
                assert_eq!(
                    verdict(&root, &other),
                    Verdict::Elsewhere,
                    "made {made}: {other}"
                );
            }
        }
    }

    #[test]
    fn only_a_plain_path_on_a_lettered_drive_is_taken() {
        for made in [true, false] {
            let StandIn { scratch, root } = stand_in(made);
            for folder in [root.clone(), scratch.join("elsewhere")] {
                let written = text(&folder);
                let (drive, rest) = written.split_at(1);
                let rest = &rest[1..];
                for spelling in [
                    format!(r"\\?\{written}"),
                    format!(r"\\?\{}", written.to_uppercase()),
                    format!(r"\\?\{written}."),
                    format!(r"\\?\{}", written.replace('\\', "/")),
                    format!(r"\\?\{}", written.replace(r"\Roaming\", r"\Roaming.\")),
                    format!(r"\\.\{written}"),
                    format!(r"\\.\{written}\backups"),
                    format!(r"\\localhost\{drive}${rest}"),
                    format!(r"\\127.0.0.1\{drive}${rest}"),
                    format!(r"//localhost/{drive}${}", rest.replace('\\', "/")),
                    format!(r"\\?\UNC\localhost\{drive}${rest}"),
                    format!(r"\\.\UNC\localhost\{drive}${rest}"),
                    format!(r"\\?\GLOBALROOT\Device\HarddiskVolume1{rest}"),
                ] {
                    assert_eq!(
                        verdict(&root, &spelling),
                        Verdict::Unclear(FolderError::NotPlain),
                        "made {made}: {spelling}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_path_that_resolves_to_a_share_is_not_local() {
        for (path, resolved, expected) in [
            (r"D:\folder", false, true),
            (r"d:\", false, true),
            (r"\\?\D:\folder", true, true),
            (r"D:\folder", true, true),
            (r"\\?\D:\folder", false, false),
            (r"\\?\UNC\server\share\folder", true, false),
            (r"\\server\share\folder", true, false),
            (r"\\server\share\folder", false, false),
            (r"\\.\D:\folder", true, false),
            (r"\\?\Volume{0}\folder", true, false),
            (r"\folder", false, false),
            (r"folder", false, false),
            (r"D:folder", false, true),
        ] {
            assert_eq!(
                on_a_lettered_drive(Path::new(path), resolved),
                expected,
                "{path} resolved {resolved}"
            );
        }
    }

    #[test]
    fn a_junction_to_the_folder_or_into_it_is_the_real_one() {
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
            assert_eq!(
                verdict(&root, &inside),
                Verdict::Real,
                "{}",
                inside.display()
            );
        }

        let away = scratch.join("away");
        junction(&away, &scratch.join("elsewhere"));
        let out = root.join("out");
        junction(&out, &scratch.join("elsewhere"));
        assert_eq!(verdict(&root, &away), Verdict::Elsewhere);
        assert_eq!(verdict(&root, away.join("sub")), Verdict::Elsewhere);
        assert_eq!(
            verdict(&root, &out),
            Verdict::Real,
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
            assert_eq!(
                verdict(&through, &inside),
                Verdict::Real,
                "{}",
                inside.display()
            );
        }
        assert_eq!(
            verdict(&through, scratch.join("elsewhere")),
            Verdict::Elsewhere
        );
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
        assert_eq!(verdict(&root, &link), Verdict::Real);
        assert_eq!(verdict(&root, link.join("sub")), Verdict::Real);

        let twice = scratch.join("twice");
        fs::create_dir(&root).unwrap();
        junction(&twice, &root.join("backups"));
        fs::remove_dir(&root).unwrap();
        assert_eq!(verdict(&root, &twice), Verdict::Real);
        assert_eq!(
            verdict(&scratch.join("elsewhere"), &twice),
            Verdict::Elsewhere
        );
        for link in [&link, &twice] {
            fs::remove_dir(link).unwrap();
        }
    }

    fn dead_chain(scratch: &Scratch, links: usize, end: &Path) -> Vec<PathBuf> {
        let chain: Vec<PathBuf> = (0..links)
            .map(|index| scratch.join(&format!("hop-{index}")))
            .collect();
        for (index, link) in chain.iter().enumerate().rev() {
            junction(link, chain.get(index + 1).map_or(end, PathBuf::as_path));
        }
        chain
    }

    #[test]
    fn a_chain_of_dead_links_is_followed_so_far_and_no_further() {
        let StandIn { scratch, root } = stand_in(false);
        let short = dead_chain(&scratch, MOST_LINKS, &root.join("backups"));
        assert!(fs::canonicalize(&short[0]).is_err());
        assert_eq!(verdict(&root, &short[0]), Verdict::Real);
        assert_eq!(
            verdict(&scratch.join("elsewhere"), &short[0]),
            Verdict::Elsewhere
        );
        for link in &short {
            fs::remove_dir(link).unwrap();
        }

        let long = dead_chain(&scratch, MOST_LINKS + 1, &scratch.join("nowhere"));
        assert_eq!(
            verdict(&root, &long[0]),
            Verdict::Unclear(FolderError::TooManyLinks)
        );
        for link in &long {
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
        let refused = verdict(&root, &one);
        assert!(
            matches!(refused, Verdict::Unclear(FolderError::Lookup(_))),
            "{refused:?}"
        );
        for link in [&one, &two] {
            fs::remove_dir(link).unwrap();
        }
    }

    #[test]
    fn a_dead_link_with_a_relative_target_is_refused() {
        let StandIn { scratch, root } = stand_in(false);
        let link = scratch.join("relative");
        let made = std::os::windows::fs::symlink_dir(r"Roaming\OneLaunch", &link);
        if made.is_err() {
            // Without developer mode or an elevated prompt Windows lets nobody make a symbolic link; a junction cannot hold a relative target.
            eprintln!("not run: this user may not make a symbolic link");
            return;
        }
        assert_eq!(
            verdict(&root, &link),
            Verdict::Unclear(FolderError::DeadLink)
        );
        assert_eq!(
            verdict(&root, link.join("sub")),
            Verdict::Unclear(FolderError::DeadLink)
        );
        fs::create_dir(&root).unwrap();
        assert_eq!(verdict(&root, &link), Verdict::Real);
        fs::remove_dir(&link).unwrap();
    }

    #[test]
    fn a_link_to_a_share_is_refused() {
        let StandIn { scratch, root } = stand_in(true);
        let link = scratch.join("shared");
        let made = std::os::windows::fs::symlink_dir(r"\\localhost\no-such-share\folder", &link);
        if made.is_err() {
            eprintln!("not run: this user may not make a symbolic link");
            return;
        }
        for path in [link.clone(), link.join("sub")] {
            let refused = verdict(&root, &path);
            assert!(
                matches!(
                    refused,
                    Verdict::Unclear(FolderError::NotLocal | FolderError::Lookup(_))
                ),
                "{refused:?}"
            );
        }
        fs::remove_dir(&link).unwrap();
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
                assert_eq!(
                    verdict(&root, &spelling),
                    Verdict::Unclear(FolderError::NotPlain),
                    "made {made}: {spelling}"
                );
            }
        }
    }

    #[test]
    fn a_name_windows_cannot_look_up_is_refused() {
        let StandIn { scratch, root } = stand_in(true);
        assert_eq!(
            verdict(&root, scratch.join("a*b")),
            Verdict::Unclear(FolderError::Lookup(io::ErrorKind::InvalidFilename))
        );
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the variable is the second opinion on which folder Windows means"
    )]
    fn windows_names_the_roaming_folder() {
        let folder = real_data_folder().unwrap();
        assert!(folder.is_absolute());
        assert_eq!(folder.file_name().unwrap(), APP_FOLDER);
        let roaming = PathBuf::from(std::env::var_os("APPDATA").unwrap());
        assert_eq!(
            key(folder.parent().unwrap()).unwrap(),
            key(&roaming).unwrap(),
            "not the folder %APPDATA% names"
        );
    }

    #[test]
    fn a_refusal_names_its_argument_and_no_folder() {
        let StandIn { scratch, root } = stand_in(true);
        let refused = |argument, path: &Path| {
            let refusal = refuse_within(Some(&root), argument, path).unwrap_err();
            assert!(!refusal.to_string().contains(&text(scratch.path())));
            assert!(!format!("{refusal:?}").contains(&text(scratch.path())));
            refusal
        };

        let real = refused("--data-dir", &root.join("BACKUPS"));
        assert_eq!(real, Refusal::RealDataFolder("--data-dir"));
        assert_eq!(
            real.to_string(),
            "--data-dir is the real data folder (%APPDATA%\\OneLaunch) or inside it; this build runs only on a copy of it or on a scratch folder"
        );
        let shot = refused("--capture", &root.join("shot.png"));
        assert!(
            shot.to_string()
                .starts_with("--capture is the real data folder")
        );

        let unclear = refused("--data-dir", &scratch.join("a*b"));
        assert_eq!(
            unclear.to_string(),
            "--data-dir cannot be told apart from the real data folder: Windows could not look it up (invalid filename)"
        );
        let unclear = refused("--capture", Path::new(&format!("{}:x", text(&root))));
        assert_eq!(
            unclear.to_string(),
            "--capture cannot be told apart from the real data folder: it is not written as a plain path on a lettered drive, such as D:\\folder"
        );

        let unknown = refuse_within(None, "--data-dir", &scratch.join("elsewhere")).unwrap_err();
        assert_eq!(unknown, Refusal::NoRoamingFolder("--data-dir"));
        assert_eq!(
            unknown.to_string(),
            "Windows did not name the roaming AppData folder, so --data-dir cannot be told apart from the real data folder"
        );

        refuse_within(Some(&root), "--data-dir", &scratch.join("elsewhere")).unwrap();
        refuse_within(Some(&root), "--data-dir", &scratch.join("not there yet")).unwrap();
    }
}
