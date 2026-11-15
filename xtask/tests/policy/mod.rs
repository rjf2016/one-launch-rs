mod rules;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::event::{self, PullRequest};
use super::git::Repo;
use super::{Report, Rule};
use crate::workspace;

pub const OWNER: u64 = 7;
pub const BODY: &str =
    "## Summary\n\nA fixture.\n\n## Evidence\n\n```evidence\n```\n\n## Merge Danger\n\nNone.\n";

static FIXTURES: AtomicUsize = AtomicUsize::new(0);

pub struct Fixture {
    dir: PathBuf,
    pub repo: Repo,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Fixture {
    pub fn new(files: &[(&str, &str)]) -> (Self, String) {
        let dir = std::env::temp_dir().join(format!(
            "ol-policy-{}-{}",
            std::process::id(),
            FIXTURES.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = Self {
            repo: Repo::at(&dir),
            dir,
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        assert!(
            fixture.dir.join(".git").is_dir(),
            "the fixture must be a repository of its own"
        );
        for (path, text) in files {
            fixture.write(path, text);
        }
        let base = fixture.commit("The base");
        (fixture, base)
    }

    // A fixture sets all it needs on each call, so the name, the address and the line endings of the machine's own git config play no part.
    pub fn git(&self, args: &[&str]) -> String {
        let dir = self.dir.to_string_lossy();
        let hooks = format!("core.hooksPath={}", self.dir.join("no-hooks").display());
        let mut all = vec![
            "-C",
            &*dir,
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "advice.detachedHead=false",
            "-c",
            &hooks,
        ];
        all.extend(args);
        let printed = workspace::output("git", &all).unwrap();
        String::from_utf8_lossy(&printed).trim().to_owned()
    }

    pub fn path(&self, path: &str) -> PathBuf {
        self.dir.join(path)
    }

    pub fn write(&self, path: &str, text: &str) {
        let on_disk = self.path(path);
        std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        std::fs::write(on_disk, text).unwrap();
    }

    pub fn remove(&self, path: &str) {
        std::fs::remove_file(self.path(path)).unwrap();
    }

    pub fn on(&self, branch: &str, from: &str) {
        self.git(&["checkout", "-q", "-f", "-B", branch, from]);
    }

    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.commit_the_index(message)
    }

    pub fn commit_the_index(&self, message: &str) -> String {
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    // What a Windows checkout cannot hold (another case of a name that exists, a link, a submodule, an executable bit) goes into the index alone, and the index is put back afterwards so that the next head starts clean.
    pub fn staged(&self, from: &str, entries: &[(&str, &str)], message: &str) -> String {
        self.on("work", from);
        let blob = self.git(&["rev-parse", &format!("{from}:README.md")]);
        for (mode, path) in entries {
            let object = if *mode == "160000" { from } else { &blob };
            self.git(&[
                "-c",
                "core.ignorecase=false",
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("{mode},{object},{path}"),
            ]);
        }
        self.settled(from, message)
    }

    pub fn executable(&self, from: &str, path: &str, message: &str) -> String {
        self.on("work", from);
        self.git(&["update-index", "--chmod=+x", path]);
        self.settled(from, message)
    }

    fn settled(&self, from: &str, message: &str) -> String {
        let head = self.commit_the_index(message);
        self.git(&["reset", "-q", "--mixed", from]);
        head
    }

    pub fn head(&self, from: &str, files: &[(&str, Option<&str>)], message: &str) -> String {
        self.on("work", from);
        for (path, text) in files {
            match text {
                Some(text) => self.write(path, text),
                None => self.remove(path),
            }
        }
        self.commit(message)
    }

    // git on Windows refuses a quote, a newline or a backslash in a name it is asked to add, so the tree that holds one is written byte by byte.
    pub fn head_with_a_name(&self, from: &str, name: &str) -> String {
        self.head_with_names(from, None, &[name.as_bytes()])
    }

    pub fn head_with_names(&self, from: &str, folder: Option<&str>, names: &[&[u8]]) -> String {
        let blob = self.git(&["rev-parse", &format!("{from}:README.md")]);
        let added: Vec<(Vec<u8>, String, bool)> = names
            .iter()
            .map(|name| (name.to_vec(), blob.clone(), false))
            .collect();
        let mut entries: Vec<(Vec<u8>, String, bool)> = self
            .git(&["ls-tree", from])
            .lines()
            .map(|line| {
                let (about, name) = line.split_once('\t').unwrap();
                let fields: Vec<&str> = about.split(' ').collect();
                (
                    name.as_bytes().to_vec(),
                    fields[2].to_owned(),
                    fields[1] == "tree",
                )
            })
            .collect();
        match folder {
            Some(folder) => entries.push((folder.as_bytes().to_vec(), self.tree(added), true)),
            None => entries.extend(added),
        }
        let tree = self.tree(entries);
        self.git(&[
            "commit-tree",
            &tree,
            "-p",
            from,
            "-m",
            "A name git on Windows will not add",
        ])
    }

    fn tree(&self, mut entries: Vec<(Vec<u8>, String, bool)>) -> String {
        entries.sort_by_key(|(name, _, is_tree)| {
            let mut in_tree_order = name.clone();
            in_tree_order.extend(is_tree.then_some(b'/'));
            in_tree_order
        });
        let mut tree = Vec::new();
        for (name, object, is_tree) in &entries {
            tree.extend(if *is_tree { "40000 " } else { "100644 " }.bytes());
            tree.extend(name);
            tree.push(0);
            tree.extend(
                (0..object.len())
                    .step_by(2)
                    .map(|at| u8::from_str_radix(&object[at..at + 2], 16).unwrap()),
            );
        }
        std::fs::write(self.path(".git/tree.bin"), tree).unwrap();
        self.git(&["hash-object", "-t", "tree", "-w", ".git/tree.bin"])
    }

    pub fn request(&self, branch: &str, base: &str, head: &str) -> PullRequest {
        PullRequest {
            branch: branch.to_owned(),
            title: "A fixture".to_owned(),
            body: BODY.to_owned(),
            base: base.to_owned(),
            head: head.to_owned(),
            owner: OWNER,
        }
    }

    pub fn judged(&self, branch: &str, base: &str, head: &str) -> Report {
        super::pull_request(&self.repo, base, &self.request(branch, base, head), None).unwrap()
    }
}

pub fn ticket(kind: &str, scope: &[&str]) -> String {
    let scope: String = scope.iter().map(|entry| format!("  - {entry}\n")).collect();
    format!(
        "---\nid: a-fixture\ntitle: A fixture\nkind: {kind}\nrisk: plain\ndepends: []\nwrite_scope:\n{scope}read:\n  design: []\n---\n\n## Outcome\n\nNothing.\n"
    )
}

pub fn reviews(reviews: &[(u64, &str, &str)]) -> Vec<event::Review> {
    let reviews: Vec<String> = reviews
        .iter()
        .enumerate()
        .map(|(at, (user, state, commit))| {
            format!(
                r#"{{"id": {at}, "user": {{"id": {user}}}, "state": "{state}", "commit_id": "{commit}", "submitted_at": "2026-11-14T10:{at:02}:00Z"}}"#
            )
        })
        .collect();
    event::reviews(&format!("[{}]", reviews.join(","))).unwrap()
}

pub fn failed(report: &Report) -> Vec<Rule> {
    report.failed()
}

pub fn problems(report: &Report, rule: Rule) -> Vec<String> {
    report
        .verdicts
        .iter()
        .filter(|verdict| verdict.rule == rule)
        .flat_map(|verdict| verdict.problems.clone())
        .collect()
}

pub fn notes(report: &Report, rule: Rule) -> Vec<String> {
    report
        .verdicts
        .iter()
        .filter(|verdict| verdict.rule == rule)
        .flat_map(|verdict| verdict.notes.clone())
        .collect()
}

pub fn judged_rules(report: &Report) -> Vec<Rule> {
    report.verdicts.iter().map(|verdict| verdict.rule).collect()
}
