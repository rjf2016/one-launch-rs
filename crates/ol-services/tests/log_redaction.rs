#![forbid(unsafe_code)]

// The redaction against what a line could carry beyond the recorded cases. Each test names what must not reach the file.

use ol_model::LogLevel;
use ol_model::redact::AccountNames;
use ol_services::log::redact::{MAX_TEXT_LENGTH, format_line, redact_string};
use serde_json::{Value, json};

const TOKEN_HALF: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f6071";
const JWT: &str = "eyJaaaaaaaaaaaaaaaaaaaa.aaaaaaaaaaaaaaaaaaaa.aaaaaaaaaaaaaaaaaaaa";

// A SteamID64 and a profile folder are put together here, so the source holds neither whole.
fn steam_id(n: u32) -> String {
    format!("76561198{n:09}")
}

fn profile(separator: &str, user: &str) -> String {
    format!("C:{separator}Users{separator}{user}{separator}AppData")
}

fn names(given: &[&str]) -> AccountNames {
    let mut names = AccountNames::new();
    names.add(given.iter().copied());
    names
}

fn line(names: &AccountNames, message: &str, fields: Option<Value>) -> String {
    format_line(names, "t", LogLevel::Info, "main", message, fields.as_ref())
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
    let written = line(
        &names(&[]),
        "saved",
        Some(json!({ "first": TOKEN_HALF, "second": TOKEN_HALF.to_uppercase() })),
    );
    assert_absent(&written, TOKEN_HALF);
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
    let names = names(&["gordon_f", "Jo Smith"]);
    for path in [
        r"D:\Saves\gordon_f\slot1",
        r"D:\Saves\gordon_f.cfg",
        "d:/saves/GORDON_F/x",
        &profile("\\", "Jo Smith"),
        &profile("/", "someone"),
        &profile("\\\\", "someone"),
    ] {
        let written = redact_string(&names, path);
        assert_absent(&written, "gordon_f");
        assert_absent(&written, "jo smith");
        assert_absent(&written, "someone");
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
fn keys_and_patterns_in_upper_case_are_redacted() {
    let written = redact_string(
        &names(&[]),
        "PASSWORD=hunter22 TOKEN: tok1 SOME.ONE@EXAMPLE.COM AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA C:\\USERS\\SOMEONE\\X -LOGIN user1 hunter33",
    );
    for secret in [
        "hunter22",
        "tok1",
        "some.one@example.com",
        "aaaaaaaa-aaaa-4aaa-8aaa",
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
fn a_secret_across_the_cut_leaves_no_start_of_itself() {
    let names = names(&["gordon freeman"]);
    // Whatever a rule puts in place of the secret takes the message past the limit, so the cut falls inside the mask.
    let filler = format!("{} ", "x".repeat(MAX_TEXT_LENGTH - 5));
    for secret_part in [
        format!("token={TOKEN_HALF}"),
        JWT.to_owned(),
        "gordon freeman".to_owned(),
    ] {
        let written = line(&names, &format!("{filler}{secret_part}"), None);
        assert!(written.contains("more characters]"), "the message was cut");
        assert_absent(&written, &secret_part[..8]);
        assert_absent(&written, &TOKEN_HALF[..6]);
        assert_absent(&written, "gordon");
    }
    let fields = json!({ "stack": format!("{filler}{JWT}") });
    assert_absent(&line(&names, "m", Some(fields)), &JWT[..12]);
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
    let epic_id = "0a1b2c3d".repeat(4);
    let names = names(&[epic_id.as_str(), steam_id(9).as_str()]);
    let written = redact_string(
        &names,
        &format!("linked {} and {}", epic_id.to_uppercase(), steam_id(9)),
    );
    assert_absent(&written, &epic_id);
    assert_absent(&written, &steam_id(9));
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
