---
id: M1-F6e
title: Test integrity and the computed ledger
kind: feature
risk: safety
depends: [M1-F6c, M1-F2]
write_scope:
  - xtask/src/policy/integrity/**
  - xtask/src/policy/ledger/**
  - xtask/tests/integrity/**
  - crates/ol-testkit/src/parity.rs
  - crates/ol-testkit-macros/**
  - .github/workflows/ci.yml          # committed by the agent, pushed by the owner
read:
  design: [plan/design/workflow.md "Definition of done" and "Failure modes and countermeasures", plan/design/oracle.md "Test catalogue", oracle/tests/README.md, oracle/schemas/catalogue.schema.json]
  source: []
acceptance:
  checks:
    - "fixture diffs in xtask's tests: a removed test, a renamed test, a test given `#[ignore]`, a test that loses one assertion, and a changed file under oracle/ each fail with their own message; adding a test passes"
    - "a test tagged `#[parity(\"<catalogue id>\")]` that ran and passed makes its row covered; the same test failing, filtered out or ignored does not"
    - "a tag naming an id that is not in oracle/tests/catalogue.json, oracle/l1, oracle/l2 or oracle/ui fails"
    - "a head whose covered set lacks a row covered on the base fails"
    - "`cargo xtask ledger` prints covered, waived and open counts per layer, and the totals equal the bundle's: 1,031 tests, 149 vector files, 22 replays, 174 checklist rows"
owner_step: "1. Push the workflow commit: `git push origin <branch>`."
---

## Outcome

- **Integrity.** `cargo xtask integrity --base <ref>` compares the test inventory of the head with the base: no test removed, renamed or ignored, and no test with fewer assertions than before. It also verifies every file under `oracle/` against `oracle/MANIFEST.json`.
- **Ledger.** A row is an oracle id: a catalogued test, a vector file, a replay script or a row of the screenshot checklist. `#[parity("id")]` on a Rust test claims rows. The ledger counts a row as covered only when CI ran that test and it passed, as read from the test runner's machine-readable result. A row is waived only by `plan/waivers.toml`. Covered never falls from base to head.
- Tier 1 runs both after the tests; `gate` fails when either fails. The pull request's evidence block carries the ledger delta and the tests added, removed and ignored, and tier 1 recomputes them and fails on a mismatch.

## Out of scope

Mutation testing in CI, verifier receipts and canaries (F7c and Milestone 2). Waiver rows themselves: none is written here.

## Traps

- `#[parity]` is an attribute that returns its item unchanged. It needs the proc-macro crate F1 created empty (`ol-testkit-macros`) and nothing else: no `syn`, no `quote`.
- Counting assertions needs no Rust parser: none is approved. Count the assertion macros inside each test function's braces as text, and treat a test whose body cannot be delimited as a failure, not as zero.
- The base inventory is read from the base commit's sources. Do not build the base.
- A test that is compiled out by `cfg` on the CI platform did not run. It covers nothing there.
- The row totals come from the bundle. If a count differs from the gate report of Milestone 0, stop and say so; do not adjust either.
