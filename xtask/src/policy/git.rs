use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::workspace;

const LOCAL_READS: [&str; 8] = [
    "branch",
    "cat-file",
    "diff",
    "log",
    "ls-files",
    "ls-tree",
    "merge-base",
    "rev-parse",
];

pub struct Repo {
    dir: String,
}

pub enum Tree<'a> {
    Commit(&'a str),
    Checkout,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: String,
    pub added: bool,
}

pub struct Commit {
    pub id: String,
    pub message: String,
}

pub struct Entry {
    pub path: String,
    pub object: String,
    pub file: bool,
}

#[derive(Debug, PartialEq)]
pub enum Content {
    Missing,
    NotAFile,
    Text(String),
}

pub fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

impl Repo {
    pub fn workspace() -> Self {
        Self::at(workspace::root())
    }

    pub fn at(dir: &Path) -> Self {
        Self {
            dir: dir.to_string_lossy().into_owned(),
        }
    }

    // Each of these reads the local repository and none can reach a remote, so no check here makes a network call.
    pub fn git(&self, args: &[&str]) -> Result<Vec<u8>> {
        let command = args.first().copied().unwrap_or_default();
        if !LOCAL_READS.contains(&command) {
            bail!("git {command} is not a command the policy runs");
        }
        let output = self
            .command(args)
            .stderr(Stdio::inherit())
            .output()
            .context("could not start git")?;
        if !output.status.success() {
            bail!("`git {}` failed", args.join(" "));
        }
        Ok(output.stdout)
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "the policy starts git itself, to say what git may not do"
    )]
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        // In a partial clone git fetches an object it lacks, and a check that reads must never reach a remote.
        command
            .env("GIT_NO_LAZY_FETCH", "1")
            .args(["--no-optional-locks", "-C", self.dir.as_str()])
            .args(args)
            .current_dir(workspace::root());
        command
    }

    pub fn commit(&self, name: &str) -> Result<String> {
        let plain = !name.is_empty()
            && !name.starts_with('-')
            && name.chars().all(|c| c.is_ascii_graphic());
        if !plain {
            bail!("a commit is named by a ref or an id, with no space and no leading dash");
        }
        let found = self
            .git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{name}^{{commit}}"),
            ])
            .with_context(|| format!("{name} is not a commit in this checkout"))?;
        Ok(text(&found).trim().to_owned())
    }

    pub fn fork(&self, base: &str, head: &str) -> Result<String> {
        let found = self
            .git(&["merge-base", base, head])
            .context("the base and the head share no history")?;
        Ok(text(&found).trim().to_owned())
    }

    pub fn branch(&self) -> Result<Option<String>> {
        let name = text(&self.git(&["branch", "--show-current"])?)
            .trim()
            .to_owned();
        Ok((!name.is_empty()).then_some(name))
    }

    pub fn changes(&self, fork: &str, head: &Tree) -> Result<Vec<Change>> {
        let mut found = BTreeMap::new();
        match head {
            Tree::Commit(head) => self.diff(&[fork, head], &mut found)?,
            // What is committed, what is staged, what is changed and not staged, and what is not tracked: each can hold a path the others do not.
            Tree::Checkout => {
                self.diff(&[fork, "HEAD"], &mut found)?;
                self.diff(&["--cached", fork], &mut found)?;
                self.diff(&[], &mut found)?;
                let untracked = self.git(&["ls-files", "-z", "--others", "--exclude-standard"])?;
                for path in paths(&untracked)? {
                    found.entry(path).or_insert(true);
                }
            }
        }
        Ok(found
            .into_iter()
            .map(|(path, added)| Change { path, added })
            .collect())
    }

    // A rename is a removal and an addition, and a submodule's new commit is a change: both are asked for by name, whatever git's config says.
    fn diff(&self, between: &[&str], found: &mut BTreeMap<String, bool>) -> Result<()> {
        let mut args = vec![
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--ignore-submodules=none",
            "--no-ext-diff",
        ];
        args.extend(between);
        args.push("--");
        for (status, path) in statuses(&self.git(&args)?)? {
            found.entry(path).or_insert(status == 'A');
        }
        Ok(())
    }

    pub fn commits(&self, range: &[&str]) -> Result<Vec<Commit>> {
        let mut args = vec!["log", "--no-show-signature", "-z", "--format=%H%n%B"];
        args.extend(range);
        args.push("--");
        records(&self.git(&args)?)
            .into_iter()
            .map(|record| {
                let (id, message) = record
                    .split_once('\n')
                    .context("git log printed a commit without a message")?;
                Ok(Commit {
                    id: id.to_owned(),
                    message: message.to_owned(),
                })
            })
            .collect()
    }

    pub fn entries(&self, commit: &str, under: &str) -> Result<Vec<Entry>> {
        listed(&self.git(&["ls-tree", "-r", "-z", "--full-tree", commit, "--", under])?)
    }

    pub fn blob(&self, object: &str) -> Result<String> {
        Ok(text(&self.git(&["cat-file", "blob", object])?).into_owned())
    }

    pub fn read(&self, tree: &Tree, path: &str) -> Result<Content> {
        match tree {
            Tree::Commit(commit) => {
                let entries = self.entries(commit, path)?;
                match entries.iter().find(|entry| entry.path == path) {
                    Some(entry) if entry.file => Ok(Content::Text(self.blob(&entry.object)?)),
                    Some(_) => Ok(Content::NotAFile),
                    None if entries.is_empty() => Ok(Content::Missing),
                    None => Ok(Content::NotAFile),
                }
            }
            Tree::Checkout => {
                let on_disk = Path::new(&self.dir).join(path);
                match std::fs::symlink_metadata(&on_disk) {
                    Ok(found) if found.is_file() => {
                        let bytes = std::fs::read(&on_disk)
                            .with_context(|| format!("could not read {path}"))?;
                        Ok(Content::Text(text(&bytes).into_owned()))
                    }
                    Ok(_) => Ok(Content::NotAFile),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Ok(Content::Missing)
                    }
                    Err(error) => Err(error).with_context(|| format!("could not read {path}")),
                }
            }
        }
    }
}

