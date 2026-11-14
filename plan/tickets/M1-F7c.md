---
id: M1-F7c
title: The Workflow script and `run-wave`
kind: feature
risk: plain
depends: [M1-F7a, M1-F7b]
write_scope:
  - .claude/workflows/wave.js
  - .claude/skills/run-wave/**
  - xtask/src/ops/**                  # verdict and canary modules, and their registration
  - xtask/src/main.rs
  - xtask/tests/ops/**
  - plan/design/workflow.md
  - CLAUDE.md
read:
  design: [plan/PLAN.md "One ticket, start to finish", plan/design/workflow.md "Verifier protocol", "Orchestration", "Models" and "Stop and ask the owner when"]
  source: []
acceptance:
  checks:
    - "`cargo xtask ops verdict check` rejects a verdict bound to another commit than the head, and one that falls short of any minimum its ticket's kind requires (table below); it accepts a spike's verdict that has no mutation receipt and no probe"
    - "a canary run: the script hands the verifier a diff with a planted defect; a passing verdict stops the wave and writes the stop into the wave summary"
    - "one real wave of two tickets, F2 and F3, runs end to end: implemented in slots, verified, auto-merge requested by the app, merged by GitHub, state on `ops`, a wave summary with tokens and wall time per ticket"
    - "killing the script mid-ticket and starting `run-wave` again resumes that ticket from its branch as a new round"
owner_step: "1. Read the one changed rule in `CLAUDE.md` in this pull request, and overrule it if it is not what you meant."
---

## Outcome

- `.claude/workflows/wave.js`: three lanes, one per slot. Per ticket: write the slot's current-ticket file, implement, ship, wait for CI, verify, fix (three rounds at most), request auto-merge, confirm the merge, write state. It sets model and effort per role from the ticket's `risk` and `kind`, allows one GPUI build at a time, feeds a canary to about one verifier run in ten, and stops at ten tickets or at any condition of "Stop and ask the owner when".
- `run-wave`: the orchestrator's skill. `cargo xtask plan next`, start the script, write the wave summary. It reads only schema-checked results.
- `cargo xtask ops verdict check` enforces the binding to the head commit and the minimum counts. The verdict's shape is F7a's schema; the minimums depend on the ticket:

| The ticket | Mutation receipts | Probes | New call sites listed |
| --- | --- | --- | --- |
| Adds or changes code that has tests (`feature`, `fix`, `interface`) | At least 3, and 6 for `risk: safety`, each on behaviour one of the ticket's acceptance lines names | 200 for each function under `acceptance.probes`; none listed, none required | Always |
| `spike` | None. Instead every acceptance line is re-run by the verifier and set beside the record's claim | None | Always |
| Plan text only (tickets, decision records, triage rows, a spec amendment) | No verifier run on that pull request: the policy checks and, on owner-approved paths, the owner's review judge it. A triage row or a waiver is still checked by the verifier of the ticket that cites it | | |

- `workflow.md` › "Verifier protocol" gains that table: the protocol as written speaks only of spec-cited behaviour, and says nothing for a ticket that has none.
- `CLAUDE.md`: the rule "Commit only when the owner asks, until the agent workflow of Milestone 1 is in place" is replaced, now that it is in place, by its successor: an agent commits inside a ticket, on that ticket's branch, and nowhere else.
- The cost per merged ticket (tokens, wall time, rounds) is in every wave summary; the gate report of Milestone 1 takes it from there.

F2 and F3 are held back for the first wave. They are the first tickets with behaviour taken from the oracle, so they are the first on which mutation receipts and probes mean something, and Milestone 1's exit gate asks for the cost of the first tickets.

## Out of scope

The diagnostician and the audits (first use). Mutation receipts re-run by CI (Milestone 2, with the first code that has behaviour to mutate).

## Traps

- No agent merges. The script calls `scripts/agent/gh.ps1 pr merge --auto --squash` and then only watches.
- A pull request that touches an owner-approved path or holds a workflow commit cannot finish inside a wave. The script parks it as "waiting for the owner" and goes on; that is not a failed round.
- The verifier is never a weaker model than the implementer, and round two and later escalate.
- A dead agent is replaced by a fresh one that resumes from the branch. That counts as a round.
- Load the workflow authoring reference before writing the script; its API is not to be guessed.
- `.claude/` is owner-approved, so this pull request waits for the owner. The commit rule in `CLAUDE.md` is the owner's own: change that one sentence and nothing else in the file.
