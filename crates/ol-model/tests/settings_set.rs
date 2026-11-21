#![forbid(unsafe_code)]

// Every case of oracle/l1 for settings:set whose key and value are text, put to the rule of each key. The others are refused by the shape of the call, which the Rust types rule out.

use ol_model::{SettingKey, check_setting};
use ol_testkit::vectors::{Vectors, plain};
use serde_json::Value;

#[test]
fn every_refusal_of_a_text_value_is_the_electron_apps() {
    let vectors = Vectors::load("l1/core/ipc.settings-set.json");
    let (mut checked, mut unknown_keys, mut by_shape) = (0, 0, 0);
    let mut failures = Vec::new();
    for case in &vectors.cases {
        let envelope = case.out().expect("every case answers an envelope");
        let expected = envelope["error"]["message"]
            .as_str()
            .expect("every case is refused");
        let trusted = case.args[2]["trusted"].as_bool() == Some(true);
        let payload = &case.args[1];
        // A recorded string is plain text or, where it needs one, a list of UTF-16 units; a date or bytes are no text.
        let text = |field: &str| {
            let raw = payload.get(field)?;
            let is_text = raw.is_string() || raw.get("$utf16").is_some();
            match plain(raw) {
                Value::String(text) if is_text && trusted && !case.has_unpaired_surrogate() => {
                    Some(text)
                }
                _ => None,
            }
        };
        let (Some(key), Some(value)) = (text("key"), text("value")) else {
            by_shape += 1;
            continue;
        };
        let Some(key) = SettingKey::parse(&key) else {
            unknown_keys += 1;
            assert_eq!(expected, "key is not an allowed value", "{}", case.id);
            continue;
        };
        checked += 1;
        match check_setting(key, &value) {
            Err(refusal) if refusal.to_string() == expected && refusal.code() == "BAD_REQUEST" => {}
            other => failures.push(format!("{}: {other:?}, expected {expected}", case.id)),
        }
    }
    println!(
        "settings:set: {} cases, {checked} checked, {unknown_keys} unknown keys, {by_shape} refused by their shape",
        vectors.cases.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!((checked, unknown_keys, by_shape), (144, 10, 60));
}

#[test]
fn every_value_the_settings_screen_writes_is_taken() {
    for key in SettingKey::TOGGLES {
        for value in ["true", "false"] {
            assert_eq!(check_setting(key, value), Ok(()), "{key:?} {value}");
        }
    }
    for (key, values) in [
        (
            SettingKey::LogLevel,
            &["debug", "info", "warn", "error"][..],
        ),
        (SettingKey::EpicLaunchUri, &["catalog", "appname"]),
        (SettingKey::BattlenetLaunch, &["exec", "uri"]),
        (
            SettingKey::BackupsKeep,
            &["1", "5", "20", "100", "200", "007"],
        ),
        (
            SettingKey::WhatsNewLastSeen,
            &["1.0.0", "1.0.0-beta.2", "10.20.30-rc-1.x"],
        ),
    ] {
        for value in values {
            assert_eq!(check_setting(key, value), Ok(()), "{key:?} {value}");
        }
    }
    let refused =
        |key, value: &str| check_setting(key, value).map_err(|refusal| refusal.to_string());
    assert_eq!(
        refused(SettingKey::DryRun, &"é".repeat(4096)),
        Err("dryRun must be true or false".to_owned())
    );
    assert_eq!(
        refused(SettingKey::DryRun, &"🎮".repeat(2049)),
        Err("value exceeds 4096 characters".to_owned())
    );
    assert_eq!(
        refused(SettingKey::SteamPath, "true"),
        Err("steam.path is not user-settable".to_owned())
    );
}
