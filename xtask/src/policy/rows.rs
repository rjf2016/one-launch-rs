use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use super::git::{Change, Content};
use super::paths::{folded, printable};
use super::toml;

pub const TRIAGE: &str = "plan/triage.toml";
pub const WAIVERS: &str = "plan/waivers.toml";
const THE_OWNERS: [&str; 2] = ["\"\"", "\"plan\""];
const DELEGATED: &str = "\"delegated\"";
const ON_THE_BASE: &str = " on the base";
const NO_ID: &str =
    "the row's `id` is not one string in double quotes with no escape and nothing after it";

static CATEGORY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#   ([a-z][a-z0-9-]*) {2,}\S").expect("a fixed pattern"));

struct Row<'a> {
    line: usize,
    fields: Vec<(&'a str, &'a str)>,
}

impl<'a> Row<'a> {
    fn get(&self, key: &str) -> Option<&'a str> {
        self.fields
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }

    fn id(&self) -> Option<&'a str> {
        self.get("id").and_then(plain)
    }
}

// Rows are told apart by the text of a value, so the values that decide whose a row is must have one spelling only: a string in double quotes with no escape in it and nothing after it. Then two spellings of one id cannot pass as two ids.
fn plain(value: &str) -> Option<&str> {
    let text = value.strip_prefix('"')?.strip_suffix('"')?;
    (!text.contains(['"', '\\'])).then_some(text)
}

pub fn touched(changes: &[Change], file: &str) -> bool {
    changes.iter().any(|change| folded(&change.path) == file)
}

pub fn spellings(changes: &[Change]) -> Vec<String> {
    let mut problems = Vec::new();
    for file in [TRIAGE, WAIVERS] {
        if changes
            .iter()
            .any(|change| change.path != file && folded(&change.path) == file)
        {
            problems.push(format!(
                "{file}: a path that is this file under another spelling is changed"
            ));
        }
    }
    problems
}

pub fn triage(fork: &Content, head: &Content) -> Vec<String> {
    let mut problems = Vec::new();
    let before = rows(TRIAGE, ON_THE_BASE, fork, "item", &mut problems);
    let after = rows(TRIAGE, "", head, "item", &mut problems);
    let (Some(before), Some(after)) = (before, after) else {
        return problems;
    };

    let mut now: BTreeMap<&str, &Row> = BTreeMap::new();
    for row in &after {
        let at = format!("{TRIAGE}:{}", row.line);
        if !row
            .get("source")
            .is_some_and(|source| THE_OWNERS.contains(&source) || source == DELEGATED)
        {
            problems.push(format!(
                "{at}: the row's `source` is not \"\", \"plan\" or \"delegated\""
            ));
        }
        match row.id() {
            None => problems.push(format!("{at}: {NO_ID}")),
            Some(id) => {
                if let Some(first) = now.insert(id, row) {
                    problems.push(format!(
                        "{at}: a second row with the id of the row at line {}",
                        first.line
                    ));
                }
            }
        }
    }

    // On the base every row that is not plainly delegated is the owner's.
    let mut the_owners = BTreeSet::new();
    for row in &before {
        if row.get("source") == Some(DELEGATED) {
            continue;
        }
        let Some(id) = row.id() else {
            problems.push(format!("{TRIAGE}:{}{ON_THE_BASE}: {NO_ID}", row.line));
            continue;
        };
        the_owners.insert(id);
        match now.get(id) {
            None => problems.push(format!(
                "{TRIAGE}: the owner's row {} is removed",
                printable(id)
            )),
            Some(found) if found.fields != row.fields => problems.push(format!(
                "{TRIAGE}:{}: the owner's row {} is changed",
                found.line,
                printable(id)
            )),
            Some(_) => {}
        }
    }
    for row in &after {
        let the_owners_now = row
            .get("source")
            .is_some_and(|source| THE_OWNERS.contains(&source));
        if the_owners_now && !row.id().is_some_and(|id| the_owners.contains(id)) {
            problems.push(format!(
                "{TRIAGE}:{}: a row with `source` \"\" or \"plan\" is added, and those are the owner's",
                row.line
            ));
        }
    }
    problems
}

