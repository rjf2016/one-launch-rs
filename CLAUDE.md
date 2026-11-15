# One Launch (Rust)

The rewrite of One Launch (Electron) in Rust + GPUI: the same app for the user, lighter and faster. Windows only. `README.md` says what each folder holds.

With a ticket, read it and only what its `read` field cites. Without one, start at `plan/PLAN.md` and read only the design or milestone file the task needs.

## Standing rules

- Never modify launcher-owned files or registry keys. In the finished app the only such writes live in the switch pipeline (`runSwitch` / `applyRestore` equivalents); don't add others.
- The owner performs all real account switches and launches. Never run one, never start or stop a launcher, never touch `%APPDATA%\OneLaunch` (it holds secrets). Run the app only with `--data-dir` on a scratch folder and fake launchers.
- Never run an installer on this machine. Installer runs happen on GitHub-hosted runners only.
- Never log, print, or commit secrets, emails, account ids, or session material. Fixtures and docs use placeholders.
- Riot Games: never touch a Riot game process or Vanguard (image names only; no `tasklist`, `taskkill`, WMI or `Get-Process` for anything Riot), never switch while a Riot game runs, never send anything to Riot's servers (the one local exception is the launch request to the client's own loopback server), never export a Riot session. The full rules are in the Electron repo at `docs/adapters/riot.md` and come before convenience.
- Dependencies: only the approved list in `plan/PLAN.md`. Anything else needs the owner's OK.
- Releases are the owner's. Never push a tag, create or publish a release, or handle signing keys.
- Commit only when the owner asks, until the agent workflow of Milestone 1 is in place.

## The oracle

- The Electron source at `D:\Coding\slop\one-launch`, at the commit in `oracle/PIN`, is the reference for what the app does. It is read-only; never write Rust there.
- Expectations come from `oracle/`, captured from that source. The implementer never writes or changes one.
- Never edit `spec/`, `oracle/`, a test, a triage row or a waiver to make a check pass. Spec amendments and differences from Electron follow `plan/design/workflow.md`.

## What must match

The owner's rule: the goal is the same working app in Rust, with the same features and flows and the same main UI, using less time and memory. The Electron app is a proof of concept to match in what it does for the user. It is not code to translate line by line.

- Do not carry over what is wrong, unneeded or fragile in the Electron code (a JavaScript quirk, a bug on odd input, dead code, a test of an implementation detail) when leaving it out changes nothing a user would notice.
- When the better way changes what the app does for the user, note it, use judgement, and ask the owner when it removes or changes something a user would notice or care about.
- Where an oracle artefact or a catalogued test would differ, the difference is recorded as `plan/design/workflow.md` › "Differences from Electron" says. An unrecorded difference is a defect.
- These stay exact whatever the Electron code looks like: the data folder, DB schema, vault format, installer identity and update feed (existing installs are taken over in place); the keys and values a switch writes to launcher-owned files and registry keys, with every other byte left alone; the standing rules above.
- Performance is the point of the rewrite. Where doing something Electron's way costs startup time, memory or idle CPU, take the lighter way if the user sees the same result.

## Tickets

- One ticket per agent. The ticket is a file in `plan/tickets/` and is immutable: never edit one.
- Its `write_scope` is the complete list of paths you may create or change. If the ticket cannot be met inside it, stop and report.
- A pull request that touches a path in `.github/CODEOWNERS` merges only with the owner's approval. A ticket may name such a path; its pull request then waits for the owner.
- Never merge a pull request. GitHub merges it, through auto-merge; `plan/design/workflow.md` › "Identity and merging" says when that may be requested.
- Stop and ask the owner in the cases `plan/design/workflow.md` lists under "Stop and ask the owner when".

## GitHub

- Act on GitHub only as the app, through `pwsh -NoProfile -File scripts/agent/git.ps1 <git arguments>` and `pwsh -NoProfile -File scripts/agent/gh.ps1 <gh arguments>`. Commit through the first as well, so the commit carries the app's name.
- On the owner's PC a plain `gh` and a plain `git push` act as the owner, with admin rights. Never use them: a plain `gh` not even to read.
- The app cannot push a change under `.github/workflows/`. Commit it and stop: `plan/design/workflow.md` › "Acting as the app".

## Rust

- `unsafe` only in `ol-platform-win`. Every other crate root starts with `#![forbid(unsafe_code)]`; the spikes are exempt.
- Errors are typed, with `thiserror`. `anyhow` only in binaries.
- No comments by default. The exception: one line above code whose intent is not obvious, saying why.
- Take a dependency from `[workspace.dependencies]` in `Cargo.toml`. Never run `cargo add`, `cargo update` or `cargo install`; builds and tests take `--locked`.
- The crate graph is the table in `plan/design/architecture.md`. Only `ol-ui-kit`, `ol-ui`, `ol-automation` and `ol-app` reach GPUI.

## GPUI

- Never write GPUI from memory. Copy from an entry in `docs/cookbook/`, or read the pinned source in the cargo registry and cite the file. `Cargo.toml` and `plan/decisions/M1-gpui-pin.md` hold the pins.
- `plan/design/gpui-notes.md` is unverified, apart from what it says a decision record confirmed.
- One GPUI build at a time on the owner's PC.

## Commands

- `cargo xtask check`: everything a pull request must pass. `--tier 0` runs formatting, the lints, the secret scan, the oracle's hashes and `cargo deny`; `--tier 1` runs clippy, build and tests of the GPUI-free crates.
- One crate's tests: `cargo nextest run --locked -p <crate>`.
- The app is `crates/ol-app`: `cargo run --locked -p ol-app --features fake -- --data-dir <scratch folder>`. The feature `fake` holds the fake launchers. Today its `main` is empty: it reads no argument and exits at once.

## Docs and commits

- Change a doc only when a change makes it wrong or adds a rule or decision a later agent needs.
- A commit message is `<ticket id>: <what> - <why, in plain words>`, in the voice of `git log`.
- No AI attribution anywhere: no `Co-authored-by` trailer, no "generated with" line, no session link, in a commit, a pull request or a file.
