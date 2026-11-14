---
id: M1-F7c
title: The Workflow script and `run-wave`
kind: feature
risk: plain
depends: [M1-F7a, M1-F7b]
write_scope:
  - .claude/workflows/wave.js
  - .claude/skills/run-wave/**
  - xtask/src/ops/verdict.rs
  - xtask/src/ops/canary.rs
  - xtask/tests/ops/**
  - plan/schemas/verdict.schema.json
  - plan/schemas/state.schema.json
read:
  design: [plan/PLAN.md "One ticket, start to finish", plan/design/workflow.md "Verifier protocol", "Orchestration", "Models" and "Stop and ask the owner when"]
  source: []
acceptance:
  checks:
    - "a verdict with too few mutation receipts, too few probes, a missing call-site list, or another commit than the head is rejected by `cargo xtask ops verdict check`"
    - "a canary run: the script hands the verifier a diff with a planted defect; a passing verdict stops the wave and writes the stop into the wave summary"
    - "one real wave of two plain Milestone 1 tickets runs end to end: implemented in slots, verified, auto-merge requested by the app, merged by GitHub, state on `ops`, a wave summary with tokens and wall time per ticket"
    - "killing the script mid-ticket and starting `run-wave` again resumes that ticket from its branch as a new round"
owner_step: none
---

## Outcome

- `.claude/workflows/wave.js`: three lanes, one per slot. Per ticket: write the slot's current-ticket file, implement, ship, wait for CI, verify, fix (three rounds at most), request auto-merge, confirm the merge, write state. It sets model and effort per role from the ticket's `risk` and `kind`, allows one GPUI build at a time, feeds a canary to about one verifier run in ten, and stops at ten tickets or at any condition of "Stop and ask the owner when".
- `run-wave`: the orchestrator's skill. `cargo xtask plan next`, start the script, write the wave summary. It reads only schema-checked results.
- Schemas for the verdict and the state file; `cargo xtask ops verdict check` enforces the minimum counts and the binding to the head commit.
- The cost per merged ticket (tokens, wall time, rounds) is in every wave summary; the gate report of Milestone 1 takes it from there.

## Out of scope

The diagnostician and the audits (first use). Mutation receipts re-run by CI (Milestone 2, with the first code that has behaviour to mutate).

## Traps

- No agent merges. The script calls `scripts/agent/gh.ps1 pr merge --auto --squash` and then only watches.
- A pull request that touches an owner-approved path or holds a workflow commit cannot finish inside a wave. The script parks it as "waiting for the owner" and goes on; that is not a failed round.
- The verifier is never a weaker model than the implementer, and round two and later escalate.
- A dead agent is replaced by a fresh one that resumes from the branch. That counts as a round.
- Load the workflow authoring reference before writing the script; its API is not to be guessed.
