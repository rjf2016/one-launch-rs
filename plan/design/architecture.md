# Architecture

Target design for the Rust workspace. `E\` means the Electron repo at the pinned commit. GPUI and GPUI Kit claims are unverified until the Milestone 1 spikes; see `gpui-notes.md`.

## Workspace

```
ol-model <- ol-formats
ol-model <- ol-platform-api <- ol-runtime
            ol-store, ol-vault <- ol-adapters <- ol-services <- ol-api
ol-api <- ol-ui (<- ol-ui-kit), ol-automation, ol-replay <- ol-app
implementations of ol-platform-api: ol-platform-win, ol-net (real) | ol-platform-fake, ol-sim (feature `fake`)
```

| Crate | Responsibility | Ports |
| --- | --- | --- |
| `ol-model` | Enums, entities, serde DTOs, limits, error codes, typed accessors over the string settings table, What's New data | `E\src\shared\*` |
| `ol-formats` | Byte-preserving VDF, INI, Battle.net config, Riot YAML subset, product.db decoder, log parsers, `winpath`; pure functions | `E\src\main\launchers\*\{vdf,loginusers,manifests,ini,config,products,session,*-log}.ts`, `fs-path.ts` |
| `ol-platform-api` | Traits, plain types, shared validation (image-name and registry-path rules) | interfaces in `E\src\main\platform\*.ts` |
| `ol-runtime` | `CancelToken`, `sleep`, `wait_until`, timing merge, watch channel, virtual-clock executor | `E\src\main\launchers\wait.ts` |
| `ol-store` | rusqlite open, pragmas, migrations 1 to 5 verbatim, DB copies, repositories | `E\src\main\db\*` |
| `ol-vault` | os_crypt `v10` codec, `Local State` read and create (unknown keys preserved), health | `E\src\main\services\{vault,vault-health,secrets}.ts` |
| `ol-adapters` | Adapter trait, shared helpers, four launcher modules | `E\src\main\launchers\**` |
| `ol-services` | Logger, accounts, sessions, backup, journal, launchers, library, artwork, launch, clipboard, updates | `E\src\main\services\*`, `E\src\main\updates\*` |
| `ol-api` | `Backend` composition root, envelope, validators, `require_idle`, `dispatch_json`, events | `E\src\main\ipc\*`, `E\src\shared\ipc-contract.ts` |
| `ol-platform-win` | The only crate with `unsafe` and the `windows` dependency | `E\src\main\platform\*`, parts of `index.ts`, `tray.ts` |
| `ol-net` | `HttpClient` on ureq with host allowlists and caps; the Riot loopback POST | fetch use in `artwork.ts`, `riot\local-launch.ts` |
| `ol-platform-fake`, `ol-sim` | Static registry, processes and login item, recording clipboard, operation log; simulators, scenario and knobs | `registry.ts` (StaticRegistry), `processes.ts` (Static/Sim controllers), `E\src\main\launchers\sim\*` |
| `ol-testkit`, `ol-replay` | Vector loader, JSON normalisers; headless replay runner | new |
| `ol-testkit-macros` | The `#[parity]` attribute that marks a test ported from the oracle; a proc-macro crate with no dependency, re-exported by `ol-testkit` | new |
| `ol-ui-kit`, `ol-ui`, `ol-automation`, `ol-app` | The only GPUI crates. `ol-app` builds `OneLaunch.exe` | `E\src\renderer\**`, `index.ts` |
| `xtask`, `installer\` | Packaging, `latest.yml`, notices, sign tool, lint gates | `E\scripts\*`, `E\build\installer.nsh` |

Enforcement:

- `#![forbid(unsafe_code)]` is the first line of code in every crate root except `ol-platform-win`'s. The spikes under `spikes/` are exempt, and have no place in the crate graph.
- The CI core job tests the workspace with the four GPUI crates excluded.
- Pure crates use `winpath` instead of `std::path`, so they also test on Linux runners.
- Crate graph and public-API snapshot are checked in CI.

