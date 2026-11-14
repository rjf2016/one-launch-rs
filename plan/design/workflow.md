# Workflow

How agents do the work and what keeps it honest. The summary is in `../PLAN.md`; this file holds the detail Milestone 1 turns into hooks, CI and skills. GitHub facts here (code-owner rules with zero required approvals, auto-merge behaviour) are to be verified during Milestone 1 setup.

Principle: if a rule can be code, it is not a prompt.

## Identity and merging

- Agents act as the GitHub App `one-launch-agents` (App ID 5752844), installed on this repo only: contents and pull requests, nothing else. No admin, no workflow-file pushes. Its tokens last one hour and are minted from a key the owner keeps outside every repo, in `D:\Coding\slop\one-launch-rs-bot`. Its commits carry `356454087+one-launch-agents[bot]@users.noreply.github.com`.
- The app replaced the plan's bot account at the M0 gate: a bot account's token cannot be limited to one repo under a personal account, and an app needs no second account. If the token step cannot be made to work in Milestone 1, the fallback is a bot account in an organization that owns the repo.
- Contents access would let the app create a release, so a tag ruleset only the owner bypasses blocks every tag creation, update and deletion.
- GitHub merges, not an agent: auto-merge (squash) on required checks. `gh pr merge` without `--auto` is hook-blocked.
- One aggregate required check (`gate`). Required checks are pinned to the Actions app. The owner adds it to the `main` ruleset once CI reports it; until then the ruleset holds pull requests only, squash merges, code-owner review and no bypass.
- No strict up-to-date rule. Main runs the full suite after each merge; a red main is auto-reverted and stops the line.
- Existing installs update from the old repo's releases. The app is not installed there, so agents cannot ship to users.

### Acting as the app

Checked in Milestone 1 (`../decisions/M1-F0.md`).

- `scripts/agent/git.ps1 <git arguments>` runs git with the app's name and address, over HTTPS with SSH switched off, with a credential helper that mints a token when GitHub asks for one, and with every extra HTTP header from git's config cleared, so no other credential can ride along. `scripts/agent/gh.ps1 <gh arguments>` runs gh with a fresh token. A token reaches this repo only, with contents and pull requests, and lasts one hour. None is written to disk or printed, and neither wrapper leaves a variable changed in the session that called it.
- `scripts/agent/check.ps1` reads back the app's permissions and every repo it is installed on, and fails on any difference from the first bullet of this section.
- On the owner's PC a plain `gh` and a plain `git push` act as the owner, with admin rights. Agents use the two wrappers for everything that reaches GitHub; the hooks of F6 enforce it.
- Workflow files: GitHub refuses any push from the app that creates or changes a file under `.github/workflows/`. The agent commits the change on its branch and stops. The owner pushes that commit (`git push origin <branch>`), the app opens the pull request, and the owner approves it as code owner. An agent never pushes with the owner's login.

## Owner-approved paths

CODEOWNERS plus ruleset: `spec/`, `oracle/`, `.github/`, `.claude/`, `deny.toml`, and the policy code in `xtask`.

`plan/triage.toml` and `plan/waivers.toml` are guarded by a policy check instead: a row with `source` `""` or `"plan"` and a waiver category change only with the owner's approval; agents add `delegated` rows and waivers as the next section says.

## Differences from Electron

The owner's rule of 2026-11-13: the Rust app is the same app for the user (same features and flows, same main UI, lighter and faster), not a translation of the Electron code. The Electron app is a proof of concept. Where its code is wrong, unneeded or fragile, the port does the better thing.

| The better way... | Examples | Who decides | Record |
| --- | --- | --- | --- |
| changes no oracle artefact and no catalogued test | internal structure, a lighter algorithm, push in place of polling | the implementer | none |
| changes an artefact or a test, and nothing a user would notice | a JavaScript quirk on malformed input, the engine's own error text, a bug on odd input, dead code, a test of an implementation detail | ticket-prep, or the diagnostician for a parked ticket | a `delegated` row in `plan/triage.toml` with `affects`; a waiver for a test that is not ported |
| changes what the app does for the user | a flow, a setting, copy or layout on a main screen, a timing the user sees | the same, by judgement; the owner is asked first when it removes or changes something a user would notice or care about | the same row, with what the user would notice in `why` |

