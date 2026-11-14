---
id: M1-F6c
title: "Policy checks in xtask: scope, pull request shape, attribution"
kind: feature
risk: safety
depends: [M1-F1]
write_scope:
  - xtask/src/policy/**
  - xtask/src/main.rs
  - xtask/tests/policy/**
  - .github/workflows/ci.yml          # committed by the agent, pushed by the owner
read:
  design: [plan/design/workflow.md "Owner-approved paths", "Differences from Electron", "Ticket", "PR evidence" and "CI lints", plan/tickets/TEMPLATE.md]
  source: []
acceptance:
  checks:
    - "xtask's own tests pass: one fixture repository per rule below, each with a diff that must pass and one that must fail"
    - "`cargo xtask check --ticket M1-F1 --base <commit>` passes on the F1 merge and fails on the same diff plus one file outside its write_scope"
    - "`cargo xtask check --ticket <id>` passes on a branch that has no pull request, and makes no network call"
    - "tier 0 runs `cargo xtask policy` on every pull request and `gate` fails when it fails"
owner_step: "1. Push the workflow commit: `git push origin <branch>`."
---

## Outcome

`cargo xtask policy --base <ref>` judges a pull request: its diff, its commits, its body and its reviews. Tier 0 runs it on every pull request. `cargo xtask check --ticket <id>` runs the rules that need no pull request, with the ticket's limits: rules 1 to 4, the commit-message half of rule 6, and rule 7. Rules 5 and 8 and the body half of rule 6 run only in `policy`, because an implementer's branch has no pull request yet when its agent stops. The hooks of F6a call the local rules only. Rules, each with a named failure message:

1. **Ticket.** The branch names one ticket (`<milestone>/<ticket>-...`), and that ticket's file exists on the base.
2. **Scope.** Every changed path matches the ticket's `write_scope`. Fifteen files at most, unless the ticket is `kind: interface`.
3. **Tickets are immutable.** No pull request changes a ticket file that exists on the base. A pull request that adds ticket files touches nothing outside `plan/`.
4. **Plan and code apart.** A pull request that touches `plan/triage.toml`, `plan/waivers.toml` or `spec/` touches no path under `crates/`.
5. **The owner's rows.** In `plan/triage.toml` a row with `source` `""` or `"plan"` is unchanged and not removed; in `plan/waivers.toml` no category is added, changed or removed. The check passes these only when the owner has approved the pull request at its head commit.
6. **Attribution.** No commit message and no pull request body carries an AI attribution line.
7. **Dependencies.** `Cargo.lock`, `[workspace.dependencies]` and `deny.toml` change only in a ticket of `kind: deps` or `interface`.
8. **Evidence.** The pull request body has the Summary, Evidence and Merge Danger sections, and its evidence block parses. What the block must contain is fixed by F6e and later tickets; here an empty block is valid.

## Out of scope

Test integrity and the ledger (F6e). The hooks (F6a). Clippy, deny and the unsafe lint (F1).

## Traps

- Rule 5 reads reviews through the API with the workflow's own token. Count only an approval by the owner on the current head commit; an older approval does not carry over.
- A rename is a removal plus an addition: judge both paths against the scope.
- `write_scope` entries are globs with `**`. Match with forward slashes whatever the platform.
- The policy code is owner-approved. Put all of it under `xtask/src/policy/` so the owners file can name one folder.
- No new dependency: globs and TOML rows are simple enough to read with what is approved. If `plan/triage.toml` cannot be read without a TOML parser, stop: that is a `deps` request for the owner.