### Crate graph

The diagram above gives the layers. This table is the record: `cargo xtask check` reads it and fails when a manifest and a row differ, when a crate outside the four GPUI crates reaches a `gpui` package through any dependency, or when a crate depends on a spike. "Depends on" is the crate's dependencies and build dependencies on other members, optional ones included; "In tests" is its dev-dependencies.

| Crate | Depends on | In tests |
| --- | --- | --- |
| `ol-model` | none | `ol-testkit` |
| `ol-formats` | `ol-model` | `ol-testkit` |
| `ol-platform-api` | `ol-model` | `ol-testkit` |
| `ol-runtime` | `ol-model`, `ol-platform-api` | `ol-testkit` |
| `ol-store` | `ol-model` | `ol-testkit` |
| `ol-vault` | `ol-model`, `ol-platform-api` | `ol-platform-win`, `ol-testkit` |
| `ol-adapters` | `ol-formats`, `ol-model`, `ol-platform-api`, `ol-runtime`, `ol-store`, `ol-vault` | `ol-platform-fake`, `ol-sim`, `ol-testkit` |
| `ol-services` | `ol-adapters`, `ol-formats`, `ol-model`, `ol-platform-api`, `ol-runtime`, `ol-store`, `ol-vault` | `ol-platform-fake`, `ol-sim`, `ol-testkit` |
| `ol-api` | `ol-adapters`, `ol-model`, `ol-platform-api`, `ol-runtime`, `ol-services`, `ol-store`, `ol-vault` | `ol-platform-fake`, `ol-sim`, `ol-testkit` |
| `ol-platform-win` | `ol-model`, `ol-platform-api` | `ol-testkit` |
| `ol-net` | `ol-model`, `ol-platform-api` | `ol-testkit` |
| `ol-platform-fake` | `ol-model`, `ol-platform-api` | `ol-testkit` |
| `ol-sim` | `ol-formats`, `ol-model`, `ol-platform-api`, `ol-platform-fake`, `ol-runtime` | `ol-testkit` |
| `ol-testkit-macros` | none | none |
| `ol-testkit` | `ol-testkit-macros` | none |
| `ol-replay` | `ol-api`, `ol-formats`, `ol-model`, `ol-platform-api`, `ol-platform-fake`, `ol-runtime`, `ol-sim`, `ol-testkit`, `ol-vault` | none |
| `ol-ui-kit` | none | `ol-testkit` |
| `ol-ui` | `ol-api`, `ol-model`, `ol-runtime`, `ol-ui-kit` | `ol-platform-fake`, `ol-sim`, `ol-testkit` |
| `ol-automation` | `ol-api`, `ol-model`, `ol-platform-fake`, `ol-ui-kit` | `ol-testkit` |
| `ol-app` | `ol-api`, `ol-automation`, `ol-model`, `ol-net`, `ol-platform-api`, `ol-platform-fake`, `ol-platform-win`, `ol-replay`, `ol-runtime`, `ol-sim`, `ol-ui`, `ol-ui-kit` | `ol-testkit` |
| `xtask` | none | none |

In `ol-app`, `ol-automation` is behind the feature `automation`, and `ol-platform-fake`, `ol-sim` and `ol-replay` behind the feature `fake`.

## Platform traits

- `RegistryReader` / `RegistryWriter`: the methods of `E\src\main\platform\registry.ts` (6 read, 4 write).
- `ProcessLister` (`is_running`, `any_running`, `running_names`, `list_processes`) and `ProcessRunner` (`start`, `close`, `kill{tree}`).
- `Shell` (`open_url`, `open_path`, `pick_file{title, filter}`).
- `Clipboard` (`write_text`, `read_text`, `clear`, `write_private`).
- `DataProtector` (DPAPI) beneath `Vault`.
- `HttpClient`, `LoopbackTls`, `LoginItem`, `InstallProbe`, `Roots`.
- `Clock` (`now_utc`, `instant`, `sleep`) and `IdSource`.

