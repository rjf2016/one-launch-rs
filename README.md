# One Launch (Rust)

The Rust + GPUI rewrite of [One Launch](https://github.com/rjf2016/one-launch): launch any game, from any launcher, on any account. Windows only.

No Rust exists yet. The repo holds the plan, the behavioural spec and the seed of the oracle (reference data captured from the Electron app).

| Path | What it is |
| --- | --- |
| `plan/PLAN.md` | The approved plan. Start here |
| `plan/design/` | The design behind the plan: architecture, oracle, workflow, takeover contract, GPUI notes |
| `plan/milestones/` | One file per milestone: goal, tickets, exit gate, the owner's part |
| `plan/tickets/` | Ticket files and the template |
| `plan/decisions/` | Decision records of Milestone 1: pass, or the named fallback |
| `plan/triage.toml` | Keep-or-drop decisions for known bugs, dead code and open questions |
| `plan/waivers.toml` | Tests and parity rows the owner agreed not to port |
| `spec/SPEC.md` | Behavioural spec of the Electron app at the pinned commit |
| `oracle/` | Pin, fixtures, screenshot checklist; Milestone 0 fills in the rest |
| `scripts/agent/` | How the agents sign in to GitHub as the app `one-launch-agents` |
