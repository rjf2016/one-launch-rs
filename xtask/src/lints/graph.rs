use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use regex::Regex;

use crate::workspace::{self, Member, Workspace};

pub const DESIGN: &str = "plan/design/architecture.md";
const HEADING: &str = "### Crate graph";
const GPUI_CRATES: [&str; 4] = ["ol-app", "ol-automation", "ol-ui", "ol-ui-kit"];

#[derive(Debug, Default, PartialEq)]
pub struct Row {
    pub depends: BTreeSet<String>,
    pub tests: BTreeSet<String>,
}

pub fn check(workspace: &Workspace) -> Result<Vec<String>> {
    let design = parse(&workspace::read(DESIGN)?)?;
    Ok(problems(&design, &workspace.members))
}

pub fn parse(markdown: &str) -> Result<BTreeMap<String, Row>> {
    let cell = Regex::new(r"^(?:none|`[a-z0-9-]+`(?:, `[a-z0-9-]+`)*)$")?;
    let name = Regex::new(r"`([a-z0-9-]+)`")?;
    let names = |text: &str| -> BTreeSet<String> {
        name.captures_iter(text)
            .map(|found| found[1].to_owned())
            .collect()
    };

    let table = markdown
        .lines()
        .skip_while(|line| line.trim() != HEADING)
        .skip(1)
        .take_while(|line| !line.starts_with('#'))
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'));
    let mut rows = BTreeMap::new();
    for line in table.skip(2) {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        let [krate, depends, tests] = cells.as_slice() else {
            bail!("{DESIGN}: the crate graph row `{line}` does not have three cells");
        };
        if !cell.is_match(krate) || *krate == "none" || krate.contains(',') {
            bail!("{DESIGN}: `{krate}` is not one crate name in backticks");
        }
        for list in [depends, tests] {
            if !cell.is_match(list) {
                bail!("{DESIGN}: `{list}` is neither `none` nor crate names in backticks");
            }
        }
        let row = Row {
            depends: names(depends),
            tests: names(tests),
        };
        if rows
            .insert(krate.trim_matches('`').to_owned(), row)
            .is_some()
        {
            bail!("{DESIGN}: the crate graph lists {krate} twice");
        }
    }
    if rows.is_empty() {
        bail!("{DESIGN}: no table under `{HEADING}`");
    }
    Ok(rows)
}

pub fn problems(design: &BTreeMap<String, Row>, members: &[Member]) -> Vec<String> {
    let mut problems = Vec::new();
    let all: BTreeSet<&str> = members.iter().map(|member| member.name.as_str()).collect();
    let spikes: BTreeSet<&str> = members
        .iter()
        .filter(|member| member.is_spike())
        .map(|member| member.name.as_str())
        .collect();

    for member in members {
        let name = &member.name;
        if member.is_spike() {
            if design.contains_key(name) {
                problems.push(format!("{name}: a spike has no row in the crate graph"));
            }
            continue;
        }
        if !member.dir.starts_with("crates/") && member.dir != "xtask" {
            problems.push(format!(
                "{name}: {} is outside crates/, spikes/ and xtask",
                member.dir
            ));
        }

        let declared = |dev: bool| -> BTreeSet<String> {
            member
                .edges
                .iter()
                .filter(|edge| edge.dev == dev && all.contains(edge.name.as_str()))
                .map(|edge| edge.name.clone())
                .collect()
        };
        let (depends, tests) = (declared(false), declared(true));

        for spike in depends.iter().chain(&tests) {
            if spikes.contains(spike.as_str()) {
                problems.push(format!("{name}: depends on the spike {spike}"));
            }
        }

        match design.get(name) {
            None => problems.push(format!("{name}: has no row in the crate graph of {DESIGN}")),
            Some(row) => {
                compare(&mut problems, name, "depends on", &row.depends, &depends);
                compare(&mut problems, name, "uses in tests", &row.tests, &tests);
            }
        }

        if !GPUI_CRATES.contains(&name.as_str()) {
            let gpui: Vec<&str> = member
                .reach
                .iter()
                .map(String::as_str)
                .filter(|package| workspace::is_gpui(package))
                .collect();
            if let Some(first) = gpui.first() {
                problems.push(format!(
                    "{name}: reaches {first} ({} GPUI packages in all); only {} may reach GPUI",
                    gpui.len(),
                    GPUI_CRATES.join(", ")
                ));
            }
        }
    }

    for name in design.keys() {
        if !all.contains(name.as_str()) {
            problems.push(format!(
                "{name}: has a row in the crate graph of {DESIGN} but is not a workspace member"
            ));
        }
    }
    problems
}

