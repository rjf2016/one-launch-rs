// What a registry port and a file port must both do, the fake's and the real one: the tests of each run these, so the pipeline proven on the fakes meets the same behaviour on the machine.

use std::fs::OpenOptions;
use std::io;
use std::os::windows::fs::OpenOptionsExt;

use time::{Duration, OffsetDateTime};

use crate::{Files, Hive, Registry, RegistryData};

fn names(listed: &[crate::RegistryNamedValue]) -> Vec<&str> {
    listed.iter().map(|value| value.name.as_str()).collect()
}

// `base` is a key under HKCU that holds nothing yet; everything is written below it.
pub fn registry(registry: &dyn Registry, base: &str) {
    let hive = Hive::Hkcu;
    let key = format!(r"{base}\Launcher");
    let nowhere = format!(r"{base}\Nowhere");

    assert_eq!(registry.get_string(hive, &key, "Name").unwrap(), None);
    assert_eq!(registry.get_dword(hive, &key, "Count").unwrap(), None);
    assert_eq!(registry.get_string(hive, &key, "").unwrap(), None);
    assert!(registry.list_values(hive, &nowhere).unwrap().is_empty());
    assert!(
        registry
            .keys_holding(hive, &nowhere, "remove")
            .unwrap()
            .is_empty()
    );
    registry.delete_value(hive, &nowhere, "Name").unwrap();
    assert!(registry.list_values(hive, &nowhere).unwrap().is_empty());

    registry
        .set_string(hive, &key, "Name", "Ünïcode name")
        .unwrap();
    registry.set_dword(hive, &key, "Count", 7).unwrap();
    registry.set_binary(hive, &key, "Token", "0a0bFF").unwrap();
    registry.set_string(hive, &key, "", "the default").unwrap();
    assert_eq!(
        registry
            .get_string(hive, &key.to_uppercase(), "name")
            .unwrap()
            .as_deref(),
        Some("Ünïcode name")
    );
    assert_eq!(registry.get_dword(hive, &key, "COUNT").unwrap(), Some(7));
    assert_eq!(registry.get_dword(hive, &key, "Name").unwrap(), None);
    assert_eq!(registry.get_string(hive, &key, "Count").unwrap(), None);
    assert_eq!(registry.get_string(hive, &key, "Token").unwrap(), None);
    assert_eq!(registry.get_dword(hive, &key, "Token").unwrap(), None);
    assert_eq!(
        registry.get_string(hive, &key, "").unwrap().as_deref(),
        Some("the default")
    );
    assert_eq!(registry.get_string(Hive::Hklm, &key, "Name").unwrap(), None);

    let listed = registry.list_values(hive, &key).unwrap();
    assert_eq!(names(&listed), ["", "Count", "Name", "Token"]);
    assert!(listed[1].data == RegistryData::Dword(7));
    assert!(listed[2].data == RegistryData::Text("Ünïcode name".to_owned()));
    assert!(listed[3].data == RegistryData::Binary("0A0BFF".to_owned()));

    registry.set_dword(hive, &key, "name", 1).unwrap();
    assert_eq!(registry.get_string(hive, &key, "Name").unwrap(), None);
    assert_eq!(registry.get_dword(hive, &key, "Name").unwrap(), Some(1));
    registry.set_string(hive, &key, "Name", "back").unwrap();
    assert_eq!(
        registry.get_string(hive, &key, "Name").unwrap().as_deref(),
        Some("back")
    );

    registry.delete_value(hive, &key, "COUNT").unwrap();
    assert_eq!(registry.get_dword(hive, &key, "Count").unwrap(), None);
    registry.delete_value(hive, &key, "Count").unwrap();
    assert_eq!(
        names(&registry.list_values(hive, &key).unwrap()),
        ["", "Name", "Token"]
    );

    let products = format!(r"{base}\Products");
    let text = |below: &str, name: &str, value: &str| {
        let at = if below.is_empty() {
            products.clone()
        } else {
            format!(r"{products}\{below}")
        };
        registry.set_string(hive, &at, name, value).unwrap();
    };
    text("One", "Uninstall", "Remove One.exe");
    text("One", "Name", "One");
    text("Two", "Uninstall", "other.exe");
    text(r"Deep\Er", "Uninstall", "REMOVE x");
    text("", "Own", "remove");
    registry
        .set_dword(hive, &format!(r"{products}\Three"), "Remove", 3)
        .unwrap();
    assert_eq!(
        registry.keys_holding(hive, &products, "remove").unwrap(),
        [r"Deep\Er", "One"]
    );
    assert_eq!(
        registry
            .keys_holding(hive, &products.to_lowercase(), "OTHER.EXE")
            .unwrap(),
        ["Two"]
    );
    assert!(
        registry
            .keys_holding(hive, &products, "nothing holds this")
            .unwrap()
            .is_empty()
    );
    let own = registry.list_values(hive, &products).unwrap();
    assert!(own.len() == 1 && own[0].name == "Own");
}

