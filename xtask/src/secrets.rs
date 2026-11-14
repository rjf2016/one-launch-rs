use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use regex::{Captures, Regex};

use crate::workspace;

const ALLOW: &str = "xtask/secrets.allow";
const SAFE_USER_FOLDERS: [&str; 4] = ["someone", "placeholder", "public", "default"];

// Cargo.lock is all checksums.
const LOCK: &str = "Cargo.lock";
// The oracle's ids, sessions and profile paths are made up but shaped like real ones, and it was scanned when it was captured.
const SEALED: &str = "oracle/";
const NEVER_MADE_UP: [&str; 3] = ["private-key", "github-token", "authorization"];

pub fn check(files: &[String]) -> Result<Vec<String>> {
    let scanner = Scanner::new(allowed(&workspace::read(ALLOW)?));
    let mut problems = Vec::new();
    for file in files {
        if file == LOCK {
            continue;
        }
        let sealed = file.starts_with(SEALED);
        let in_name = scanner.scan(file, sealed);
        let shown = redacted(file, &in_name);
        for finding in &in_name {
            problems.push(format!("{shown}: in the file name: {finding}"));
        }
        let path = workspace::root().join(file);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) if !path.exists() => continue,
            Err(error) => {
                problems.push(format!("{shown}: could not be read ({error})"));
                continue;
            }
        };
        for (index, line) in text(bytes).lines().enumerate() {
            for finding in scanner.scan(line, sealed) {
                problems.push(format!("{shown}:{}: {finding}", index + 1));
            }
        }
    }
    Ok(problems)
}

fn redacted(text: &str, findings: &[Finding]) -> String {
    let mut spans: Vec<&Finding> = findings.iter().collect();
    spans.sort_by_key(|finding| finding.span.0);
    let mut shown = String::new();
    let mut from = 0;
    for finding in spans {
        shown.push_str(&text[from..finding.span.0]);
        shown.push_str(&finding.masked);
        from = finding.span.1;
    }
    shown.push_str(&text[from..]);
    shown
}

// Without its NUL bytes UTF-16 text reads as ASCII, and so do the strings inside a binary file.
fn text(bytes: Vec<u8>) -> String {
    let bytes: Vec<u8> = bytes.into_iter().filter(|byte| *byte != 0).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn allowed(list: &str) -> BTreeSet<String> {
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

struct Finding {
    rule: &'static str,
    masked: String,
    span: (usize, usize),
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.rule, self.masked)
    }
}

type Value = fn(&Scanner, &Captures, &str) -> Option<String>;

struct Rule {
    id: &'static str,
    pattern: Regex,
    value: Value,
    whole_run: bool,
}

fn rule(id: &'static str, pattern: &str, value: Value) -> Rule {
    Rule {
        id,
        pattern: Regex::new(pattern).expect("a fixed pattern"),
        value,
        whole_run: false,
    }
}

// A match counts only when no letter or digit touches it, so part of a longer run is not taken for an id.
fn whole_run(id: &'static str, pattern: &str, value: Value) -> Rule {
    Rule {
        whole_run: true,
        ..rule(id, pattern, value)
    }
}

struct Scanner {
    rules: Vec<Rule>,
    allow: BTreeSet<String>,
}

