---
id: M1-F6d
title: "Proof: the guardrails reject one bad pull request of each kind"
kind: spike
risk: safety
depends: [M1-F6a, M1-F6b, M1-F6c, M1-F6e]
write_scope:
  - plan/gate/M1-guardrails.md
read:
  design: [plan/milestones/M1.md "Exit gate", plan/design/workflow.md "Identity and merging" and "Owner-approved paths", plan/decisions/M1-F0.md]
  source: []
acceptance:
  checks:
    - "each of the eight pull requests below ended as stated, with its number, the failing check or the blocked state, and the message, in plan/gate/M1-guardrails.md"
    - "every scratch pull request is closed unmerged and its branch deleted; `main` holds none of their commits"
owner_step: "1. Push the waiting workflow commit of scratch pull request number 5 (`git push origin <branch>`). 2. Approve that pull request. It is closed straight after and never merged."
---

## Outcome

Scratch pull requests, opened by the app against `main`, each carrying one deliberate fault. Auto-merge is requested on every one but number 5, so a guardrail that fails to hold would show as a merge. Number 5 never has auto-merge on: the owner's approval makes it mergeable as number 6, and with auto-merge on that approval would merge it.

| # | The pull request | Must end as |
| --- | --- | --- |
| 1 | Edits a file outside its ticket's `write_scope` | `gate` fails: scope |
| 2 | Removes a test | `gate` fails: integrity |
| 3 | Marks a test `#[ignore]` | `gate` fails: integrity |
| 4 | Adds the banned crate `sysinfo` | `gate` fails: deny |
| 5 | Adds a comment line to `.github/workflows/ci.yml`; the owner pushes that commit | `gate` passes, merge blocked: code-owner review required |
| 6 | Number 5 once the owner has approved it | Mergeable although the last push was the owner's; closed without merging |
| 7 | Changes a `plan` row of `plan/triage.toml` | `gate` fails: the owner's rows |
| 8 | Carries an attribution line in a commit message | `gate` fails: attribution |

Also recorded, as GitHub facts the plan relies on:

- a pull request by the app that touches no owner-approved path merges through auto-merge with no review;
- a push by the app that changes `.github/workflows/` is refused (repeat of F0);
- the hooks refuse each fault locally before it can be committed, where a hook exists for it. For this ticket the faults are committed from a session with the ticket's scope set to the scratch paths, and the report says which hook had to be satisfied that way.

The owner's part of a pull request opened by the owner cannot be tried by an agent; the report states it as untested unless the owner tries it.

## Out of scope

Fixing a guardrail that fails to hold: that is a `fix` ticket, and this one stays open until it merges.

## Traps

- Numbers 5 and 6 are one pull request, and it must not merge. Before the owner is asked to approve, read back that auto-merge is off on it. Close it as soon as its mergeable state is read.
- If any scratch pull request merges, stop the line: revert through a pull request and tell the owner.
- Do not weaken a hook to commit a fault. If a fault cannot be committed with the hooks on, that is the finding.
- Never test the tag rule by pushing a tag.