fn text(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

const NOT_UTF8: &str = "git printed a path that is not UTF-8, which this check cannot judge";

// Two names that are not UTF-8 can read as one once their bytes are replaced, so a record that holds a path is never decoded loosely.
fn paths(listing: &[u8]) -> Result<Vec<String>> {
    listing
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| String::from_utf8(record.to_vec()).map_err(|_| anyhow::anyhow!(NOT_UTF8)))
        .collect()
}

fn records(listing: &[u8]) -> Vec<String> {
    text(listing)
        .split('\0')
        .filter(|record| !record.is_empty())
        .map(str::to_owned)
        .collect()
}

fn listed(listing: &[u8]) -> Result<Vec<Entry>> {
    paths(listing)?
        .into_iter()
        .map(|record| {
            let fields = record
                .split_once('\t')
                .map(|(about, path)| (about.split(' ').collect::<Vec<_>>(), path));
            let Some(([mode, _, object], path)) = fields
                .as_ref()
                .map(|(about, path)| (about.as_slice(), path))
            else {
                bail!("git ls-tree printed an entry this check does not read");
            };
            Ok(Entry {
                path: (*path).to_owned(),
                object: (*object).to_owned(),
                file: mode.starts_with("100"),
            })
        })
        .collect()
}

fn statuses(listing: &[u8]) -> Result<Vec<(char, String)>> {
    let fields = paths(listing)?;
    let mut found = Vec::new();
    for pair in fields.chunks(2) {
        let [status, path] = pair else {
            bail!("git diff printed a status without a path");
        };
        match status.as_str() {
            "A" => found.push(('A', path.clone())),
            "M" | "D" | "T" | "U" => found.push(('M', path.clone())),
            _ => bail!("git diff printed a status this check does not read"),
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_is_split_at_the_nul_bytes_whatever_a_name_holds() {
        let listing = b"A\0docs/a b.md\0M\0docs/q\"x\n.md\0D\0crates\\x.rs\0T\0link\0";
        assert_eq!(
            statuses(listing).unwrap(),
            [
                ('A', "docs/a b.md".to_owned()),
                ('M', "docs/q\"x\n.md".to_owned()),
                ('M', "crates\\x.rs".to_owned()),
                ('M', "link".to_owned()),
            ]
        );
    }

    #[test]
    fn a_status_this_check_does_not_read_is_an_error() {
        assert!(statuses(b"R100\0old.rs\0new.rs\0").is_err());
        assert!(statuses(b"X\0a.rs\0").is_err());
        assert!(statuses(b"M\0").is_err());
    }

    #[test]
    fn only_commands_that_read_the_local_repository_run() {
        let repo = Repo::workspace();
        for command in [
            "fetch",
            "pull",
            "push",
            "clone",
            "ls-remote",
            "remote",
            "submodule",
            "",
        ] {
            let refused = repo.git(&[command]).unwrap_err().to_string();
            assert!(
                refused.contains("is not a command the policy runs"),
                "{command}"
            );
        }
    }

    #[test]
    fn git_is_started_with_lazy_fetching_off() {
        let command = Repo::workspace().command(&["rev-parse", "HEAD"]);
        let set: Vec<_> = command.get_envs().collect();
        assert_eq!(
            set,
            [(
                std::ffi::OsStr::new("GIT_NO_LAZY_FETCH"),
                Some(std::ffi::OsStr::new("1"))
            )]
        );
        assert_eq!(command.get_program(), "git");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args[0], "--no-optional-locks");
        assert_eq!(args[3..], ["rev-parse", "HEAD"]);
    }

    #[test]
    fn a_path_that_is_not_utf8_is_an_error_that_quotes_nothing() {
        for listing in [
            b"A\0crates/f\x80.rs\0".as_slice(),
            b"A\0crates/ok.rs\0M\0docs/\xff\xfe.md\0",
            b"M\0\xc3\0",
        ] {
            let error = statuses(listing).unwrap_err().to_string();
            assert_eq!(error, NOT_UTF8);
        }
        assert_eq!(paths(b"docs/\xff.md\0").unwrap_err().to_string(), NOT_UTF8);
        assert_eq!(
            paths("docs/\u{e9}t\u{e9}.md\0b\0".as_bytes()).unwrap(),
            ["docs/\u{e9}t\u{e9}.md", "b"]
        );
        let tree =
            b"100644 blob 78981922613b2afb6025042ff6bd878ac1994e85\tplan/tickets/M1-F\xff.md\0";
        assert_eq!(listed(tree).err().expect("not UTF-8").to_string(), NOT_UTF8);
    }

    #[test]
    fn a_tree_listing_gives_each_entry_its_path_its_object_and_its_kind() {
        let tree = [
            "100644 blob 78981922613b2afb6025042ff6bd878ac1994e85\tplan/tickets/M1 F1.md",
            "120000 blob 1916a083dfa163cf6c9c0a417959bb15bfd9f0f3\tplan/tickets/link",
            "160000 commit 3aab493e184d5faca798322c34bdc6eb9e6e3ce3\tvendor/sub",
            "",
        ]
        .join("\0");
        let entries = listed(tree.as_bytes()).unwrap();
        let read: Vec<(&str, bool)> = entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry.file))
            .collect();
        assert_eq!(
            read,
            [
                ("plan/tickets/M1 F1.md", true),
                ("plan/tickets/link", false),
                ("vendor/sub", false)
            ]
        );
        assert_eq!(
            entries[0].object,
            "78981922613b2afb6025042ff6bd878ac1994e85"
        );
        assert!(listed(b"100644 blob\0").is_err());
    }

    #[test]
    fn a_commit_name_that_could_be_an_option_is_refused() {
        let repo = Repo::workspace();
        for name in ["", "-h", "--all", "-", "HEAD x", "HEAD\n", "HE\u{e9}D"] {
            let refused = repo.commit(name).unwrap_err().to_string();
            assert!(refused.starts_with("a commit is named by"), "{name}");
        }
        assert!(repo.commit("HEAD").is_ok());
        let unknown = repo.commit("no-such-ref").unwrap_err().to_string();
        assert_eq!(unknown, "no-such-ref is not a commit in this checkout");
    }
}