What keeps this honest:

- An implementer never adds or edits a row or a waiver. If its ticket cannot pass without one, it stops and the ticket goes back to prep or parks.
- A PR that touches `plan/triage.toml` or `plan/waivers.toml` may not touch `crates/`.
- The verifier treats a recorded difference as expected and anything unrecorded as a failure. For each row a ticket cites it checks that the Rust behaviour is what the row says.
- The gate report lists every row and waiver added in the milestone, those with a user-visible effect first. The owner may overrule any; an overruled row becomes a ticket.
- Not open to judgement: what existing installs depend on (`takeover.md`), the keys and values a switch writes to launcher-owned files and registry keys, the safety rules, the dependency list.
- A lighter way is preferred whenever the user sees the same result; the budgets against `oracle/perf/baseline.json` are gates, not hopes.

## Ticket

Template: `../tickets/TEMPLATE.md`. Key fields: `kind`, `risk`, `depends`, `write_scope`, `read` (spec sections, Electron files, cookbook entries), `acceptance` (vectors, tests, replays, shots, probes), `budget`, `owner_step`.

- Budgets: about 1,200 cited lines to read, about 350 source lines to write, 15 files at most.
- A ticket is immutable once ready. A PR may not edit its own ticket.
- Each milestone opens with one serial interface ticket (module tree, traits, DTOs, error enum). Hot files (workspace manifest, lockfile, DTOs, settings registry, cookbook) are single-writer.

## Definition of done

- Every acceptance id passes in CI on the head commit.
- Named ledger rows are covered, with no row lost and no waiver added.
- Test-integrity and policy lints are clean, and the diff stays inside `write_scope`.
- Mutation thresholds are met.
- UI tickets: the scenario is driven by input only, the sidecar is within tolerance, and shots at 1280x800 and 960x600 have been looked at.
- The verifier's verdict passes with receipts, and every Copilot comment is answered.
- Any owner step is queued for the gate (five numbered steps at most), never blocking.

## PR evidence

The body keeps the Summary / Evidence / Merge Danger shape. Evidence is a machine-readable block that CI recomputes and fails on mismatch:

- per-acceptance results
- ledger delta
- tests added, removed, ignored
- mutation caught and missed
- spec amendments and dependencies added
- safety counters (writes outside the switch pipeline, unsafe added, new process or network call sites)
- for UI: oracle, Rust and diff contact sheets on the `pr-assets` branch

## Verifier protocol

The verifier gets the ticket, the diff and CI artefacts, not the implementer's narrative. Its agent type has no Edit or Write. It must:

1. Run the acceptance subset in its own clean slot.
2. Build an assert-by-assert equivalence table against each TS original.
3. Produce at least 3 mutation receipts (6 for `risk: safety`) on spec-cited behaviour: a patch plus the test expected to go red.
4. Run at least 200 differential probes per listed function, including empty, oversized, CRLF/BOM, non-ASCII and duplicate-key inputs.
5. Enumerate every new call site into platform write, process, registry or network APIs and map each to a spec clause (the Riot checklist when relevant).
6. Read the public-API and crate-graph diff.
7. For UI, drive the scenario itself and look at the shots.

The verdict is schema-validated JSON bound to the head commit, with minimum counts. CI re-runs the receipts. About 1 run in 10 the script feeds the verifier a seeded-defect diff; passing one stops the wave. The verifier is never a weaker model than the implementer.

## Other roles

- **Ticket-prep.** Whoever defines "done" and translates tests is not the implementer.
- **Copilot.** A cheap cross-model reviewer; the verifier triages its comments.
- **Architecture audit.** After each interface ticket, every 10 merges, and before a gate; at most 5 ranked findings, each becoming a ticket.
- **Parity audit.** Before a gate, 10% of covered rows against the TS originals; a high error rate triggers a full re-audit of the milestone.
- **Safety audit.** Before a gate, whole codebase.
- **Diagnostician.** Classifies a parked ticket: ambiguity, oracle wrong, interface, GPUI limit, flake, too big. Proposes a split, amendment or interface ticket.

## Orchestration

