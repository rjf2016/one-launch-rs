---
id: M1-F7b
title: "Skills and agent types: ticket-prep, implement-ticket, verify-pr, parity, ship, gate-report"
kind: feature
risk: plain
depends: [M1-F6a, M1-F6b, M1-F7a]
write_scope:
  - .claude/skills/**
  - .claude/agents/**
read:
  design: [plan/design/workflow.md "Ticket", "Definition of done", "PR evidence", "Verifier protocol", "Other roles", "Skills" and "Gate report", plan/tickets/TEMPLATE.md, plan/gate/M0.md]
  source: []
acceptance:
  checks:
    - "each skill file loads (front matter valid, under 150 lines) and names only commands and files that exist"
    - "the verifier agent type has no Edit, Write or NotebookEdit tool, and its skill refuses to start without a ticket id and a head commit"
    - "a dry run on a merged Milestone 1 ticket: `verify-pr` produces a verdict that validates against the schema of F7c, bound to that commit"
    - "`ship` on a scratch branch writes a pull request body with the three sections and an evidence block that `cargo xtask policy` accepts"
owner_step: none
---

## Outcome

Six skills under `.claude/skills/`, each the whole instruction for one role, and the agent types they run as under `.claude/agents/`:

| Skill | The role does | Ends with |
| --- | --- | --- |
| `ticket-prep` | Writes a ticket and its red tests from the oracle and the cited Electron source; records a difference from Electron as a `delegated` row in its own pull request | A ticket pull request, no code |
| `implement-ticket` | One ticket in one slot: read only what the ticket cites, commit per acceptance id, `cargo xtask check --ticket` | A pushed branch |
| `ship` | The pull request body: Summary, Evidence, Merge Danger, and the machine-readable evidence block | An open pull request by the app |
| `verify-pr` | The verifier protocol of `workflow.md`, steps 1 to 7, from the ticket, the diff and CI's artefacts only | A verdict file |
| `parity` | Runs and reads the vector, replay and UI diffs for a crate or a ticket | A table of differences, each recorded or not |
| `gate-report` | Assembles the milestone's gate report from `ops`, the ledger and the decision records | `plan/gate/<milestone>.md` |

`run-wave` is F7c's, with the Workflow script it starts.

## Out of scope

`run-app`, `oracle-probe`, `gpui-pattern`, `spec-amend`, `diagnose`, the audits and `release-prep`: each is written by the first ticket that needs it.

## Traps

- A skill is a prompt, and the plan's principle is that a rule which can be code is not a prompt. A skill states the order of work and points to `xtask`; it does not restate a rule a hook or CI already enforces.
- The verifier never receives the implementer's explanation. Its skill takes a ticket id and a commit, and fetches the rest itself.
- The implementer never writes a row in `plan/triage.toml` or `plan/waivers.toml`. Its skill says to stop instead.
- `.claude/` is owner-approved: this pull request waits for the owner.
