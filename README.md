# One Launch (Rust)

The Rust + GPUI rewrite of [One Launch](https://github.com/rjf2016/one-launch): launch any game, from any launcher, on any account. Windows only.

The repo holds the plan, the behavioural spec, the seed of the oracle (reference data captured from the Electron app) and the Cargo workspace, whose crates are empty until their tickets land. `cargo xtask check` runs everything a pull request must pass.

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
| `crates/` | The product, one crate per row of `plan/design/architecture.md`; `ol-app` builds `OneLaunch.exe` |
| `spikes/` | The proofs of Milestone 1, `s01` to `s12`. Not default members, and no crate depends on one |
| `xtask/` | `cargo xtask check`: formatting, the lints, the secret scan, the oracle's hashes, `cargo deny`, clippy, build and tests |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `tools.toml` | The pins: every dependency at an exact version, the Rust toolchain, the cargo tools |
| `deny.toml`, `clippy.toml`, `rustfmt.toml` | Banned crates, accepted licences and sources; banned calls; formatting |