- **Orchestrator session** (`/run-wave`): runs `xtask plan next`, starts the Workflow script, writes the wave summary. It makes no code judgments and reads only small schema'd results. One session per wave of at most 10 tickets.
- **Workflow script:** three lanes, one per slot. Per ticket: implement, verify, fix (3 rounds at most), enable auto-merge, confirm. It sets model and effort per role, injects canaries, and checks the budget.
- **Slots:** three long-lived checkouts at short paths, each with its own target dir, a shared sccache, and one GPUI build at a time. A fresh worktree per ticket would mean a cold build each time.
- **Resume:** one WIP commit per acceptance id. A dead agent is replaced by a fresh one resuming from the branch, counted as a round.

State:

- Tickets, waivers and triage live on `main`.
- Run state lives on an orphan `ops` branch: `state/<ticket>.json` (status, PR, round, slot, park class, repro), `waves/`, `verdicts/`, `gate/`.
- Precedence: GitHub PR state, then `ops`, then the Workflow journal.

Stop and ask the owner when:

- a safety hook or lint trips
- Electron contradicts the spec or the triage table
- a difference from Electron would remove or change something a user would notice or care about
- a dependency or protected-path change is needed
- three parks share a cause, or more than a quarter of a wave parks
- a canary passes the verifier
- an auto-revert fails
- a budget cap is hit

## Failure modes and countermeasures

| Failure | Countermeasure |
| --- | --- |
| Verifier rubber-stamps | Verdict bound to the commit with re-executable receipts; read-only agent type; canaries |
| Both sides agree on the wrong thing | Expectations extracted by running the TS; `xtask probe` differential fuzzing; ticket-prep writes translations; TS coverage under the oracle measured in Milestone 0 and gaps listed |
| Normaliser hides differences | Volatile fields are those that differ between repeated Electron recordings, nothing else; normaliser is protected |
| Ledger gaming | Ledger computed from `#[parity("T-0412")]`-tagged tests that ran and passed; waivers only in `plan/waivers.toml`; covered never falls; a waiver or a recorded difference never comes from the PR that needs it and is listed at the gate |
| Spec drift | A PR touching `spec/` may not touch `crates/`; an amendment is clause plus citation plus a new vector from Electron; a checker resolves every `path:line` at the pin |
| Weakened tests | Test-inventory diff (removed, renamed or `#[ignore]`d fails); per-test assertion count cannot drop; snapshot-accept banned for parity; oracle hash manifest verified; `cargo mutants --in-diff` with zero missed in switch, restore, journal, backup, vault and platform |
| Worktree conflicts | Disjoint `write_scope` scheduling; hook rejects edits outside it; serial interface ticket per milestone; single-writer hot files |
| Cargo.lock churn | Whole approved set resolved into `[workspace.dependencies]` in Milestone 1; `--locked` everywhere; `cargo add` and `cargo update` hook-blocked; `deps` tickets are serial |
| Context exhaustion | Ticket budgets; `xtask spec show §x` prints only the cited sections; WIP commit per acceptance id |
| Flaky UI automation | Automation mode (bundled fonts, animations off, virtual clock, fixed scale, in-process input, `wait_idle`); rerun only to classify; quarantine list capped and shown at the gate |
| Pixel threshold creep | Hard gate is the layout, text and token sidecar; pixel score is advisory |
| UI scenarios bypass the UI | State-changing API passthrough disabled in UI scenarios |
| Compounding architecture errors | Crate graph and public-API snapshot in CI; capability tokens; audits |
| CI weakened to get green | Required checks pinned; one aggregate job; the token cannot push workflow files |
| Hooks silently absent | Hooks in a committed `.claude/settings.json`; CI repeats every hook check |

## CLAUDE.md outline (Milestone 1)

Short; every subagent pays for it.

- What this is, and read only the ticket's cited sections.
- The standing rules carried over verbatim: launcher writes, real switches, data folder, Riot, secrets, releases, dependencies.
- Oracle rules: expectations come from `oracle/`; never edit a test, spec or reference file to get green.
- Workflow rules: one ticket per agent, stay in `write_scope`, never merge.
- Rust rules: unsafe only in `ol-platform-win`, typed errors, no comments by default.
- GPUI: copy from the cookbook or cite the pinned source file, never guess.
- Commands, docs policy, commit voice with no attribution.

## Skills

