# One Launch (Rust): the lean plan

Decided by the owner on 2026-11-15. This page is the plan. It replaces the milestones, the ticket workflow and the gates in `PLAN.md`, `milestones/`, `tickets/` and `design/workflow.md`. Those files stay as reference for what they describe (the architecture, the approved dependencies, the takeover contract). They are no longer a process to follow.

## What counts

1. The app works: the same features and flows as the Electron app.
2. It looks close to the original. The screenshots in `oracle/ui/` are what the builder compares its own captures with while working. They are not a pass or fail gate.
3. Nothing sensitive leaves the owner's PC: no account id, password, launcher email or session material in the source, a fixture, a log, a commit or a pull request.
4. The code is secure.
5. It is lighter and faster than the Electron app.

## How 3 and 4 are kept

- The standing rules in `CLAUDE.md`.
- The safety hooks: an agent's command is read before it runs; the real data folder, real launchers and the app's key are refused; what a commit would hold is scanned before the commit.
- `CLAUDE.md`, the hooks in `.claude/` and the wrappers in `scripts/agent/` are in the main checkout on the owner's PC, where the agents run. Git ignores them, so the repository and a worktree do not hold them.
- CI scans every pull request for secrets, addresses and profile paths, and `cargo deny` holds the dependency list.
- An agent never opens the real data folder, nor a copy of it. Development runs on `oracle/fixtures/` and on scratch folders with fake launchers. When a slice has to be tried on real data, the owner runs the build on a copy of their own folder.
- A development build refuses to start without `--data-dir=<folder>`.
- Secrets stay in the vault format the Electron app uses, are wiped from memory after use and are never logged. `unsafe` only in `ol-platform-win`.
- Before the first release: one security review of the vault, the switch writes and the updater.

## How the work goes

- One slice at a time, on a branch, as a pull request opened by the app. CI must pass; GitHub merges it. A pull request that touches a path in `.github/CODEOWNERS` waits for the owner.
- Tests go where a wrong byte hurts: what a switch writes to launcher files and registry keys (`oracle/l1`, `oracle/l2`), and the vault and database formats (`oracle/fixtures`, `oracle/takeover`). The UI is checked by running it, capturing the window and comparing the capture with the oracle screenshots.
- When a slice lands, the owner gets a short list of what to try.
- The owner is asked before: a change a user would notice; a new dependency; anything that needs a real launcher, a real switch or the real data folder; a release.
- Work that is not the app (tooling, guardrails, checks) gets two rounds at most. Then the owner decides whether more is worth it.

## Slices

| # | Slice | Done when |
| --- | --- | --- |
| 1 | GPUI proof: a frameless title bar, the tray, a poster grid and a text field in one window | Each of the four works or has a named limit, and the owner says go or no-go on GPUI |
| 2 | Data: read a data folder (database, vault, settings); the Library shows its rows | The fixture folder reads correctly, and a secret written by Rust reads back |
| 3 | Steam end to end | Switch and launch pass on fakes against the oracle; the owner does one real switch |
| 4 | Epic, Battle.net and Riot | The same for each launcher; the Riot rules come first |
| 5 | The remaining screens, dialogs, settings and the tray menu | Every flow of the Electron app can be done |
| 6 | Installer, updater, taking over an existing install | A fresh install and an Electron-to-Rust takeover pass on a CI runner |
| 7 | Speed and memory against the Electron baseline; cutover | The owner says go |

A small number of beta testers have the Electron app installed. So slice 6 keeps the data folder, the database schema, the vault format, the installer identity and the update feed exact: their installs are taken over in place, with their saved accounts intact.

## Dropped

Tickets and write scopes; the verifier role and its rounds; the policy check on pull requests (#9, closed); the ledger; waves and slots; pixel tolerance as a gate; the spike sheet; the remaining Milestone 1 guardrail tickets (F6d, F6e, F7a to F7c).
