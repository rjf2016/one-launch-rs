# One Launch (Rust)

The rewrite of One Launch (Electron) in Rust + GPUI: the same app for the user, lighter and faster. Windows only. `README.md` says what each folder holds.

`plan/LEAN.md` is the plan: what counts, how the work goes and the slices. Of `plan/PLAN.md`, the design files, the spec and the Electron source, read only the part a task needs.

## Standing rules

- Never modify launcher-owned files or registry keys. In the finished app the only such writes live in the switch pipeline (`runSwitch` / `applyRestore` equivalents); don't add others.
- The owner performs all real account switches and launches. Never run one, never start or stop a launcher, never touch `%APPDATA%\OneLaunch` (it holds secrets). Run the app only with `--data-dir` on a scratch folder and fake launchers.
- Never run an installer on this machine. Installer runs happen on GitHub-hosted runners only.
- Never log, print, or commit secrets, emails, account ids, or session material. Fixtures and docs use placeholders.
- Riot Games: never touch a Riot game process or Vanguard (image names only; no `tasklist`, `taskkill`, WMI or `Get-Process` for anything Riot), never switch while a Riot game runs, never send anything to Riot's servers (the one local exception is the launch request to the client's own loopback server), never export a Riot session. The full rules are in the Electron repo at `docs/adapters/riot.md` and come before convenience.
- Dependencies: only the approved list in `plan/PLAN.md`. Anything else needs the owner's OK.
- Releases are the owner's. Never push a tag, create or publish a release, or handle signing keys.
- Commit on a branch, never on `main`. Work reaches `main` as a pull request that GitHub merges.

## The oracle

- The Electron source at `D:\Coding\slop\one-launch`, at the commit in `oracle/PIN`, is the reference for what the app does. It is read-only; never write Rust there.
- Expectations come from `oracle/`, captured from that source. An agent never writes or changes one.
- Never edit `spec/`, `oracle/` or a test to make a check pass.

## What must match

The owner's rule: the goal is the same working app in Rust, with the same features and flows and the same main UI, using less time and memory. The Electron app is a proof of concept to match in what it does for the user. It is not code to translate line by line.

- Do not carry over what is wrong, unneeded or fragile in the Electron code (a JavaScript quirk, a bug on odd input, dead code, a test of an implementation detail) when leaving it out changes nothing a user would notice.
- When the better way changes what the app does for the user, note it, use judgement, and ask the owner when it removes or changes something a user would notice or care about.
- Where the Rust app differs from an oracle artefact on purpose, the pull request says so and why. A difference nobody wrote down is a defect.
- These stay exact whatever the Electron code looks like: the data folder, DB schema, vault format, installer identity and update feed (existing installs are taken over in place); the keys and values a switch writes to launcher-owned files and registry keys, with every other byte left alone; the standing rules above.
- Performance is the point of the rewrite. Where doing something Electron's way costs startup time, memory or idle CPU, take the lighter way if the user sees the same result.

## How the work goes

- One slice at a time, on a branch, as a pull request. `plan/LEAN.md` lists the slices. There are no tickets.
- What counts, in the owner's order: the app works; it looks close to the original; nothing sensitive leaves this PC; the code is secure.
- A pull request that touches a path in `.github/CODEOWNERS` merges only with the owner's approval.
- Never merge a pull request. GitHub merges it, through auto-merge, requested while the required check is still running.
- Ask the owner before: a change a user would notice; a new dependency; anything that needs a real launcher, a real switch or the real data folder; a release.
- Work that is not the app (tooling, guardrails, checks) gets two rounds at most. Then the owner decides whether more is worth it.
- Check the UI by running it: capture the window and compare the capture with the screenshots in `oracle/ui/`. Close is the goal, not equal pixels.

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
- The app is `crates/ol-app`: `cargo run --locked -p ol-app -- --data-dir=<scratch folder>`, in that spelling; it refuses to start without it, and on the real data folder or a folder inside it. The folder, like the file of `--capture`, is written as a plain path on a lettered drive (`D:\folder`): a `\\?\` form, a share or a link to one is refused, because it cannot be told apart from the real data folder. Today the window shows the Library of that folder: its installed games with their stored artwork, the Recent strip, the search, the filter and the sort. A start writes to that folder: it makes the folder and `onelaunch.db` when they are missing, and copies the database to `backups\db` after the first paint. So give it a copy of `oracle/fixtures/data-folder` under `target/`, never the fixture itself. Started like this the app reaches no launcher, because no port to the real registry, process list or shell exists: a scan finds nothing, and a click on a game opens a dialog that says so.
- Fake launchers: `cargo run --locked -p ol-app --features fake -- --data-dir=<scratch>\data --fake-launchers=<scratch>\launchers --fake-scenario=<repo>\oracle\fixtures\dev-only\fake-launchers\all-four --fake-fixtures=<repo>\oracle\fixtures\launchers`, each folder as a plain path on a lettered drive. The window then scans, switches and launches Steam and Epic on a machine that exists only in the process and under `<scratch>\launchers`, in real time, and the title bar says FAKE LAUNCHERS. `--fake-launchers` names the folder of a laid-down scenario: `scenario.json`, and `knobs.json` to override its knobs, timings and process table (`spec/SPEC.md` Appendix A.2). The other two lay the scenario down there when the folder is new or empty, and are left out from then on. The folder is the fake launchers' own: one that holds anything but a scenario is refused. A data folder without accounts gets the scenario's accounts, settings and saved Epic sign-ins, made-up ones; a copy of `oracle/fixtures/data-folder` keeps its own. A build without the feature does not know the three arguments. The feature also adds `--fake-vault` to `--doctor`.
- `--doctor` reads the data folder without writing to it, prints counts and ok or failed only (schema version, accounts per launcher, games, settings, secrets, whether the vault key unwraps, how many encrypted values decrypt) and exits with 1 when a check fails. On a copy of `oracle/fixtures/data-folder` under `target/` it takes `--features fake` and `--fake-vault`, because the fixtures hold the stand-in ciphertext and no key.
- To see the window without looking at the screen: `--capture=<file.png>` writes it to a PNG and exits; keep captures under `target/`. Steps before the shot: `--capture-size=<width>x<height>` resizes the client area, `--capture-row=<n>` scrolls the grid, `--capture-type=<text>` types into the search field, `--capture-keys=<a,b>` presses keys, clicks or moves the pointer at a place of the client area for a step written `click:<x>x<y>` or `move:<x>x<y>`, and waits for a step written `wait:<milliseconds>` (a fake switch takes seconds), `--capture-sweep=<frames>` scrolls and prints frame times, `--capture-focus` takes the keyboard (a selection is only drawn in the active window). `--probe` checks the title bar, the frame, the minimum size and the tray, quits through the tray menu, and exits with 1 when a check fails.

## Docs and commits

- Change a doc only when a change makes it wrong or adds a rule or decision a later agent needs.
- A commit message is `<area>: <what> - <why, in plain words>`, in the voice of `git log`.
- No AI attribution anywhere: no `Co-authored-by` trailer, no "generated with" line, no session link, in a commit, a pull request or a file.
