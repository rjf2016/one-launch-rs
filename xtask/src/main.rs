#![forbid(unsafe_code)]

mod check;
mod lints;
mod oracle;
mod secrets;
mod tools;
mod workspace;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask check [--tier 0|1]";

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
        Some((command, rest)) if command == "check" => check::run(rest),
        _ => anyhow::bail!(USAGE),
    }
}
