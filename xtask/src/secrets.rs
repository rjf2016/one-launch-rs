use std::collections::BTreeSet;

use anyhow::Result;
use regex::{Captures, Regex};

use crate::workspace;

const ALLOW: &str = "xtask/secrets.allow";
const SAFE_USER_FOLDERS: [&str; 4] = ["someone", "placeholder", "public", "default"];

pub fn check(files: &[String]) -> Result<Vec<String>> {
    let scanner = Scanner::new(allowed(&workspace::read(ALLOW)?));
    let mut problems = Vec::new();
    for file in files {
        if skipped(file) {
            continue;
        }
        for finding in scanner.scan(file) {
            problems.push(format!("{file}: in the file name: {finding}"));
        }
        let path = workspace::root().join(file);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) if !path.exists() => continue,
            Err(error) => {
                problems.push(format!("{file}: could not be read ({error})"));
                continue;
            }
        };
        if bytes.iter().take(8000).any(|byte| *byte == 0) {
            continue;
        }
        for (index, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
            for finding in scanner.scan(line) {
                problems.push(format!("{file}:{}: {finding}", index + 1));
            }
        }
    }
    Ok(problems)
}

// oracle/ is sealed by its manifest and was scanned when it was captured; Cargo.lock is all checksums.
fn skipped(file: &str) -> bool {
    file.starts_with("oracle/") || file == "Cargo.lock"
}

fn allowed(list: &str) -> BTreeSet<String> {
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

type Value = fn(&Scanner, &Captures, &str) -> Option<String>;

struct Rule {
    id: &'static str,
    pattern: Regex,
    value: Value,
}

fn rule(id: &'static str, pattern: &str, value: Value) -> Rule {
    Rule {
        id,
        pattern: Regex::new(pattern).expect("a fixed pattern"),
        value,
    }
}

struct Scanner {
    rules: Vec<Rule>,
    placeholder: Regex,
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
                rule(
                    "github-token",
                    r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})",
                    Scanner::unless_synthetic::<0>,
                ),
                rule(
                    "jwt",
                    r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}(?:\.[A-Za-z0-9_-]*)?",
                    Scanner::unless_synthetic::<0>,
                ),
                rule(
                    "authorization",
                    r#"(?i)\bauthorization["']?\s*[:=]\s*["']?(?:bearer|basic)\s+([A-Za-z0-9._~+/=-]{20,})"#,
                    Scanner::unless_synthetic::<1>,
                ),
                rule(
                    "credential",
                    r#"(?i)\b(?:refresh_token|access_token|id_token|ssid|clid|web_token|client_secret|password)["']?\s*[:=]\s*["'`]([^"'`\s]{20,})["'`]"#,
                    Scanner::unless_synthetic::<1>,
                ),
                rule(
                    "email",
                    r"[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,})",
                    |_, found, line| {
                        let after = line[found.get(0)?.end()..].chars().next();
                        let cut_short =
                            after.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
                        (!cut_short && &found[0] != "git@github.com" && !safe_domain(&found[1]))
                            .then(|| found[0].to_owned())
                    },
                ),
                rule(
                    "steamid64",
                    r"\b7656119([0-9]{10})\b",
                    |scanner, found, _| {
                        (!scanner.synthetic(&found[1])).then(|| found[0].to_owned())
                    },
                ),
                rule(
                    "uuid",
                    r"\b[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\b",
                    Scanner::unless_synthetic::<0>,
                ),
                rule(
                    "hex64",
                    r"\b[0-9A-Fa-f]{64}\b",
                    Scanner::unless_synthetic::<0>,
                ),
                rule(
                    "hex32",
                    r"\b[0-9A-Fa-f]{32}\b",
                    Scanner::unless_synthetic::<0>,
                ),
                rule(
                    "user-folder",
                    r#"(?i)(?:\b[a-z]:|(?:^|[^\w.])/[a-z])[\\/]{1,2}users[\\/]{1,2}([^\\/\s"'`*?|:;,)\]]+)"#,
                    |_, found, _| {
                        let name = &found[1];
                        let named = name.starts_with(['<', '%', '$', '{', '['])
                            || SAFE_USER_FOLDERS.contains(&name.to_ascii_lowercase().as_str());
                        (!named).then(|| name.to_owned())
                    },
                ),
            ],
            placeholder: Regex::new(
                r"(?i)placeholder|fake|dummy|example|sample|(?:^|[^a-z])test(?:[^a-z]|$)",
            )
            .expect("a fixed pattern"),
        }
    }

    fn scan(&self, line: &str) -> Vec<String> {
        let mut findings = Vec::new();
        let mut taken: Vec<(usize, usize)> = Vec::new();
        for rule in &self.rules {
            for found in rule.pattern.captures_iter(line) {
                let whole = found.get(0).expect("group 0 always matches");
                let (start, end) = (whole.start(), whole.end());
                if taken.iter().any(|(from, to)| start < *to && end > *from) {
                    continue;
                }
                if let Some(value) = (rule.value)(self, &found, line) {
                    taken.push((start, end));
                    if !self.allow.contains(&value) {
                        findings.push(format!("{} {}", rule.id, mask(&value)));
                    }
                }
            }
        }
        findings
    }

    fn unless_synthetic<const GROUP: usize>(&self, found: &Captures, _: &str) -> Option<String> {
        let value = &found[GROUP];
        (!self.synthetic(value)).then(|| value.to_owned())
    }

    // A word beside the value excuses nothing: only the value itself can show that it is made up.
    fn synthetic(&self, value: &str) -> bool {
        let chars: Vec<char> = value.to_ascii_lowercase().chars().collect();
        let one_char_run = chars.windows(6).any(|run| run.iter().all(|c| *c == run[0]));
        let periodic = (1..=16.min(chars.len() / 2))
            .any(|period| chars[period..] == chars[..chars.len() - period]);
        let few_chars = chars
            .iter()
            .filter(|c| **c != '-')
            .collect::<BTreeSet<_>>()
            .len()
            <= 3;
        one_char_run || periodic || few_chars || self.placeholder.is_match(value)
    }
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
            .scan(line)
            .into_iter()
            .map(|finding| finding.split(' ').next().unwrap().to_owned())
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
    }

    #[test]
    fn values_that_show_they_are_made_up_pass() {
        for line in [
            "11111111-1111-4111-8111-111111111111".to_owned(),
            "0123456789abcdef0123456789abcdef".to_owned(),
            "7656119".to_owned() + "0000000000",
            format!("gh{}_{}", "p", "x".repeat(36)),
            format!("ey{}PLACEHOLDER.ey{}PLACEHOLDER.PLACEHOLDER", "J", "J"),
            r#"let password = "fake-password-for-the-vault";"#.to_owned(),
            format!("Authorization: Bearer {}", "example-token-".repeat(2)),
        ] {
            assert_eq!(rules(&line), Vec::<String>::new(), "{line}");
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
    fn a_user_profile_path_is_found() {
        let windows = format!(r"C:\{}\{}\AppData\Roaming", "Users", "jdoe");
        assert_eq!(rules(&windows), ["user-folder"]);
        let escaped = format!(r"C:\\{}\\{}\\file.txt", "Users", "jdoe");
        assert_eq!(rules(&escaped), ["user-folder"]);
        let posix = format!("cd /c/{}/{}/project", "Users", "jdoe");
        assert_eq!(rules(&posix), ["user-folder"]);
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
        let finding = &Scanner::new(BTreeSet::new()).scan(&token)[0];
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
