use std::sync::LazyLock;

use anyhow::{Result, bail};
use regex::Regex;

use super::git::{Change, Entry};
use super::paths::shown;

pub const FOLDER: &str = "plan/tickets/";
pub const TEMPLATE: &str = "plan/tickets/TEMPLATE.md";
pub const MOST_FILES: usize = 15;
const INTERFACE: &str = "interface";
const LISTED: usize = 20;

static BRANCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z0-9]+)/([A-Za-z0-9]+)-.+$").expect("a fixed pattern"));
static ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9]+-[A-Za-z0-9]+$").expect("a fixed pattern"));
static COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#.*$").expect("a fixed pattern"));

#[derive(Debug, PartialEq)]
pub struct Ticket {
    pub id: String,
    pub kind: String,
    pub scope: Vec<String>,
}

pub fn named(branch: &str) -> Option<String> {
    let found = BRANCH.captures(branch)?;
    Some(format!("{}-{}", &found[1], &found[2]).to_ascii_lowercase())
}

pub fn is_id(text: &str) -> bool {
    ID.is_match(text)
}

pub fn id_of(path: &str) -> Option<&str> {
    let name = path.strip_prefix(FOLDER)?.strip_suffix(".md")?;
    (path != TEMPLATE && !name.contains('/')).then_some(name)
}

pub fn on_base<'a>(entries: &'a [Entry], id: &str) -> Vec<&'a Entry> {
    entries
        .iter()
        .filter(|entry| id_of(&entry.path).is_some_and(|name| name.eq_ignore_ascii_case(id)))
        .collect()
}

pub fn parse(id: &str, text: &str) -> Result<Ticket> {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        bail!("it does not open with a `---` line");
    }
    let (mut kind, mut scope) = (None::<String>, None::<Vec<String>>);
    let (mut listing, mut closed) = (false, false);
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            closed = true;
            break;
        }
        let indented = line.starts_with([' ', '\t']);
        let text = line.trim_start();
        if listing {
            if text.is_empty() || text.starts_with('#') {
                continue;
            }
            if let Some(entry) = text.strip_prefix("- ") {
                scope.get_or_insert_default().push(usable(&plain(entry))?);
                continue;
            }
            if indented {
                bail!("`write_scope` holds a line that is not `- <path>`");
            }
            listing = false;
        }
        if indented {
            continue;
        }
        if let Some(value) = line.strip_prefix("kind:") {
            if kind.replace(plain(value)).is_some() {
                bail!("it has two `kind` lines");
            }
        } else if let Some(value) = line.strip_prefix("write_scope:") {
            if scope.replace(Vec::new()).is_some() {
                bail!("it has two `write_scope` lines");
            }
            match plain(value).as_str() {
                "" => listing = true,
                "[]" => {}
                _ => bail!("`write_scope` is not a list of `- <path>` lines"),
            }
        }
    }
    if !closed {
        bail!("its front matter has no closing `---` line");
    }
    match (kind.filter(|kind| !kind.is_empty()), scope) {
        (Some(kind), Some(scope)) => Ok(Ticket {
            id: id.to_owned(),
            kind,
            scope,
        }),
        (None, _) => bail!("it has no `kind`"),
        (_, None) => bail!("it has no `write_scope`"),
    }
}

fn plain(value: &str) -> String {
    let value = COMMENT.replace(value, "");
    let value = value.trim();
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

fn usable(entry: &str) -> Result<String> {
    let from_the_root = !entry.contains('\\')
        && entry
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if !from_the_root {
        bail!(
            "a `write_scope` entry is not a path from the repo's root with `/` between its parts"
        );
    }
    Ok(entry.to_owned())
}

// The scope is a list of what may change, so a path matches only as git spells it: another case or a backslash is another path.
pub fn matches(pattern: &str, path: &str) -> bool {
    if path.contains('\\') {
        return false;
    }
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    parts(&pattern, &path)
}

fn parts(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", [])) => !path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skipped| parts(rest, &path[skipped..])),
        Some((first, rest)) => path
            .split_first()
            .is_some_and(|(name, deeper)| part(first, name) && parts(rest, deeper)),
    }
}

