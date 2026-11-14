# One Launch (Rust)

The rewrite of One Launch (Electron) in Rust + GPUI: the same app for the user, lighter and faster. Windows only. No Rust exists yet: the repo holds the plan, the spec and the oracle seed. Start at `plan/PLAN.md` and read only the design or milestone file the task needs.

This file is the minimum that must hold from the first session. Milestone 1 replaces it with the full version outlined in `plan/design/workflow.md`.

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
- Never edit `spec/`, `oracle/`, a test, a triage row or a waiver to make a check pass. Spec amendments and differences from Electron follow `plan/design/workflow.md`.

## What must match

The owner's rule: the goal is the same working app in Rust, with the same features and flows and the same main UI, using less time and memory. The Electron app is a proof of concept to match in what it does for the user. It is not code to translate line by line.

- Do not carry over what is wrong, unneeded or fragile in the Electron code (a JavaScript quirk, a bug on odd input, dead code, a test of an implementation detail) when leaving it out changes nothing a user would notice.
- When the better way changes what the app does for the user, note it, use judgement, and ask the owner when it removes or changes something a user would notice or care about.
- Where an oracle artefact or a catalogued test would differ, the difference is recorded as `plan/design/workflow.md` › "Differences from Electron" says. An unrecorded difference is a defect.
- These stay exact whatever the Electron code looks like: the data folder, DB schema, vault format, installer identity and update feed (existing installs are taken over in place); the keys and values a switch writes to launcher-owned files and registry keys, with every other byte left alone; the standing rules above.
- Performance is the point of the rewrite. Where doing something Electron's way costs startup time, memory or idle CPU, take the lighter way if the user sees the same result.
