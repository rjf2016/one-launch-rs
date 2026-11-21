#![forbid(unsafe_code)]

use std::sync::LazyLock;

use ol_model::LogLevel;
use ol_model::redact::AccountNames;
use ol_services::log::redact::{MAX_TEXT_LENGTH, format_line, redact_string};
use ol_testkit::oracle;
use ol_testkit::vectors::{Vectors, plain};
use regex::Regex;
use serde_json::{Value, json};

// Values of each shape, taken from the oracle's own files.
struct Shapes {
    steam_id: String,
    uuid_upper: String,
    jwt: String,
    epic_id: String,
    token: String,
    blob: String,
    profiles: Vec<String>,
}

fn vector_text(vectors: &Vectors, id: &str) -> String {
    let case = vectors
        .cases
        .iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("oracle/l1/core/redactString.json has {id}"));
    plain(&case.args[1]).as_str().unwrap().to_owned()
}

fn found(pattern: &str, text: &str) -> String {
    Regex::new(pattern)
        .unwrap()
        .find(text)
        .expect("the oracle's text has the value")
        .as_str()
        .to_owned()
}

static SHAPES: LazyLock<Shapes> = LazyLock::new(|| {
    let vectors = Vectors::load("l1/core/redactString.json");
    let scenario: Value = serde_json::from_str(
        &std::fs::read_to_string(
            oracle("fixtures/dev-only/fake-launchers/all-four").join("scenario.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let registry = scenario["registry"].as_object().unwrap();
    let token = registry.values().next().unwrap().as_str().unwrap();
    Shapes {
        steam_id: found(r"7656119[0-9]{10}", &vector_text(&vectors, "steamid")),
        uuid_upper: found(
            r"[0-9A-Fa-f]{8}-[0-9A-Fa-f-]{27}",
            &vector_text(&vectors, "uuid-upper"),
        ),
        jwt: found(r"eyJ\S+", &vector_text(&vectors, "jwt")),
        epic_id: scenario["onDisk"]["epic"].as_str().unwrap().to_owned(),
        token: token.strip_prefix("binary:").unwrap().to_owned(),
        blob: std::fs::read_to_string(oracle("fixtures/launchers/epic/GameUserSettings.ini"))
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("Data="))
            .unwrap()
            .to_owned(),
        profiles: [
            "profile-path-spaced-name",
            "profile-path-forward",
            "profile-path-json-escaped",
        ]
        .iter()
        .map(|id| vector_text(&vectors, id))
        .collect(),
    }
});

fn names(given: &[&str]) -> AccountNames {
    let mut names = AccountNames::new();
    names.add(given.iter().copied());
    names
}

fn line(names: &AccountNames, message: &str, fields: Option<Value>) -> String {
    format_line(names, "t", LogLevel::Info, "main", message, fields.as_ref())
}

fn fields_of(line: &str) -> Value {
    let json = line
        .trim_end_matches('\n')
        .strip_prefix("t INFO  [main] m ")
        .expect("a line with fields");
    serde_json::from_str(json).expect("the fields are JSON")
}

fn assert_absent(written: &str, secret: &str) {
    assert!(
        !written.to_lowercase().contains(&secret.to_lowercase()),
        "the line still holds a value {} characters long",
        secret.len()
    );
}

#[test]
fn a_token_split_over_two_fields_leaves_neither_half() {
    let blob = &SHAPES.blob;
    let (first, second) = blob.split_at(blob.len() / 2);
    let written = line(
        &names(&[]),
        "m",
        Some(json!({ "first": first, "second": second })),
    );
    assert_absent(&written, first);
    assert_absent(&written, second);
    let written = line(
        &names(&["gordon_f"]),
        "signed in as",
        Some(json!({ "who": "gordon_f" })),
    );
    assert_absent(&written, "gordon_f");
}

#[test]
fn a_secret_deep_in_a_value_never_comes_out() {
    let deep =
        json!({ "a": { "b": { "c": { "d": { "e": { "f": { "g": "token=deepvalue" } } } } } } });
    assert_absent(&line(&names(&[]), "m", Some(deep)), "deepvalue");
    let nested =
        json!({ "list": [{ "inner": [{ "session_id": "sessionvalue" }] }], "x": ["gordon_f"] });
    let written = line(&names(&["gordon_f"]), "m", Some(nested));
    assert_absent(&written, "sessionvalue");
    assert_absent(&written, "gordon_f");
    let keyed = json!({ "gordon_f": 1 });
    assert_absent(&line(&names(&["gordon_f"]), "m", Some(keyed)), "gordon_f");
}

#[test]
fn a_name_in_a_path_is_masked_and_the_windows_user_too() {
    let names = names(&["gordon_f"]);
    for path in [
        r"D:\Saves\gordon_f\slot1",
        r"D:\Saves\gordon_f.cfg",
        "d:/saves/GORDON_F/x",
    ] {
        assert_absent(&redact_string(&names, path), "gordon_f");
    }
    for path in &SHAPES.profiles {
        let written = redact_string(&names, path);
        assert_absent(&written, "jo smith");
        assert_absent(&written, "someone");
        assert!(written.contains("[USER]"));
    }
}

#[test]
fn the_windows_user_is_masked_outside_the_users_folder() {
    let mut names = AccountNames::new();
    names.add_user("jo.smith");
    names.add_user("jsmith");
    for path in [
        r"D:\Profiles\jo.smith\AppData\Local\Temp",
        r"\\server\redirect$\jo.smith\Documents",
        r#"{"dataDir":"D:\\Profiles\\JO.SMITH\\AppData"}"#,
        "e:/home/jsmith/x",
    ] {
        let written = redact_string(&names, path);
        assert_absent(&written, "jo.smith");
        assert_absent(&written, "jsmith");
        assert!(written.contains("[USER]"));
    }
}

#[test]
fn a_url_keeps_no_token_name_or_password() {
    let names = names(&["gordon_f"]);
    let written = redact_string(
        &names,
        "https://example.com/cb?access_token=abc123xyz&state=1 https://example.com/u/gordon_f/profile https://user:hunter22@example.com/",
    );
    assert_absent(&written, "abc123xyz");
    assert_absent(&written, "gordon_f");
    assert_absent(&written, "hunter22");
}

#[test]
fn json_inside_a_string_is_redacted_however_often_it_was_escaped() {
    let names = names(&[]);
    let plain = redact_string(
        &names,
        r#"body {"password":"hunter22","refresh_token":"tok1"}"#,
    );
    assert_absent(&plain, "hunter22");
    assert_absent(&plain, "tok1");
    let escaped = redact_string(&names, r#"body {\"password\":\"hunter22\"}"#);
    assert_absent(&escaped, "hunter22");
    let in_field = line(
        &names,
        "m",
        Some(json!({ "body": r#"{"token":"tokvalue","nested":"{\"secret\":\"deeper\"}"}"# })),
    );
    assert_absent(&in_field, "tokvalue");
    assert_absent(&in_field, "deeper");
}

#[test]
fn the_fields_stay_json_whatever_the_rules_take_out() {
    let names = names(&["gordon_f"]);
    let written = line(
        &names,
        "m",
        Some(json!({
            "body": r#"{"password":"hunter22","next":"{\"secret\":\"deeper\"}"}"#,
            "ini": "[RememberMe]\nData=blobvalue",
            "command": "steam.exe -login gordon_f hunter33",
            "path": r"C:\Saves\gordon_f",
        })),
    );
    let fields = fields_of(&written);
    let text = fields.to_string();
    for secret in ["hunter22", "deeper", "blobvalue", "hunter33", "gordon_f"] {
        assert_absent(&text, secret);
    }
}

#[test]
fn a_name_in_another_case_or_with_accents_is_masked() {
    let names = names(&["José_99", "Gordon_F"]);
    for text in [
        "JOSÉ_99 here",
        "josé_99 here",
        "gordon_f",
        "GORDON_F",
        "gOrDoN_f",
    ] {
        let written = redact_string(&names, text);
        assert_absent(&written, "josé_99");
        assert_absent(&written, "gordon_f");
    }
}

#[test]
fn a_name_whose_lower_case_folds_elsewhere_is_masked_as_it_was_written() {
    for special in [
        '\u{130}', '\u{1e9e}', '\u{212a}', '\u{2126}', '\u{212b}', '\u{3f4}',
    ] {
        for name in [
            format!("{special}lker"),
            format!("il{special}ker"),
            format!("ilke{special}"),
        ] {
            let names = names(&[name.as_str()]);
            let written = redact_string(&names, &format!("Steam signed in as {name} instead"));
            assert_eq!(
                written, "Steam signed in as [ACCOUNT] instead",
                "U+{:04X}",
                special as u32
            );
        }
    }
}

#[test]
fn keys_and_patterns_in_upper_case_are_redacted() {
    let shapes = &*SHAPES;
    let written = redact_string(
        &names(&[]),
        &format!(
            "PASSWORD=hunter22 TOKEN: tok1 SOME.ONE@EXAMPLE.COM {} {} -LOGIN user1 hunter33",
            shapes.uuid_upper,
            shapes.profiles[1].to_uppercase()
        ),
    );
    for secret in [
        "hunter22",
        "tok1",
        "some.one@example.com",
        &shapes.uuid_upper[..20],
        "someone",
        "hunter33",
    ] {
        assert_absent(&written, secret);
    }
}

#[test]
fn a_name_inside_punctuation_is_masked() {
    let names = names(&["gordon_f"]);
    for text in [
        "(gordon_f)",
        "gordon_f's",
        "gordon_f:",
        "\"gordon_f\"",
        "gordon_f.",
        "-gordon_f-",
    ] {
        assert_absent(&redact_string(&names, text), "gordon_f");
    }
}

#[test]
fn a_name_inside_an_email_or_a_token_does_not_let_the_rest_through() {
    let names = names(&["bob", "gordon_f"]);
    let written = redact_string(&names, "signed in as smith.bob@example.com");
    assert_eq!(written, "signed in as [EMAIL]");
    let run = format!("{0}-gordon_f-{0}", "A1b2".repeat(6));
    let written = redact_string(&names, &format!("blob {run} end"));
    assert_eq!(written, "blob [REDACTED] end");
    let jwt = SHAPES.jwt.replacen('.', "-gordon_f.", 1);
    assert_eq!(
        redact_string(&names, &format!("x {jwt} y")),
        "x [REDACTED] y"
    );
}

#[test]
fn a_secret_across_the_cut_leaves_no_start_of_itself() {
    let names = names(&["gordon freeman"]);
    let shapes = &*SHAPES;
    // Each filler leaves eight characters of the secret before the cut, were the cut to come first.
    let message_filler = format!("{} ", "x".repeat(MAX_TEXT_LENGTH - 9));
    let field_filler = format!(
        "{} ",
        "x".repeat(MAX_TEXT_LENGTH - 9 - r#"{"stack":""#.len())
    );
    for secret in [
        shapes.token.clone(),
        shapes.jwt.clone(),
        "gordon freeman".to_owned(),
    ] {
        let written = line(&names, &format!("{message_filler}{secret}"), None);
        assert_absent(&written, &secret[..8]);
        let fields = json!({ "stack": format!("{field_filler}{secret}") });
        assert_absent(&line(&names, "m", Some(fields)), &secret[..8]);
    }
}

#[test]
fn an_account_named_like_a_common_word_or_part_of_a_launcher_is_masked_anyway() {
    let names = names(&["Gamer", "Battle", "Overwatch"]);
    let written = redact_string(
        &names,
        "the gamer started Battle.net and launched Overwatch for gamer",
    );
    assert_absent(&written, "gamer");
    assert_absent(&written, "battle");
    assert_absent(&written, "overwatch");
}

#[test]
fn an_email_in_an_error_from_windows_is_masked() {
    let error = std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        r"Access is denied. (os error 5): D:\Profiles\some.one+tag@example.co.uk\settings.ini",
    );
    let written = line(
        &names(&[]),
        "could not read",
        Some(json!({ "error": error.to_string() })),
    );
    assert_absent(&written, "some.one+tag@example.co.uk");
    assert_absent(&written, "some.one");
}

#[test]
fn a_launcher_account_id_registered_with_the_names_is_masked() {
    let shapes = &*SHAPES;
    let names = names(&[shapes.epic_id.as_str(), shapes.steam_id.as_str()]);
    let written = redact_string(
        &names,
        &format!(
            "linked {} and {}",
            shapes.epic_id.to_uppercase(),
            shapes.steam_id
        ),
    );
    assert_absent(&written, &shapes.epic_id);
    assert_absent(&written, &shapes.steam_id);
}

#[test]
fn the_end_of_a_launcher_id_that_a_message_names_is_masked_too() {
    let written = redact_string(
        &names(&[]),
        "Epic Games Launcher signed in as Main (account \u{2026}a3c9e1), which is another account",
    );
    assert_absent(&written, "a3c9e1");
    assert!(written.contains("(account \u{2026}[REDACTED])"));
    assert_eq!(
        redact_string(&names(&[]), "Checking\u{2026} done"),
        "Checking\u{2026} done"
    );
}