| Skill | Purpose |
| --- | --- |
| `run-wave` | Start or resume a wave |
| `ticket-prep` | Write the ticket and translated tests |
| `implement-ticket` | The implementer's loop |
| `verify-pr` | The verifier protocol |
| `run-app` | Drive the Rust app over the pipe against the fake platform |
| `parity` | Run and read the vector, replay and UI diffs |
| `oracle-probe` | Differential CLI against the live TS |
| `gpui-pattern` | Find or add a cookbook pattern |
| `spec-amend` | Evidence-backed amendment |
| `ship` | PR body and evidence |
| `diagnose` | Classify a parked ticket |
| `arch-audit`, `safety-audit` | The periodic audits |
| `gate-report` | Assemble the gate report |
| `release-prep` | As today; ends at the owner's steps |

## Hooks

PreToolUse on Bash and PowerShell blocks:

- process tools: `tasklist`, `taskkill`, `wmic`, `Get-Process`, `Get-CimInstance`
- any path under `%APPDATA%\OneLaunch`
- starting a launcher or an installer
- registry writes
- `git tag`, `gh release`, `gh pr merge` without `--auto`, `--no-verify`, force-push
- `cargo add`, `cargo update`, `cargo install`
- running the app without `--data-dir`

Also: the secret scan before commit, an Edit/Write hook for protected paths and `write_scope`, rustfmt on edit, and a SubagentStop hook requiring `xtask check --ticket` to pass.

## CI lints

- Clippy `disallowed-methods`: `Command::new`, HTTP clients, env and data-dir access outside their crates.
- `#![forbid(unsafe_code)]` present everywhere but `ol-platform-win`.
- `OpenProcess` only with `PROCESS_TERMINATE | SYNCHRONIZE`.
- cargo-deny: banned crates, licences, sources (crates.io only).
- Network host allowlist.
- A trace job asserting that process listing opens no foreign process.
- The release binary contains no automation or fake-platform strings; the automation feature excludes the real platform crate.
- Until the owner flips a latch in Milestone 6, the binary refuses the default data folder.
- Attribution lint on commits and PR bodies.

## CI tiers

- Tier 0, Linux, about 3 minutes: policy, deny, secrets, integrity.
- Tier 1, Windows: affected GPUI-free crates plus vectors and replay.
- Tier 2: GPUI plus UI parity, only when UI crates change.
- Tier 3, post-merge and nightly: full workspace, all shots, fuzz, packaging, takeover rehearsal, performance.

Caching: rust-cache saved from main only, separate core and UI families, nextest archive, prebuilt tools. No self-hosted runner on the owner's PC.

## Models

- Strongest: ticket-prep, the verifier on `risk: safety`, diagnostician, auditors, spikes, `ol-platform-win`, vault, switch and journal, the UI kit.
- Default implementer and verifier: the next tier down.
- Vector-driven translation and screens assembled from the kit: a cheaper tier.
- Round 2 and later escalates. Decide from cost per merged ticket on the first ten tickets per class before raising parallelism.

## Gate report

One page: ledger delta and parity by layer; oracle changes, spec amendments, triage requests; parked, escalated and quarantined items; audit findings and the parity-sample error rate; dependencies added, performance against baseline, cost; next-milestone risks. Plus a fake-platform demo build with a start script.

| Gate | The owner does personally |
| --- | --- |
| M0 | Approve triage, waiver categories and the dependency list; about 12 manual captures; create the repo, the GitHub App and the rulesets |
| M1 | GPUI go/no-go from the spike sheet; read a one-page architecture |
| M2 | Run `doctor --data-dir=<copy>` on a copy of the real data folder; it prints counts only |
| M3 | Ten minutes in the demo; one real Steam switch on a scratch data folder |
| M4 | One real switch per remaining launcher; kit gallery at 125% and 150%, tray |
| M5 | Demo; four real switches through the UI |
| M6 | One real update on the real install; tag, sign, publish |
| M7 | Go/no-go; move the tree into the old repo |

## Ticket counts (estimates)

About 150 to 250 including fix and split tickets: M0 15 to 20, M1 20 to 25, M2 20 to 25, M3 18 to 22, M4 40 to 50 with the UI kit track, M5 25 to 35, M6 10 to 14, M7 6 to 10.