// `folder` is an empty folder the port may write in.
pub fn files(files: &dyn Files, folder: &str) {
    let path = |relative: &str| format!(r"{folder}\{relative}");
    let first = path(r"sub\a.txt");
    let second = path("b.txt");

    assert!(!files.exists(&first));
    assert_eq!(
        files.read(&first).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert!(files.size(&first).is_err());
    assert!(files.list(&path("sub")).is_err());
    assert!(files.resolved(&path("missing")).is_none());

    files
        .write_atomic(&[(&first, b"0123456789"), (&second, b"old")])
        .unwrap();
    assert!(files.exists(&first));
    assert!(files.exists(&first.to_uppercase()));
    assert!(files.exists(&path("sub")));
    assert_eq!(files.size(&first).unwrap(), 10);
    assert_eq!(files.read(&first).unwrap(), b"0123456789");
    assert_eq!(files.read_range(&first, 7, 100).unwrap(), b"789");
    assert_eq!(files.read_range(&first, 2, 3).unwrap(), b"234");
    assert!(files.read_range(&first, 20, 5).unwrap().is_empty());
    assert_eq!(files.list(&path("sub")).unwrap(), ["a.txt"]);
    let age = OffsetDateTime::now_utc() - files.modified(&first).unwrap();
    assert!(age < Duration::hours(1) && age > -Duration::hours(1));
    let resolved = files.resolved(folder).unwrap();
    assert!(resolved.eq_ignore_ascii_case(folder), "{resolved}");

    files
        .write_atomic(&[(&second, b"new"), (&first, b"abc")])
        .unwrap();
    assert_eq!(files.read(&second).unwrap(), b"new");
    assert_eq!(files.read(&first).unwrap(), b"abc");
    assert_eq!(files.list(folder).unwrap(), ["b.txt", "sub"]);

    files
        .write_atomic(&[(&path("blocker"), b"a file where a folder is wanted")])
        .unwrap();
    assert!(
        files
            .write_atomic(&[(&second, b"newer"), (&path(r"blocker\c.txt"), b"x")])
            .is_err()
    );
    assert_eq!(files.read(&second).unwrap(), b"new");
    assert_eq!(files.list(folder).unwrap(), ["b.txt", "blocker", "sub"]);
    assert_eq!(files.list(&path("sub")).unwrap(), ["a.txt"]);

    // Held open with no sharing, as a running launcher can hold its file: the second move fails after the first target was replaced.
    let held = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&second)
        .unwrap();
    let refused = files.write_atomic(&[(&first, b"first new"), (&second, b"second new")]);
    drop(held);
    assert!(refused.is_err());
    assert_eq!(files.read(&first).unwrap(), b"abc");
    assert_eq!(files.read(&second).unwrap(), b"new");
    assert!(
        files
            .write_atomic(&[(&first, b"first new"), (&path("sub"), b"over a folder")])
            .is_err()
    );
    assert_eq!(files.read(&first).unwrap(), b"abc");
    assert_eq!(files.list(folder).unwrap(), ["b.txt", "blocker", "sub"]);
    assert_eq!(files.list(&path("sub")).unwrap(), ["a.txt"]);

    // The folders a write that failed made are gone again; one that was there stays.
    assert!(
        files
            .write_atomic(&[
                (&path(r"made\deeper\c.txt"), b"x"),
                (&path(r"sub\new\d.txt"), b"y"),
                (&path(r"blocker\e.txt"), b"z"),
            ])
            .is_err()
    );
    assert_eq!(files.list(folder).unwrap(), ["b.txt", "blocker", "sub"]);
    assert_eq!(files.list(&path("sub")).unwrap(), ["a.txt"]);

    // A file that already has a name the write could pick for itself is neither followed, emptied nor moved.
    let planted = |suffix: &str| format!("{second}.onelaunch-{}.{suffix}", std::process::id());
    for suffix in ["tmp", "old"] {
        std::fs::write(planted(suffix), b"planted").unwrap();
    }
    files.write_atomic(&[(&second, b"newest")]).unwrap();
    assert_eq!(files.read(&second).unwrap(), b"newest");
    for suffix in ["tmp", "old"] {
        assert_eq!(
            files.read(&planted(suffix)).unwrap(),
            b"planted",
            "{suffix}"
        );
        std::fs::remove_file(planted(suffix)).unwrap();
    }
    assert_eq!(files.list(folder).unwrap(), ["b.txt", "blocker", "sub"]);
}