As built in slice 3, launcher files go through a port as well: `FileReader` and `FileWriter` (`write_atomic`). The fake behind them, `ScratchFiles`, is the real disk under the scenario folder and the data folder and refuses every other path, so a test cannot reach a real launcher's files even by mistake. A snapshot's manifest is written through the same port, as the Electron app writes it through its atomic writer.

Launcher-owned writes are the four functions at the top of `crates/ol-services/src/switch.rs`, each through `FileWriter` or `RegistryWriter`; no adapter is handed a writer. The `LauncherWrite` token that would make this a compile error is not built.

## Adapter trait

From `E\src\main\launchers\types.ts`. The overloaded `switchAccount` is split into `plan_switch` and `switch_account`.

```rust
#[async_trait(?Send)]
pub trait LauncherAdapter {
  fn id(&self) -> LauncherId;
  fn display_name(&self) -> &'static str;
  async fn detect_install(&self) -> Result<Option<LauncherInstall>>;
  fn chosen_install_dir(&self) -> Option<WinPathBuf>;
  fn install_from_exe(&self, exe: &WinPath) -> Option<LauncherInstall>;
  async fn list_installed_games(&self) -> Result<Vec<InstalledGame>>;
  async fn list_remembered_accounts(&self) -> Result<Vec<RememberedAccountEntry>>;
  async fn is_running(&self) -> Result<bool>;
  async fn running_game(&self) -> Result<Option<RunningGame>>;
  async fn shutdown(&self, timeout: Option<Duration>, cancel: &CancelToken) -> Result<()>;
  async fn preflight(&self, a: &AccountRef, for_game: Option<&GameRef>) -> Result<SwitchPreflight>;
  async fn plan_switch(&self, a: &AccountRef, game: Option<&GameRef>) -> Result<SwitchPlan>;
  async fn switch_account(&self, a: &AccountRef, ctx: &mut SwitchCtx<'_>) -> Result<SwitchResult>;
  async fn launch_game(&self, g: &GameRef, cancel: &CancelToken, started_by_switch: bool) -> Result<LaunchSent>;
  async fn game_icon(&self, g: &GameRef) -> Result<Option<WinPathBuf>> { Ok(None) }
  async fn restore_backup(&self, id: &str) -> Result<RestoreResult>;
  async fn health_check(&self, known: Option<KnownLauncherState>) -> Result<HealthReport>;
  fn sessions(&self) -> Option<&dyn SessionCapable> { None }
}
```

`SwitchCtx` carries `confirm_close`, `cancel`, a `SwitchSink` (progress, backup, attention), a password callback and the game. `AdapterError::code()` returns the TS error codes unchanged.

## Async model

Target: runtime-agnostic `async fn`, driven by a single-threaded executor on one dedicated backend thread. As built in slice 3, there is no executor: the adapter and the switch pipeline are plain blocking code over the ports, a launch job runs on a thread of its own (`switch`) that never touches the database and hands its result to the backend thread, and every wait goes through `Clock::sleep` with a `CancelToken`. The fake clock moves time only when something sleeps, which gives the same determinism the virtual-clock executor was for. The points below describe the target.

- The TS code is cooperative and single-threaded: the job, the IPC handlers and the simulator timers interleave only at `await`, and nothing is locked. A single-threaded executor reproduces that exactly, so `Backend` owns the DB, services and adapters without `Arc<Mutex>`.
- Commands arrive on an `async-channel` with oneshot replies; events leave on a watch channel.
- Registry, process and SQLite calls are native and fast, so they run inline, as better-sqlite3 does today. HTTP and hashing go to a small worker pool behind `HttpClient`.
- `AbortSignal` becomes `CancelToken`. `sleep(ms, &cancel)` never errors. `wait_until` keeps the exact order: abort, condition, deadline, sleep.
- Virtual clock: the test executor runs until stalled, then jumps virtual time to the next timer. That is the Node fake-timer model the existing tests were written against, so replay and the operation log are deterministic.

