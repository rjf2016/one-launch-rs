use std::collections::BTreeSet;

use anyhow::Result;
use regex::Regex;

use crate::workspace::{self, CRATES_IO, Member, Workspace};

const CARGO_CONFIG: &str = ".cargo/config.toml";
const THE_ALIAS: &str = "[alias]\nxtask = \"run --locked --package xtask --\"\n";

pub fn check(workspace: &Workspace) -> Result<Vec<String>> {
    let mut problems = workspace_dependencies(&workspace::read("Cargo.toml")?);
    for member in &workspace.members {
        let path = format!("{}/Cargo.toml", member.dir);
        problems.extend(member_dependencies(&path, &workspace::read(&path)?));
    }
    problems.extend(lock_sources(&workspace::read("Cargo.lock")?));
    problems.extend(resolved(&workspace.members));
    for package in &workspace.foreign {
        problems.push(format!(
            "Cargo.lock: {package} does not come from crates.io"
        ));
    }
    problems.extend(cargo_config(
        &workspace::files()?,
        &workspace::read(CARGO_CONFIG)?,
    ));
    Ok(problems)
}

// A cargo config can swap the registry, cap the lints or point `cargo xtask` at another binary.
fn cargo_config(files: &[String], config: &str) -> Vec<String> {
    let mut problems: Vec<String> = files
        .iter()
        .filter(|file| file.ends_with(".cargo/config.toml") || file.ends_with(".cargo/config"))
        .filter(|file| *file != CARGO_CONFIG)
        .map(|file| format!("{file}: only {CARGO_CONFIG} may configure cargo"))
        .collect();
    if config.replace("\r\n", "\n") != THE_ALIAS {
        problems.push(format!(
            "{CARGO_CONFIG}: must hold the xtask alias and nothing else"
        ));
    }
    problems
}

fn exact(requirement: &str) -> bool {
    let Some(version) = requirement.strip_prefix('=') else {
        return false;
    };
    let numbers: Vec<&str> = version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
}

// What cargo resolved, whatever way a manifest spells it: the text checks above read one spelling only.
fn resolved(members: &[Member]) -> Vec<String> {
    let names: BTreeSet<&str> = members.iter().map(|member| member.name.as_str()).collect();
    let mut problems = Vec::new();
    for member in members {
        let name = &member.name;
        for edge in &member.edges {
            let (dependency, requirement) = (&edge.name, &edge.requirement);
            match edge.source.as_deref() {
                Some(CRATES_IO) if exact(requirement) => {}
                Some(CRATES_IO) => problems.push(format!(
                    "{name}: asks for {dependency} {requirement}, which is not an exact `=` version"
                )),
                Some(other) => problems.push(format!(
                    "{name}: takes {dependency} from {other}, not from crates.io"
                )),
                None if names.contains(dependency.as_str()) => {}
                None => problems.push(format!(
                    "{name}: {dependency} is a path dependency outside the workspace"
                )),
            }
        }
    }
    problems
}