impl Scanner {
    fn new(allow: BTreeSet<String>) -> Self {
        Self {
            allow,
            rules: vec![
                rule(
                    "private-key",
                    r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----",
                    |_, found, _| Some(found[0].to_owned()),
                ),
                whole_run(
                    "github-token",
                    r"(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})",
                    unless_no_generator_made_it::<0>,
                ),
                whole_run(
                    "jwt",
                    r"(?:eyJ|eyAi)[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}(?:\.[A-Za-z0-9_-]*)?",
                    unless_no_generator_made_it::<0>,
                ),
                rule(
                    "authorization",
                    r#"(?i)\bauthorization["']?\s*[:=]\s*["']?(?:bearer|basic)\s+([A-Za-z0-9._~+/=-]{20,})"#,
                    unless_one_character::<1>,
                ),
                rule(
                    "credential",
                    r#"(?i)\b(?:refresh_token|access_token|id_token|ssid|clid|web_token|client_secret|password)["']?\s*[:=]\s*["'`]([^"'`\s]{20,})["'`]"#,
                    unless_one_character::<1>,
                ),
                rule(
                    "email",
                    r"[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,})",
                    |_, found, line| {
                        let after = line[found.get(0)?.end()..].chars().next();
                        let cut_short = after.is_some_and(char::is_alphanumeric);
                        (!cut_short && &found[0] != "git@github.com" && !safe_domain(&found[1]))
                            .then(|| found[0].to_owned())
                    },
                ),
                whole_run("steamid64", r"76561(?:19|20)([0-9]{10})", |_, found, _| {
                    (!no_generator_made(&found[1], 2)).then(|| found[0].to_owned())
                }),
                whole_run(
                    "uuid",
                    r"[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}",
                    unless_no_generator_made_it::<0>,
                ),
                whole_run(
                    "hex64",
                    r"[0-9A-Fa-f]{64}",
                    unless_no_generator_made_it::<0>,
                ),
                whole_run(
                    "hex32",
                    r"[0-9A-Fa-f]{32}",
                    unless_no_generator_made_it::<0>,
                ),
                rule(
                    "user-folder",
                    r#"(?i)(?:\b[a-z]:|(?:^|[^\w.])(?:/mnt)?/[a-z])[\\/]{1,4}users[\\/]{1,4}([^\\/\s"'`*?|:;,)\]]+)"#,
                    |_, found, _| {
                        let name = &found[1];
                        let named = name.starts_with(['<', '%', '$', '{', '['])
                            || SAFE_USER_FOLDERS.contains(&name.to_ascii_lowercase().as_str());
                        (!named).then(|| name.to_owned())
                    },
                ),
            ],
        }
    }

    fn scan(&self, line: &str, sealed: bool) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut taken: Vec<(usize, usize)> = Vec::new();
        let rules = self
            .rules
            .iter()
            .filter(|rule| !sealed || NEVER_MADE_UP.contains(&rule.id));
        for rule in rules {
            for found in rule.pattern.captures_iter(line) {
                let whole = found.get(0).expect("group 0 always matches");
                let (start, end) = (whole.start(), whole.end());
                if taken.iter().any(|(from, to)| start < *to && end > *from) {
                    continue;
                }
                let neighbours = [
                    line[..start].chars().next_back(),
                    line[end..].chars().next(),
                ];
                if rule.whole_run && neighbours.iter().flatten().any(char::is_ascii_alphanumeric) {
                    continue;
                }
                if let Some(value) = (rule.value)(self, &found, line) {
                    taken.push((start, end));
                    if !self.allow.contains(&value) {
                        findings.push(Finding {
                            rule: rule.id,
                            masked: mask(&value),
                            span: (start, end),
                        });
                    }
                }
            }
        }
        findings
    }
}

fn unless_no_generator_made_it<const GROUP: usize>(
    _: &Scanner,
    found: &Captures,
    _: &str,
) -> Option<String> {
    let value = &found[GROUP];
    (!no_generator_made(value, 4)).then(|| value.to_owned())
}

// A secret a person chose can be anything, so no pattern or word in it shows that it is made up: only one character from end to end does.
fn unless_one_character<const GROUP: usize>(
    _: &Scanner,
    found: &Captures,
    _: &str,
) -> Option<String> {
    let value = &found[GROUP];
    let mut chars = value.chars();
    let one_character = chars.next().is_some_and(|first| chars.all(|c| c == first));
    (!one_character).then(|| value.to_owned())
}

// An id or a token comes out of a generator, which all but never gives one character throughout but for a few, or a short unit repeated.
fn no_generator_made(value: &str, odd: usize) -> bool {
    let chars: Vec<char> = value
        .to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let mut counts: BTreeMap<char, usize> = BTreeMap::new();
    for c in &chars {
        *counts.entry(*c).or_default() += 1;
    }
    let most = counts.values().max().copied().unwrap_or_default();
    let nearly_one_character = chars.len() - most <= odd;
    let a_unit_repeated = (1..=16.min(chars.len() / 2))
        .any(|period| chars[period..] == chars[..chars.len() - period]);
    nearly_one_character || a_unit_repeated
}

