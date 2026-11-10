# GPUI notes

A snapshot of web research, not verified locally. Every fact here is confirmed or replaced by a Milestone 1 spike before anything depends on it. Versions are as found on crates.io and GitHub when the plan was written.

## Which GPUI

- The official `gpui` crate on crates.io is stale (0.2.2). Zed's own tree still carries that version number.
- The practical dependency is `gpui-pre` (0.3.12 at research time), a weekly snapshot of a Zed commit, with `gpui-pre-platform`, `-macros`, `-http-client` and `-reqwest-client` at the same version. It is published by the GPUI Kit maintainer, not by Zed. Pin exactly and commit `Cargo.lock`.
- Alternatives: a pinned git rev of zed-industries/zed, or forks (`gpui-ce` and others). Mixing lineages causes trait mismatches.
- There is no GPUI 1.0 and no separate repo. The README says breaking changes are common.
- Zed pins Rust 1.98.1. `windows-registry` 0.100 needs 1.95.
- Licence: `gpui` and the `gpui-pre-*` crates are Apache-2.0. Many other Zed crates are GPL-3.0; take only gpui and its platform crates.

## Windows backend

- DirectX 11 with HLSL shaders, presented through DirectComposition (`GPUI_DISABLE_DIRECT_COMPOSITION` turns that off). Text is DirectWrite.
- Mica, Mica Alt, Transparent and Blurred backdrops are available.
- IME handling was reworked during 2026.
- No reports found on mixed-DPI multi-monitor behaviour. Build time, binary size and memory have no measured public numbers.

## GPUI Kit (formerly gpui-component)

- Repo `longbridge/gpui-kit`; crates `gpui-kit` and `gpui-component` 0.9.1; Apache-2.0; pins `gpui-pre` exactly. Weekly releases, some breaking.
- Has TitleBar, Dialog, Notification, Select, Menu, Popover, Tooltip, Input (masked, with toggle), Textarea, Switch, Checkbox, Tabs, Scrollable, VirtualList, TextView, Image, Theme, Focus Trap.
- Ships an AGENTS.md and agent skills. Claims AccessKit accessibility and headless UI integration tests.
- This project uses it for text input only; see `architecture.md`.

## What GPUI has and lacks

Has: `prompt_for_paths`, `prompt_for_new_path`, `open_url`, `register_url_scheme`, `set_http_client`, `set_quit_mode` (run with no windows), `start_window_move`, `show_window_menu`, `set_background_appearance`; `img()` with fallback, loading, object-fit and image caches; `Animation` with `with_animation` and easing; foreground and background executors with timers and a deterministic test scheduler; AccessKit re-exported (elements with an id appear in the tree).

Lacks:

- **System tray.** `tray-icon` with `muda` on the main thread (no public GPUI example found), or `Shell_NotifyIconW` directly.
- **Single instance.** Named mutex plus a named pipe.
- **CSS-like transitions.** Kit adds a `TransitionState`.
- **Headless pixel rendering on Windows.** Only Metal has it, so screenshots need a real window plus OS capture (Windows.Graphics.Capture).
- **A remote inspection protocol.** The inspector is in-app only.

## Driving the app

- In-process: `#[gpui::test]`, `TestAppContext`, `VisualTestContext`.
- A young third-party project, gpui-mcp, shows the in-app bridge pattern (UI tree, click, type, wait, screenshot through Windows.Graphics.Capture). It lags current `gpui-pre` versions. Treat it as a reference, not a dependency.
- Lessons from another GPUI app: keep stable element ids or clicks break between mouse-down and mouse-up; polling timers cause idle repaints, so push updates.

## Other crates (versions at research time)

`rusqlite` 0.41, `windows` 0.62, `ed25519-dalek` 3.0, `ureq` 3.5, `tracing` 0.1, `tray-icon` 0.26, `muda` 0.21, `winresource` 0.1. The SQLite file and WAL formats are stable, so a database written by better-sqlite3 should open; the vault spike checks it.

## Packaging

Velopack and cargo-packager were considered and rejected: both change the install identity or the update trust chain, and takeover needs the existing contract kept exactly. See `takeover.md`.

## Lessons from large agent-driven ports

1. Build the judge first: tests that run against both implementations, and confirm they fail on broken code.
2. Diff real scenarios; any behaviour change counts as a bug, with no "intended improvement" exceptions.
3. Write the rulebook up front, trial it on a sample, throw the output away and refine.
4. Build the dependency map with a script, not by asking the model.
5. Use a resumable queue where done means the output exists on disk.
6. Review in separate contexts. When the same mistake recurs, fix the rulebook and redo the batch.
7. Let the compiler, diffs and tests decide correctness, and serialise expensive builds.
