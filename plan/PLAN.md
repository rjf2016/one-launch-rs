# One Launch: Rust + GPUI rewrite plan

## Context

One Launch is a Windows Electron app (about 28k lines of TypeScript, 1031 tests) at `1.0.0-beta.3`. The decision is made to rewrite it in Rust with GPUI, 1:1, lighter and faster, with all the work done by AI agents. This plan says what the agents check their work against, how the work is cut up, who does what, and where you step in.

What exists today:

- `D:\Coding\slop\one-launch`: the Electron app, public on GitHub. Master is at `f25111b` (merge of PR #36), clean, and nothing has landed since the spec was written.
- `D:\Coding\slop\rewrite-prep`: `SPEC.md` (3,858 lines, 1,296 citations, 35 open questions in §12), `SCREENSHOTS.md` (174 states, no images yet), `fixtures/` (89 sample inputs, no expected outputs).
- No Rust toolchain on this machine yet. VS 2022 Build Tools are installed.

The central idea: the frozen Electron app is the **oracle**. Before any Rust is written, agents capture machine-readable reference data from it. After that, "done" is decided by diffs against that data, by CI, never by an agent's say-so.

## Decisions

Made by you in this session:

| Question | Decision |
| --- | --- |
| Reference for agents | Spec plus the frozen Electron source, read-only |
| Existing installs | In-place takeover: same data folder, DB, vault format and installer identity; the current auto-updater delivers the Rust build |
| Known bugs and dead code | Match behaviour exactly; one keep-or-drop triage table approved before any Rust is written |
| Review | Agents review and merge their own PRs; you review at milestone gates |
| Agent login | A separate bot account, scoped to the new repo |
| Installer runs | Agents may run installers on GitHub-hosted runners only, never on your PC |
| New repo | Public and separate until cutover (working name `one-launch-rs`, at `D:\Coding\slop\one-launch-rs`) |
| Electron app meanwhile | Critical fixes only; each one re-captures the affected reference data and amends the spec |

My recommended answers to the spec's open product questions (§12.1), approved with this plan:

- Fake launchers stay, behind a cargo feature that release builds never enable.
- Polling is replaced by push inside the app; every user-visible timing stays.
- Registry and process access go native (no `reg.exe`, no PowerShell), under the same no-handle rule and the same Riot rules.
- The updater flow, the two pinned keys and the signed `latest.yml` contract stay as they are. Your tag, sign and publish runbook does not change.
- The DB stays at schema v5 and the vault stays in Electron's format (read and write) until cutover is declared stable, so going back to the Electron build always works.
- Settings stay string key/value on disk, with typed accessors in Rust.
- A full copy inventory is extracted automatically in Milestone 0.

## What 1:1 is checked against

| Check | What is compared | Covers |
| --- | --- | --- |
| Vectors | Output of every pure TS function on fixed inputs, against the Rust port. `xtask probe` adds differential fuzzing against the live TS | Parsers, byte-exact rewriters, redaction, update manifest, request validators |
| Ported tests | All 1031 tests are catalogued; each maps to a Rust test or to a waiver you approved | Adapter flows, services |
| Replay | 22 scripted scenarios run headlessly on Electron's real services with fake launchers. The Rust backend replays the same script; job states, the ordered platform operations (registry writes, file writes, process starts and closes) and the final data folder must match | The whole backend |
| UI parity | 174 screenshots, each with a layout file listing every text run and control (role, name, box, style). The Rust app must match text exactly and boxes within 2 px | Every screen |
| Takeover rehearsal | On a GitHub-hosted VM: install the public Electron release, seed data, run the Rust installer with the exact arguments Electron's updater uses, then check files, registry, shortcuts, vault and DB | Installer, updater, data compatibility |
| Real steps | Yours, at gates: real switches and one real update | What fakes cannot show |

Two limits to know up front:

- **Pixels are not a gate.** Chromium and DirectWrite rasterise text differently, so pixel-identical is not achievable. The gate is layout, copy, colours and timings; screenshots are review material. A few CSS effects (backdrop blur, inset shadow, scale transforms, colour transitions) may have no GPUI equivalent; each confirmed gap becomes a triage row for you.
- **The native registry and process layer has no fake to diff against.** It is covered by the ported read-only `*-live` tests on your machine and by a CI trace job asserting that process listing opens no handle.

## Target architecture

One Cargo workspace. Only four crates depend on GPUI, so most tickets build and test in seconds.

| Crate | Holds |
| --- | --- |
| `ol-model`, `ol-formats` | Types, error codes, DTOs; byte-preserving VDF, INI, Battle.net config, Riot YAML and log parsers |
| `ol-platform-api`, `ol-runtime` | Traits for registry, processes, shell, clipboard, DPAPI, HTTP, clock; cancel tokens, waits, virtual clock |
| `ol-store`, `ol-vault` | SQLite with migrations 1 to 5 verbatim; Electron-compatible vault |
| `ol-adapters`, `ol-services`, `ol-api` | Four launcher adapters; launch job, journal, backups, artwork, updates; the command surface |
| `ol-platform-win`, `ol-net` | Real Windows and network implementations. The only crate allowed `unsafe` is `ol-platform-win` |
| `ol-platform-fake`, `ol-sim` | Fake platform and the four launcher simulators (feature `fake`) |
| `ol-ui-kit`, `ol-ui`, `ol-automation`, `ol-app` | GPUI: components, screens, the dev-only automation pipe, `OneLaunch.exe` |
| `ol-testkit`, `ol-replay`, `xtask`, `installer/` | Vector loader, headless replay runner, tooling, NSIS script |

Key choices:

- **One command surface.** `ol-api` keeps the former IPC channels as `dispatch_json(channel, value)`. The UI calls through it, so replay exercises the same path the UI uses.
- **Backend on one thread** with a single-threaded async executor, which mirrors how the TypeScript interleaves today. Tests run it on a virtual clock, so replays are deterministic.
- **GPUI Kit only for text input.** Dropdown, menus, dialogs, tabs, title bar and the rest are hand-built to match; the originals are small and their rules are exact.
- **Automation pipe.** A dev-only named pipe exposes the element tree, click, type, wait and screenshot. This replaces the CDP driver. A CI check proves it is absent from release builds.
- **Installer.** electron-builder's NSIS templates are vendored and frozen with today's settings, so the first Rust installer satisfies the contract the Electron updater expects (same GUID key, uninstaller name, arguments, Run value handling).
- **GPUI pin.** One exact version, upgraded only by a deliberate ticket at a milestone boundary.

## Milestones

Expect 150 to 250 pull requests in total, not the "dozens" I said earlier.

| # | Goal | Exit gate | Your part |
| --- | --- | --- | --- |
| M0 Oracle | Reference bundle captured from Electron; triage table | Every capture repeats identically; verify script passes | Approve triage table and waiver categories; about 12 manual captures (tray, installer pages) |
| M1 Foundation | Repo, CI, guardrails, 11 spikes, a walking skeleton (fixture DB to Library grid in a real window to one compared screenshot), first takeover rehearsal | Every spike has a pass or a chosen fallback | GPUI go/no-go from the spike sheet; read a one-page architecture |
| M2 Core | Models, parsers, DB, vault, logger, native registry and process layer, fakes | All vectors pass; trace shows zero handle opens; an Electron-written folder round-trips | Run `doctor` on a copy of your data folder (prints counts only) |
| M3 Steam slice | Steam end to end: adapter, simulator, launch service, replay, Play through the UI | Steam replays match; Play works in the window | Ten-minute demo; one real Steam switch on a scratch data folder |
| M4 Backend | Epic, Battle.net and Riot in parallel; remaining services; all commands; UI kit gallery | Zero unexplained replay diffs; gallery within tolerance | One real switch per remaining launcher; look at the gallery at 125% and 150% |
| M5 Screens | Every view and dialog, tray, window rules, all 30 scenarios through the UI | All scenarios pass on fakes; all 174 states within tolerance or triaged | Demo; four real switches through the UI |
| M6 Release | Updater, installer, release workflow, sign-tool port | Contract suite passes on CI: fresh install, Electron to Rust, Rust to Rust, Rust to Electron | One real update on your install; tag, sign, publish |
| M7 Cutover | Parity audit, performance against the Electron baseline, soak | Ledger fully covered or waived; budgets met | Go/no-go; move the tree into `one-launch` |

The UI kit and shell are a parallel track from M1 to M4, since they need only frozen DTOs and a mock backend. Installer and updater tickets may also start after M1.

## The workflow

### Roles

Each is a fresh-context agent. Nobody checks their own work.

| Role | Does | Never |
| --- | --- | --- |
| Orchestrator (your session, `/run-wave`) | Picks ready tickets, starts the Workflow script, writes the wave summary | Writes code or reads diffs |
| Ticket-prep | Writes the ticket and its failing tests from the oracle | Implements |
| Implementer | One ticket, inside its declared files | Edits tests, spec or oracle to get green |
| Verifier | Re-runs, mutates, probes, compares against the TS; issues a verdict | Edits anything (no write tools) |
| Diagnostician | Classifies a ticket that failed three rounds | |
| Auditors | Architecture (every 10 merges and before a gate), parity sample (10% of covered rows re-checked against the TS), safety (whole codebase before a gate) | |

Copilot review stays on as a second model's opinion; the verifier triages its comments.

### One ticket, start to finish

1. **Prep.** A ticket file lands in `plan/tickets/` with: spec sections, Electron source files, acceptance ids (vectors, tests, replays, shots, probes), the files it may touch, a size budget (about 350 source lines) and a risk class. Its red tests land with it. The ticket is immutable afterwards.
2. **Schedule.** `xtask plan next` offers tickets whose dependencies are merged and whose file scopes do not overlap anything running. Three run at once, one GPUI build at a time.
3. **Implement.** The agent works in one of three long-lived checkouts with warm build caches, commits per acceptance id, and opens a PR as the bot with a machine-readable evidence block.
4. **CI recomputes the evidence** and fails on any mismatch.
5. **Verify.** The verifier gets the ticket, the diff and CI artefacts, not the implementer's explanation. It must produce: an assert-by-assert comparison with the TS originals, at least 3 mutation receipts (break the code, name the test that goes red; 6 for safety tickets), 200 differential probes per listed function, and a list of every new call into registry, process, launcher-file or network APIs mapped to a spec clause. For UI tickets it drives the scenario itself. CI re-runs its receipts.
6. **Merge.** On a pass, auto-merge is enabled and GitHub squash-merges once required checks are green. No agent merges directly. On a fail, the implementer gets a fix round, three at most.
7. **Park.** After three failed rounds the diagnostician classifies the cause and proposes a split or a spec amendment. One retry with a fresh agent, then it comes to you.
8. **Wave end.** At most ten tickets per orchestrator session, then a summary and a new session.
9. **Milestone end.** Audits run, the gate report is written, you do your part, and a retro updates `CLAUDE.md` and the GPUI cookbook.

### Guardrails that are code, not prompts

- **Protected paths** need your approval as code owner: `spec/`, `oracle/`, `.github/`, `.claude/`, `deny.toml`, `plan/triage.toml`, `plan/waivers.toml`, and the policy code in `xtask`.
- **Spec amendments** are their own PRs (clause, Electron citation, a new vector generated from Electron) and may not touch `crates/`.
- **The ledger is computed.** A row counts as covered only when CI finds a tagged test that ran and passed. Covered never falls; waivers never rise without you.
- **Test integrity.** Removing, renaming or ignoring a test fails CI, as does a drop in a test's assertion count. The oracle's hash manifest is verified.
- **Verifier canaries.** About one verifier run in ten gets a diff with a planted defect. Passing it stops the wave.
- **Scope.** A hook rejects edits outside the ticket's files; CI re-checks; 15 files per PR at most.
- **Dependencies.** The approved set is resolved once in M1. `cargo add`, `update` and `install` are blocked; anything new is a `deps` ticket with your OK.
- **Safety hooks** block process tools, the real data folder, starting a launcher or installer, registry writes, tags, releases, direct merges, force-push, and running the app without `--data-dir`.
- **CI lints.** `unsafe` only in `ol-platform-win`; banned crates (`sysinfo`, `wmi` and others that open process handles); a network host allowlist; no automation or fake-launcher strings in the release exe; no AI attribution.
- **Compile-time rule.** Writing a launcher-owned file or key requires a token only the switch pipeline can construct.
- **Data-folder latch.** Until you flip it in M6, the Rust binary refuses the default `%APPDATA%\OneLaunch`.

### Where state lives

- `main`: code, spec, oracle, tickets, triage, waivers, cookbook.
- `ops` (orphan branch, like `pr-assets` today): per-ticket state, verdicts, wave summaries, gate reports.
- Any fresh session resumes from GitHub PR state first, then `ops`.

### Agents stop and ask you when

- A safety hook or lint trips.
- Electron contradicts the spec or the triage table.
- A dependency or protected-path change is needed.
- Three parked tickets share a cause, or a quarter of a wave parks.
- A canary passes the verifier, or main goes red and the auto-revert fails.

## Milestone 0 in detail

Runs in the Electron repo on a branch `oracle-tooling` that never merges and never ships. The only `src/` changes are about 60 lines of recording hooks that do nothing unless a dev-only variable is set.

| # | Ticket | Done when |
| --- | --- | --- |
| T0 | Branch, schemas, normaliser, verify script with secret scan | The check passes under the oracle clock (`plan/milestones/M0.md`); `src/` unchanged |
| T1 | Triage table: one row per §11 and §12 item, plus the rows found during planning | Every row has your decision |
| T2 | Test catalogue and result goldens | Count equals the test reporter's total |
| T3 | Vectors (five agents: four launchers plus core) | Every listed function has cases; two runs byte-identical |
| T4 | Recording hooks; spike of the CDP layout capture | With the variable unset, check passes and `play-all --fake` leaves no trace file |
| T5 | Headless replay harness and 22 scripts | Five runs identical; five scripts cross-checked through real Electron |
| T6 | Update transcripts for six feed conditions | Three runs identical |
| T7 | Screenshot and layout capture (five agents by area) | All 174 rows captured, waived or marked yours; no user paths in any file |
| T8 | Copy inventory, style tables, tokens, 56 icons, What's New data | Every observed text run maps to a source string |
| T9 | Your captures: tray, native dialogs, installer pages, window chrome | Captured or waived |
| T10 | Performance baseline: warm starts, memory, idle CPU, install size | Numbers for every metric |
| T11 | Takeover contract and vault fixtures | Installer contract written down; vault cross-read vector exists |
| T12 | Verify and self-diff demo | Verify passes; gate report ready |

T1, T2, T3, T4, T10 and T11 run in parallel after T0. T5 to T8 follow T4.

Triage rows already found, beyond §11 and §12: where the two sidebar and sort preferences live (today in Chromium's localStorage), system-locale relative dates, the stock installer's WMI process check, a first-run backup of the DB and vault key before the Rust app writes, and the dropped channels `app:ping` and `launchers:games`.

## Milestone 1 spikes

Each ends in a short decision record: pass, or the named fallback.

| Spike | Pass means |
| --- | --- |
| Tray, close-to-tray, hidden start | Starts with no window; tray click shows it; close hides and keeps view state |
| Frameless title bar | Drag, double-click maximise, Windows 11 snap flyout, resize edges, 960x600 minimum |
| Poster grid | 500 cards, smooth scrolling, memory under 250 MB, no decoding on the UI thread |
| Input and focus | Masked input with reveal, IME, clipboard, Tab order, modal focus trap |
| Automation and capture | Element tree with bounds and text; click and type without focus; correct capture when the window is covered |
| Vault and DB | A folder written by Electron decrypts fully in Rust, and the reverse |
| Process listing | Paths for all processes; the trace shows zero handle opens |
| Build metrics | Build time, exe size, first frame and idle memory measured; budgets set against the Electron baseline |
| CI window | A GPUI window renders and captures on `windows-latest`, five runs out of five |
| Takeover dry run | The installer contract passes on a hosted runner against the public Electron release |
| Text metrics | Sample strings land within 1 px per 100 px of Chromium's widths |

## One-time setup and approved dependencies

Yours, before M1:

- Create the public repo and the bot account, with a token limited to that repo (contents and pull requests; no admin, no workflow edits, no releases).
- Rulesets: PRs only, one required aggregate check, code-owner review on protected paths, no bypass.

Tools, listed once per your usual rule: rustup, NSIS, `sccache`, `cargo-nextest`, `cargo-deny`, `cargo-mutants`, `cargo-about`. Approving this plan approves the list; I install the user-level ones at the start of M1, and anything that needs elevation is yours.

Approved crates: `gpui-pre` and `gpui-pre-platform` (exact pin), `gpui-kit` (input only), `rusqlite` (bundled), `windows`, `serde`, `serde_json`, `aes-gcm`, `sha2`, `base64`, `ed25519-dalek`, `getrandom`, `zeroize`, `uuid`, `time`, `semver`, `regex`, `thiserror`, `anyhow`, `futures-lite`, `async-channel`, `async-task`, `async-trait`, `event-listener`, `ureq`, `image`, `winresource`; `tray-icon` and `muda` only if the tray spike picks them.

Banned: `sysinfo`, `wmi`, `tokio`, `reqwest`, generic VDF, INI and YAML parsers (the byte-preserving ports replace them), third-party updaters, and crates.io `gpui` 0.2.x.

## Risks

| Risk | Containment |
| --- | --- |
| Takeover is one-way: publishing reaches every install at once | Rehearsal on CI from M1; schema and vault frozen; the installer contract is symmetric, so an Electron build can replace a Rust one; you update first |
| GPUI and its kit change weekly, and `gpui-pre` is a third-party snapshot | Exact pins, GPUI in four crates, kit used for input only, M1 is the go/no-go |
| Two implementations agree on the wrong thing | Expectations are generated from the TS, never hand-written by the implementer; differential probes; parity sample audits |
| A transitive crate opens a handle to a Riot process | Ban list, trace job in CI, image-name guard at the lowest layer |
| Agents weaken checks to get green | Protected paths, computed ledger, test-integrity checks, canaries |
| Nobody human reads the code between gates | Architecture audits every ten merges; crate graph and public API snapshot enforced in CI |

## Verification

- **Per ticket:** the acceptance ids in the ticket, recomputed by CI, plus the verifier's receipts.
- **Per milestone:** the exit gate in the table above, the three audits, and your gate steps.
- **Whole rewrite:** every ledger row covered or waived; all 22 replays and 30 UI scenarios match; all 174 states within tolerance or triaged; the contract suite passes in all four directions; performance budgets met against the M0 baseline; your real update and real switches pass.

## Kickoff after approval

In this session, nothing is pushed and nothing is installed:

1. Create `D:\Coding\slop\one-launch-rs` as a local git repo holding this plan as `plan/PLAN.md`, the milestone files, the ticket template, `spec/SPEC.md` and `oracle/` seeded from `rewrite-prep`.
2. Draft `plan/triage.toml` from spec §11 and §12 plus the rows above, each with a recommendation, for your decisions.
3. Write the M0 tickets.

Then it waits on you for the triage decisions, the repo and the bot account. M0 starts in a fresh session in the Electron repo.
