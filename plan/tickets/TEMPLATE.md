---
id: M3-012
title: Epic install detection
kind: feature        # feature | interface | spike | oracle | spec-amend | deps | fix
risk: plain          # safety | parity | ui | plain
depends: [M3-001]
write_scope:
  - crates/ol-adapters/src/epic/detect.rs
  - crates/ol-adapters/tests/epic_detect.rs
read:
  spec: ["§5.5 Epic: Detection"]
  source: [src/main/launchers/epic/adapter.ts]
  cookbook: []
acceptance:
  vectors: []        # ids under oracle/l1
  tests: []          # catalogue ids this ticket covers
  replays: []        # oracle/l2 script names
  shots: []          # oracle/ui shot ids
  probes: []         # functions for differential probing
budget: { src_lines: 350, read_lines: 1200 }
owner_step: none     # or up to five numbered steps queued for the gate
---

## Outcome

One paragraph: what exists when this is merged, in terms a check can confirm.

## Out of scope

What a reader might expect here that belongs to another ticket (name it).

## Traps

Things the spec or the Electron source make easy to get wrong, each with its citation.

<!--
Rules
- A ticket is immutable once it is ready. A PR may not edit its own ticket.
- Ticket-prep writes the ticket and its red tests. The implementer never writes the expectations.
- Done means: every acceptance id passes in CI on the head commit; named ledger rows are covered with none lost and no waiver added; the diff stays inside write_scope and under 15 files; mutation thresholds are met; the verifier's verdict passes with receipts; every review comment is answered.
-->