Rejected: threads and channels (virtual time needs quiescence detection across threads; ordering becomes nondeterministic), GPUI's executor in the core (drags GPUI in and forces `Send` and locks), tokio (unnecessary weight).

## Command surface

`ApiCall { const CHANNEL; type Req; type Res }` plus `Backend::dispatch_json(channel, Value) -> Envelope`. Validators are hand-ported over `serde_json::Value`, because the `BAD_REQUEST` texts are behaviour (`E\src\main\ipc\handle.ts`). The UI's typed `invoke::<C>` serialises through `dispatch_json`, so replay covers exactly the path the UI uses. `launch:status` stays as a query for replay even though the UI consumes events.

## UI

State:

- Entity stores replace zustand (`E\src\renderer\src\store\ui.ts`): `Nav`, `Overlays`, `LaunchStore`, `SettingsStore`, `LibraryStore`, `AccountsStore`, `LaunchersStore`, `UpdatesStore`.
- `Query<T>` (`data`, `error`, `loading`, `reload()`, seeded from the startup bundle) replaces `useIpcQuery`.
- One foreground task pumps `BackendEvent::{Job, JobFinished, LibraryChanged, ArtworkStatus, UpdateStatus, SettingsChanged, AccountsChanged}` into the stores. This removes the 150 ms, 0.5 to 4 s and 1 to 60 s polls.
- User-visible timers stay: 8 s banner dismissal, flash durations, 30 s clipboard clear.

Structure mirrors the renderer files one to one: shell (`title_bar`, `sidebar`, `launch_host`), views (`library`, `accounts`, `account_detail`, `settings`), dialogs (`launch`, `confirm`, `recovery`, `import_accounts`, `setup_guide`, `whats_new`).

GPUI Kit is adopted narrowly:

| Decision | Components | Reason |
| --- | --- | --- |
| Adopt, wrapped and themed | `Input` (masked plus reveal), `Textarea` | IME, selection, clipboard and undo are the expensive pieces; `TagInput` builds on it |
| Wrap if geometry allows, else hand-build | Scrollbar, FocusTrap | The app's scrollbar is a 6 px thumb inset 3 px, shown on hover (`E\src\renderer\src\index.css`) |
| Hand-build in `ol-ui-kit` | TitleBar, Dropdown, MenuButton, CardMenu, Modal, Tabs, Toggle, Button, StatusPill, Panel, Field, Collapsible, Tooltip, Spinner, poster grid | The originals are small with exact rules (6 px offset, 8 px viewport clamp, flip above, re-select deselects, Tab closes). Restyling Kit costs more than writing them |
| Do not use | Kit TitleBar, Dialog, Select, Menu, Notification, TextView, Theme | Same reason; the 36 px strip and caption buttons use `WindowControlArea` directly |

Known parity gaps to confirm by spike and settle in the triage table: backdrop blur (4 uses), inset shadow in the panel shadow, icon drop-shadow, scale transforms on non-SVG elements, 150 ms colour transitions, glyph rasterisation. CDP baselines do not contain the native caption-button overlay, so that rectangle is masked in comparisons.

Poster grid: virtualised by rows; columns computed from `repeat(auto-fill, minmax(160px, 1fr))` with gap 16, and 190/20 at 1536 px and wider (`E\src\renderer\src\views\LibraryView.tsx`). Decode at display size off the UI thread into an LRU; 300 full-size posters would be about 650 MB.

## Automation

