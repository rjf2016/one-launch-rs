use std::collections::BTreeMap;
use std::path::Path;

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
        let mut all = vec!["--no-optional-locks", "-C", self.dir.as_str()];
        all.extend(args);
        workspace::output("git", &all)
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
                for path in records(&untracked) {
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
        let listing = self.git(&["ls-tree", "-r", "-z", "--full-tree", commit, "--", under])?;
        records(&listing)
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

fn records(listing: &[u8]) -> Vec<String> {
    text(listing)
        .split('\0')
        .filter(|record| !record.is_empty())
        .map(str::to_owned)
        .collect()
}

fn statuses(listing: &[u8]) -> Result<Vec<(char, String)>> {
    let fields = records(listing);
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
