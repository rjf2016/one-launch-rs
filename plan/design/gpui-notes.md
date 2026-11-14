# GPUI notes

A snapshot of web research, not verified locally. Every fact here is confirmed or replaced by a Milestone 1 spike before anything depends on it. Versions are as found on crates.io and GitHub when the plan was written.

Checked against crates.io, the pinned source and a build by F1 (`../decisions/M1-gpui-pin.md`): "Which GPUI", the first two points of "GPUI Kit", the AccessKit re-export, and "Other crates". Everything else is still unverified.

## Which GPUI

- The official `gpui` crate on crates.io is stale (0.2.2). Zed's own tree still carries that version number.
- The dependency is `gpui-pre` 0.3.12, a weekly snapshot of a Zed commit (`d2a4fb6`), published by the GPUI Kit maintainer, not by Zed. Pinned exactly, with `Cargo.lock` committed. The package builds a library named `gpui`, and `gpui-pre-platform` one named `gpui_platform`; depend on them under those names.
- With it, at the same version, come `gpui-pre-platform`, `-windows`, `-macros`, `-collections`, `-refineable`, `-derive-refineable`, `-scheduler`, `-shared-string`, `-sum-tree`, `-util`, `-http-client`, `-ztracing` and `-ztracing-macro`. `gpui-pre-reqwest-client` exists but nothing we take depends on it.
- Alternatives: a pinned git rev of zed-industries/zed, or forks (`gpui-ce` and others). Mixing lineages causes trait mismatches, and GPUI Kit requires `gpui-pre` by exact version.
- There is no GPUI 1.0 and no separate repo. The README says breaking changes are common.
- Zed pins Rust 1.98.1 at that commit. The `gpui-pre` crates and GPUI Kit declare no Rust version; the highest one declared in the Windows build is 1.89. `windows-registry` 0.100 needs 1.95, but the lockfile takes 0.6.1, which needs 1.82. The workspace pins 1.100.0.
- Licence: `gpui` and the `gpui-pre-*` crates are Apache-2.0. Many other Zed crates are GPL-3.0; take only gpui and its platform crates.
- `gpui-pre` depends on `chrono`, and its build script on `embed-resource`, which uses `winreg` to find the Windows SDK. Both are on our ban list; `deny.toml` allows them under those crates only. Its feature `screen-capture` brings `sysinfo` and stays off.

## Windows backend

- DirectX 11 with HLSL shaders, presented through DirectComposition (`GPUI_DISABLE_DIRECT_COMPOSITION` turns that off). Text is DirectWrite.
- Mica, Mica Alt, Transparent and Blurred backdrops are available.
- IME handling was reworked during 2026.
- No reports found on mixed-DPI multi-monitor behaviour. Build time, binary size and memory have no measured public numbers.

## GPUI Kit (formerly gpui-component)

- Repo `longbridge/gpui-kit`; crates `gpui-kit` and `gpui-component` 0.9.1; Apache-2.0; pins `gpui-pre` exactly. Weekly releases, some breaking.
- `gpui-kit` is a facade over layers. `gpui-base`, always on, is the unstyled one: the input with masking, the textarea, the scrollbar, the focus trap and the virtual list are there. `gpui-component` and `gpui-kit-assets` are the styled one, behind the default features `component` and `assets`: the title bar, the menu and the notification are there. The workspace takes `gpui-kit` with default features off.
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

Pinned by F1 in `Cargo.toml`: `rusqlite` 0.42.0, `windows` 0.62.2, `ed25519-dalek` 3.0.0, `ureq` 3.5.2, `winresource` 0.1.34; the rest of the approved list is in `../decisions/M1-gpui-pin.md`. Not pinned: `tray-icon` 0.26 and `muda` 0.21, which wait for the tray spike, and `tracing`, which is not on the approved list. The SQLite file and WAL formats are stable, so a database written by better-sqlite3 should open; the vault spike checks it.

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
