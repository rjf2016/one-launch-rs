use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub struct Workspace {
    pub members: Vec<Member>,
}

pub struct Member {
    pub name: String,
    pub dir: String,
    pub edges: Vec<Edge>,
    pub roots: Vec<PathBuf>,
    pub reach: BTreeSet<String>,
}

pub struct Edge {
    pub name: String,
    pub dev: bool,
}

impl Member {
    pub fn is_spike(&self) -> bool {
        self.dir.starts_with("spikes/")
    }
}

impl Workspace {
    pub fn load() -> Result<Self> {
        let json = output(
            "cargo",
            &[
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--all-features",
            ],
        )?;
        let metadata: Metadata =
            serde_json::from_slice(&json).context("cargo metadata printed something unexpected")?;
        let names: BTreeMap<&str, &str> = metadata
            .packages
            .iter()
            .map(|package| (package.id.as_str(), package.name.as_str()))
            .collect();
        let nodes: BTreeMap<&str, &[String]> = metadata
            .resolve
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.dependencies.as_slice()))
            .collect();

        let mut members = Vec::new();
        for package in &metadata.packages {
            if !metadata.workspace_members.contains(&package.id) {
                continue;
            }
            let manifest = Path::new(&package.manifest_path);
            let dir = manifest
                .parent()
                .and_then(|dir| dir.strip_prefix(&metadata.workspace_root).ok())
                .with_context(|| format!("{} is outside the workspace", package.manifest_path))?
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            members.push(Member {
                name: package.name.clone(),
                dir,
                edges: package
                    .dependencies
                    .iter()
                    .map(|dependency| Edge {
                        name: dependency.name.clone(),
                        dev: dependency.kind.as_deref() == Some("dev"),
                    })
                    .collect(),
                roots: package
                    .targets
                    .iter()
                    .map(|target| PathBuf::from(&target.src_path))
                    .collect(),
                reach: reach(&package.id, &nodes, &names),
            });
        }
        members.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { members })
    }

    pub fn gpui_free(&self) -> Vec<&str> {
        self.members
            .iter()
            .filter(|member| !member.is_spike() && !member.reach.iter().any(|name| is_gpui(name)))
            .map(|member| member.name.as_str())
            .collect()
    }
}

pub fn is_gpui(package: &str) -> bool {
    package == "gpui" || package.starts_with("gpui-") || package.starts_with("gpui_")
}

fn reach(
    from: &str,
    nodes: &BTreeMap<&str, &[String]>,
    names: &BTreeMap<&str, &str>,
) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut queue = vec![from];
    while let Some(id) = queue.pop() {
        for dependency in nodes.get(id).copied().unwrap_or_default() {
            if seen.insert(dependency.as_str()) {
                queue.push(dependency);
            }
        }
    }
    seen.into_iter()
        .filter_map(|id| names.get(id))
        .map(|name| (*name).to_owned())
        .collect()
}

pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one folder below the workspace root")
}

pub fn read(path: &str) -> Result<String> {
    std::fs::read_to_string(root().join(path)).with_context(|| format!("could not read {path}"))
}

#[allow(
    clippy::disallowed_methods,
    reason = "xtask is the crate that runs cargo and git"
)]
fn command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args).current_dir(root());
    command
}

pub fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = command(program, args)
        .status()
        .with_context(|| format!("could not start {program}"))?;
    if !status.success() {
        bail!("`{program} {}` failed", args.join(" "));
    }
    Ok(())
}

pub fn output(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = command(program, args)
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("could not start {program}"))?;
    if !output.status.success() {
        bail!("`{program} {}` failed", args.join(" "));
    }
    Ok(output.stdout)
}

pub fn files() -> Result<Vec<String>> {
    let listing = output(
        "git",
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    Ok(String::from_utf8_lossy(&listing)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: String,
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    id: String,
    manifest_path: String,
    dependencies: Vec<Dependency>,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

#[derive(Deserialize)]
struct Target {
    src_path: String,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    dependencies: Vec<String>,
}