fn workspace_dependencies(manifest: &str) -> Vec<String> {
    let entry = Regex::new(r#"^[A-Za-z0-9_-]+ = (?:"(=[^"]*)"|\{.*\bversion = "(=[^"]*)".*\})$"#)
        .expect("a fixed pattern");
    let git = Regex::new(r"\bgit\s*=").expect("a fixed pattern");
    let elsewhere =
        Regex::new(r"^(?:workspace\s*\.\s*)?dependencies\s*[.=]").expect("a fixed pattern");
    let mut problems = Vec::new();
    for (table, line) in entries(manifest) {
        if table.starts_with("workspace.dependencies.") {
            if line.is_empty() {
                problems.push(format!(
                    "Cargo.toml: [{table}] must be one line under [workspace.dependencies]"
                ));
            }
        } else if table == "workspace.dependencies" && !line.is_empty() {
            let pinned = entry
                .captures(line)
                .and_then(|found| found.get(1).or(found.get(2)))
                .is_some_and(|version| exact(version.as_str()));
            if git.is_match(line) {
                problems.push(format!("Cargo.toml: `{line}` is a git dependency"));
            } else if !pinned {
                problems.push(format!(
                    "Cargo.toml: `{line}` is not pinned to an exact `=x.y.z` version"
                ));
            }
        } else if matches!(table, "" | "workspace") && elsewhere.is_match(line) {
            problems.push(format!(
                "Cargo.toml: `{line}` must be one line under [workspace.dependencies]"
            ));
        }
    }
    problems
}

fn member_dependencies(path: &str, manifest: &str) -> Vec<String> {
    let inherited =
        Regex::new(r"^[A-Za-z0-9_-]+(?:\.workspace = true|\s*=\s*\{\s*workspace = true\b.*\})$")
            .expect("a fixed pattern");
    let nested = Regex::new(r"(?:^|\.)(?:dev-|build-)?dependencies\.").expect("a fixed pattern");
    let elsewhere = Regex::new(r"^(?:target\b.*\.\s*)?(?:dev-|build-)?dependencies\s*[.=]")
        .expect("a fixed pattern");
    let mut problems = Vec::new();
    for (table, line) in entries(manifest) {
        let last = table.rsplit('.').next().unwrap_or_default();
        if nested.is_match(table) {
            if line.is_empty() {
                problems.push(format!(
                    "{path}: [{table}] must be one line, `name.workspace = true`"
                ));
            }
        } else if matches!(
            last,
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            if !line.is_empty() && !inherited.is_match(line) {
                problems.push(format!("{path}: `{line}` does not say `workspace = true`"));
            }
        } else if (table.is_empty() || table.starts_with("target")) && elsewhere.is_match(line) {
            problems.push(format!(
                "{path}: `{line}` must be one line under a [dependencies] header"
            ));
        }
    }
    problems
}

fn lock_sources(lock: &str) -> Vec<String> {
    let crates_io = format!("source = \"{CRATES_IO}\"");
    lock.lines()
        .filter(|line| line.starts_with("source = ") && *line != crates_io)
        .map(|line| format!("Cargo.lock: `{line}` is not crates.io"))
        .collect()
}

fn entries(manifest: &str) -> Vec<(&str, &str)> {
    let mut table = "";
    let mut entries = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        let before_comment = line.split('#').next().unwrap_or_default().trim();
        if let Some(header) = before_comment
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            table = header.trim_matches(['[', ']', ' ']);
            entries.push((table, ""));
        } else if !line.is_empty() {
            entries.push((table, line));
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Edge;

    #[test]
    fn exact_versions_pass() {
        let manifest = r#"
[workspace]
members = ["crates/*"]

[workspace.dependencies]
ol-model = { version = "=0.0.0", path = "crates/ol-model" }
serde = { version = "=1.0.229", features = ["derive"] }
anyhow = "=1.0.104"
"#;
        assert_eq!(workspace_dependencies(manifest), Vec::<String>::new());
    }

    #[test]
    fn a_loose_version_fails() {
        for line in [
            r#"serde = "1.0.229""#,
            r#"serde = { version = "1", features = ["derive"] }"#,
            r#"serde = { version = "^1.0.229" }"#,
            r#"serde = { version = ">=1.0.229" }"#,
            r#"serde = { version = "=1" }"#,
            r#"serde = "=1.0""#,
            r#"ol-model = { path = "crates/ol-model" }"#,
        ] {
            let manifest = format!("[workspace.dependencies]\n{line}\n");
            let problems = workspace_dependencies(&manifest);
            assert_eq!(problems.len(), 1, "{line}");
            assert!(problems[0].contains("is not pinned to an exact"), "{line}");
        }
    }

    #[test]
    fn a_git_dependency_fails() {
        let manifest = r#"
[workspace.dependencies]
gpui = { git = "https://github.com/zed-industries/zed", rev = "d2a4fb6" }
"#;
        let problems = workspace_dependencies(manifest);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("is a git dependency"));
    }

    #[test]
    fn a_dependency_written_as_a_table_fails() {
        let manifest = "[workspace.dependencies.serde]\nversion = \"=1.0.229\"\n";
        assert_eq!(workspace_dependencies(manifest).len(), 1);
        let manifest = "[dependencies.serde]\nworkspace = true\n";
        assert_eq!(member_dependencies("x/Cargo.toml", manifest).len(), 1);
        let manifest = "[target.'cfg(windows)'.dependencies.windows]\nversion = \"0.62\"\n";
        assert_eq!(member_dependencies("x/Cargo.toml", manifest).len(), 1);
    }

    #[test]
    fn inherited_dependencies_pass() {
        let manifest = r#"
[package]
name = "x"
version.workspace = true

[[bin]]
name = "OneLaunch"

[features]
fake = ["dep:ol-sim"]

[dependencies]
serde.workspace = true
ol-sim = { workspace = true, optional = true }
windows = { workspace = true, features = ["Win32_Foundation"] }

[target.'cfg(windows)'.dependencies]
windows.workspace = true

[dev-dependencies]
ol-testkit.workspace = true

[build-dependencies]
winresource.workspace = true
"#;
        assert_eq!(
            member_dependencies("x/Cargo.toml", manifest),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_dependency_of_its_own_fails() {
        for (table, line) in [
            ("dependencies", r#"sysinfo = "0.40""#),
            ("dependencies", r#"serde = { version = "=1.0.229" }"#),
            (
                "dev-dependencies",
                r#"ol-testkit = { path = "../ol-testkit" }"#,
            ),
            ("build-dependencies", r#"winresource = "=0.1.34""#),
            (
                "target.'cfg(windows)'.dependencies",
                r#"windows = "=0.62.2""#,
            ),
            ("dependencies", "serde = { workspace = false }"),
        ] {
            let manifest = format!("[{table}]\n{line}\n");
            let problems = member_dependencies("x/Cargo.toml", &manifest);
            assert_eq!(problems.len(), 1, "{line}");
            assert!(
                problems[0].contains("does not say `workspace = true`"),
                "{line}"
            );
        }
    }

    #[test]
    fn a_source_that_is_not_crates_io_fails() {
        let lock = r#"
[[package]]
name = "serde"
version = "1.0.229"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "gpui"
version = "0.2.2"
source = "git+https://github.com/zed-industries/zed?rev=d2a4fb6#d2a4fb6ded7c470be25d1541281da3882495b65b"

[[package]]
name = "other"
version = "1.0.0"
source = "registry+https://example.com/index"
"#;
        assert_eq!(lock_sources(lock).len(), 2);
    }

    #[test]
    fn only_a_full_version_is_exact() {
        for requirement in ["=1.0.229", "=0.0.0", "=2.0.0-beta.1", "=0.6.0+11769913"] {
            assert!(exact(requirement), "{requirement}");
        }
        for requirement in ["=1", "=1.2", "=1.2.x", "=1.2.", "1.2.3", "^1.2.3", "=", ""] {
            assert!(!exact(requirement), "{requirement}");
        }
    }

    #[test]
    fn a_header_with_a_comment_or_spaces_is_still_a_header() {
        for header in ["[dependencies] # runtime", "[ dependencies ]"] {
            let manifest = format!("{header}\nserde = \"=1.0.229\"\n");
            let problems = member_dependencies("x/Cargo.toml", &manifest);
            assert_eq!(problems.len(), 1, "{header}");
        }
    }

    #[test]
    fn a_dependency_written_outside_a_header_fails() {
        for manifest in [
            "dependencies.serde = \"=1.0.229\"\n[package]\nname = \"x\"\n",
            "build-dependencies = { semver = \"=1.0.28\" }\n[package]\nname = \"x\"\n",
            "[target.'cfg(windows)']\ndependencies = { windows = \"=0.62.2\" }\n",
            "target.'cfg(windows)'.dependencies.windows = \"=0.62.2\"\n",
        ] {
            let problems = member_dependencies("x/Cargo.toml", manifest);
            assert_eq!(problems.len(), 1, "{manifest}");
            assert!(problems[0].contains("under a [dependencies] header"));
        }
        for manifest in [
            "workspace.dependencies.serde = \"=1.0.229\"\n",
            "[workspace]\ndependencies = { serde = \"=1.0.229\" }\n",
            "[workspace]\ndependencies.serde = \"=1.0.229\"\n",
        ] {
            assert_eq!(workspace_dependencies(manifest).len(), 1, "{manifest}");
        }
    }

    #[test]
    fn the_cargo_config_holds_the_alias_and_nothing_else() {
        let root = [CARGO_CONFIG.to_owned(), "Cargo.toml".to_owned()];
        assert_eq!(cargo_config(&root, THE_ALIAS), Vec::<String>::new());
        assert_eq!(
            cargo_config(&root, &THE_ALIAS.replace('\n', "\r\n")),
            Vec::<String>::new()
        );
        for config in [
            "[alias]\nxtask = \"run --locked --package s10 --\"\n".to_owned(),
            format!("{THE_ALIAS}\n[build]\nrustflags = [\"--cap-lints\", \"allow\"]\n"),
            format!("{THE_ALIAS}\n[source.crates-io]\nreplace-with = \"vendored\"\n"),
            String::new(),
        ] {
            assert_eq!(cargo_config(&root, &config).len(), 1, "{config}");
        }
        let nested = [
            CARGO_CONFIG.to_owned(),
            "crates/ol-model/.cargo/config.toml".to_owned(),
            "xtask/.cargo/config".to_owned(),
        ];
        assert_eq!(cargo_config(&nested, THE_ALIAS).len(), 2);
    }

    #[test]
    fn what_cargo_resolved_is_checked_too() {
        let edge = |name: &str, requirement: &str, source: Option<&str>| Edge {
            name: name.to_owned(),
            requirement: requirement.to_owned(),
            source: source.map(str::to_owned),
            ..Edge::default()
        };
        let member = |name: &str, edges: Vec<Edge>| Member {
            name: name.to_owned(),
            dir: format!("crates/{name}"),
            edges,
            roots: Vec::new(),
            reach: BTreeSet::new(),
        };
        let git = "git+https://github.com/zed-industries/zed?rev=d2a4fb6";
        let members = [
            member("ol-model", vec![edge("serde", "=1.0.229", Some(CRATES_IO))]),
            member(
                "ol-formats",
                vec![
                    edge("ol-model", "=0.0.0", None),
                    edge("regex", "^1.13", Some(CRATES_IO)),
                    edge("gpui", "*", Some(git)),
                    edge("vendored", "*", None),
                ],
            ),
        ];
        assert_eq!(
            resolved(&members),
            [
                "ol-formats: asks for regex ^1.13, which is not an exact `=` version".to_owned(),
                format!("ol-formats: takes gpui from {git}, not from crates.io"),
                "ol-formats: vendored is a path dependency outside the workspace".to_owned(),
            ]
        );
    }

    #[test]
    fn the_workspace_manifests_pass() {
        let workspace = Workspace::load().unwrap();
        assert_eq!(check(&workspace).unwrap(), Vec::<String>::new());
    }
}