- `ol-automation` sits behind feature `automation`, with `compile_error!` when combined with a release profile. CI also scans the release exe for the pipe-name sentinel.
- Transport: `\\.\pipe\onelaunch-automation-<pid>`, current user only, JSON lines, started only with `--automation`.
- Element source: every kit component takes a stable id, role and label and sets GPUI accessibility properties from the renderer's ARIA attributes. The server serialises the AccessKit tree (id, role, name, value, bounds, states) as both the query tree and the UI-parity sidecar. If bounds or text are incomplete, a `Probe` wrapper records them at prepaint into a per-frame registry.
- Verbs: `tree`, `click`, `type`, `key`, `wait_for`, `wait_idle`, `screenshot`, `api.invoke`, `platform.oplog`. State-changing `api.invoke` is disabled in UI scenarios.
- Input goes through GPUI event dispatch or `PostMessage` to the HWND; never `SendInput`.
- Automation mode: bundled fonts, animations off, virtual clock, fixed scale.
- Screenshots: Windows.Graphics.Capture on the HWND, cropped to the client rect, rounded corners masked, fixed 1280x800 at 100%. Fallback is `PrintWindow(PW_RENDERFULLCONTENT)`.

## Dependencies

Approved:

- UI: `gpui-pre` (exact pin), `gpui-pre-platform`, `gpui-kit` (exact pin, default features off, `ol-ui-kit` only), `image`.
- Storage and platform: `rusqlite` (bundled, backup), `windows` (`ol-platform-win` only).
- Serialisation: `serde`, `serde_json` with `preserve_order`.
- Crypto: `aes-gcm`, `sha2`, `base64`, `ed25519-dalek`, `getrandom`, `zeroize`.
- Utility: `uuid`, `time`, `semver`, `regex` (linear time), `thiserror`; `anyhow` in binaries only.
- Async: `futures-lite`, `async-channel`, `event-listener`, `async-task`, `async-trait`.
- Network: `ureq` with rustls.
- Build and tools: `winresource`, `cargo-deny`, `cargo-about`.
- Conditional on the tray spike: `tray-icon`, `muda`.

Banned:

- Process and registry: `sysinfo`, `heim`, `wmi`, any tasklist wrapper, `winreg`.
- Stack: `tokio`, `reqwest`, `openssl`, `native-tls`, `chrono`, `fancy-regex`.
- Generic parsers that would replace the byte-preserving ports: `serde_yaml`, `serde_yml`, `keyvalues-parser`, ini crates, `prost`.
- Wrappers implemented in-house: `arboard`, `clipboard-win`, `keyring`, DPAPI wrappers, single-instance and auto-launch crates, `rfd`.
- Updaters: `self_update`, `velopack`, the cargo-packager updater.
- Lineage: crates.io `gpui` 0.2.x and all git sources.

A ban covers our crates and everything they pull in. GPUI itself brings two banned crates, `chrono` and, in a build script, `winreg`; `deny.toml` allows each under the crates that bring it and nowhere else (`../decisions/M1-gpui-pin.md`). Every version in `[workspace.dependencies]` is exact, and every member takes its dependencies from there.

## Risks the design contains

1. Takeover is a one-way door (the shared staging id makes staged rollout impossible). Symmetric installer contract, frozen schema and vault, CI contract suite from Milestone 1, owner updates first.
2. Visual parity is not pixel-achievable. Sidecar-first comparison, text-metrics spike, triage table.
3. GPUI and Kit churn. Exact pins, GPUI in four crates, Kit for input only, vendoring fallback.
4. Anti-cheat rules regress through a transitive dependency. Deny list, trace test in CI, a Riot image-name guard at the lowest layer of `ol-platform-win`.
5. Adapter semantics drift across 6.4k lines of timing-sensitive code. Steam slice first, single-threaded executor, vectors for every parser, replay on a virtual clock.
6. Both apps on one data folder. No migrations, `Local State` keys preserved; a lockfile guard is an unverified idea for the vault spike.
7. Windows shell gaps in GPUI (hide, session end, tray). HWND subclass and a helper window inside `ol-platform-win`, proven by spike.
