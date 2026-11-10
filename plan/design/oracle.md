# The oracle

Design for Milestone 0: the reference bundle captured from the Electron app, which every later check diffs against. `E\` is the Electron repo at the pinned commit. Derived from reading the source; three CDP and build assumptions are untested and sit in ticket T4.

All tooling lives on the branch `oracle-tooling` in the Electron repo. It never merges and never ships. Output is written to this repo's `oracle/`.

## Layers

| Layer | Path | Produced by |
| --- | --- | --- |
| Vectors | `oracle/l1/` | Vitest generator files that import the real TS functions |
| Test catalogue and result goldens | `oracle/tests/` | `vitest run --reporter=json` plus a setup shim |
| Replay scripts and expectations | `oracle/l2/` | A headless vitest harness with the real services and fake launchers |
| UI ground truth | `oracle/ui/` | The CDP driver, extended with a layout sidecar |
| Copy, tokens, icons, data | `oracle/assets/` | Compiler walk, style aggregation, server-side icon render |
| Performance baseline | `oracle/perf/` | Extended `startup-timing` scenario on the packaged app |
| Takeover fixtures and contract | `oracle/takeover/` | See `takeover.md` |
| Manifest | `oracle/MANIFEST.json` | Pin, tooling commit, per-file sha256 |

## Vectors

- Generator files under `E\scripts\oracle\l1\` with their own vitest config. `scripts/` is eslint-ignored and outside the main vitest include, so `npm run check` is unaffected.
- Inputs are bytes, never JSON strings, because BOM, CRLF and invalid UTF-8 matter.
- The existing `fixtures/_generator/gen-fixtures.test.ts` resolves the repo as `../../..`, so it only runs when the folder sits inside the repo; relocate or junction it.

Targets:

| Area | Functions |
| --- | --- |
| Steam | `parseVdf` and getters, `parseLoginUsers`, `planAutoLoginRewrite`, `parseLibraryFolders`, `parseAppManifest`, `parseLoginStates`, `classifyReason`, `outcomeSince`, `updateInProgress` |
| Epic | `splitIni`, `joinIni`, `rewriteSection`, `parseItemManifest`, `parseLauncherInstalled`, `launchUri`, `parseLogEvents`, `lastSignIn`, `isSessionRejection`, `planSessionWrite`, blob serialise and parse |
| Battle.net | `parseConfig`, `serializeConfig`, `planSavedAccountNames`, client-log events, `productStates`, `PRODUCTS`, `parseProductDb`, `parseAggregate`, `launchCommand`, token blobs v1 and v2 |
| Riot | `parseLogEvents` over the 7 fixture logs, `readSession`, `spliceSessionBlock`, `planSignOut`, `parseProductSettings`, `parseInstalls`, `launchArgs`, `parseLockfile` |
| Core | `canonicalPayload`, `verifyManifest` per refusal, `updateAvailability`, `redactString`, `redactValue`, `formatLine`, `resolveArtworkPath`, `sniffImage`, `normalizeLauncherPath`, `pathKey`, `dataDirFromArgv`, window-guard truth tables, `loginItemAvailable`, `compareVersions`, `pickWhatsNew`, v1 to v5 migration schema dump |
| IPC | Bad-payload matrix per channel, giving the envelope |
| Renderer | `lib/phases.ts`, `lib/recovery.ts`, `lib/time.ts`, sort comparators |

Inputs come from `E\test\fixtures\**`, `oracle/fixtures/**`, literals lifted from each test file, and a seeded mutation corpus for the byte-exact rewriters. Add adversarial cases where JS and Rust differ: `\d`, `\s` and `\b` Unicode-ness, UTF-16 length limits with astral characters, `parseInt` leniency, lone surrogates in JSON, `localeCompare(..., {sensitivity:'base'})` ordering.

Translate by hand instead of generating vectors: the four adapter suites (248 tests, which script `onStart` callbacks mid-flow), the stateful services (`launch-service`, `pipeline`, `backup`, `switch-journal`, `artwork`, `update-service`, `db`, `accounts`), and renderer behaviour tests (as GPUI tests). Ticket-prep does the translation, never the implementer.

Waiver candidates: `preload`, `use-ipc-query`, the `drive-*` tooling tests, and the `reg.exe` and PowerShell output parsers once the platform layer is native.

## Test catalogue

`vitest run --reporter=json` gives file, `ancestorTitles`, title and status with `it.each` expanded. A `setupFiles` shim wraps adapter `preflight`, `switchAccount`, `launchGame` and `restoreBackup` and appends `{currentTestName, method, result}`, so each hand-ported Rust test also compares the full result JSON. No `src/` change.

## Replay

The source is a headless harness, not UI-driven Electron runs. `E\test\pipeline.test.ts` already composes real adapters, simulators, `LaunchService` and `registerIpcHandlers` with `electron` mocked and `FakeVault`. That removes renderer polls, CDP and DPAPI from the transcript. Electron is used only to cross-check five scripts.

Scripts are state-gated, not time-ordered (`stop` and `handover` act when a step is reached):

```json
{"fake":"all-four","knobs":{},"steps":[
 {"op":"invoke","channel":"launch:start","req":{"accountId":"@acct:steam:user_3","gameId":"@game:steam:252950","confirmClose":true}},
 {"op":"awaitJob","job":"@job:1","until":{"step":"wait","status":"running"}},
 {"op":"crash"},{"op":"restart"},{"op":"snapshot"}]}
```

Harness: lay files with the driver's `materializeScenario` (`E\scripts\app-driver.mjs`), build the platform with the app's own wiring (`E\src\main\launchers\sim\scenario.ts`), wire services in `E\src\main\index.ts` order, record at the mocked `ipcMain.handle`.

Gaps in `fake-platform.json` (`E\src\main\launchers\sim\report.ts`) that the hooks close:

- Each kind has its own array, so cross-kind order (close, write, start) is lost. `FakeReport` gets an `onEvent` with a global `seq`.
- The kill listener drops the tree flag.
- File writes are not recorded. All adapter writes funnel through `writeFilesAtomic` (`E\src\main\services\backup.ts`), so one observer there separates adapter writes from simulator writes.
- Clipboard clears and login-item sets are absent.
- A 150 ms poll misses short job states. An optional `trace` dependency on `LaunchService` is called from `timeSteps`, `onAttention` and `finally` (`E\src\main\services\launch.ts`).

The IPC recorder goes in `E\src\main\ipc\handle.ts` (fold the three returns into one envelope and call `opts.record?.()`), gated like `resolveFakeLaunchersDir` in `index.ts`: unpackaged and fake mode only. It redacts `accounts:create.password`, `accounts:setPassword.password`, `vault:setSecret.value` and the `vault:getSecret` response. Its real use is the map from UI interaction to IPC call.

Excluded from replay: `app:openPath` and `app:openUrl` (call `shell` directly even in fake mode), `launchers:locate` and `artwork:setManual` (native dialogs).

Normalisation:

| Source of variance | Rule |
| --- | --- |
| UUIDs | Semantic aliases: `@acct:steam:user_3`, `@game:steam:252950`, `@job:n`, `@backup:steam:n` |
| ISO timestamps, `ms` values | `<ts>`, `<ms>` |
| Scratch roots | `{launchers}`, `{data}` in all four encodings (raw, lower-case forward-slash, VDF-escaped, JSON-escaped); this also keeps the Windows username out |
| `*_enc` columns | `{present, plaintextSha256}` through the harness vault |
| Simulator-stamped files | Compare parsed projections, not bytes |
| Poll channels | Collapse to distinct states |
| Job stream | Subsequence match; platform stream exact order; final state exact |
| Log lines | Advisory, except the WARN and ERROR set |

Which fields are volatile is measured, not guessed: each script is recorded several times, and only fields that differ between those runs may be normalised. The normaliser is protected oracle code.

Expected output shape:

```json
{"events":[{"seq":3,"k":"job","job":"@job:1","state":"running","steps":[["close","running","Close Steam",""]]},
  {"seq":4,"k":"plat","op":"proc.close","image":"steam.exe"},
  {"seq":5,"k":"plat","op":"file.write","path":"{launchers}\\steam\\config\\loginusers.vdf","sha256":"..."},
  {"seq":6,"k":"plat","op":"reg.set","path":"HKCU\\...\\AutoLoginUser","value":"user_3"}],
 "final":{"db":{"user_version":5,"tables":{}},"journal":null,"backups":{},"launcherFiles":{},"log":{"warn":[],"error":[]}}}
```

Scenario classes (30 files in `E\scripts\scenarios\`):

- Headless replay (20): `play-all`, `play-steam`, `play-epic`, `play-battlenet`, `play-riot`, `already-active`, `elevated`, `game-running`, `handover`, `import`, `needs-update`, `preview`, `recovery`, `restore`, `riot-stay-signed-in`, `rollback`, `self-update`, `stop`, `unowned`, `wrong-account`.
- Partial (2): `quit-during-switch` (the shutdown op only), `first-run` (empty-platform reads).
- Through Electron only (1): `update`; the engine needs electron-updater.
- UI-only (2): `views`, `whats-new`.
- OS and window (2): `close-to-tray`, `start-with-windows`.
- Packaging and performance (2): `packaged-smoke`, `startup-timing`.
- Utility (1): `hold`.

## UI ground truth

- `Driver.shot()` is a bare `Page.captureScreenshot`; `viewport()` sets device metrics at scale factor 1; `d.send()` passes any CDP method, so the driver needs no change.
- Sidecar: `DOMSnapshot.captureSnapshot` with a `computedStyles` whitelist, `includePaintOrder` and `includeDOMRects` gives line-broken text boxes and per-node styles in one call. Join `Accessibility.getFullAXTree` on `backendDOMNodeId` for role, name and state. An injected script only for scroll metrics, focus and `document.getAnimations()`.
- Style tables: aggregate the snapshot's styles by tag plus class signature. Ship the compiled `out/renderer/assets/index-*.css` for hover, focus and disabled rules, plus `CSS.forcePseudoState` for about 12 primitives.
- Determinism: `Emulation.setLocaleOverride` and `setTimezoneOverride` (the UI uses `Intl.RelativeTimeFormat(undefined)` and `toLocale*`); finish finite animations and pause infinite ones at t=0; wait for `document.fonts.ready` and all images; mark time-like text `volatile`; point `TEMP` at a neutral folder, because Settings › Advanced renders the data path and PNGs cannot be secret-scanned.
- The Rust side matches sidecar elements by (role, name, ordinal).

```json
{"id":72,"viewport":[1280,800],"dpr":1,"locale":"en-US","tz":"UTC","focus":"button:Close Steam and continue",
 "text":[{"t":"...","box":[412,318,356,20],"font":["Segoe UI Variable Text",14,400,20.3],"color":"#e8eaf0","volatile":false}],
 "elements":[{"role":"button","name":"...","ord":0,"box":[],"state":{"disabled":false,"checked":null,"expanded":null},
   "style":{"bg":"#8b5cf6","radius":8,"padding":[8,14,8,14]},"cls":"..."}]}
```

The 174 rows of `SCREENSHOTS.md`:

| Bucket | Rows | Notes |
| --- | --- | --- |
| Automatic | about 145 | About 100 already name a scenario; the rest need knobs, custom scenario folders (13, 16, 63, 76, 136), planted journal fixtures (102, 104, 105), a held loopback-feed response (125, 127), deleting `chromium/` (157 to 159), or a direct DB edit (137) |
| Need a stall trick | 9 (1, 11, 20, 23, 62, 71, 111, 133, 143) | Transient states |
| Owner | 12 (164 to 174, 138) | Native dialogs, tray, installer, SmartScreen |
| Skip unless fault-injected | 8 (8, 14, 36, 89, 92, 160, 162, 163) | |

One more owner capture: real window chrome. Caption buttons are an OS overlay and never appear in CDP shots.

## Assets and copy

- Copy inventory: a TypeScript-compiler walk over `src/` collects JSX text, literals and templates with `file:line`, unioned with text observed in sidecars and replay step titles.
- Lucide: 54 distinct imports; render each with `react-dom/server` for exact SVG.
- `.ico` from the `npm run build` output; tray PNGs via a `scripts/render-icon.cjs`-style script so the resize output is exact.
- Tokens from `E\src\renderer\src\index.css`, checked against runtime `getComputedStyle`.
- What's New entries as JSON. Notices are npm-specific; carry only the MIT text and install location.

## Performance baseline

- Extend `E\scripts\scenarios\startup-timing.mjs` (packaged) to at least 10 warm starts.
- PIDs from the browser target's `SystemInfo.getProcessInfo`, then per-PID memory and CPU. Never match by image name; the owner's installed copy may be in the tray.
- Idle CPU visible and hidden, with `oracle/fixtures/db/onelaunch-large-v5.db`.
- `launchers:status` latency, read-only against real launchers.
- Fake mode is unpackaged-only, so switch flows have no packaged baseline.

## Triage linkage

Each row of `plan/triage.toml` lists the artefacts it affects (`affects`). Where the owner decides `fix` or `drop`, the affected expectations are regenerated or marked so the diff is not permanently red on an intentional difference.

## What will not work

- Pixel diffs as a gate. Gate on sidecar boxes; treat PNGs as review material.
- Fakes cover none of the code rewritten from scratch (the native registry and process layer). Port the five `*-live` tests and compare hashed digests on the owner's machine; nothing is committed.
- DPAPI artefacts cannot be golden files. Vault parity is a same-job cross-read: Electron writes and Rust reads, then the reverse.
- Hand-built per-variant style tables and UI-recorded IPC transcripts as the replay source are not worth their cost; the aggregation and the scripted harness replace them.

## After a critical fix to Electron

The fix lands on `master` through its usual PR. `oracle-tooling` is rebased onto the new commit, the affected layers are re-captured, `oracle/PIN` and `MANIFEST.json` move, and the spec is amended in the same owner-approved change.
