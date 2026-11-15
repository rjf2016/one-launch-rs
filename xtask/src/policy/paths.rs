use std::collections::BTreeMap;

use super::git::{Change, Entry};
use super::ticket;
use crate::secrets;

const IN_THE_NAME: &str = ": in the file name: ";
const NOT_SHOWN: &str = "(a path the secret scan would not show)";

// A rule that guards a path must see through every spelling a checkout on Windows or macOS would write to the same file: case, a backslash, a dot or a space at the end of a name.
pub fn folded(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .map(|part| part.trim_end_matches(['.', ' ']))
        .collect::<Vec<_>>()
        .join("/")
        .chars()
        .flat_map(char::to_uppercase)
        .flat_map(char::to_lowercase)
        .collect()
}

// Text a pull request chose is printed after the words of a message, never first on a line, and without the characters that start a line, move the cursor or reorder what follows.
pub fn printable(text: &str) -> String {
    let unseen = |c: char| {
        c.is_control()
            || matches!(c, '\u{ad}' | '\u{61c}' | '\u{200b}'..='\u{200f}')
            || matches!(c, '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}')
            || matches!(c, '\u{feff}' | '\u{fff9}'..='\u{fffb}')
    };
    text.chars()
        .map(|c| if unseen(c) { '?' } else { c })
        .collect()
}

// A file name is printed as the secret scan would print it: the NUL makes a name no file has, so the scan reads the name alone.
pub fn shown(path: &str) -> String {
    let plain = printable(path);
    let Ok(findings) = secrets::check(&[format!("{plain}\0")]) else {
        return NOT_SHOWN.to_owned();
    };
    match findings
        .first()
        .map(|finding| finding.split_once(IN_THE_NAME))
    {
        None => plain,
        Some(Some((masked, _))) => masked.trim_end_matches('\0').to_owned(),
        Some(None) => NOT_SHOWN.to_owned(),
    }
}

pub fn immutable(changes: &[Change], on_base: &[Entry]) -> Vec<String> {
    let template = folded(ticket::TEMPLATE);
    let is_ticket = |name: &str| name.starts_with(ticket::FOLDER) && name != template;
    let tickets: BTreeMap<String, &str> = on_base
        .iter()
        .map(|entry| (folded(&entry.path), entry.path.as_str()))
        .filter(|(name, _)| is_ticket(name))
        .collect();

    let mut problems = Vec::new();
    let mut adds = false;
    for change in changes {
        let name = folded(&change.path);
        if !is_ticket(&name) {
            continue;
        }
        match tickets.get(&name) {
            Some(ticket) if *ticket == change.path => problems.push(format!(
                "a ticket that is on the base is changed: {}",
                shown(ticket)
            )),
            Some(ticket) => problems.push(format!(
                "a ticket that is on the base is changed under another spelling of its path: {}",
                shown(ticket)
            )),
            None if !change.added => problems.push(format!(
                "a ticket that was there when the branch was cut is changed: {}",
                shown(&change.path)
            )),
            None => adds = true,
        }
    }
    let outside: Vec<&Change> = changes
        .iter()
        .filter(|change| !change.path.starts_with("plan/"))
        .collect();
    if adds && let Some(first) = outside.first() {
        problems.push(format!(
            "ticket files are added together with {} paths outside plan/, the first: {}",
            outside.len(),
            shown(&first.path)
        ));
    }
    problems
}

