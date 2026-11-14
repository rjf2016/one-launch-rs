---
id: M1-F6b
title: Owners file and the full CLAUDE.md
kind: feature
risk: safety
depends: [M1-F1]
write_scope:
  - .github/CODEOWNERS
  - CLAUDE.md
  - docs/cookbook/README.md
read:
  design: [plan/design/workflow.md "Owner-approved paths" and "CLAUDE.md outline (Milestone 1)", plan/decisions/M1-F0.md, CLAUDE.md]
  source: []
acceptance:
  checks:
    - "GitHub reports no error for the owners file (`gh api repos/{owner}/{repo}/codeowners/errors` is empty)"
    - "every standing rule of the current CLAUDE.md appears in the new one word for word"
    - "the new CLAUDE.md is under 120 lines and names no file that does not exist"
owner_step: none
---

## Outcome

- `.github/CODEOWNERS` names the owner for `spec/`, `oracle/`, `.github/`, `.claude/`, `deny.toml`, `scripts/agent/` and the policy code of `xtask` (the folder F6c creates; name it here first). Nothing else has an owner, so a pull request that stays clear of these paths merges without review.
- `CLAUDE.md` is the full version of the outline in `workflow.md`: what this is; the standing rules, unchanged; the oracle rules; one ticket per agent, stay in `write_scope`, never merge; act on GitHub only through `scripts/agent/`; the Rust rules; GPUI from the cookbook or the pinned source, never from memory; the commands (`cargo xtask check`, how to run the app); the commit voice.
- `docs/cookbook/README.md` says what a cookbook entry is: one file per pattern, the pinned GPUI source file it was taken from, a minimal snippet that compiled in a named spike or crate.

## Out of scope

The hooks (F6a). Proving the owners file works on real pull requests (F6d).

## Traps

- The owners file is not a workflow file, so the app can push it. Once it is on `main`, every later change under `.github/` and `.claude/` waits for the owner.
- The pull request is opened by the app. One opened by the owner could not be approved by anyone.
- Every subagent pays for `CLAUDE.md` on every run. No history, no rationale, no duplicate of a design file: point to it.
- "Commit only when the owner asks" ends here; say what replaces it (commits happen inside a ticket, on its branch).