fn part(pattern: &str, name: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let pieces: Vec<&str> = pieces.collect();
    let Some((last, between)) = pieces.split_last() else {
        return rest.is_empty();
    };
    for piece in between {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

pub fn scope(ticket: &Ticket, changes: &[Change]) -> Vec<String> {
    let id = &ticket.id;
    let outside: Vec<&Change> = changes
        .iter()
        .filter(|change| {
            !ticket
                .scope
                .iter()
                .any(|entry| matches(entry, &change.path))
        })
        .collect();
    let mut problems: Vec<String> = outside
        .iter()
        .take(LISTED)
        .map(|change| format!("outside the write_scope of {id}: {}", shown(&change.path)))
        .collect();
    if outside.len() > LISTED {
        problems.push(format!(
            "outside the write_scope of {id}: {} more paths",
            outside.len() - LISTED
        ));
    }
    if ticket.kind != INTERFACE && changes.len() > MOST_FILES {
        problems.push(format!(
            "{} files are changed, {MOST_FILES} at most unless the ticket is `kind: interface` (a rename counts as two)",
            changes.len()
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace;

    fn entry(path: &str) -> Entry {
        Entry {
            path: path.to_owned(),
            object: String::new(),
            file: true,
        }
    }

    fn change(path: &str) -> Change {
        Change {
            path: path.to_owned(),
            added: false,
        }
    }

    #[test]
    fn a_branch_names_its_ticket_whatever_the_case() {
        assert_eq!(named("m1/f1-workspace").as_deref(), Some("m1-f1"));
        assert_eq!(named("m1/f6c-policy").as_deref(), Some("m1-f6c"));
        assert_eq!(named("M1/S10-fonts").as_deref(), Some("m1-s10"));
        assert_eq!(named("m0/t01-a-b-c").as_deref(), Some("m0-t01"));
    }

    #[test]
    fn a_branch_of_another_shape_names_none() {
        for branch in [
            "main",
            "m1/f1",
            "m1/f1-",
            "m1-f1-workspace",
            "user/m1/f1-workspace",
            "m1/f1_workspace",
            "/f1-x",
            "m1/-x",
            "m1/f1-x\nm1/f2-y",
            "",
        ] {
            assert_eq!(named(branch), None, "{branch}");
        }
    }

    #[test]
    fn a_ticket_file_is_matched_without_regard_to_case() {
        let entries = [
            entry("plan/tickets/M1-F6c.md"),
            entry("plan/tickets/M1-F6.md"),
            entry("plan/tickets/TEMPLATE.md"),
            entry("plan/tickets/old/M1-F6c.md"),
            entry("plan/tickets/M1-F6c.txt"),
        ];
        let found = on_base(&entries, "m1-f6c");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "plan/tickets/M1-F6c.md");
        assert_eq!(on_base(&entries, "m1-f7").len(), 0);
        assert_eq!(on_base(&entries, "template").len(), 0);
    }

    #[test]
    fn two_files_that_differ_only_in_case_are_both_found() {
        let entries = [
            entry("plan/tickets/M1-F1.md"),
            entry("plan/tickets/m1-f1.md"),
        ];
        assert_eq!(on_base(&entries, "m1-f1").len(), 2);
    }

    const FRONT: &str = "---\nid: M3-012\ntitle: Epic install detection\nkind: feature        # feature | interface\nrisk: plain\nwrite_scope:\n  - crates/ol-adapters/src/epic/detect.rs\n  - .github/workflows/ci.yml          # committed by the agent\n\n  # a note\n  - \"crates/ol-adapters/tests/**\"\nread:\n  spec: [\"x\"]\n  kind: nested\n---\n\n## Outcome\n\nkind: interface\nwrite_scope:\n  - spec/**\n";

    #[test]
    fn the_front_matter_gives_the_kind_and_the_scope() {
        assert_eq!(
            parse("M3-012", FRONT).unwrap(),
            Ticket {
                id: "M3-012".to_owned(),
                kind: "feature".to_owned(),
                scope: vec![
                    "crates/ol-adapters/src/epic/detect.rs".to_owned(),
                    ".github/workflows/ci.yml".to_owned(),
                    "crates/ol-adapters/tests/**".to_owned(),
                ],
            }
        );
    }

    #[test]
    fn a_list_at_the_margin_and_an_empty_list_are_read() {
        let at_the_margin =
            "---\nkind: deps\nwrite_scope:\n- Cargo.lock\n- Cargo.toml\nread: {}\n---\n";
        assert_eq!(
            parse("M1-D1", at_the_margin).unwrap().scope,
            ["Cargo.lock", "Cargo.toml"]
        );
        let empty = "---\nkind: spike\nwrite_scope: []\n---\n";
        assert_eq!(parse("M1-S1", empty).unwrap().scope, Vec::<String>::new());
    }

    #[test]
    fn front_matter_this_check_cannot_read_is_an_error() {
        for (text, why) in [
            ("id: M1-F1\nkind: feature\n", "does not open"),
            ("---\nkind: feature\nwrite_scope: []\n", "no closing"),
            ("---\nwrite_scope: []\n---\n", "no `kind`"),
            ("---\nkind:\nwrite_scope: []\n---\n", "no `kind`"),
            ("---\nkind: feature\n---\n", "no `write_scope`"),
            (
                "---\nkind: feature\nkind: interface\nwrite_scope: []\n---\n",
                "two `kind`",
            ),
            (
                "---\nkind: feature\nwrite_scope: []\nwrite_scope:\n  - spec/**\n---\n",
                "two `write_scope`",
            ),
            (
                "---\nkind: feature\nwrite_scope: [crates/**]\n---\n",
                "not a list",
            ),
            (
                "---\nkind: feature\nwrite_scope:\n  crates/**\n---\n",
                "not `- <path>`",
            ),
            (
                "---\nkind: feature\nwrite_scope:\n  - ../other/**\n---\n",
                "not a path from the repo's root",
            ),
            (
                "---\nkind: feature\nwrite_scope:\n  - /crates/**\n---\n",
                "not a path from the repo's root",
            ),
            (
                "---\nkind: feature\nwrite_scope:\n  - crates\\\\**\n---\n",
                "not a path from the repo's root",
            ),
            (
                "---\nkind: feature\nwrite_scope:\n  - crates/\n---\n",
                "not a path from the repo's root",
            ),
        ] {
            let error = parse("M1-F1", text).unwrap_err().to_string();
            assert!(error.contains(why), "{text}: {error}");
        }
    }

    #[test]
    fn every_ticket_in_this_repo_is_read() {
        let tickets: Vec<String> = workspace::files()
            .unwrap()
            .into_iter()
            .filter(|path| id_of(path).is_some())
            .collect();
        assert!(tickets.len() >= 34, "{}", tickets.len());
        for path in &tickets {
            let ticket = parse(id_of(path).unwrap(), &workspace::read(path).unwrap());
            let ticket = ticket.unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(!ticket.scope.is_empty(), "{path}");
        }
        let own = parse(
            "M1-F6c",
            &workspace::read("plan/tickets/M1-F6c.md").unwrap(),
        )
        .unwrap();
        assert_eq!(own.kind, "feature");
        assert_eq!(
            own.scope,
            [
                "xtask/src/policy/**",
                "xtask/src/main.rs",
                "xtask/tests/policy/**",
                ".github/workflows/ci.yml",
            ]
        );
    }

    #[test]
    fn two_stars_match_folders_and_one_star_stays_in_a_name() {
        for (pattern, path) in [
            ("xtask/src/main.rs", "xtask/src/main.rs"),
            ("xtask/src/policy/**", "xtask/src/policy/mod.rs"),
            ("xtask/src/policy/**", "xtask/src/policy/a/b/c.rs"),
            ("crates/**/tests/*.rs", "crates/tests/a.rs"),
            ("crates/**/tests/*.rs", "crates/ol-model/tests/a.rs"),
            ("crates/**/tests/*.rs", "crates/ol-model/deep/tests/a.rs"),
            ("**/Cargo.toml", "Cargo.toml"),
            ("**/Cargo.toml", "crates/ol-model/Cargo.toml"),
            ("**", "anything/at/all.rs"),
            ("spikes/s*/src/*", "spikes/s05/src/main.rs"),
            ("docs/a*b*c.md", "docs/a-b-b-c.md"),
            ("xtask/**", "xtask/.cargo/config.toml"),
            ("docs/**", "docs/a b \"c\"\n.md"),
        ] {
            assert!(matches(pattern, path), "{pattern} {path}");
        }
        for (pattern, path) in [
            ("xtask/src/main.rs", "xtask/src/main.rs.bak"),
            ("xtask/src/main.rs", "xtask/src/Main.rs"),
            ("xtask/src/policy/**", "xtask/src/policy"),
            ("xtask/src/policy/**", "xtask/src/policy.rs"),
            ("xtask/src/policy/**", "xtask/src/policies/mod.rs"),
            ("xtask/src/policy/**", "Xtask/src/policy/mod.rs"),
            ("xtask/src/policy/**", "xtask\\src\\policy\\mod.rs"),
            ("xtask/src/policy/**", "xtask/src/policy/a\\b.rs"),
            ("**", "a\\b.rs"),
            ("crates/**/tests/*.rs", "crates/ol-model/tests/deep/a.rs"),
            ("crates/**/tests/*.rs", "crates/ol-model/src/a.rs"),
            ("crates/*", "crates/ol-model/src/lib.rs"),
            ("spikes/s*/src/*", "spikes/s05/src/deep/main.rs"),
            ("docs/a*a.md", "docs/a.md"),
            ("docs/*.md", "docs/a.md/b"),
        ] {
            assert!(!matches(pattern, path), "{pattern} {path}");
        }
    }

    fn of_kind(kind: &str) -> Ticket {
        Ticket {
            id: "M1-F1".to_owned(),
            kind: kind.to_owned(),
            scope: vec!["crates/**".to_owned()],
        }
    }

    #[test]
    fn fifteen_files_pass_and_sixteen_fail_unless_the_ticket_is_an_interface() {
        let files = |count: usize| -> Vec<Change> {
            (0..count)
                .map(|index| change(&format!("crates/a/src/f{index}.rs")))
                .collect()
        };
        assert_eq!(scope(&of_kind("feature"), &files(15)), Vec::<String>::new());
        assert_eq!(
            scope(&of_kind("feature"), &files(16)),
            [
                "16 files are changed, 15 at most unless the ticket is `kind: interface` (a rename counts as two)"
            ]
        );
        assert_eq!(scope(&of_kind("deps"), &files(16)).len(), 1);
        assert_eq!(
            scope(&of_kind("interface"), &files(200)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_long_list_of_paths_outside_the_scope_is_cut() {
        let outside: Vec<Change> = (0..30)
            .map(|index| change(&format!("docs/f{index}.md")))
            .collect();
        let problems = scope(&of_kind("interface"), &outside);
        assert_eq!(problems.len(), 21, "{problems:?}");
        assert_eq!(problems[0], "outside the write_scope of M1-F1: docs/f0.md");
        assert_eq!(
            problems[20],
            "outside the write_scope of M1-F1: 10 more paths"
        );
    }
}
