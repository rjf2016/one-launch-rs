---
id: M1-F7a
title: "`xtask plan`, run state on the `ops` branch, the three slots"
kind: feature
risk: plain
depends: [M1-F6c]
write_scope:
  - xtask/src/plan/**
  - xtask/src/ops/**
  - xtask/src/main.rs
  - xtask/tests/plan/**
  - scripts/agent/new-slot.ps1
read:
  design: [plan/design/workflow.md "Orchestration" and "Acting as the app", plan/decisions/M1-F0.md, plan/tickets/TEMPLATE.md]
  source: []
acceptance:
  checks:
    - "xtask's tests: from a fixture set of tickets and states, `plan next` offers only tickets whose `depends` are all merged, whose `write_scope` overlaps no running ticket, at most three, and at most one that builds a GPUI crate"
    - "`plan next` on the real repo lists the Milestone 1 tickets that are ready today and no other"
    - "a state file written by `ops set` reads back, validates against its schema, and lands as one commit on `ops` pushed as the app"
    - "in a new slot, plain `git push` goes out as the app over HTTPS, `git config user.email` is the app's address, and SSH is refused"
owner_step: none
---

## Outcome

- `cargo xtask plan next` reads every ticket's front matter, the merged pull requests (through `scripts/agent/gh.ps1`) and the run state, and prints the tickets ready to start as JSON. `plan status` prints one line per ticket of a milestone.
- Run state lives on an orphan branch `ops`: `state/<ticket>.json` (status, pull request, round, slot, park class, repro), `waves/`, `verdicts/`, `gate/`. `cargo xtask ops get|set` reads and writes it through a second worktree, never through the code checkout. GitHub's pull request state wins over `ops` where they differ.
- `scripts/agent/new-slot.ps1 <path>` makes a slot: a clone over HTTPS whose local git config holds the app's name and address, the credential helper and `protocol.ssh.allow=never`, with its own target folder and the shared sccache. Three slots at short paths.

## Out of scope

The Workflow script and the skills (F7b, F7c). Verdict schemas (F7c).

## Traps

- `ops` is an orphan branch on the same repo. The `main` ruleset does not cover it, so the app pushes to it directly; it must never be merged into `main` or the reverse.
- A ticket is merged when its pull request is, not when a state file says so.
- Overlap is decided on globs, not on files that exist: `crates/ol-store/**` overlaps `crates/ol-store/src/db.rs`.
- Slot paths are short because GPUI's build tree is deep. The path is the caller's choice; the script refuses a folder inside another checkout.
- `scripts/agent/` is owner-approved once F6b lands.
