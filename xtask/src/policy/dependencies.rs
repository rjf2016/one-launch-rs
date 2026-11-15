use anyhow::Result;

use super::git::{Change, Content};
use super::paths::folded;
use super::ticket::Ticket;
use super::toml;

pub const MANIFEST: &str = "Cargo.toml";
const MAY_CHANGE_THEM: [&str; 2] = ["deps", "interface"];
const TABLE: &str = "workspace.dependencies";

pub fn check(
    ticket: &Ticket,
    changes: &[Change],
    manifests: impl FnOnce() -> Result<(Content, Content)>,
) -> Result<Vec<String>> {
    if MAY_CHANGE_THEM.contains(&ticket.kind.as_str()) {
        return Ok(Vec::new());
    }
    let mut changed = Vec::new();
    let mut manifest = false;
    for change in changes {
        match folded(&change.path).as_str() {
            "cargo.lock" => changed.push("Cargo.lock is changed".to_owned()),
            "deny.toml" => changed.push("deny.toml is changed".to_owned()),
            "cargo.toml" if change.path == MANIFEST => manifest = true,
            "cargo.toml" => changed.push(format!(
                "a path that is {MANIFEST} under another spelling is changed"
            )),
            _ => {}
        }
    }
    if manifest {
        let (fork, head) = manifests()?;
        changed.extend(declared_differently(&fork, &head));
    }
    Ok(changed
        .into_iter()
        .map(|what| {
            format!(
                "{what}, which only a ticket of `kind: deps` or `interface` may do ({} is `kind: {}`)",
                ticket.id, ticket.kind
            )
        })
        .collect())
}

fn declared_differently(fork: &Content, head: &Content) -> Option<String> {
    match (declared(fork), declared(head)) {
        (Ok(before), Ok(after)) if before == after => None,
        (Ok(_), Ok(_)) => Some(format!("[{TABLE}] of {MANIFEST} is changed")),
        (Err(why), _) => Some(format!(
            "{MANIFEST} is changed and its dependencies on the base cannot be read ({why})"
        )),
        (_, Err(why)) => Some(format!(
            "{MANIFEST} is changed and its dependencies cannot be read ({why})"
        )),
    }
}

