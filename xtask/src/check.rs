use anyhow::{Result, bail};

use crate::workspace::{self, Workspace};
use crate::{USAGE, lints, oracle, secrets, tools};

type Step = (&'static str, fn(&Workspace) -> Result<()>);

const TIER_0: [Step; 7] = [
    ("fmt", fmt),
    ("manifests", |workspace| {
        report(lints::manifests::check(workspace)?)
    }),
    ("crate graph", |workspace| {
        report(lints::graph::check(workspace)?)
    }),
    ("forbid unsafe", |workspace| {
        report(lints::unsafe_code::check(workspace)?)
    }),
    ("secrets", |_| report(secrets::check(&workspace::files()?)?)),
    ("oracle manifest", |_| {
        report(oracle::check(&workspace::files()?)?)
    }),
    ("deny", deny),
];

const TIER_1: [Step; 3] = [("clippy", clippy), ("build", build), ("nextest", nextest)];

pub fn run(args: &[String]) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let steps: Vec<Step> = match args.as_slice() {
        [] => TIER_0.into_iter().chain(TIER_1).collect(),
        ["--tier", "0"] => TIER_0.to_vec(),
        ["--tier", "1"] => TIER_1.to_vec(),
        _ => bail!(USAGE),
    };

    let workspace = Workspace::load()?;
    let mut failed = Vec::new();
    for (name, step) in steps {
        println!("== {name}");
        if let Err(error) = step(&workspace) {
            println!("{error:#}");
            failed.push(name);
        }
    }
    if !failed.is_empty() {
        bail!("check failed: {}", failed.join(", "));
    }
    println!("== check passed");
    Ok(())
}

fn report(problems: Vec<String>) -> Result<()> {
    if problems.is_empty() {
        return Ok(());
    }
    bail!("{}", problems.join("\n"));
}

fn fmt(_: &Workspace) -> Result<()> {
    workspace::run("cargo", &["fmt", "--all", "--check"])
}

fn deny(_: &Workspace) -> Result<()> {
    tools::require("cargo-deny")?;
    workspace::run(
        "cargo",
        &["deny", "--locked", "check", "bans", "licenses", "sources"],
    )
}

fn clippy(workspace: &Workspace) -> Result<()> {
    let mut args = vec!["clippy", "--locked", "--all-targets"];
    args.extend(packages(&workspace.gpui_free()));
    args.extend(["--", "-D", "warnings"]);
    workspace::run("cargo", &args)
}

fn build(workspace: &Workspace) -> Result<()> {
    let mut args = vec!["build", "--locked"];
    args.extend(packages(&without_xtask(workspace)));
    workspace::run("cargo", &args)
}

fn nextest(workspace: &Workspace) -> Result<()> {
    tools::require("cargo-nextest")?;
    workspace::run("cargo", &["nextest", "run", "--locked", "-p", "xtask"])?;
    let mut args = vec!["nextest", "run", "--locked", "--no-tests=warn"];
    args.extend(packages(&without_xtask(workspace)));
    workspace::run("cargo", &args)
}

// xtask.exe is running: a build that picked other features for its dependencies would have to replace it.
fn without_xtask(workspace: &Workspace) -> Vec<&str> {
    let mut names = workspace.gpui_free();
    names.retain(|name| *name != "xtask");
    names
}

fn packages<'a>(names: &[&'a str]) -> Vec<&'a str> {
    names.iter().flat_map(|name| ["-p", *name]).collect()
}
