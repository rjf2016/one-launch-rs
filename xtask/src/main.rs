#![forbid(unsafe_code)]

mod check;
mod lints;
mod oracle;
mod policy;
mod secrets;
mod tools;
mod workspace;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask check [--tier 0|1]
       cargo xtask check --ticket <id> [--base <ref>]
       cargo xtask policy --base <ref> --event <file> [--reviews <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> anyhow::Result<()> {
    match args.split_first() {
        Some((command, rest)) if command == "check" && rest.iter().any(|arg| arg == "--ticket") => {
            policy::check(rest)
        }
        Some((command, rest)) if command == "check" => check::run(rest),
        Some((command, rest)) if command == "policy" => policy::run(rest),
        _ => anyhow::bail!(USAGE),
    }
}