// Every line that can declare a workspace dependency, in any of the places TOML lets it sit; the order of a table's entries means nothing.
fn declared(manifest: &Content) -> Result<Vec<String>, String> {
    let text = match manifest {
        Content::Missing => return Ok(Vec::new()),
        Content::NotAFile => return Err("it is not a regular file".to_owned()),
        Content::Text(text) => text,
    };
    let under = |key: &str, name: &str| {
        key.strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    };
    let lines = toml::lines(text).map_err(|line| {
        format!("line {line} is not a table name or one `key = value` that ends on its line")
    })?;
    let mut declared = Vec::new();
    for line in lines {
        let table = line.table;
        let own = under(table, TABLE);
        match line.entry {
            None if own => declared.push(format!("[{table}]")),
            Some((key, value))
                if own
                    || (table == "workspace" && under(key, "dependencies"))
                    || (table.is_empty() && (key == "workspace" || under(key, TABLE))) =>
            {
                declared.push(format!("[{table}] {key} = {value}"));
            }
            _ => {}
        }
    }
    declared.sort();
    Ok(declared)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\", \"xtask\"]\n\n[workspace.dependencies]\nol-model = { version = \"=0.0.0\", path = \"crates/ol-model\" }\nserde = { version = \"=1.0.229\", features = [\"derive\"] }\nanyhow = { version = \"=1.0.104\" }\n\n[workspace.lints.cargo]\nunused_dependencies = \"allow\"\n";

    fn ticket(kind: &str) -> Ticket {
        Ticket {
            id: "M1-F2".to_owned(),
            kind: kind.to_owned(),
            scope: vec!["**".to_owned()],
        }
    }

    fn changed(paths: &[&str]) -> Vec<Change> {
        paths
            .iter()
            .map(|path| Change {
                path: (*path).to_owned(),
                added: false,
            })
            .collect()
    }

    fn with_manifest(kind: &str, head: &str) -> Vec<String> {
        check(&ticket(kind), &changed(&[MANIFEST]), || {
            Ok((
                Content::Text(BASE.to_owned()),
                Content::Text(head.to_owned()),
            ))
        })
        .unwrap()
    }

    fn unread() -> Result<(Content, Content)> {
        panic!("the manifest is read only when it is changed")
    }

    const ONLY: &str =
        ", which only a ticket of `kind: deps` or `interface` may do (M1-F2 is `kind: feature`)";

    #[test]
    fn the_lockfile_and_the_deny_list_change_only_in_a_deps_or_interface_ticket() {
        let changes = changed(&["Cargo.lock", "deny.toml", "crates/a/src/lib.rs"]);
        assert_eq!(
            check(&ticket("feature"), &changes, unread).unwrap(),
            [
                format!("Cargo.lock is changed{ONLY}"),
                format!("deny.toml is changed{ONLY}")
            ]
        );
        for kind in [
            "spike",
            "fix",
            "oracle",
            "spec-amend",
            "Deps",
            "interfaces",
            "",
        ] {
            assert_eq!(
                check(&ticket(kind), &changes, unread).unwrap().len(),
                2,
                "{kind}"
            );
        }
        for kind in ["deps", "interface"] {
            assert_eq!(
                check(&ticket(kind), &changes, unread).unwrap(),
                Vec::<String>::new()
            );
        }
    }

    #[test]
    fn another_spelling_of_a_guarded_file_counts_as_that_file() {
        for path in [
            "cargo.lock",
            "Cargo.LOCK",
            "Deny.toml",
            "deny.toml.",
            "cargo.toml",
        ] {
            let problems = check(&ticket("feature"), &changed(&[path]), unread).unwrap();
            assert_eq!(problems.len(), 1, "{path}");
        }
        for path in [
            "crates/a/Cargo.lock",
            "crates/a/Cargo.toml",
            "docs/deny.toml",
            "Cargo.lock.md",
        ] {
            let problems = check(&ticket("feature"), &changed(&[path]), unread).unwrap();
            assert_eq!(problems, Vec::<String>::new(), "{path}");
        }
    }

    #[test]
    fn a_manifest_change_outside_the_dependencies_passes() {
        for head in [
            BASE.to_owned(),
            BASE.replace("\"crates/*\", \"xtask\"", "\"crates/*\", \"tools/*\", \"xtask\""),
            format!("{BASE}\n[profile.release]\nlto = \"fat\"\n"),
            BASE.replace("unused_dependencies = \"allow\"", "unused_dependencies = \"warn\""),
            BASE.replace(
                "serde = { version = \"=1.0.229\", features = [\"derive\"] }\nanyhow = { version = \"=1.0.104\" }\n",
                "anyhow = { version = \"=1.0.104\" }\n\n# A comment.\nserde = { version = \"=1.0.229\", features = [\"derive\"] }\n",
            ),
        ] {
            assert_eq!(with_manifest("feature", &head), Vec::<String>::new(), "{head}");
        }
    }

    #[test]
    fn a_dependency_declared_differently_fails_however_it_is_spelled() {
        let changed = format!("[workspace.dependencies] of Cargo.toml is changed{ONLY}");
        for head in [
            BASE.replace("features = [\"derive\"]", "features = [\"derive\", \"rc\"]"),
            BASE.replace("\"=1.0.104\"", "\"=1.0.105\""),
            BASE.replace("\"=1.0.104\"", "\"1\""),
            BASE.replace("anyhow = { version = \"=1.0.104\" }\n", ""),
            BASE.replace(
                "[workspace.lints.cargo]",
                "sysinfo = \"=0.31.4\"\n\n[workspace.lints.cargo]",
            ),
            format!("{BASE}\n[workspace.dependencies.sysinfo]\nversion = \"=0.31.4\"\n"),
            format!("{BASE}\n[workspace.dependencies]\n"),
            BASE.replace(
                "resolver = \"3\"",
                "resolver = \"3\"\ndependencies.sysinfo = \"=0.31.4\"",
            ),
            BASE.replace(
                "resolver = \"3\"",
                "resolver = \"3\"\ndependencies = { sysinfo = \"=0.31.4\" }",
            ),
            format!("workspace.dependencies.sysinfo = \"=0.31.4\"\n{BASE}"),
            format!("workspace = {{ dependencies = {{ sysinfo = \"=0.31.4\" }} }}\n{BASE}"),
        ] {
            assert_eq!(
                with_manifest("feature", &head),
                [changed.as_str()],
                "{head}"
            );
            assert_eq!(with_manifest("deps", &head), Vec::<String>::new());
            assert_eq!(with_manifest("interface", &head), Vec::<String>::new());
        }
    }

    #[test]
    fn a_manifest_that_could_hide_a_dependency_fails() {
        for head in [
            BASE.replace(
                "anyhow = { version = \"=1.0.104\" }",
                "anyhow = { version = \"=1.0.104\", x = \"\"\" }\n[workspace.lints.clippy]\n\"\"\" }\nsysinfo = \"=0.31.4\"",
            ),
            BASE.replace(
                "anyhow = { version = \"=1.0.104\" }",
                "anyhow = { version = \"=1.0.104\", features = [\n] }\nsysinfo = \"=0.31.4\"",
            ),
            format!("{BASE}\n[workspace.\"dependencies\"]\nsysinfo = \"=0.31.4\"\n"),
            format!("{BASE}\n[workspace . dependencies]\nsysinfo = \"=0.31.4\"\n"),
            format!("{BASE}\n[workspace.\"\\u0064ependencies\"]\nsysinfo = \"=0.31.4\"\n"),
            format!("\"workspace\".dependencies.sysinfo = \"=0.31.4\"\n{BASE}"),
        ] {
            let problems = with_manifest("feature", &head);
            assert_eq!(problems.len(), 1, "{head}");
            assert!(
                problems[0].starts_with("Cargo.toml is changed and its dependencies cannot be read (line "),
                "{problems:?}"
            );
        }
        let as_a_link = check(&ticket("feature"), &changed(&[MANIFEST]), || {
            Ok((Content::Text(BASE.to_owned()), Content::NotAFile))
        });
        assert_eq!(
            as_a_link.unwrap(),
            [format!(
                "Cargo.toml is changed and its dependencies cannot be read (it is not a regular file){ONLY}"
            )]
        );
        let removed = check(&ticket("feature"), &changed(&[MANIFEST]), || {
            Ok((Content::Text(BASE.to_owned()), Content::Missing))
        });
        assert_eq!(removed.unwrap().len(), 1);
    }

    #[test]
    fn the_dependencies_of_this_repo_are_read() {
        let manifest = Content::Text(crate::workspace::read(MANIFEST).unwrap());
        let declared = declared(&manifest).unwrap();
        assert!(declared.len() > 40, "{}", declared.len());
        assert!(declared.contains(&"[workspace.dependencies]".to_owned()));
        assert!(
            declared
                .iter()
                .any(|line| line.starts_with("[workspace.dependencies] serde = "))
        );
    }
}