fn safe_domain(domain: &str) -> bool {
    let domain = domain.to_ascii_lowercase();
    let labels: Vec<&str> = domain.split('.').collect();
    let reserved = labels.len() >= 2
        && matches!(
            labels[labels.len() - 1],
            "example" | "invalid" | "test" | "localhost"
        );
    let example = labels
        .iter()
        .position(|label| *label == "example")
        .is_some_and(|at| at + 1 < labels.len());
    reserved || example || domain.ends_with("users.noreply.github.com")
}

fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() >= 12 {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 2..].iter().collect();
        format!("{head}…{tail}")
    } else {
        format!("{}…({} chars)", chars[0], chars.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(line: &str) -> Vec<String> {
        Scanner::new(BTreeSet::new())
            .scan(line, false)
            .into_iter()
            .map(|finding| finding.rule.to_owned())
            .collect()
    }

    fn random(alphabet: &str, length: usize) -> String {
        let alphabet: Vec<char> = alphabet.chars().collect();
        let mut state = 0x2545_f491_u64;
        (0..length)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                alphabet[(state >> 33) as usize % alphabet.len()]
            })
            .collect()
    }

    const BASE62: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const HEX: &str = "0123456789abcdef";

    #[test]
    fn a_path_that_cannot_be_read_is_a_finding() {
        let problems = check(&["crates".to_owned(), "no-such-file".to_owned()]).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("crates: could not be read"));
    }

    #[test]
    fn a_private_key_is_found() {
        let line = format!("-----BEGIN {} KEY-----", "RSA PRIVATE");
        assert_eq!(rules(&line), ["private-key"]);
    }

    #[test]
    fn a_github_token_is_found() {
        assert_eq!(
            rules(&format!("gh{}_{}", "s", random(BASE62, 36))),
            ["github-token"]
        );
        assert_eq!(
            rules(&format!("token: github_{}_{}", "pat", random(BASE62, 40))),
            ["github-token"]
        );
    }

    #[test]
    fn a_jwt_is_found() {
        let line = format!(
            "ey{}{}.{}.{}",
            "J",
            random(BASE62, 20),
            random(BASE62, 30),
            random(BASE62, 20)
        );
        assert_eq!(rules(&line), ["jwt"]);
    }

    #[test]
    fn a_header_and_a_quoted_credential_are_found() {
        let header = format!("Authorization: Bearer {}", random(BASE62, 32));
        assert_eq!(rules(&header), ["authorization"]);
        let assignment = format!("let password = \"{}\";", random(BASE62, 24));
        assert_eq!(rules(&assignment), ["credential"]);
    }

    #[test]
    fn an_email_address_is_found() {
        let line = format!("contact {}@{}.io today", "someone", "corp-mail");
        assert_eq!(rules(&line), ["email"]);
    }

    #[test]
    fn reserved_and_noreply_addresses_pass() {
        for line in [
            "user@example.com",
            "user@mail.example.co.uk",
            "user@host.test",
            "user@box.invalid",
            "1234+name@users.noreply.github.com",
            "git@github.com:owner/repo.git",
            "cargo-deny@0.20.2",
            "actions/checkout@v5",
        ] {
            assert_eq!(rules(line), Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn account_ids_are_found() {
        assert_eq!(
            rules(&format!("id {}{}", "7656119", "8134509276")),
            ["steamid64"]
        );
        let hex = random(HEX, 32);
        assert_eq!(rules(&format!("account {hex}")), ["hex32"]);
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        );
        assert_eq!(rules(&format!("puuid {uuid}")), ["uuid"]);
        assert_eq!(rules(&format!("key {}", random(HEX, 64))), ["hex64"]);
    }

    #[test]
    fn a_commit_hash_is_not_an_account_id() {
        assert_eq!(rules(&random(HEX, 40)), Vec::<String>::new());
        assert_eq!(
            rules(&format!("0{}{}", "7656119", "8134509276")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_id_between_underscores_is_found() {
        let hex = random(HEX, 32);
        assert_eq!(rules(&format!("acct_{hex}_grid.png")), ["hex32"]);
        assert_eq!(
            rules(&format!("steam_{}{}", "7656120", "1134509276")),
            ["steamid64"]
        );
        let jwt = format!(
            "token_ey{}{}.{}",
            "Ai",
            random(BASE62, 20),
            random(BASE62, 30)
        );
        assert_eq!(rules(&jwt), ["jwt"]);
    }

    #[test]
    fn utf16_text_is_read() {
        let line = format!(r"C:\{}\{}\AppData", "Users", "jdoe");
        let utf16: Vec<u8> = line.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(rules(&text(utf16)), ["user-folder"]);
    }

    #[test]
    fn an_id_or_a_token_no_generator_made_passes() {
        for line in [
            "11111111-1111-4111-8111-111111111111".to_owned(),
            "0123456789abcdef".repeat(2),
            "7656119".to_owned() + "0000000000",
            "7656119".to_owned() + "8000000000",
            format!("gh{}_{}", "p", "x".repeat(36)),
            format!("ey{}{}.{}", "J", "x".repeat(20), "x".repeat(30)),
        ] {
            assert_eq!(rules(&line), Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn an_id_or_a_token_a_generator_could_have_made_is_found() {
        let one_character_a_group = [8, 4, 4, 4, 12]
            .iter()
            .zip(["1", "2", "3", "4", "5"])
            .map(|(length, digit)| digit.repeat(*length))
            .collect::<Vec<_>>()
            .join("-");
        assert_eq!(rules(&one_character_a_group), ["uuid"]);
        assert_eq!(
            rules(&format!("ey{0}{1}.ey{0}{1}.{1}", "J", "PLACEHOLDER")),
            ["jwt"]
        );
        assert_eq!(
            rules(&format!("id {}{}", "7656119", "8000000123")),
            ["steamid64"]
        );
    }

    #[test]
    fn a_secret_someone_chose_passes_only_as_one_character() {
        for line in [
            format!("let password = \"{}\";", "a".repeat(24)),
            format!("Authorization: Bearer {}", "x".repeat(24)),
        ] {
            assert_eq!(rules(&line), Vec::<String>::new(), "{line}");
        }
        let a_separator_after_each: String =
            random(BASE62, 16).chars().flat_map(|c| [c, '-']).collect();
        for value in [
            ["fake", "password", "for", "the", "vault"].join("-"),
            format!("example-{}", random(BASE62, 16)),
            format!("test_{}", random(BASE62, 20)),
            random(BASE62, 11) + &"a".repeat(11),
            random(BASE62, 16).repeat(2),
            random(BASE62, 5) + &"x".repeat(30),
            a_separator_after_each,
            "ab".repeat(12),
            "a".repeat(23) + "b",
            format!("{}aaaa", "!@#$%^&*()_+{}|:<>?~"),
        ] {
            let line = format!("let password = \"{value}\";");
            assert_eq!(rules(&line), ["credential"], "{value}");
        }
        for value in ["abcd".repeat(6), "example-token-".repeat(2)] {
            let line = format!("Authorization: Bearer {value}");
            assert_eq!(rules(&line), ["authorization"], "{value}");
        }
    }

    #[test]
    fn a_word_beside_a_value_does_not_excuse_it() {
        let hex = random(HEX, 32);
        assert_eq!(rules(&format!("placeholder account {hex}")), ["hex32"]);
        assert_eq!(
            rules(&format!("fake test id {}{}", "7656119", "8134509276")),
            ["steamid64"]
        );
        assert_eq!(
            rules(&format!(
                "example: Authorization: Bearer {}",
                random(BASE62, 32)
            )),
            ["authorization"]
        );
    }

    #[test]
    fn a_made_up_stretch_does_not_excuse_the_rest() {
        for value in [
            random(BASE62, 16) + &"a".repeat(6),
            "0".repeat(6) + &random(BASE62, 16),
            format!("{}example{}", random(BASE62, 10), random(BASE62, 10)),
            format!("{}-sample-{}", random(BASE62, 10), random(BASE62, 10)),
            format!("Example{}", random(BASE62, 20)),
            format!("{}-test", random(BASE62, 20)),
        ] {
            let line = format!("let password = \"{value}\";");
            assert_eq!(rules(&line), ["credential"], "{value}");
        }
        let hex = random(HEX, 26) + "000000";
        assert_eq!(rules(&format!("account {hex}")), ["hex32"]);
    }

    #[test]
    fn in_the_oracle_only_what_is_never_made_up_is_reported() {
        let scanner = Scanner::new(BTreeSet::new());
        let sealed = |line: &str| -> Vec<&'static str> {
            scanner
                .scan(line, true)
                .into_iter()
                .map(|finding| finding.rule)
                .collect()
        };
        let hex = random(HEX, 32);
        for line in [
            format!("account {hex}"),
            format!("id {}{}", "7656119", "8134509276"),
            format!(r"C:\{}\{}\AppData", "Users", "jdoe"),
            format!(
                "ssid: \"ey{}{}.{}\"",
                "J",
                random(BASE62, 20),
                random(BASE62, 30)
            ),
        ] {
            assert_eq!(sealed(&line), Vec::<&str>::new(), "{line}");
        }
        assert_eq!(
            sealed(&format!("gh{}_{}", "s", random(BASE62, 36))),
            ["github-token"]
        );
        assert_eq!(
            sealed(&format!("-----BEGIN {} KEY-----", "RSA PRIVATE")),
            ["private-key"]
        );
        assert_eq!(
            sealed(&format!("Authorization: Bearer {}", random(BASE62, 32))),
            ["authorization"]
        );
    }

    #[test]
    fn a_file_name_that_holds_a_finding_is_never_shown_whole() {
        let token = format!("gh{}_{}", "s", random(BASE62, 36));
        let hex = random(HEX, 32);
        let name = format!("notes/{token}/acct_{hex}_grid.png");
        let problems = check(std::slice::from_ref(&name)).unwrap();
        assert_eq!(problems.len(), 2, "{problems:?}");
        for problem in &problems {
            assert!(problem.starts_with("notes/ghs_…"), "{problem}");
            assert!(!problem.contains(&token) && !problem.contains(&hex));
        }
        let in_the_oracle = check(&[format!("oracle/{token}/acct_{hex}_grid.png")]).unwrap();
        assert_eq!(in_the_oracle.len(), 1, "{in_the_oracle:?}");
        assert!(in_the_oracle[0].contains(&hex) && !in_the_oracle[0].contains(&token));
    }

    #[test]
    fn a_user_profile_path_is_found() {
        let windows = format!(r"C:\{}\{}\AppData\Roaming", "Users", "jdoe");
        assert_eq!(rules(&windows), ["user-folder"]);
        let escaped = format!(r"C:\\{}\\{}\\file.txt", "Users", "jdoe");
        assert_eq!(rules(&escaped), ["user-folder"]);
        let posix = format!("cd /c/{}/{}/project", "Users", "jdoe");
        assert_eq!(rules(&posix), ["user-folder"]);
        let twice_escaped = format!(r"C:\\\\{}\\\\{}\\\\file.txt", "Users", "jdoe");
        assert_eq!(rules(&twice_escaped), ["user-folder"]);
        let wsl = format!("ls /mnt/c/{}/{}/project", "Users", "jdoe");
        assert_eq!(rules(&wsl), ["user-folder"]);
    }

    #[test]
    fn a_placeholder_profile_path_passes() {
        for line in [
            r"C:\Users\<name>\AppData",
            r"C:\Users\%USERNAME%\AppData",
            r"C:\Users\someone\AppData",
            r"C:\Users\Public\Desktop",
            r"C:\Users\$env:USERNAME\x",
        ] {
            assert_eq!(rules(line), Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn a_finding_never_shows_the_whole_value() {
        let token = format!("gh{}_{}", "s", random(BASE62, 36));
        let finding = Scanner::new(BTreeSet::new()).scan(&token, false)[0].to_string();
        assert!(!finding.contains(&token));
        assert!(finding.starts_with("github-token ghs_…"));
    }

    #[test]
    fn one_stretch_of_text_is_reported_once() {
        let line = format!(
            "ey{}.{}.{}",
            "J".to_owned() + &random(HEX, 32),
            random(HEX, 32),
            random(HEX, 32)
        );
        assert_eq!(rules(&line), ["jwt"]);
    }
}