fn compare(
    problems: &mut Vec<String>,
    name: &str,
    verb: &str,
    design: &BTreeSet<String>,
    manifest: &BTreeSet<String>,
) {
    for extra in manifest.difference(design) {
        problems.push(format!(
            "{name}: {verb} {extra} in its manifest, which the crate graph of {DESIGN} does not list"
        ));
    }
    for missing in design.difference(manifest) {
        problems.push(format!(
            "{name}: {verb} {missing} in the crate graph of {DESIGN}, but not in its manifest"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Edge;

    const TABLE: &str = "\
### Crate graph

Some words.

| Crate | Depends on | In tests |
| --- | --- | --- |
| `ol-model` | none | none |
| `ol-formats` | `ol-model` | `ol-testkit` |
| `ol-testkit` | none | none |
| `ol-ui` | `ol-model` | none |

After the table.
";

    fn member(name: &str, dir: &str, depends: &[&str], tests: &[&str], reach: &[&str]) -> Member {
        let edge = |dev: bool| {
            move |name: &&str| Edge {
                name: (*name).to_owned(),
                dev,
                ..Edge::default()
            }
        };
        Member {
            name: name.to_owned(),
            dir: dir.to_owned(),
            edges: depends
                .iter()
                .map(edge(false))
                .chain(tests.iter().map(edge(true)))
                .collect(),
            roots: Vec::new(),
            reach: reach.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn matching() -> Vec<Member> {
        vec![
            member("ol-model", "crates/ol-model", &["serde"], &[], &["serde"]),
            member(
                "ol-formats",
                "crates/ol-formats",
                &["ol-model"],
                &["ol-testkit"],
                &["ol-model", "ol-testkit"],
            ),
            member("ol-testkit", "crates/ol-testkit", &[], &[], &[]),
            member(
                "ol-ui",
                "crates/ol-ui",
                &["ol-model", "gpui-pre"],
                &[],
                &["ol-model", "gpui-pre", "gpui-pre-macros"],
            ),
            member("s01", "spikes/s01", &["gpui-pre"], &[], &["gpui-pre"]),
        ]
    }

    fn found(members: &[Member]) -> Vec<String> {
        problems(&parse(TABLE).unwrap(), members)
    }

    #[test]
    fn the_table_is_read() {
        let design = parse(TABLE).unwrap();
        assert_eq!(design.len(), 4);
        assert_eq!(
            design["ol-formats"],
            Row {
                depends: BTreeSet::from(["ol-model".to_owned()]),
                tests: BTreeSet::from(["ol-testkit".to_owned()]),
            }
        );
    }

    #[test]
    fn a_missing_table_is_an_error() {
        assert!(parse("# Architecture\n\nNo graph here.\n").is_err());
        assert!(
            parse("### Crate graph\n\n| Crate | Depends on | In tests |\n| --- | --- | --- |\n")
                .is_err()
        );
    }

    #[test]
    fn a_cell_that_is_not_a_list_of_names_is_an_error() {
        let bad = TABLE.replace(
            "| `ol-formats` | `ol-model` |",
            "| `ol-formats` | ol-model |",
        );
        assert!(parse(&bad).is_err());
    }

    #[test]
    fn matching_manifests_pass() {
        assert_eq!(found(&matching()), Vec::<String>::new());
    }

    #[test]
    fn an_edge_the_design_does_not_list_fails() {
        let mut members = matching();
        members[0] = member("ol-model", "crates/ol-model", &["ol-formats"], &[], &[]);
        let problems = found(&members);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("ol-model: depends on ol-formats in its manifest"));
    }

    #[test]
    fn an_edge_missing_from_the_manifest_fails() {
        let mut members = matching();
        members[1] = member("ol-formats", "crates/ol-formats", &[], &["ol-testkit"], &[]);
        let problems = found(&members);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("but not in its manifest"));
    }

    #[test]
    fn a_test_edge_is_not_a_normal_edge() {
        let mut members = matching();
        members[1] = member(
            "ol-formats",
            "crates/ol-formats",
            &["ol-model", "ol-testkit"],
            &[],
            &[],
        );
        assert_eq!(found(&members).len(), 2);
    }

    #[test]
    fn a_crate_outside_the_four_that_reaches_gpui_fails() {
        let mut members = matching();
        members[0] = member(
            "ol-model",
            "crates/ol-model",
            &["gpui-pre"],
            &[],
            &["gpui-pre", "gpui-pre-macros"],
        );
        let problems = found(&members);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("ol-model: reaches gpui-pre (2 GPUI packages in all)"));
    }

    #[test]
    fn gpui_reached_through_another_crate_fails() {
        let mut members = matching();
        members[1] = member(
            "ol-formats",
            "crates/ol-formats",
            &["ol-model"],
            &["ol-testkit"],
            &["ol-model", "some-widget", "gpui-kit"],
        );
        let problems = found(&members);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("ol-formats: reaches gpui-kit"));
    }

    #[test]
    fn a_product_crate_that_depends_on_a_spike_fails() {
        let mut members = matching();
        members[2] = member("ol-testkit", "crates/ol-testkit", &[], &["s01"], &[]);
        let problems = found(&members);
        assert!(problems.contains(&"ol-testkit: depends on the spike s01".to_owned()));
    }

    #[test]
    fn a_crate_without_a_row_and_a_row_without_a_crate_fail() {
        let mut members = matching();
        members[2] = member("ol-extra", "crates/ol-extra", &[], &[], &[]);
        let problems = found(&members);
        assert!(
            problems
                .iter()
                .any(|problem| problem.starts_with("ol-extra: has no row"))
        );
        assert!(
            problems
                .iter()
                .any(|problem| problem.starts_with("ol-testkit: has a row"))
        );
    }

    #[test]
    fn a_member_in_another_folder_fails() {
        let mut members = matching();
        members[2] = member("ol-testkit", "tools/ol-testkit", &[], &[], &[]);
        assert!(found(&members)[0].contains("is outside crates/, spikes/ and xtask"));
    }

    #[test]
    fn the_workspace_matches_the_design() {
        let workspace = Workspace::load().unwrap();
        assert_eq!(check(&workspace).unwrap(), Vec::<String>::new());
    }
}