pub fn apart(changes: &[Change]) -> Vec<String> {
    let under = |name: &str, folder: &str| {
        name.strip_prefix(folder)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    let names: Vec<(String, &Change)> = changes
        .iter()
        .map(|change| (folded(&change.path), change))
        .collect();
    let plan = names.iter().find_map(|(name, _)| match name.as_str() {
        "plan/triage.toml" => Some("plan/triage.toml"),
        "plan/waivers.toml" => Some("plan/waivers.toml"),
        name if under(name, "spec") => Some("spec/"),
        _ => None,
    });
    let code: Vec<&Change> = names
        .iter()
        .filter(|(name, _)| under(name, "crates"))
        .map(|(_, change)| *change)
        .collect();
    match (plan, code.first()) {
        (Some(plan), Some(first)) => vec![format!(
            "{plan} is changed together with {} paths under crates/, the first: {}",
            code.len(),
            shown(&first.path)
        )],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(path: &str) -> Change {
        Change {
            path: path.to_owned(),
            added: false,
        }
    }

    fn added(path: &str) -> Change {
        Change {
            path: path.to_owned(),
            added: true,
        }
    }

    fn entry(path: &str) -> Entry {
        Entry {
            path: path.to_owned(),
            object: String::new(),
            file: true,
        }
    }

    #[test]
    fn spellings_of_one_file_fold_to_one_name() {
        for spelling in [
            "plan/triage.toml",
            "Plan/Triage.TOML",
            "plan\\triage.toml",
            "plan/triage.toml.",
            "plan /triage.toml ",
            "PLAN/TRIAGE.TOML",
        ] {
            assert_eq!(folded(spelling), "plan/triage.toml", "{spelling}");
        }
        assert_eq!(folded("\u{17f}pec/SPEC.md"), "spec/spec.md");
        assert_eq!(
            folded("plan/tic\u{212a}ets/M1-F1.md"),
            "plan/tickets/m1-f1.md"
        );
        assert_ne!(folded("plan/triage.toml.bak"), "plan/triage.toml");
        assert_ne!(folded("plans/triage.toml"), "plan/triage.toml");
    }

    #[test]
    fn a_path_is_shown_without_control_characters() {
        assert_eq!(shown("docs/a b.md"), "docs/a b.md");
        assert_eq!(shown("docs/a\nb\r\u{1b}[31m.md"), "docs/a?b??[31m.md");
        assert_eq!(
            shown("::error::x\n::stop-commands::y"),
            "::error::x?::stop-commands::y"
        );
        assert_eq!(
            shown("docs/a\u{202e}dm.b\u{200b}\u{2028}\u{feff}c"),
            "docs/a?dm.b???c"
        );
    }

    #[test]
    fn every_message_that_names_a_path_shows_it_that_way() {
        let odd = "plan/tickets/M9-X1\n::add-mask::x.md";
        let shown = "plan/tickets/M9-X1?::add-mask::x.md";
        let on_base = [entry(odd)];
        assert_eq!(
            immutable(&[changed(odd)], &on_base),
            [format!("a ticket that is on the base is changed: {shown}")]
        );
        assert_eq!(
            immutable(&[added(&odd.to_uppercase())], &on_base),
            [format!(
                "a ticket that is on the base is changed under another spelling of its path: {shown}"
            )]
        );
        assert_eq!(
            immutable(&[changed(odd)], &[]),
            [format!(
                "a ticket that was there when the branch was cut is changed: {shown}"
            )]
        );
        assert_eq!(
            immutable(
                &[added("plan/tickets/M9-X2.md"), added("docs/a\nb.md")],
                &[]
            ),
            ["ticket files are added together with 1 paths outside plan/, the first: docs/a?b.md"]
        );
        assert_eq!(
            apart(&[changed("spec/SPEC.md"), changed("crates/a\nb.rs")]),
            ["spec/ is changed together with 1 paths under crates/, the first: crates/a?b.rs"]
        );
    }

    #[test]
    fn a_path_is_shown_as_the_secret_scan_would_show_it() {
        let token = format!("gh{}_{}", "s", "Ab1Cd2Ef3Gh4Ij5Kl6Mn7Op8Qr9St0UvWxYz12");
        let address = format!("{}@{}.io", "someone", "corp-mail");
        let path = format!("notes/{token}/{address}.md");
        let seen = shown(&path);
        assert!(seen.starts_with("notes/ghs_"), "{seen}");
        assert!(!seen.contains(&token) && !seen.contains(&address), "{seen}");
        assert_ne!(seen, NOT_SHOWN);
    }

    #[test]
    fn a_ticket_on_the_base_may_not_change() {
        let on_base = [
            entry("plan/tickets/M1-F1.md"),
            entry("plan/tickets/TEMPLATE.md"),
        ];
        assert_eq!(
            immutable(&[changed("plan/tickets/M1-F1.md")], &on_base),
            ["a ticket that is on the base is changed: plan/tickets/M1-F1.md"]
        );
        for spelling in [
            "plan/tickets/m1-f1.md",
            "Plan/Tickets/M1-F1.md",
            "plan\\tickets\\M1-F1.md",
            "plan/tickets/M1-F1.md.",
        ] {
            assert_eq!(
                immutable(&[added(spelling)], &on_base),
                [
                    "a ticket that is on the base is changed under another spelling of its path: plan/tickets/M1-F1.md"
                ],
                "{spelling}"
            );
        }
        assert_eq!(
            immutable(&[changed("plan/tickets/M0-T99.md")], &on_base),
            ["a ticket that was there when the branch was cut is changed: plan/tickets/M0-T99.md"]
        );
    }

    #[test]
    fn the_template_and_other_plan_files_may_change() {
        let on_base = [
            entry("plan/tickets/M1-F1.md"),
            entry("plan/tickets/TEMPLATE.md"),
        ];
        let changes = [
            changed("plan/tickets/TEMPLATE.md"),
            changed("plan/PLAN.md"),
            changed("crates/a/src/lib.rs"),
        ];
        assert_eq!(immutable(&changes, &on_base), Vec::<String>::new());
    }

    #[test]
    fn new_tickets_come_with_nothing_outside_plan() {
        let on_base = [entry("plan/tickets/M1-F1.md")];
        let plan_only = [
            added("plan/tickets/M2-001.md"),
            added("plan/tickets/notes/M2.txt"),
            changed("plan/milestones/M2.md"),
        ];
        assert_eq!(immutable(&plan_only, &on_base), Vec::<String>::new());
        let with_code = [
            added("plan/tickets/M2-001.md"),
            changed("crates/a/src/lib.rs"),
            changed("Plan/x.md"),
        ];
        assert_eq!(
            immutable(&with_code, &on_base),
            [
                "ticket files are added together with 2 paths outside plan/, the first: crates/a/src/lib.rs"
            ]
        );
    }

    #[test]
    fn the_plan_files_and_the_spec_stay_apart_from_the_crates() {
        for plan in [
            "plan/triage.toml",
            "plan/waivers.toml",
            "spec/SPEC.md",
            "spec",
            "Plan/Triage.toml",
            "SPEC/x.md",
            "spec\\x.md",
        ] {
            for code in [
                "crates/a/src/lib.rs",
                "Crates/a.rs",
                "crates",
                "crates\\a.rs",
            ] {
                let problems = apart(&[changed(plan), changed(code)]);
                assert_eq!(problems.len(), 1, "{plan} {code}");
                assert!(problems[0].contains("is changed together with 1 paths under crates/"));
            }
        }
        assert_eq!(
            apart(&[changed("spec/SPEC.md"), changed("crates/a/src/lib.rs")]),
            [
                "spec/ is changed together with 1 paths under crates/, the first: crates/a/src/lib.rs"
            ]
        );
    }

    #[test]
    fn either_side_alone_passes() {
        for changes in [
            vec![changed("plan/triage.toml"), changed("xtask/src/main.rs")],
            vec![changed("spec/SPEC.md"), changed("plan/PLAN.md")],
            vec![changed("crates/a/src/lib.rs"), changed("plan/PLAN.md")],
            vec![changed("crates/a/src/lib.rs"), changed("specs/x.md")],
            vec![changed("spec/SPEC.md"), changed("crates.md")],
        ] {
            assert_eq!(apart(&changes), Vec::<String>::new(), "{changes:?}");
        }
    }
}