// The categories that count are the ones the tip of the base defines; what the pull request itself did to the file is its difference from the commit it was cut from.
pub fn waivers(fork: &Content, head: &Content, base: &Content) -> Vec<String> {
    let mut problems = Vec::new();
    let before = rows(WAIVERS, ON_THE_BASE, fork, "waiver", &mut problems);
    let after = rows(WAIVERS, "", head, "waiver", &mut problems);
    if above(fork) != above(head) {
        problems.push(format!(
            "{WAIVERS}: the comment above the first waiver, which defines the categories, is changed"
        ));
    }
    let (Some(before), Some(after)) = (before, after) else {
        return problems;
    };

    let defined: BTreeSet<&str> = above(base)
        .iter()
        .filter_map(|line| Some(CATEGORY.captures(line)?.get(1)?.as_str()))
        .collect();
    let mut was = BTreeMap::new();
    for row in &before {
        match row.id() {
            Some(id) => {
                was.insert(id, row.get("category"));
            }
            None => problems.push(format!("{WAIVERS}:{}{ON_THE_BASE}: {NO_ID}", row.line)),
        }
    }
    let mut seen = BTreeMap::new();
    for row in &after {
        let at = format!("{WAIVERS}:{}", row.line);
        let category = row.get("category");
        if !category
            .and_then(plain)
            .is_some_and(|name| defined.contains(name))
        {
            problems.push(format!(
                "{at}: the waiver's category is not one the base defines"
            ));
        }
        let Some(id) = row.id() else {
            problems.push(format!("{at}: {NO_ID}"));
            continue;
        };
        if let Some(first) = seen.insert(id, row.line) {
            problems.push(format!(
                "{at}: a second waiver with the id of the waiver at line {first}"
            ));
        }
        if was.get(id).is_some_and(|old| *old != category) {
            problems.push(format!(
                "{at}: the category of waiver {}, which is on the base, is changed",
                printable(id)
            ));
        }
    }
    problems
}

fn above(content: &Content) -> Vec<&str> {
    let Content::Text(text) = content else {
        return Vec::new();
    };
    let mut lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .take_while(|line| !line.trim_start().starts_with('['))
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn rows<'a>(
    file: &str,
    side: &str,
    content: &'a Content,
    table: &str,
    problems: &mut Vec<String>,
) -> Option<Vec<Row<'a>>> {
    let text = match content {
        Content::Missing => "",
        Content::Text(text) => text,
        Content::NotAFile => {
            problems.push(format!("{file}{side} is not a regular file"));
            return None;
        }
    };
    match read(text, table) {
        Ok(rows) => Some(rows),
        Err(line) => {
            problems.push(format!(
                "{file}:{line}{side}: not a comment, `[[{table}]]` or one `key = value` that ends on its line, which is all this check reads"
            ));
            None
        }
    }
}

