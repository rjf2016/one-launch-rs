# One Launch (Rust)

The 1:1 rewrite of One Launch (Electron) in Rust + GPUI. Windows only. No Rust exists yet: the repo holds the plan, the spec and the oracle seed. Start at `plan/PLAN.md` and read only the design or milestone file the task needs.

This file is the minimum that must hold from the first session. Milestone 1 replaces it with the full version outlined in `plan/design/workflow.md`.

## Standing rules

- Never modify launcher-owned files or registry keys. In the finished app the only such writes live in the switch pipeline (`runSwitch` / `applyRestore` equivalents); don't add others.
- The owner performs all real account switches and launches. Never run one, never start or stop a launcher, never touch `%APPDATA%\OneLaunch` (it holds secrets). Run the app only with `--data-dir` on a scratch folder and fake launchers.
- Never run an installer on this machine. Installer runs happen on GitHub-hosted runners only.
- Never log, print, or commit secrets, emails, account ids, or session material. Fixtures and docs use placeholders.
- Riot Games: never touch a Riot game process or Vanguard (image names only; no `tasklist`, `taskkill`, WMI or `Get-Process` for anything Riot), never switch while a Riot game runs, never send anything to Riot's servers (the one local exception is the launch request to the client's own loopback server), never export a Riot session. The full rules are in the Electron repo at `docs/adapters/riot.md` and come before convenience.
- Dependencies: only the approved list in `plan/PLAN.md`. Anything else needs the owner's OK.
- Releases are the owner's. Never push a tag, create or publish a release, or handle signing keys.
- Commit only when the owner asks, until the bot workflow of Milestone 1 is in place.

## The oracle

- The Electron source at `D:\Coding\slop\one-launch`, at the commit in `oracle/PIN`, is the ground truth. It is read-only reference; never write Rust there.
- Never edit `spec/`, `oracle/`, a test or a waiver to make a check pass. Spec amendments and waivers follow `plan/design/workflow.md` and need the owner's approval.
- Behaviour is matched exactly. The only exceptions are rows the owner decided in `plan/triage.toml`.