fn read<'a>(text: &'a str, table: &str) -> Result<Vec<Row<'a>>, usize> {
    let mut rows: Vec<Row> = Vec::new();
    for line in toml::lines(text)? {
        match (line.entry, rows.last_mut()) {
            (None, _) if line.table == table && line.array => rows.push(Row {
                line: line.number,
                fields: Vec::new(),
            }),
            (Some((key, value)), Some(row)) if row.get(key).is_none() => {
                row.fields.push((key, value));
            }
            _ => return Err(line.number),
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Content {
        Content::Text(text.to_owned())
    }

    fn row(id: &str, source: &str, decision: &str) -> String {
        format!(
            "[[item]]\nid = \"{id}\"\nsummary = \"What {id} is about.\"\ndecision = \"{decision}\"\nsource = \"{source}\"\naffects = []\n\n"
        )
    }

    fn base() -> String {
        format!(
            "# The triage table.\n\n{}{}{}",
            row("S1-01", "", "keep"),
            row("S1-02", "plan", "drop"),
            row("X-01", "delegated", "fix")
        )
    }

    fn triaged(head: &str) -> Vec<String> {
        triage(&text(&base()), &text(head))
    }

    #[test]
    fn a_delegated_row_may_be_added_changed_and_removed() {
        for head in [
            base(),
            base() + &row("X-02", "delegated", "drop"),
            base().replace("decision = \"fix\"", "decision = \"drop: not needed\""),
            base().replace(&row("X-01", "delegated", "fix"), ""),
            format!(
                "# Another comment.\n{}# Between rows.\n{}{}",
                row("X-01", "delegated", "fix"),
                row("S1-02", "plan", "drop"),
                row("S1-01", "", "keep")
            ),
        ] {
            assert_eq!(triaged(&head), Vec::<String>::new(), "{head}");
        }
    }

    #[test]
    fn the_owners_row_may_not_change_or_go() {
        assert_eq!(
            triaged(&base().replace("decision = \"keep\"", "decision = \"drop\"")),
            ["plan/triage.toml:3: the owner's row S1-01 is changed"]
        );
        assert_eq!(
            triaged(&base().replace(&row("S1-02", "plan", "drop"), "")),
            ["plan/triage.toml: the owner's row S1-02 is removed"]
        );
        assert_eq!(
            triaged(&base().replace("source = \"plan\"", "source = \"delegated\"")),
            ["plan/triage.toml:10: the owner's row S1-02 is changed"]
        );
        assert_eq!(
            triaged(&base().replace(
                "affects = []\n\n[[item]]\nid = \"S1-02\"",
                "affects = [\"x\"]\n\n[[item]]\nid = \"S1-02\""
            )),
            ["plan/triage.toml:3: the owner's row S1-01 is changed"]
        );
        assert_eq!(
            triaged(&base().replace("id = \"S1-01\"", "id = \"S1-99\"")),
            [
                "plan/triage.toml: the owner's row S1-01 is removed",
                "plan/triage.toml:3: a row with `source` \"\" or \"plan\" is added, and those are the owner's"
            ]
        );
        assert_eq!(triage(&text(&base()), &Content::Missing).len(), 2);
    }

    #[test]
    fn a_row_in_the_owners_name_may_not_be_added() {
        let added = "plan/triage.toml:24: a row with `source` \"\" or \"plan\" is added, and those are the owner's";
        assert_eq!(triaged(&(base() + &row("S1-03", "plan", "drop"))), [added]);
        assert_eq!(triaged(&(base() + &row("S1-03", "", "keep"))), [added]);
        assert_eq!(
            triaged(&base().replace("source = \"delegated\"", "source = \"plan\"")),
            [added.replace(":24:", ":17:")]
        );
        assert_eq!(triage(&Content::Missing, &text(&base())).len(), 2);
    }

    #[test]
    fn a_row_this_check_cannot_place_fails() {
        for (head, problem) in [
            (
                base() + &row("X-02", "delegated", "drop").replace("\"delegated\"", "'plan'"),
                "plan/triage.toml:24: the row's `source` is not \"\", \"plan\" or \"delegated\"",
            ),
            (
                base() + &row("X-02", "delegated", "drop").replace("source = \"delegated\"\n", ""),
                "plan/triage.toml:24: the row's `source` is not \"\", \"plan\" or \"delegated\"",
            ),
            (
                base() + &row("X-02", "delegated", "drop").replace("id = \"X-02\"\n", ""),
                &format!("plan/triage.toml:24: {NO_ID}"),
            ),
            (
                base() + &row("S1-01", "delegated", "drop"),
                "plan/triage.toml:24: a second row with the id of the row at line 3",
            ),
        ] {
            let problems = triaged(&head);
            assert!(problems.contains(&problem.to_owned()), "{problems:?}");
        }
    }

    #[test]
    fn a_file_outside_the_one_spelling_fails_whole() {
        let unreadable = |line: usize| {
            format!(
                "plan/triage.toml:{line}: not a comment, `[[item]]` or one `key = value` that ends on its line, which is all this check reads"
            )
        };
        let hidden = row("X-02", "delegated", "drop").replace(
            "decision = \"drop\"",
            &format!("decision = \"\"\"\n{}\"\"\"", row("S1-02", "plan", "drop")),
        );
        let without = base().replace(&row("S1-02", "plan", "drop"), "");
        assert_eq!(triaged(&(without + &hidden)), [unreadable(20)]);
        for (extra, line) in [
            ("[item]\nid = \"X-09\"\n", 24),
            ("[[items]]\nid = \"X-09\"\n", 24),
            ("[[item]]\nid = \"X-09\"\nid = \"X-10\"\n", 26),
            ("[[item]]\naffects = [\n  \"x\",\n]\n", 25),
        ] {
            assert_eq!(triaged(&(base() + extra)), [unreadable(line)], "{extra}");
        }
        assert_eq!(
            triaged(&format!("id = \"S1-00\"\n{}", base())),
            [unreadable(1)]
        );
        assert_eq!(
            triage(&text("[[item]]\nwhy = \"\"\"\n"), &text(&base())),
            [
                "plan/triage.toml:2 on the base: not a comment, `[[item]]` or one `key = value` that ends on its line, which is all this check reads"
            ]
        );
        assert_eq!(
            triage(&text(&base()), &Content::NotAFile),
            ["plan/triage.toml is not a regular file"]
        );
    }

    const CATEGORIES: &str = "# Waivers.\n#\n# Categories the owner approved.\n#   web-only          behaviour that exists because the UI is a web page:\n#                     the preload bridge\n#   tooling           tests of scripts that do not ship\n#\n# [[waiver]]\n# id = \"T-0000\"\n# category = \"web-only\"         one of the categories above\n\n";

    fn waiver(id: &str, category: &str) -> String {
        format!(
            "[[waiver]]\nid = \"{id}\"\ncategory = \"{category}\"\nreason = \"Why {id} is not ported.\"\n\n"
        )
    }

    fn waived(head: &str) -> Vec<String> {
        let base = format!("{CATEGORIES}{}", waiver("T-0001", "tooling"));
        waivers(&text(&base), &text(head), &text(&base))
    }

    #[test]
    fn a_waiver_in_a_category_the_base_defines_may_be_added_and_removed() {
        for head in [
            format!("{CATEGORIES}{}", waiver("T-0001", "tooling")),
            format!(
                "{CATEGORIES}{}{}",
                waiver("T-0001", "tooling"),
                waiver("T-0002", "web-only")
            ),
            CATEGORIES.to_owned(),
            format!(
                "{CATEGORIES}{}\n# Later.\n{}",
                waiver("T-0003", "web-only"),
                waiver("T-0001", "tooling")
            )
            .replace("Why T-0001 is not ported.", "Another reason."),
        ] {
            assert_eq!(waived(&head), Vec::<String>::new(), "{head}");
        }
    }

    #[test]
    fn a_category_may_not_be_added_changed_or_removed() {
        let changed = "plan/waivers.toml: the comment above the first waiver, which defines the categories, is changed";
        let undefined = "plan/waivers.toml:12: the waiver's category is not one the base defines";
        let with = |categories: &str, category: &str| {
            waived(&format!("{categories}{}", waiver("T-0001", category)))
        };
        assert_eq!(
            with(
                &CATEGORIES.replace("#\n# [[waiver]]", "#   too-hard          tests nobody wants to port\n#\n# [[waiver]]"),
                "too-hard"
            ),
            [
                changed.to_owned(),
                undefined.replace(":12:", ":13:"),
                "plan/waivers.toml:13: the category of waiver T-0001, which is on the base, is changed".to_owned()
            ]
        );
        assert_eq!(
            with(
                &CATEGORIES.replace("that do not ship", "of any kind"),
                "tooling"
            ),
            [changed]
        );
        assert_eq!(
            with(
                &CATEGORIES.replace(
                    "#   tooling           tests of scripts that do not ship\n",
                    ""
                ),
                "tooling"
            ),
            [changed]
        );
        assert_eq!(
            with(CATEGORIES, "too-hard"),
            [
                undefined.to_owned(),
                "plan/waivers.toml:12: the category of waiver T-0001, which is on the base, is changed".to_owned()
            ]
        );
        assert_eq!(
            with(CATEGORIES, "web-only"),
            [
                "plan/waivers.toml:12: the category of waiver T-0001, which is on the base, is changed"
            ]
        );
        let base = format!("{CATEGORIES}{}", waiver("T-0001", "tooling"));
        assert_eq!(
            waivers(&text(&base), &Content::Missing, &text(&base)),
            [changed]
        );
        assert_eq!(
            waivers(&Content::Missing, &text(&base), &Content::Missing),
            [changed.to_owned(), undefined.to_owned()]
        );
    }

    #[test]
    fn a_waiver_this_check_cannot_place_fails() {
        let both = |extra: &str| {
            waived(&format!(
                "{CATEGORIES}{}{extra}",
                waiver("T-0001", "tooling")
            ))
        };
        assert_eq!(
            both(&waiver("T-0001", "web-only")),
            [
                "plan/waivers.toml:17: a second waiver with the id of the waiver at line 12",
                "plan/waivers.toml:17: the category of waiver T-0001, which is on the base, is changed"
            ]
        );
        assert_eq!(
            both("[[waiver]]\nid = \"T-0002\"\n"),
            ["plan/waivers.toml:17: the waiver's category is not one the base defines"]
        );
        assert_eq!(
            both("[[waiver]]\ncategory = \"tooling\"\n"),
            [format!("plan/waivers.toml:17: {NO_ID}")]
        );
        assert_eq!(
            both("[[waiver]]\nid = \"T-0002\"\ncategory = 'tooling'\n"),
            ["plan/waivers.toml:17: the waiver's category is not one the base defines"]
        );
        assert_eq!(
            both("[[waiver]]\nid = \"T-0002\"\ncategory = \"\"\"\ntooling\"\"\"\n").len(),
            1
        );
    }

    #[test]
    fn the_categories_of_this_repo_are_read() {
        let waivers = crate::workspace::read(WAIVERS).unwrap();
        let content = text(&waivers);
        let defined: Vec<&str> = above(&content)
            .iter()
            .filter_map(|line| Some(CATEGORY.captures(line)?.get(1)?.as_str()))
            .collect();
        assert!(!defined.is_empty());
        assert_eq!(
            super::waivers(&content, &content, &content),
            Vec::<String>::new()
        );
        let triage = text(&crate::workspace::read(TRIAGE).unwrap());
        assert_eq!(super::triage(&triage, &triage), Vec::<String>::new());
    }

    #[test]
    fn another_spelling_of_either_path_is_a_finding() {
        let changed = |path: &str| Change {
            path: path.to_owned(),
            added: true,
        };
        assert!(touched(&[changed("Plan/Triage.toml")], TRIAGE));
        assert!(!touched(&[changed("plan/triage.toml.bak")], TRIAGE));
        assert_eq!(
            spellings(&[changed("Plan/Triage.toml"), changed("plan/waivers.toml")]),
            ["plan/triage.toml: a path that is this file under another spelling is changed"]
        );
        assert_eq!(
            spellings(&[changed("plan\\waivers.toml")]),
            ["plan/waivers.toml: a path that is this file under another spelling is changed"]
        );
    }

    #[test]
    fn a_second_spelling_of_an_id_is_not_a_second_id() {
        for spelling in [
            "id = \"S1-01\" # superseded",
            "id = 'S1-01'",
            "id = \"S1-0\\u0031\"",
            "id = \"\"\"S1-01\"\"\"",
            "id = [\"S1-01\"]",
            "id = 101",
        ] {
            let twin = row("S1-01", "delegated", "drop").replace("id = \"S1-01\"", spelling);
            let problems = triaged(&(base() + &twin));
            assert_eq!(problems.len(), 1, "{spelling}: {problems:?}");
            assert!(problems[0].starts_with("plan/triage.toml:2"), "{spelling}");

            let moved = format!(
                "{CATEGORIES}{}",
                waiver("T-0001", "web-only")
                    .replace("id = \"T-0001\"", &spelling.replace("S1-01", "T-0001"))
            );
            let problems = waived(&moved);
            assert_eq!(problems.len(), 1, "{spelling}: {problems:?}");
            assert!(problems[0].starts_with("plan/waivers.toml:1"), "{spelling}");
            assert!(!problems[0].contains("category"), "{spelling}");
        }
    }

    #[test]
    fn on_the_base_a_row_that_is_not_plainly_delegated_is_the_owners() {
        for source in [
            "\"plan\" # settled",
            "'plan'",
            "\"Delegated\"",
            "\"\\u0070lan\"",
        ] {
            let before = base().replace("source = \"plan\"", &format!("source = {source}"));
            let after = before.replace("decision = \"drop\"", "decision = \"keep\"");
            let problems = triage(&text(&before), &text(&after));
            assert!(
                problems
                    .contains(&"plan/triage.toml:10: the owner's row S1-02 is changed".to_owned()),
                "{source}: {problems:?}"
            );
        }
        let no_source = base().replace("source = \"plan\"\n", "");
        assert_eq!(
            triage(
                &text(&no_source),
                &text(&base().replace(&row("S1-02", "plan", "drop"), ""))
            ),
            ["plan/triage.toml: the owner's row S1-02 is removed"]
        );
        let no_id = base().replace("id = \"S1-02\"", "id = 'S1-02'");
        assert_eq!(
            triage(&text(&no_id), &text(&base())),
            [
                format!("plan/triage.toml:10 on the base: {NO_ID}"),
                "plan/triage.toml:10: a row with `source` \"\" or \"plan\" is added, and those are the owner's".to_owned()
            ]
        );
    }

    #[test]
    fn the_categories_that_count_are_the_ones_on_the_tip_of_the_base() {
        let fork = format!("{CATEGORIES}{}", waiver("T-0001", "tooling"));
        let head = format!("{fork}{}", waiver("T-0002", "web-only"));
        let judged = |base: &str| waivers(&text(&fork), &text(&head), &text(base));
        assert_eq!(judged(&fork), Vec::<String>::new());
        let without_web_only = fork.replace(
            "#   web-only          behaviour that exists because the UI is a web page:\n#                     the preload bridge\n",
            "",
        );
        assert_eq!(
            judged(&without_web_only),
            ["plan/waivers.toml:17: the waiver's category is not one the base defines"]
        );
        let with_one_more = fork.replace(
            "#\n# [[waiver]]",
            "#   too-hard          not worth it\n#\n# [[waiver]]",
        );
        let head = format!("{fork}{}", waiver("T-0002", "too-hard"));
        assert_eq!(
            waivers(&text(&fork), &text(&head), &text(&with_one_more)),
            Vec::<String>::new()
        );
        assert_eq!(
            waivers(&text(&fork), &text(&head), &Content::Missing),
            [
                "plan/waivers.toml:12: the waiver's category is not one the base defines",
                "plan/waivers.toml:17: the waiver's category is not one the base defines"
            ]
        );
    }

    #[test]
    fn an_id_is_printed_without_what_would_start_a_line_or_reorder_it() {
        let odd = base().replace("S1-02", "S1\u{1b}[31m\u{202e}02");
        assert_eq!(
            triage(
                &text(&odd),
                &text(&odd.replace("decision = \"drop\"", "decision = \"keep\""))
            ),
            ["plan/triage.toml:10: the owner's row S1?[31m?02 is changed"]
        );
    }
}
