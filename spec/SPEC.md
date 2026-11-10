# One Launch: behavioural specification for the rewrite

This document describes what the One Launch Electron app does today, in enough detail to rebuild it as a native Windows app and to check the rebuild against it. It was written from the TypeScript source and the project's own docs; every claim cites `path:line` in the one-launch repository (version `1.0.0-beta.3`, after pull request #36). Section 1 explains the conventions. Companion files: `fixtures/` (sample copies of every persisted format, with `fixtures/README.md`) and `SCREENSHOTS.md` (the capture checklist for section 2).

## Contents

- [1. Overview](#1-overview)
  - [Reading this document](#reading-this-document)
- [2. Screens & UI](#2-screens--ui)
  - [App shell](#app-shell)
  - [Sidebar](#sidebar)
  - [Library view](#library-view)
  - [Accounts view](#accounts-view)
  - [Account detail: create form](#account-detail-create-form)
  - [Account detail: edit page](#account-detail-edit-page)
  - [Import accounts dialog](#import-accounts-dialog)
  - [Launch dialog](#launch-dialog)
  - [Launch banner](#launch-banner)
  - [Confirm dialog](#confirm-dialog)
  - [Recovery dialog](#recovery-dialog)
  - [Setup guide](#setup-guide)
  - [What's New card](#whats-new-card)
  - [Settings view](#settings-view)
  - [Native dialogs (main process)](#native-dialogs-main-process)
  - [Global behaviours](#global-behaviours)
- [3. Data model](#3-data-model)
  - [3.1 Enumerations](#31-enumerations)
  - [3.2 Entities](#32-entities)
  - [3.3 SQLite schema (as it stands at user_version = 5)](#33-sqlite-schema-as-it-stands-at-userversion--5)
  - [3.4 Files outside the database](#34-files-outside-the-database)
  - [3.5 Cross-entity invariants](#35-cross-entity-invariants)
- [4. Persistence](#4-persistence)
  - [4.1 The data folder and how it is chosen](#41-the-data-folder-and-how-it-is-chosen)
  - [4.2 Layout](#42-layout)
  - [4.3 The database](#43-the-database)
  - [4.4 Encryption (the vault) and where the key lives](#44-encryption-the-vault-and-where-the-key-lives)
  - [4.5 Renderer-side storage](#45-renderer-side-storage)
  - [4.6 The switch journal (switch.json)](#46-the-switch-journal-switchjson)
  - [4.7 Backups of launcher files (BackupService)](#47-backups-of-launcher-files-backupservice)
  - [4.8 Logs](#48-logs)
  - [4.9 Artwork cache](#49-artwork-cache)
  - [4.10 Outside the data folder](#410-outside-the-data-folder)
  - [4.11 Developer-only files](#411-developer-only-files)
- [5. IPC & main-process logic](#5-ipc--main-process-logic)
  - [5.1 Transport](#51-transport)
  - [5.2 Channel table](#52-channel-table)
  - [5.3 Channels in detail](#53-channels-in-detail)
  - [5.4 The launcher gateway (LauncherService)](#54-the-launcher-gateway-launcherservice)
  - [5.5 Launcher adapters](#55-launcher-adapters)
- [6. Background work](#6-background-work)
  - [6.1 Startup timeline](#61-startup-timeline)
  - [6.2 Quit timeline](#62-quit-timeline)
  - [6.3 Timers, polls and watchers](#63-timers-polls-and-watchers)
  - [6.4 Network work](#64-network-work)
  - [6.5 The library scan](#65-the-library-scan)
- [7. External integrations](#7-external-integrations)
  - [7.1 What leaves the PC, and what never does](#71-what-leaves-the-pc-and-what-never-does)
  - [7.2 Artwork: Steam (no key)](#72-artwork-steam-no-key)
  - [7.3 Artwork: SteamGridDB (api/v2, key required)](#73-artwork-steamgriddb-apiv2-key-required)
  - [7.4 Artwork: image downloads (common rules)](#74-artwork-image-downloads-common-rules)
  - [7.5 Updates (electron-updater over GitHub releases)](#75-updates-electron-updater-over-github-releases)
  - [7.6 Hand-offs to the system shell (shell.openExternal, shell.openPath)](#76-hand-offs-to-the-system-shell-shellopenexternal-shellopenpath)
  - [7.7 The Riot Client loopback request](#77-the-riot-client-loopback-request)
  - [7.8 The onelaunch-art: protocol handler](#78-the-onelaunch-art-protocol-handler)
- [8. OS integration](#8-os-integration)
  - [8.1 The main window](#81-the-main-window)
  - [8.2 Window placement memory](#82-window-placement-memory)
  - [8.3 Single instance, identity, hardening](#83-single-instance-identity-hardening)
  - [8.4 Tray icon](#84-tray-icon)
  - [8.5 Close, hide and quit rules (src/main/window-guard.ts)](#85-close-hide-and-quit-rules-srcmainwindow-guardts)
  - [8.6 Native dialogs owned by main](#86-native-dialogs-owned-by-main)
  - [8.7 Start with Windows](#87-start-with-windows)
  - [8.8 Installer and uninstaller (electron-builder NSIS)](#88-installer-and-uninstaller-electron-builder-nsis)
  - [8.9 Install detection and update installation](#89-install-detection-and-update-installation)
  - [8.10 Protocol scheme, folders and URLs handed to the shell](#810-protocol-scheme-folders-and-urls-handed-to-the-shell)
  - [8.11 Clipboard](#811-clipboard)
  - [8.12 Process listing and control (src/main/platform/processes.ts)](#812-process-listing-and-control-srcmainplatformprocessests)
  - [8.13 Registry access (src/main/platform/registry.ts)](#813-registry-access-srcmainplatformregistryts)
  - [8.14 Deliberately absent](#814-deliberately-absent)
- [9. Settings](#9-settings)
  - [9.1 Storage semantics (SettingsService)](#91-storage-semantics-settingsservice)
  - [9.2 Validation in settings:set](#92-validation-in-settingsset)
  - [9.3 The keys](#93-the-keys)
  - [9.4 What's New seeding on a brand-new database](#94-whats-new-seeding-on-a-brand-new-database)
  - [9.5 Main-only keys](#95-main-only-keys)
  - [9.6 Renderer-side preferences (not settings)](#96-renderer-side-preferences-not-settings)
- [10. Dependencies](#10-dependencies)
  - [Runtime dependencies (shipped in the installer)](#runtime-dependencies-shipped-in-the-installer)
  - [Dev dependencies bundled into the renderer at build time (ship as part of the page bundle)](#dev-dependencies-bundled-into-the-renderer-at-build-time-ship-as-part-of-the-page-bundle)
  - [Build, packaging and tooling (never shipped)](#build-packaging-and-tooling-never-shipped)
  - [Windows programs the app shells out to (not packages, but dependencies all the same)](#windows-programs-the-app-shells-out-to-not-packages-but-dependencies-all-the-same)
  - [Other Rust crates the spec implies](#other-rust-crates-the-spec-implies)
- [11. Known issues & bloat](#11-known-issues--bloat)
  - [11.1 Memory and CPU](#111-memory-and-cpu)
  - [11.2 Duplicated state and code](#112-duplicated-state-and-code)
  - [11.3 Dead or unused surface](#113-dead-or-unused-surface)
  - [11.4 Development-only weight in the production bundle](#114-development-only-weight-in-the-production-bundle)
  - [11.5 Behaviour that looks like a bug or a trap (keep out, or fix knowingly)](#115-behaviour-that-looks-like-a-bug-or-a-trap-keep-out-or-fix-knowingly)
- [12. Open questions](#12-open-questions)
  - [12.1 Product decisions for the rewrite](#121-product-decisions-for-the-rewrite)
  - [12.2 Where the docs and the code disagree](#122-where-the-docs-and-the-code-disagree)
  - [12.3 Behaviour the code leaves unverified or unobserved](#123-behaviour-the-code-leaves-unverified-or-unobserved)
- [Appendix A. Development-only surfaces [dev-only]](#appendix-a-development-only-surfaces-dev-only)
  - [A.1 Fake launcher platform](#a1-fake-launcher-platform)
  - [A.2 scenario.json (read by the app) and knobs.json](#a2-scenariojson-read-by-the-app-and-knobsjson)
  - [A.3 Test doubles that ship](#a3-test-doubles-that-ship)
  - [A.4 Other development-only environment variables and flags](#a4-other-development-only-environment-variables-and-flags)
  - [A.5 Developer panel](#a5-developer-panel)
  - [A.6 The driver and scenarios](#a6-the-driver-and-scenarios)

## 1. Overview

One Launch is a Windows desktop application that launches any installed game, from any of four launchers, on any account the user owns: Steam, Epic Games, Battle.net and Riot Games. The user picks a game, picks an account, and plays. The app keeps many accounts per launcher (a display name, the login, optional tags and notes, an optional encrypted password, and the launcher's own account id once a sign-in has confirmed it), discovers the games each launcher has installed, shows them as a poster wall with downloaded cover art, and runs one "switch then launch" job at a time: it closes the launcher, snapshots the launcher files it is about to change, rewrites the launcher's own "who is signed in" state to the chosen account, starts the launcher again, confirms from the launcher's log and files that it signed in as that account, and then asks the launcher to start the game. It does not replace the launchers; it orchestrates them (`docs/README.md:3`).

Each launcher is switched the way its own client remembers accounts. Steam keeps a list of remembered accounts in `loginusers.vdf` plus an auto-login registry value, so a switch marks the chosen entry as the one to sign in as and, when Steam remembers no session for it, starts Steam with `-login <name>` and hands over to the user. Epic signs in from an opaque session blob in `GameUserSettings.ini`, so One Launch stores one such blob per account (encrypted) and swaps it in. Battle.net signs in from a DPAPI token in the registry that is only accepted for the first login in its saved-account list, so a switch swaps the token and reorders the list. The Riot Client signs in from one plain-text YAML session file, so a switch puts the account's saved copy of that file back, while never touching a Riot game or the Vanguard anti-cheat, never switching while a Riot game runs, and never contacting Riot's servers except for the local loopback request the client's own Play button sends (`docs/adapters/*.md`, `docs/roadmap.md:10-14`).

Two things the launchers cannot do silently are handed back to the user: two-factor prompts and expired sessions. The app then shows what to do in the launcher window (including the one checkbox the launcher must keep ticked, such as “Remember me”, “Keep me logged in” or “Stay signed in”), offers to copy the login and password to the clipboard (cleared again after 30 seconds), and waits up to ten minutes; a time-out leaves the launcher on its sign-in form and never rolls back (`docs/switching.md:79-84`). Every write to launcher state is preceded by a snapshot kept under the app's data folder and is reversible from Settings › Backups, from the rollback a failed switch performs, or from the recovery prompt the next start shows when a switch was interrupted (`docs/switching.md:106-121`).

Everything stays on the PC. There is no backend, no account, no telemetry and no crash reporting. The only outbound traffic is cover-art lookups (game names and Steam or SteamGridDB ids) and update checks against the project's GitHub releases, which carry no identifier beyond the app version and a constant staging id (`docs/README.md:9`, `src/main/updates/engine.ts:13-17`). Secrets (passwords, session blobs, the SteamGridDB key) are encrypted with Windows DPAPI for the current Windows user through Electron's `safeStorage`; the encryption key lives in the Chromium profile under the data folder, so moving the data folder to another user or deleting that profile loses every secret (`docs/architecture.md:58-66`).

The product principles, in priority order, are: local-only security; performance (cold start to an interactive window under about a second, with the library rendering at once from placeholders); a sleek, dark, artwork-forward interface rather than an admin tool; best-effort launcher compatibility, where a broken adapter never takes down the app; and nothing hard-coded to one user's machine (`docs/README.md:7-13`). Standing product decisions the rewrite must keep are listed in `docs/roadmap.md:10-30` and repeated where they apply below: Preview mode is off by default; silent switching never needs a stored password; a launcher started by something else during a switch fails the switch and is never closed; unowned sign-ins are kept in a pinned snapshot, not adopted; launcher paths come only from the app's own file dialog; artwork and Riot support are on by default with no consent gate or warning copy; and the user performs every real switch, restore and update during verification.

Scope today: accounts, switching and launching, a library of **installed** games across the four launchers, artwork, backups and recovery, a setup guide on first run, a What's New card after an update, close-to-tray, Start with Windows, and signed self-updates from GitHub releases. Out of scope: automating two-factor authentication, owned-but-not-installed games, macOS and Linux, cloud sync, telemetry (`docs/README.md:15`, `docs/roadmap.md:74-82`). The app is a private beta at version `1.0.0-beta.3` (`package.json:3`); builds are unsigned, so SmartScreen shows an unknown publisher (`docs/roadmap.md:15`).

### Reading this document

- Paths cited as `path:line` or `path:line-range` are relative to the One Launch repository (the Electron source), not to this folder. The spec was checked against the repository state at the commit that merged pull request #36 ("Instant Play"), `package.json` version `1.0.0-beta.3`.
- Behaviour marked **[dev-only]** exists only in development runs or dev builds (fake launchers, simulators, the Developer panel) and is not product behaviour; the rewrite can skip it. Appendix A describes it so the decision is informed.
- Verbatim user-facing text is quoted in “ ” or in `code`. Template slots are written `<account>`, `<game>`, `<launcher>`. Only strings that carry behaviour (errors, prompts, outcome sentences, hand-over checklists, empty-state titles, state-dependent button labels) are quoted; decorative hints are paraphrased.
- Where the code and the project's own docs disagree, or where the code's behaviour could not be determined, the item is listed in section 12 rather than guessed.

## 2. Screens & UI

The renderer is a single window with no router. Three views (Library, Accounts, Settings) sit beside a sidebar under a title bar; dialogs and the launch banner are hosted above every view (`src/renderer/src/App.tsx:79-93`). Everything below is dark-only, 14 px Segoe UI, and reaches the main process solely through typed IPC (`src/renderer/src/lib/ipc.ts:16-26`). Where a bullet quotes a string in “ ” or a code span, the string is verbatim; `<account>`, `<game>`, `<launcher>` and `<client>` are template slots (`<launcher>` is the display name, for example "Riot Games"; `<client>` is the program as it reads mid-sentence, for example "the Riot Client", see `src/shared/models.ts:10-23`).

### App shell

**Purpose.** Hosts the window chrome, the global dialogs, the launch banner and the current view.

**Layout** (`src/renderer/src/App.tsx:79-93`). A full-height column: `ConfirmDialog` (fixed overlay), `TitleBar` (36 px strip), `RecoveryHost`, `SetupGuideHost`, `WhatsNewHost` (all fixed overlays when open), `LaunchHost` (a full-width banner strip under the title bar when a job is active and the dialog is closed, plus the fixed launch dialog), then a row of `Sidebar` and a `<main>` that clips its content. The view inside `<main>` is chosen by the store: `library`, `settings`, or `accounts` with `accountRoute.kind` deciding list vs detail (`App.tsx:17-23`).

- Building blocks used everywhere: `BrandMark` (the inline single-colour mark from `build/icon-mono.svg`, `src/renderer/src/components/BrandMark.tsx`), `LauncherBadge` and `LauncherDot` (a launcher's name or colour dot, `components/LauncherBadge.tsx`), `Button` (variants in Global behaviours), `Panel`/`Row`/`Field`/`Toggle`/`Dropdown`/`MenuButton`/`Tabs`/`Collapsible`/`EmptyState`/`StatusPill`/`HScroller`, and `lib/whatsNewImages.ts`, which bundles the What's New images with `import.meta.glob`.
- Window minimum is 960 × 600 px; the window background is `#0b0d12` (`src/main/index.ts:301-305`). Main draws the Windows caption buttons over the right end of the title strip at 35 px high, one pixel shorter than the strip, in surface colour with muted symbols (`src/main/index.ts:311`).
- **Title bar** (`src/renderer/src/components/TitleBar.tsx:17-25`): a 36 px (`h-9`) drag region (`app-drag`, `src/renderer/src/index.css:129-131`) with the brand mark in accent and the text “One Launch”. Nothing interactive lives in it. When `app:info` reports `platformSource: 'fake'` a `role="status"` badge reading “Fake launchers” (warning tone, uppercase) is shown. **[dev-only]** the badge, since only fake-launcher runs produce that source. `app:info` is read from the startup prefetch when available (`TitleBar.tsx:10-14`).
- Every modal backdrop starts 36 px down (`top-9`) so the title strip stays draggable while a dialog is up (`ConfirmDialog.tsx:28`, `LaunchDialog.tsx:504`, `ImportAccountsDialog.tsx:137`, `RecoveryDialog.tsx:45`, `SetupGuide.tsx:269`, `WhatsNewDialog.tsx:89`, `BackupsPanel.tsx:45`).
- **Boot surface before first paint** (`src/renderer/index.html:10-22`, `src/renderer/src/index.css:137-169`): the HTML ships a static `.boot` block, a 36 px bar in surface colour with the same half-opacity bottom border as the title bar, over a centred 96 px brand mark in accent. The mark starts invisible and fades in over 200 ms after a 250 ms delay, so a normal start never shows it; under `prefers-reduced-motion` the fade duration is 0 ms (the mark still appears after the delay). React's first commit replaces the block.
- **Startup render gate** (`src/renderer/src/boot.ts:4-7`, `src/renderer/src/main.tsx:24-33`): the first module imported starts the startup prefetch and resolves after all startup answers are in or after 50 ms, whichever comes first; React renders only then, inside `StrictMode` and `ErrorBoundary`. See Global behaviours › Startup prefetch.
- **Uncaught errors outside React** (`main.tsx:16-21`): `window` `error` and `unhandledrejection` events are forwarded to main over `app:reportError { source: 'window' | 'promise', message, stack | null }`, failures ignored.
- **Error boundary fallback** (`src/renderer/src/components/ErrorBoundary.tsx:25-50`): a render exception replaces the whole interface with a centred page: heading “Something went wrong in the interface”, body “The details are in the log. Reloading brings the interface back; nothing you stored is affected, and any switch that was running continues on its own.”, the error message in monospace (selectable), and two buttons: “Reload” (reloads the page) and “Open logs” (`app:openPath { target: 'logs' }`, errors ignored). The exception is reported first over `app:reportError { source: 'render', message, stack: <stack + component stack> }` (`ErrorBoundary.tsx:19-23`).

### Sidebar

**Purpose.** Primary navigation, the Preview mode warning, the Update ready card, the collapse toggle and the version (`src/renderer/src/components/Sidebar.tsx`).

**Layout.** A left `<aside>` 224 px wide (`w-56`), or 60 px when collapsed; only the width animates (200 ms ease-out, none under reduced motion) and labels fade in place over 150 ms so nothing reflows (`Sidebar.tsx:49-56`). Top: `<nav aria-label="Primary">` with the three items. Bottom (`mt-auto`): the Preview mode card, the Update ready card, then a row with the collapse button and the version.

- **Nav items** (`Sidebar.tsx:15-19, 59-78`): Library (Ctrl+1), Accounts (Ctrl+2), Settings (Ctrl+3). Each is a button with `aria-label` = label, `title` = “<Label> (<shortcut>)”, `aria-current="page"` when active, an accent 2 px bar on the left edge when active. Click calls `setView(view)` (`store/ui.ts:191-194`): a no-op when already on that view, except that Accounts from a detail page returns to the list; Settings keeps the tab the user last had open.
- **Preview mode card** (`Sidebar.tsx:82-98`): shown only while `settings:getAll` reports `dryRun === 'true'` (re-read whenever `settingsRevision` changes, seeded from the startup answer, `Sidebar.tsx:34-38`). Warning-toned button, `aria-label="Preview mode is on"`, text “Preview mode is on” over “Play shows what would happen, nothing changes.”; tooltip “Preview mode is on: Play shows what a switch would do instead of doing it. Turn it off in Settings, or from the preview itself.” Click: `openSettings('advanced')` (`store/ui.ts:202`). Collapsed: icon only, border transparent, label `aria-hidden`.
- **Update ready card** (`Sidebar.tsx:99-135`): shown only while the updater's state is `ready` (`useUpdateStatus(60_000)`, `Sidebar.tsx:21, 45`). Accent-toned button, `aria-label` “Restart to install One Launch <version>”. Text “Update ready” (or “Restarting…” after the click) over “Restart to install <version>”, which becomes “Could not restart: <message>” in danger tone if the restart call failed. Disabled while restarting or while a launch job is `running`; tooltip then “One Launch <version> is ready. Restart to install it once the switch has finished.”, otherwise “One Launch <version> is ready. Restart to install it.”. Click: `restartToUpdate()` (`src/renderer/src/lib/updates.ts:39-44`): asks the view's leave guard first (an unsaved form can keep the user; the card then returns to “Update ready”), then `updates:install`.
- **Collapse toggle** (`Sidebar.tsx:137-146`): icon button, `aria-label` “Expand sidebar” / “Collapse sidebar”, `aria-expanded`, tooltip “Expand sidebar (Ctrl+B)” / “Collapse sidebar (Ctrl+B)”. Beside it, when `app:info` has answered, “v<version>” in faint 11 px text (hidden when collapsed) (`Sidebar.tsx:147`).
- **Collapse behaviour and breakpoint** (`src/renderer/src/store/ui.ts:8, 141-142, 160, 178-189`; `App.tsx:71-77`): the media query `(max-width: 1099px)` is `sidebarNarrow`. Initial state: the stored choice if any, else collapsed when narrow. Toggling stores `sidebar` = `'expanded'` or `'collapsed'` in `localStorage`, except that a choice equal to what the width would pick anyway removes the key. Crossing the breakpoint in either direction clears the stored choice and returns to automatic (collapsed when narrow, open when wide). All `localStorage` access is wrapped in try/catch and failures are ignored (`store/ui.ts:13-29`).

### Library view

**Purpose.** The poster wall: every installed game across launchers, a Recent strip, search/filter/sort, and one-click Play (`src/renderer/src/views/LibraryView.tsx`).

**Layout.** A full-width `Page` (whole pane scrolls, 32 px side padding, no width cap; `src/renderer/src/components/Page.tsx:15-20`). A sticky `PageHeader` (`src/renderer/src/components/PageHeader.tsx:12-26`, `sticky top-0`, page background) with title “Library”, subtitle “Installed games across your launchers. Pick a game, pick an account, play.”, right-aligned actions, and a second pinned toolbar row. Below: the Recent strip, then the poster grid.

**Data** (`LibraryView.tsx:60-69`): `library:list` (returns `{ games, revision }`) and `launch:recent { limit: 8 }` (`RECENT_LIMIT`, `src/shared/ipc-contract.ts:243`), both seeded from the startup prefetch. `launchers:status` is fetched once when the library is empty and no status is cached in the store, so the empty state can say whether any launcher was found (`LibraryView.tsx:125-144`).

**Header actions** (`LibraryView.tsx:287-312`), left to right:
- An always-mounted `role="status"` flash span (see Global behaviours › Flash).
- “Fetching artwork (<n>)” with a pulsing icon while `queued + running > 0` in the artwork status (`LibraryView.tsx:277, 292-297`).
- “Add a free key for missing covers”, a text button shown when the scan has finished, no SteamGridDB key is stored, the queue is idle, and at least one game has no artwork and a recorded lookup failure (`LibraryView.tsx:279-280, 297-307`). Tooltip: “Games that are not on Steam get artwork from SteamGridDB, which needs a free API key. Add one in Settings › Artwork.” Click: `openSettings('artwork')`.
- “Rescan” (“Scanning…” with a spinning icon while busy, disabled then) (`LibraryView.tsx:308-310`). Calls `library:rescan`, reloads the list, and flashes (`LibraryView.tsx:154-176`):
  - error tone, when any launcher's scan reported an error: the failures joined with “ · ”, each as “<launcher>: <error>”;
  - error tone, when every launcher reported not installed: “No launchers found. Install Steam, Epic Games, Battle.net, or Riot Games, then rescan.”;
  - success tone otherwise: “<added> added, <removed> removed” when either is non-zero, else “Library is up to date”;
  - error tone with the IPC message when the call itself failed.

**Toolbar** (`LibraryView.tsx:313-378`):
- Search field, placeholder “Search games (Ctrl+F)”, `aria-label="Search games"`, min 256 px, max 448 px, disabled while the library is empty. Escape inside it clears the text when there is any (`LibraryView.tsx:322-327`). A clear button (`aria-label="Clear search"`) appears while text is present and refocuses the field. Ctrl+F focuses and selects the field from anywhere, fields included, unless a modal is open (`LibraryView.tsx:178-188`).
- “Filter” `Dropdown` (fixed trigger text, no caret, highlighted while a filter is on): one option per launcher in order Steam, Epic Games, Battle.net, Riot Games, labelled “<launcher> (<count>)” with the launcher dot; choosing the current option again clears the filter (`onDeselect`). Disabled while the library is empty. Tooltip “Filter: <option>” while on (`src/renderer/src/components/Dropdown.tsx:148`).
- “Sort” `Dropdown`: options “Last played” and “Name”. The choice is stored in `localStorage` `library.sort` (`'name'`, anything else reads as last played) (`LibraryView.tsx:23, 39-45, 220-228`).
- Filtering and ordering (`LibraryView.tsx:229-242`): keep games whose launcher matches the filter (if any) and whose name contains the trimmed query, case-insensitively. Name sort: locale compare, base sensitivity. Last played sort: by `lastPlayedOf(game)`, the newer of the launcher's own `lastPlayedAt` and `lastLaunch.launchedAt` (`src/renderer/src/components/GameCard.tsx:19-25`), newest first; played games before never-played; ties by name.

**Recent strip** (`LibraryView.tsx:416-453`): shown only when there are recent launches, the query is empty and no filter is on. Heading “Recent” (uppercase, clock icon). An `HScroller` (horizontal row, 40 px edge fades on whichever side still has content, `src/renderer/src/components/HScroller.tsx`) of up to 8 cards, each 240 px wide: a 32 × 48 px thumbnail (the game's grid image, else the launcher dot), the game name, and “<account> · <relative time>”. Tooltip “Play <game> as <account>”. Entries whose game is no longer in the list are skipped. Click: `openLaunch({ game, initialAccountId: entry.accountId, instant: playsAtOnce(game) })` (`LibraryView.tsx:266-270`).

**Poster grid** (`LibraryView.tsx:477-481`): CSS grid, `auto-fill` columns of at least 160 px (190 px and a wider gap at the `2xl` breakpoint), rows sized to content. One `GameCard` per visible game.

**GameCard** (`src/renderer/src/components/GameCard.tsx`):
- A 2:3 poster. While no grid image is set or it has not loaded, a two-letter monogram (first letters of the first two words, `GameCard.tsx:27-34`) at 10 % opacity sits behind the game name (up to three lines). When the artwork source is `launcher` (the game's own icon) the icon is drawn at half width at 22 % from the top and the name stays visible (`GameCard.tsx:40-41, 76, 88`). The image fades in over 200 ms when loaded; a load error keeps the monogram (`GameCard.tsx:77-91`).
- “Update” badge (warning tone, top-left) when `game.needsUpdate` (`GameCard.tsx:92-96`).
- A Play roundel over a bottom gradient appears on hover or focus-within, or stays visible with a spinner while `busy` (`GameCard.tsx:97-103`). `busy` is true while an instant launch request for this game is in flight and silent (`LibraryView.tsx:73, 480`); the button is `aria-busy` then.
- The poster button is `aria-label="Play <game>"`; click: `openCard` (`LibraryView.tsx:88-95`): if the game has a `lastLaunch` whose `accountName` is not null (the account still exists) and `playsAtOnce(game)`, `openLaunch({ game, initialAccountId: lastLaunch.accountId, instant: true })`; else `openLaunch({ game, initialAccountId: lastLaunch?.accountId ?? null })` (the picker). `playsAtOnce` (`LibraryView.tsx:53-57`): false when the game needs an update and its launcher is not Steam, or when a job is currently `running`.
- A “More” button (`aria-label="More for <game>"`, top-right, visible on hover/focus) and right-click both open the `CardMenu` at the pointer; a keyboard-initiated context menu (coordinates 0,0) opens it under the card's top-left (`GameCard.tsx:44-53, 106-113`).
- Caption: the name (truncated, full name in `title`), then the launcher dot and “<last account> · <relative time>” where the first part is `lastLaunch.accountName`, or “removed account” when the launch's account was deleted, or the launcher display name when the game was never launched here; the time part is omitted when there is no played time (`GameCard.tsx:114-124`). The relative time is re-read against a clock the Library advances every 60 s (`LibraryView.tsx:97-100`).

**CardMenu** (`src/renderer/src/components/CardMenu.tsx`; items from `LibraryView.tsx:381-395`):
- `role="menu"` `aria-label="More for <game>"`, fixed, min 208 px, clamped 8 px inside the viewport before first paint (`CardMenu.tsx:29-36`). Header: the game name and a subtitle: “Artwork: <source>” where source is “Steam” (`steamcdn`), “SteamGridDB”, “your image” (`manual`) or “the game's own icon” (`launcher`) (`LibraryView.tsx:25-30`); without artwork, the recorded `artworkError`, else “No artwork yet”.
- Items: “Play as…” (opens the picker: `openLaunch({ game, initialAccountId: lastLaunch?.accountId ?? null })`, never instant, `LibraryView.tsx:272-275`); “Refresh artwork” (`artwork:refresh { gameId, force: true }`, error flashed, `LibraryView.tsx:244-251`); “Choose portrait image…” (`artwork:setManual { gameId, kind: 'grid' }`, which opens main's file picker; the list reloads when a game comes back, nothing happens when the picker was cancelled; error flashed, `LibraryView.tsx:253-264`).
- Focus moves to the first item on open and returns to the opener on close; Down/Up wrap, Home/End jump, Escape and Tab close; any pointer-down outside, window resize or any scroll closes it (`CardMenu.tsx:38-74`).

**States** (`LibraryView.tsx:397-485`):
- Load error: “Could not load the library: <message>” in danger text, nothing else.
- Empty library (no games at all): `EmptyState` (`src/renderer/src/components/EmptyState.tsx`, 420 px tall, icon, title, one-line description, action). Title “Scanning your launchers…” while rescanning, while the list is loading, or until the artwork status reports `scanned` (main's first scan finished); otherwise “No launchers found” when the last rescan found every launcher absent or the cached launcher status has no install, else “No games found”. Description “One Launch looks for Steam, Epic Games Launcher, Battle.net, and the Riot Client. Install one of them, then rescan.” when no launchers, else “Installed Steam, Epic, Battle.net, and Riot games appear here after a scan.” Action: primary “Scan now”, disabled while scanning or before the first scan has finished.
- No match (games exist, none visible): title `No games match “<query>”` when a query is set, else “No <launcher> games installed”. Description “Only <launcher> games are shown. Clear the filter to search every launcher.” when a filter is on, else “Try a shorter name, or rescan if the game was installed after the last scan.” Action “Clear search and filters” (clears both).
- Populated: Recent strip (conditions above) then the grid.

**Background refresh** (`LibraryView.tsx:101-107, 146-153, 191-217`): the view polls `artwork:status`; when the returned `revision` differs from the revision its list was read at, it reloads `library:list` and `launch:recent`. Poll delay: 500 ms while `queued + running > 0`, 4000 ms once games are on screen, 1000 ms before that (first scan likely still running), 3000 ms after a failed poll. Unchanged status answers do not re-render the grid (`LibraryView.tsx:32-37`). A launch job leaving the `running` state also reloads both lists.

### Accounts view

**Purpose.** The accounts list, grouped by launcher, with Import and Add menus (`src/renderer/src/views/AccountsView.tsx`).

**Layout.** `Page narrow` (content capped at 65 rem, centred). Sticky header: title “Accounts”, subtitle “The accounts you switch between, grouped by launcher.”, actions: flash span, “Import” `MenuButton`, primary “Add account” `MenuButton`. Below: one section per launcher in the order Steam, Epic Games, Battle.net, Riot Games (`src/shared/models.ts:8`), skipped when empty, each with a dot, the uppercase launcher name, the count, and a card of rows.

- **Data:** `accounts:list` (`AccountsView.tsx:89`), not seeded.
- **Ordering rule** (`sortAccounts`, `AccountsView.tsx:15-23`): accounts with `lastUsedAt` first, most recent first; then the rest by display name (locale, base sensitivity). The launch dialog's picker reuses this function.
- **Row** (`AccountRow`, `AccountsView.tsx:30-86`): a full-width button. Left icon: a key in accent when the account is “ready” (`hasSession || lastUsedAt !== null`, `AccountsView.tsx:26-28`), tooltip “Ready: switches without asking you to sign in”; else a person icon, tooltip “The first switch will ask you to sign in once”. Middle: display name, tag chips, then “<login>” and, when notes exist, “ · ” and the first non-blank line of the notes with a note icon. Right: “Used <relative time>” when used, else “Sign-in saved” when ready, else “Not used yet”; under it “Password stored” (tooltip “A password is stored for the Copy button”) when `hasSecret`. Click: `openAccount(id)`.
- **Import menu** items (`AccountsView.tsx:108-137`): “From Steam” (hint “Logins Steam has saved”) and “From Battle.net” (hint “Logins Battle.net has saved”) open the Import accounts dialog for that launcher; “Epic Games: add by hand” (hint “Epic keeps no account list”) and “Riot Games: add by hand” (hint “The Riot Client keeps no account list”) call `newAccount(launcher)`.
- **Add account menu** (`AccountsView.tsx:138-143`): the four launcher names, each `newAccount(id)`.
- **Arrival with `importIntent`** (`AccountsView.tsx:102-106`): when the store carries an import launcher, the import dialog opens at once and the intent is cleared.
- **After an import** (`onImported`, `AccountsView.tsx:145-157`): the dialog closes. If a `returnTo` game is set and any imported account belongs to its launcher, the view switches to the Library and opens that game's launch dialog with the single imported account picked, or the picker when several were imported. Otherwise the flash reads “Nothing new to import” (info) or “Imported <n> account” / “Imported <n> accounts from <Steam|Battle.net>” (success, 4 s) and the list reloads.
- **States** (`AccountsView.tsx:176-211`): error “Could not load accounts: <message>”; loading with nothing yet: blank; empty: `EmptyState` title “No accounts yet”, description “Add the accounts you use on each launcher, or import the ones Steam and Battle.net already know. Passwords are optional and stay on this PC.”, actions: primary “Import accounts” menu (same items, left-aligned) and “Add account” menu; populated: the grouped sections.

### Account detail: create form

**Purpose.** Adds one account for a launcher (`CreateAccount`, `src/renderer/src/views/AccountDetailView.tsx:406-518`). Reached by `newAccount(launcher, returnTo?)` (`store/ui.ts:196`); the route is `{ kind: 'new', launcher }`.

**Layout.** `Page narrow`. Sticky header (`AccountDetailView.tsx:461-478`): left, the ghost “Accounts” back button (arrow icon, `AccountDetailView.tsx:392-399`) above the title “New <launcher> account” with a launcher badge; right, any error text in danger, “Cancel” (ghost, `backToAccounts`), primary “Create account” (“Creating…” while busy; both disabled while busy). Body: a Details panel, a Password panel, and a collapsed Advanced section.

- **Details panel** (`AccountDetailView.tsx:481-483`): description is the launcher's first-Play line (`FIRST_PLAY`, `AccountDetailView.tsx:86-91`): Steam “First Play: Steam asks you to sign in once; keep “Remember me” ticked.”; Epic “First Play: Epic Games Launcher asks you to sign in once.”; Battle.net “First Play: Battle.net asks you to sign in once; leave “Keep me logged in” on.”; Riot “First Play: the Riot Client asks you to sign in once; tick “Stay signed in”.”.
- **Form fields** (`AccountForm`, `AccountDetailView.tsx:100-148`), two columns then full width:
  - “Display name”: text, max 120 chars (`ACCOUNT_LIMITS.displayName`, `src/shared/models.ts:55-63`), autofocused on the create form only, placeholder is a per-launcher example name, hint “How this account is shown in One Launch.”
  - “Login”: text, max 256, `autocomplete=off`; hint per launcher: Steam “Your Steam account name, not your profile name.”, Epic “The email you sign in to Epic Games with.”, Riot “The username you sign in to the Riot Client with.”, Battle.net “The email you sign in to Battle.net with.”; placeholder “account name” (Steam), “username” (Riot), “you@example.com” (others).
  - “Tags”: `TagInput` (`src/renderer/src/components/TagInput.tsx`), hint “Press Enter to add one. Rank, role, region, anything.” Enter, comma, or Tab with pending text commits; Backspace on an empty field removes the last tag; blur commits pending text (`TagInput.tsx:26-34, 62`). Tags are normalised (trimmed, cut to 40 chars, de-duplicated case-insensitively, at most 32) (`normalizeTags`, `src/shared/models.ts:86-99`). The input is disabled with placeholder “Tag limit reached” at 32 tags; each chip has a remove button `aria-label="Remove tag <tag>"`.
  - “Notes”: textarea, max 20 000 chars, hint “Anything you want to remember about this account.”, per-launcher example placeholder.
- **Validation** (`validate`, `AccountDetailView.tsx:58-64`): “Display name is required.” when blank after trim; “Login is required.” when blank; “Notes must be at most 20000 characters.” Errors appear under their field, replacing the hint (`src/renderer/src/components/Field.tsx:16`), and only after the first Create attempt (`touched`, `AccountDetailView.tsx:418-419, 428-430`).
- **Password panel** (`AccountDetailView.tsx:484-510`): description “Optional. Switching does not need it; it is only there so you can copy it when a launcher asks you to sign in. Stored encrypted on this PC.” One “Password” field, max 1024 chars, `autocomplete=new-password`, with a show/hide eye button (`aria-label` “Show password” / “Hide password”).
- **Advanced** (`Collapsible`, closed by default, title “Advanced”, hint “Launcher account id”, `AccountDetailView.tsx:510-515`; `src/renderer/src/components/Collapsible.tsx`): a panel “Launcher account id” with `LauncherIdField` (`AccountDetailView.tsx:151-179`): label “SteamID64” / “Epic account id” / “Riot account id” / “Battle.net account id”; max 64 chars, monospace; Steam hint “Filled in when you import the account from Steam. Only needed if Steam’s remembered name and this account disagree.”, others “Filled in by itself the first time One Launch sees this account signed in. You normally never type it.”; placeholders “7656119…”, “32 hex characters”, “36 characters, with dashes”, “8 hex characters”.
- **Create** (`AccountDetailView.tsx:427-457`): validates (marks touched), then `accounts:create { launcher, displayName, login, launcherUid: <text or null>, tags: normalised, notes, password: <text or null> }`. Success: if `returnTo` is set and is a game of this launcher, `setView('library')` and `openLaunch({ game: returnTo, initialAccountId: <new id> })`; otherwise `openAccount(id, { created: true })`. Failure: the message shows in the header, the form stays editable, and the leave guard is re-armed.
- **Leave guard** (`AccountDetailView.tsx:420-425`): while anything is typed (draft differs from empty, or a password is entered) navigation asks “Leave without creating this account?” / “What you typed will be lost.” with buttons “Leave” / “Stay” (`ConfirmDialog`, defaults `store/ui.ts:212-213`). The Back gesture also goes through the guard (`App.tsx:38-44`).
- Keyboard: no Ctrl+S on the create form (only the edit page registers it).

### Account detail: edit page

**Purpose.** Shows and edits one account (`EditAccount`, `src/renderer/src/views/AccountDetailView.tsx:524-713`). Route `{ kind: 'detail', id, created? }`; the component remounts per id (`AccountDetailView.tsx:718`).

**Layout.** `Page narrow`. Sticky header (`AccountDetailView.tsx:631-671`): the ghost “Accounts” back button, the display name (truncated) with a launcher badge, and a line “Last used <relative>” or “Never used from One Launch”, then “ · Added <date time>”. Right: the flash span, error text, “Discard” (ghost, only while dirty; resets the draft) or “Switch to this account” (only while clean; tooltip “Switch <launcher> to this account without launching a game”; opens the launch dialog with `game: null` and this account), and primary “Save changes” (“Saving…” while busy). Body: Details, Password, Saved sign-in (Epic, Battle.net, Riot only), Advanced (collapsed), Danger zone.

- **Data** (`AccountDetailView.tsx:529-545`): `accounts:get { id }`. Error: the page shows only the back button and “Could not load this account: <message>”. Until the account arrives the page renders nothing.
- **Created flash** (`AccountDetailView.tsx:546-551`): when the route carries `created`, once: “Account created. Pick a game in the Library and press Play.” (success, 8 s).
- **Details panel**: the same `AccountForm` without autofocus. Validation runs only while dirty (`AccountDetailView.tsx:571`).
- **Save** (`AccountDetailView.tsx:573-593`): “Save changes” is disabled unless dirty, not busy and valid; tooltip “Save changes (Ctrl+S)” when dirty, else “No unsaved changes”. Sends `accounts:update { id, patch }` with only the changed fields (`displayName`, `login`, `launcherUid` as text or null, `notes`, `tags`). Success: the account and draft refresh and the flash reads “Saved” (success). Failure: message in the header. Ctrl+S (also from inside fields) triggers the same save (`AccountDetailView.tsx:608-617`).
- **Leave guard** (`AccountDetailView.tsx:555-565`): dirty → “Leave without saving?” / “Your changes to this account will be lost.”; otherwise, a typed but unsaved password → “Leave without storing the password?” / “The password you typed is not stored until you press Store password.”; buttons “Leave” / “Stay”.
- **Password panel** (`PasswordPanel`, `AccountDetailView.tsx:193-346`): same description as the create form. Header actions: “Copy login” and “Copy password” (disabled unless `hasSecret`). Copy calls `accounts:copy { id, field: 'login' | 'password' }` and flashes, for 4 s, “Login copied · clipboard clears in <n>s while One Launch is open” or “Password copied · clipboard clears in <n>s while One Launch is open”, with “ · Windows clipboard history may keep it” appended for a password when `historyExcluded` is false; `n` = `round(clearAfterMs / 1000)` from the response (`AccountDetailView.tsx:243-255`). Row “Status”: “Stored · encrypted” (success, shield icon) or “Not stored” (muted, key icon). Below, when not editing: “Update password” (if stored) or “Store password” button; when stored, also “Remove” (ghost), which becomes a two-step “Confirm remove” (danger) + “Cancel”; confirm calls `accounts:clearPassword { id }` and flashes “Password removed” (`AccountDetailView.tsx:229-241`). When editing: a field labelled “New password” (if stored) or “Password”, max 1024, autofocused, show/hide eye; Enter saves, Escape cancels; buttons primary “Update password” / “Store password” (“Saving…”; disabled while empty) and “Cancel”. Save calls `accounts:setPassword { id, password }` and flashes “Password updated” or “Password stored” (`AccountDetailView.tsx:213-227`). Errors show under the field or beside the buttons. A typed, unsaved password arms the leave guard (`AccountDetailView.tsx:201-203`).
- **Switch to this account**: `openLaunch({ game: null, initialAccountId: account.id })` (`AccountDetailView.tsx:657`); the launch dialog then runs a plain switch (see Launch dialog).
- **Saved sign-in panel** (`SessionPanel`, `src/renderer/src/components/SessionPanel.tsx`; shown for `epic`, `battlenet`, `riot`, `SessionPanel.tsx:35-44`):
  - Launcher names used in this panel: “Epic Games Launcher”, “Battle.net”, “Riot Client” (`SessionPanel.tsx:35-39`).
  - Data: `sessions:get { accountId }`, re-run when `account.updatedAt` changes (`SessionPanel.tsx:54`); returns `{ session, state, stateError }`.
  - Description: “Saved the first time you play on this account, and put back whenever you switch to it.” Header actions: “Re-check” (reloads; spinner while loading) and primary “Save sign-in” (“Save again” when a session exists, “Saving…” while busy), disabled while busy or while the launcher holds no remembered sign-in (`!state.remembered`), with tooltip “Nothing to save: <launcher name> holds no sign-in right now.” in that case (`SessionPanel.tsx:110-125`). Save calls `sessions:capture { accountId }`, hands the returned account up (so a learned launcher id appears in Advanced without losing other edits, `AccountDetailView.tsx:567-571`), shows “Sign-in saved” and reloads.
  - Row “Saved sign-in”: “Saved <relative>” (success) or, when `staleReason` is set, “Saved, but needs a fresh sign-in” (warning); “Checking…” during the first load when `account.hasSession` is already known; else “Not saved yet” (muted). Hint “Last used for a switch <relative>.” or “Not used for a switch yet.”; a blank hint holds the line while loading for an account known to have a session, so the panel does not jump (`SessionPanel.tsx:98-104, 127-140`).
  - Row “Why” (only when stale): the `staleReason` in warning text, hint “Sign in to <launcher name> once by hand; the next switch saves the fresh sign-in on its own.”
  - Row “<launcher name> is signed in as”: in danger, the query error or `stateError`; else (`describeState`, `SessionPanel.tsx:22-30`) “Nobody” (no remembered session), “This account” (success) or “This account (newer than the saved sign-in; press Save again)” when the on-disk fingerprint differs from the saved one, “Another of your accounts”, or “An account not yet linked in One Launch” with the hint “If that is this account, press Save sign-in.”; “Checking…” while unknown. The hint is prefixed with “<launcher name> is running.” when the launcher runs (`SessionPanel.tsx:95`).
  - Row “<launcher name> account” (only when the account has a launcher id): “Linked to …<last 4 chars>”, hint “Recorded when the launcher reported this account’s sign-in. The full id is under Advanced.”
  - Row “Last sign-in” (only when the launcher's log shows a failed last sign-in): the reason, sentence-cased, or “Failed” (`SessionPanel.tsx:160-164`).
  - Buttons (only when a session exists): “Forget saved sign-in” (ghost) → two-step “Yes, forget it” (danger) + “Cancel”; confirm calls `sessions:forget { accountId }` and shows “Saved sign-in removed” (`SessionPanel.tsx:78-91, 165-181`). Notes show in success text, errors in danger text beside the buttons.
- **Advanced** (`Collapsible`, title “Advanced”, hint “Launcher account id and the switch preview”, `AccountDetailView.tsx:682-688`):
  - Panel “Launcher account id”: the `LauncherIdField` above, editable and saved with the form.
  - **Switch preview** panel (`SwitchPlanPanel`, `AccountDetailView.tsx:352-389`): description “Exactly what switching <launcher> to this account would change. Previewing changes nothing.” Action button “Preview” (“Refresh” once a plan is shown; “Checking <launcher>…” while busy). Calls `launchers:switchPlan { accountId }` and renders `SwitchPlanView`; placeholder “Press Preview to see the steps.”; errors in danger text. The plan is dropped whenever `account.updatedAt` changes.
  - `SwitchPlanView` (`src/renderer/src/components/SwitchPlanView.tsx`): a pill “Remembered session” (ok) or “Needs credentials” (warn) by `plan.strategy`; a pill “<active> of <total> steps would run” (info) or “Cannot run yet” (fail) by `plan.executable`; the sentence “<launcher> is running.” / “<launcher> is not running.” with “, currently set to sign in as <login>” inserted when known; `plan.warnings` in a warning-toned list; `plan.notes` in a muted list; then every step: kind icon, “<n>.”, title, a kind chip (“Process”, “Backup”, “File”, “Registry”, “Launch”, “Wait”), “skipped” when skipped (dimmed), the target in monospace, the detail, and a before → after table (“(none)” for an empty before). Footer: “Generated <local date time>. This is a preview: nothing was changed.” (`SwitchPlanView.tsx:25-101`). Unlike the launch dialog, this view keeps the adapter's own “is running… will be closed” warning (`docs/renderer.md:53`).
- **Danger zone** panel (`AccountDetailView.tsx:689-710`): description “Deleting removes the account, its password, and its saved sign-in from One Launch only. Nothing changes in the launcher.” Button “Delete account” (danger) → two-step `Yes, delete “<account>”` (danger) + “Cancel”. Confirm calls `accounts:delete { id }`, clears the leave guard and returns to the list; failure shows the message in the header (`AccountDetailView.tsx:595-606`).
- **Navigation out**: the “Accounts” button and the Back gesture call `backToAccounts` (through the leave guard); sidebar Accounts (`setView('accounts')`) also returns to the list (`store/ui.ts:191-199`).

### Import accounts dialog

**Purpose.** Imports the accounts Steam or Battle.net remembers as One Launch accounts, names and ids only (`src/renderer/src/components/ImportAccountsDialog.tsx`). Opened from the Accounts page's Import menu, by `importIntent`, or indirectly from the launch dialog's empty picker.

**Layout.** Fixed overlay at `z-30`, backdrop `bg-bg/70`, a `role="dialog" aria-modal` card up to 672 px wide (`max-w-2xl`) with a header (title, subtitle, X `aria-label="Close"`), a scrolling list, and a footer (status text left, buttons right) (`ImportAccountsDialog.tsx:137-197`).

- **Copy** (`ImportAccountsDialog.tsx:63-77`). Steam: title “Import Steam accounts”, subtitle “Accounts Steam remembers on this PC. Only names and SteamIDs are imported.”, loading “Reading loginusers.vdf…”, empty “Steam has not remembered any account on this PC yet.” Battle.net: title “Import Battle.net accounts”, subtitle “Logins in the client's saved-accounts list (Battle.net.config). Only the logins are imported; the account id is learned at the first confirmed sign-in.”, loading “Reading Battle.net.config…”, empty “Battle.net has no saved account names on this PC yet.”
- **Data**: `launchers:rememberedAccounts { launcher }` (`ImportAccountsDialog.tsx:90`). Every entry not already linked to a One Launch account starts ticked (`ImportAccountsDialog.tsx:98-101`). An entry is identified by its `launcherUid`, or its login when the launcher records no id (`ImportAccountsDialog.tsx:19-21`).
- **Row** (`ImportAccountsDialog.tsx:23-61`): a checkbox (disabled and dimmed when already linked), the persona name or login, pills: “Signed in” (ok) when `mostRecent`; “Already in One Launch” (info) when linked; “Remember me off” (warn) when `rememberPassword` is false, else “Session may have expired” (warn) when `sessionMayBeExpired`; then the login, the launcher uid in monospace when present, and “last sign-in <relative>” when known.
- **States**: error “Could not read <launcher>'s remembered accounts: <message>”; loading text; empty text; list. Footer left: the import error in danger, or “Everything here is already in One Launch.” when entries exist but none is importable, else “<n> selected” (`ImportAccountsDialog.tsx:172-181`).
- **Actions**: “Cancel” (ghost, disabled while importing) closes. Primary “Import <n>” (“Import” when nothing is selected, disabled then; “Importing…” while busy) calls `launchers:importAccounts { launcher, launcherUids: [...selected] }` and hands `result.accounts` to the opener (`ImportAccountsDialog.tsx:120-134`). A failure shows the message in the footer and re-enables the buttons.
- **Focus and keys**: focus trap; once the list has loaded, focus moves to the primary button if anything is selected, else to Cancel, unless the user already moved it (`useFocusWhenReady`, `ImportAccountsDialog.tsx:96`; `src/renderer/src/lib/useFocusTrap.ts:48-62`). Escape closes (`ImportAccountsDialog.tsx:103-109`); backdrop click closes (`ImportAccountsDialog.tsx:137`). Neither is blocked while an import is in flight.

### Launch dialog

**Purpose.** The Play flow: pick an account, check the launcher, confirm closing it if needed, follow the switch, see the outcome; or a plain account switch when opened with `game: null` (`src/renderer/src/components/LaunchDialog.tsx`). Hosted by `LaunchHost` above every view; views only call `openLaunch({ game, initialAccountId, instant? })` (`store/ui.ts:175`), which remounts the dialog per request.

**Layout.** Fixed overlay at `z-30`, backdrop `bg-bg/70` from 36 px down; a `role="dialog" aria-modal aria-labelledby="launch-title"` card up to 672 px wide (`max-w-2xl`), full height at most: header, scrolling body, footer (`LaunchDialog.tsx:504-513`).

- **Header** (`LaunchDialog.tsx:513-546`): title = the game name, or “Switch account” for a plain switch; a launcher badge with tooltip “<launcher> id: <launcherGameId>” when a game is set; a subtitle “Play as <account>” (game) or “Switch <launcher> to <account>” (no game), or “Add an account first” when the picker is empty, or “Pick the account to use” while no account is selected. The X (`aria-label="Close"`) is disabled while busy.
- **Busy** = stage `checking` or `running`, or a `launch:start` in flight (`LaunchDialog.tsx:323`). While busy, Escape, the backdrop and X do nothing (`LaunchDialog.tsx:356-362, 504, 540`).
- **Accounts**: `accounts:list` (`LaunchDialog.tsx:300`); candidates are the accounts of the game's launcher (or of the initial account's launcher for a plain switch), ordered by `sortAccounts` (`LaunchDialog.tsx:308-313`). A load error shows “Could not load accounts: <message>” at the top of the body (`LaunchDialog.tsx:548`). The preselected account is `initialAccountId` when it is among the candidates, else the most recently used candidate (by `lastUsedAt`, descending; first in order when none used) (`LaunchDialog.tsx:347-353`).
- **Attach on mount** (`LaunchDialog.tsx:334-345`): a `running` job in the store is always followed (the dialog opens on its running stage); a finished job is followed only when it was for this same game and the request is not instant; any other stored job is cleared.

**Stages** (`LaunchDialog.tsx:56-64`):

- **pick**. Body: when the game `needsUpdate` and the picker is not empty, a warning box (`updateNote`, `LaunchDialog.tsx:72-77`): Battle.net “Battle.net wants to update <game> first. The switch still runs, but the game only starts once you update it in the Battle.net window.”; Riot “The Riot Client wants to update <game> first. The switch still runs, but the game only starts once you update it in the Riot Client window.”; other launchers “<Client> updates <game> before it starts, so the launch may take a while.” Then the `AccountPicker` (`LaunchDialog.tsx:252-285`): one row per candidate, `aria-pressed` radio-style, display name, tag chips, login, and “Used <relative>” or “Never used”. Footer: “Cancel” (ghost) and the primary action “Play” (play icon) or “Switch” (arrows icon), disabled until an account is selected; it takes the focus once an account is selected (`LaunchDialog.tsx:379, 726-735`). Enter anywhere in the dialog (not on a button or field) starts the check (`LaunchDialog.tsx:364-375`).
- **Empty picker** (no candidate for the launcher, list loaded without error; `LaunchDialog.tsx:378, 558-569, 722-726`). Body text: “No <launcher> accounts yet.” followed by, for Steam, “ Import the ones Steam remembers, or add one by hand on the Accounts page.”; for Battle.net, “ Import the logins Battle.net has saved, or add one by hand on the Accounts page.”; nothing more for Epic or Riot; with no launcher at all, “No accounts yet. Add one on the Accounts page.” The footer holds one autofocused primary button: “Import <launcher> accounts” (Steam, Battle.net), “Add <launcher> account” (Epic, Riot), or “Go to Accounts”. It closes the dialog and calls `importAccounts(launcher, game)` or `newAccount(launcher, game)`, which carry the game as `returnTo` for exactly one navigation (`store/ui.ts:143-148, 196-198`), or `setView('accounts')` (`LaunchDialog.tsx:490-496`). The Accounts page or the create form then reopens this game's launch dialog with the new account picked (see those sections).
- **checking** (`LaunchDialog.tsx:406-419`). `launch:preflight { accountId, gameId | null }`. Footer status “Checking <client>…”, the primary reads “Checking…” and is disabled, Cancel disabled, X disabled, picker still shown. The result decides the next stage: `blocked` set → blocked; `dryRun` → dryRun; `needsConfirmation` or any `plan.warnings` → confirm; otherwise start the job at once with `confirmClose: false`. A thrown error → the error stage with its message.
- **confirm** (`LaunchDialog.tsx:574-593, 736-752`). Body: when `needsConfirmation`, the `CloseBox` (warning tone, power icon): the adapter's `closeMessage`, or “<Client> is running. Switching closes it.” when none, over “<Client> is then started again as <account>.” (`LaunchDialog.tsx:128-141`). Then each plan warning in its own box (warning icon), each plan note in a muted info box, and a native `<details>` “Details” that expands the full `SwitchPlanView`. Footer: “Back” (ghost → pick) and either the danger button “Close <client> and continue” (when confirmation is needed; `start(true)`) or the primary “Continue” (`start(false)`); both read “Starting…” and are disabled while the start is in flight.
- **dryRun / preview** (`LaunchDialog.tsx:595-626, 752-761`). Body: a box “Preview mode is on, so nothing happened. A real switch would do this:” followed by a numbered list of the phases the plan would run (`planPhases`, `src/renderer/src/lib/phases.ts:128-133`, plus “Launch <game>” when a game is set), then the CloseBox if confirmation would be needed, the warnings, the notes and the “Details” expander. Footer: “Back” and primary “Turn preview off and play now” / “Turn preview off and switch now”, which calls `settings:set { key: 'dryRun', value: 'false' }`, bumps `settingsRevision`, and re-runs the check (`switchNow`, `LaunchDialog.tsx:455-464`); a failed save goes to the error stage.
- **blocked** (`LaunchDialog.tsx:628-641, 762-767`). Body: a danger box with the adapter's `blocked` text and the “Details” expander. Footer: an autofocused “Back”.
- **start** (`LaunchDialog.tsx:421-452`). `launch:start { accountId, gameId | null, confirmClose, instant }`. The returned job becomes `activeLaunch` in the store. If the request was instant and the job is `direct`, the dialog closes at once (the banner takes over). Otherwise the dialog attaches to the job and shows running. Error code `CONFIRM_REQUIRED` (the launcher started between preflight and start) re-runs the preflight and shows the confirm stage; any other error (for example `BUSY`) shows the error stage. A guard blocks a second start while one is in flight (`LaunchDialog.tsx:320, 422-424`).
- **running** (`LaunchDialog.tsx:643-699, 767-785`). `LaunchHost` polls `launch:status` every 150 ms and the dialog re-reads the job from the store (`LaunchDialog.tsx:343-345`). Body: the `AttentionPanel` when `job.attention` is set (hand-over, below); the `PhaseProgress` list; a text button “Show details” / “Hide details” that toggles `JobProgress`, the raw adapter steps with their status icon, title (“…” appended while running, “skipped” label when skipped) and the monospace, selectable detail line (`LaunchDialog.tsx:103-126`). Footer left: “Working…”, or nothing while the attention panel is up, or a cancel error note. Footer right: “Stop” (ghost; hidden while the attention panel is up, since its “Stop waiting” does the same; tooltip “Stop the switch. Until <client> is started again, anything already changed is put back; after that, <client> finishes signing in on its own.”; reads “Stopping…” and is disabled once pressed) and “Hide” (closes the dialog, the job keeps running and the banner shows it; tooltip “Keep the switch running and come back to it from the banner”). Stop calls `launch:cancel { jobId }`; a failure clears the stopping state and shows the message in the footer (`LaunchDialog.tsx:466-476`).
- **Phase list** (`PhaseProgress`, `LaunchDialog.tsx:143-167`; `src/renderer/src/lib/phases.ts`): the adapter's steps are grouped into at most five phases in order `close`, `switch`, `start`, `signin`, `launch` (`phases.ts:6-23`; unknown step ids fall into `switch`), labelled “Close <client>”, “Switch to <account>”, “Start <client>”, “Sign in”, “Launch <game>” (or “Launch” with no game) (`phases.ts:35-48`). A phase is omitted when it has no steps; marked “not needed” when all its steps are skipped; `running` while any step runs or between steps; `done` when its last live step is done; `failed` when the job stopped while it was underway or its last live step failed (`phases.ts:62-82`). Icons: check (done), spinner (running), x-circle (failed), dashed circle (pending/skipped). Under a phase that is not done, the current step's title is shown, except for the running phase while the attention panel is up (`LaunchDialog.tsx:159-161`).
- **Hand-over panel** (`AttentionPanel`, `LaunchDialog.tsx:189-250`): warning-toned, key icon; the adapter's `attention.title` (bold), `attention.message`, then `attention.steps` as a numbered list with `aria-label="What to do"` where any text in “ ” or " " is set in bold (`emphasizeQuoted`, `LaunchDialog.tsx:176-186`), then, if `attention.reason` is set, “<launcher>: <reason>” in monospace. Buttons: “Copy login”, “Copy password” (only when `attention.canCopyPassword`), and “Stop waiting” (ghost; “Stopping…” once pressed). Copy calls `accounts:copy { id: job.account.id, field }` and shows the same clipboard note as the account page (“<Login|Password> copied · clipboard clears in <n>s while One Launch is open”, plus “ · Windows clipboard history may keep it” for a password when history is not excluded) or the error message, beside the buttons (`LaunchDialog.tsx:169-173, 204-212`). There is no countdown.
- **finished** (`LaunchDialog.tsx:645-683, 786-797`), by `job.state`:
  - `done`: a success pill “Launched as <account>” when a game was launched and there is no `handOff`, else “Switched to <account>”; when `job.handOff` is set (Riot: the client only opened the game's page), a box with a play icon showing that text. Footer: primary “Done” (autofocused).
  - `needs-sign-in` or `cancelled`: a warning box with `job.error` and, under it, `stoppedNote` (`phases.ts:94-105`): “<Client> is signed in as <account>. Press <Play|Switch> again to launch the game.” when `switched`; “<Client> was put back the way it was and left closed.” when `restored === true`; “<Client> could not be put back automatically. Open Settings › Backups and restore the latest snapshot.” when `restored === false`; nothing for a cancel that never reached the hand-over; otherwise “Nothing was rolled back. Once <client> is signed in, press <Play|Switch> again to launch the game.” (the trailing “ to launch the game” only when a game is set). Footer: “Try again” (→ pick, clearing the stored job) and “Done”.
  - `failed`: a danger box with `job.error` or “The switch failed.”, then: “<Client> was put back the way it was and left closed. Start it to check which account it signs in as.” (`restored === true`); “<Client> could not be put back automatically. Open Settings › Backups and restore the latest snapshot.” (`restored === false`); “<Client> is signed in as <account>; only the game launch failed. Press Play again to retry the launch.” (`switched`); else “Nothing had been changed yet.” Footer: “Try again” and “Done”.
  - The phase list and Show details stay visible under the outcome. “Done” clears `activeLaunch` and closes (`LaunchDialog.tsx:478-481`).
- **error** (`LaunchDialog.tsx:710-715, 798`): a `role="alert"` danger box with the message (from the preflight, a non-`CONFIRM_REQUIRED` start error, or a failed `settings:set`). Footer: “Back” (→ pick).
- **Screen reader line** (`LaunchDialog.tsx:702-708`; `phases.ts:111-125`): one `sr-only` `role="status"` paragraph: “Checking <client>…” while checking; while running, the attention title or “<phase label>…” or “Working…”; when done, “Switched to <account>. <handOff>” or “Launched as <account>” / “Switched to <account>”; otherwise `job.error` or “The switch failed.”

**Instant (silent) mode** (`LaunchDialog.tsx:296-298, 381-403, 501`): a request with `instant: true` renders nothing and has no focus trap or Escape handler. Once the accounts have loaded, if the wanted account is missing or the list failed, it reveals itself on the picker; otherwise it runs the check once, as that account. It reveals itself the moment the stage is anything but pick/checking, except a running or finished job that is `direct` (nothing to show; `start` closes the dialog on such a job). So the dialog appears for: confirm (close box or warnings), preview mode, blocked, an error from the preflight or the start, or a job that has to switch (running phases). The library card's Play roundel spins while the request is silent (`LibraryView.tsx:73`).

**Focus and keys**: focus trap while visible; the primary takes focus when it becomes enabled (`useFocusWhenReady`); Escape closes when not busy; `autoFocus` on the empty-state button, “Back” (blocked) and “Done”.

### Launch banner

**Purpose.** Shows a job while the dialog is closed, and the way back to it (`LaunchBanner`, `src/renderer/src/components/LaunchHost.tsx:13-65`).

**Layout.** A full-width strip under the title bar, above the sidebar/content row (`App.tsx:86-87`), `role="status"`, 12 px text, icon + one truncated line + buttons. Shown whenever `activeLaunch` is set and no launch request (dialog) is open (`LaunchHost.tsx:116`).

- **Texts** (`LaunchHost.tsx:18-39`). Running: “Waiting for you to sign in as <account>” (attention set); “Launching <game> as <account>…” (direct job); “Switching to <account> and launching <game>…”; “Switching to <account>…” (no game). Done: “Switched to <account>. <handOff>” when a hand-off is set; “Launched <game> as <account>”; “Switched to <account>”. `needs-sign-in`: “The switch to <account> is waiting for a sign-in”. `cancelled`: “The switch to <account> was stopped”. `failed`: “Could not launch <game>” when the switch itself succeeded, else “The switch to <account> failed”.
- **Tone**: accent-soft while running (spinner, or a key icon when attention is set); success with a check when done; warning with an x-circle otherwise (`LaunchHost.tsx:40-45`).
- **Buttons**: “Show” (ghost) reopens the dialog with `openLaunch({ game, initialAccountId: job.account.id })`, which attaches to the job; “Dismiss” (icon, `aria-label="Dismiss"`) clears `activeLaunch`, shown only when the job is not running (`LaunchHost.tsx:50-62`).
- **Lifetime** (`LaunchHost.tsx:75-86`): a `done` job's banner clears itself after 8 s, counted while no dialog is open; other outcomes stay until dismissed.
- **Polling** (`LaunchHost.tsx:88-112`): while a job is `running`, `launch:status { jobId }` every 150 ms, dialog open or not; a polling error marks the stored job `failed` with the error message and the current time as `finishedAt`.

### Confirm dialog

**Purpose.** The app's own yes/no question, used by the leave guards (`src/renderer/src/components/ConfirmDialog.tsx`; `ask`, `store/ui.ts:205-223`).

- Fixed overlay at `z-50` (above every other dialog), backdrop `bg-bg/70` from 36 px down; `role="alertdialog" aria-modal aria-labelledby="confirm-title"`, up to 448 px wide (`max-w-md`); a warning triangle, the title and the message; footer: the cancel label (ghost, autofocused) and the confirm label (danger) (`ConfirmDialog.tsx:27-55`).
- Default labels “Leave” / “Stay” (`store/ui.ts:212-213`). Escape and a backdrop click answer no (`ConfirmDialog.tsx:14-24, 28`). A second question asked while one is open is answered no immediately (`store/ui.ts:206`).
- Callers and their texts: the create form, the edit page (two variants) — see those sections. `restartToUpdate` also runs the current leave guard, so the same prompt can appear before an update restart (`lib/updates.ts:39-44`).

### Recovery dialog

**Purpose.** Asked once at startup when the previous run ended during a switch (`src/renderer/src/components/RecoveryDialog.tsx`). `RecoveryHost` reads `recovery:get` from the startup prefetch, or asks main when the seed is missing (`RecoveryDialog.tsx:113-132`).

**Layout.** Fixed overlay at `z-50`, backdrop `bg-bg/70` from 36 px down, `role="alertdialog" aria-modal aria-labelledby="recovery-title"`, up to 512 px wide (`max-w-lg`). It cannot be dismissed with Escape or the backdrop (no handlers, `RecoveryDialog.tsx:45`). Focus trap; the recommended button is autofocused.

- **Wording** (`src/renderer/src/lib/recovery.ts:11-40`; `<account>` is the account name or “an account that has since been removed”; `<launcher>` is the display name):
  - Title: “The last switch could not be undone” when `phase === 'rollback-failed'`, else “One Launch was closed during a switch”.
  - No snapshot available (any phase): “It was switching <launcher> to <account>. The snapshot taken before that switch is gone, so there is nothing to restore from here. Start <launcher> and check which account it signs in as; Settings › Backups lists the snapshots that are left.” Recommends keep.
  - `writing`: “It stopped while changing <launcher>'s sign-in files for <account>, so <launcher> may be half-switched. Restoring puts <launcher> back the way it was before that switch.” Recommends restore.
  - `signing-in`: “<launcher>'s files had already been changed for <account>, and <launcher> was starting or signing in. If it is signed in as <account>, keep it as it is. If it looks wrong, restore it to the way it was before the switch.” Recommends keep.
  - `rollback-failed`: “The switch to <account> failed, and putting <launcher> back failed too. Restoring tries again with the snapshot taken before that switch.” Recommends restore.
- **Meta line** (`RecoveryDialog.tsx:68-71`): “Started <relative>. Snapshot <backupId>” (id in monospace) followed by “. <launcher> has to be closed for a restore.” when a snapshot is available, else “.”.
- **Buttons** (`RecoveryDialog.tsx:77-105`). With a snapshot: “Keep as is” and “Restore snapshot” (“Restoring…” while busy; “Try the restore again” after a failed restore); the recommended one is primary and autofocused, the other ghost; both disabled while restoring. Without a snapshot: a single primary “OK” (acts as keep).
- **Actions**: keep → `recovery:dismiss`, then the dialog closes whether or not the call succeeded (`RecoveryDialog.tsx:36-42`). Restore → `recovery:restore`; success shows the restored state: check icon, title “<launcher> was put back the way it was”, body “Start <launcher> to check which account it signs in as.”, button “Done”; failure shows the message under the body and offers the retry label (`RecoveryDialog.tsx:26-34, 54-74`).
- While it is up, the setup guide and What's New do not open (they check `recovery:get`, see below).

### Setup guide

**Purpose.** A five-step welcome for a data folder nobody has set up (`src/renderer/src/components/SetupGuide.tsx`, `SetupGuideSteps.tsx`, `SetupGuideArt.tsx`). `SetupGuideHost` opens it at startup when `settings:getAll` has `firstRun.dismissed !== 'true'` and `accounts:list` is empty (`SetupGuide.tsx:359-376`); Settings › General › “Show again” opens it for anyone (`openSetupGuide('settings')`).

**Layout.** Fixed overlay at `z-40` (under the recovery and confirm dialogs), backdrop `bg-bg/80` with blur, inert (no click handler, `SetupGuide.tsx:269`). The card is `role="dialog" aria-modal aria-labelledby="setup-title"`, 580 px tall (at most the window), up to 832 px wide (`max-w-[52rem]`), with `data-source` = `first-run` or `settings`. Header: step dots (`aria-label="Setup steps"`, the current one `aria-current="step"`, each with an sr-only label) and “<n> of 5”, plus an X (`aria-label="Skip setup"`, tooltip “Skip setup (Esc)”). Body: the step, scrolling, animated in from the right (`guide-fwd`) or left (`guide-back`). Footer: left “Back” (ghost) on every step but the first, where it reads “Takes about a minute.” instead, plus a flash span; right the secondary and primary actions (`SetupGuide.tsx:269-353`). An sr-only `role="status"` announces “Step <n> of 5: <step label>” (`SetupGuide.tsx:282-285`). On every step change the primary button takes focus, or the body when it is disabled (`SetupGuide.tsx:242-246`).

**Steps** (`SetupGuide.tsx:22-28`): Welcome, Your launchers, How Play works, Your accounts, Ready.

1. **Welcome** (`SetupGuideSteps.tsx:54-76`): brand mark, heading “Welcome to One Launch”, “Launch any game, from any launcher, on any account.”, the decorative `FlowArt` (a poster, an account chip whose name swaps between two values on a 5 s loop, and a Play button with a ringing halo, `SetupGuideArt.tsx:16-48`), and “Everything stays on this PC. There is no account, no sync, no telemetry.”
2. **Your launchers** (`SetupGuideSteps.tsx:179-210`): heading “Your launchers”; lead “One Launch is checking which launchers are installed on this PC.” while loading, “No launcher was found on this PC. Install one, or point One Launch at one that is installed somewhere unusual.” when none, else “One Launch found this launcher on this PC. Point it at any other that is installed somewhere unusual.” / “…found these launchers…”. Data: `launchers:status` (`SetupGuide.tsx:117`), pushed into the store's `launcherStatus`. Error: “Could not check launchers: <message>”. Rows (`SetupGuideSteps.tsx:82-130`; skeleton rows while loading): launcher dot, display name, the install folder in monospace or “Not found. If it is installed, point One Launch at <exe name>.” (exe names `steam.exe`, `EpicGamesLauncher.exe`, `Battle.net.exe`, `RiotClientServices.exe`, `src/shared/models.ts:26-31`); right, “Found” with a popping check, or a “Locate…” button that calls `launchers:locate { launcher }` (main's file picker) and reloads the status when an install comes back; errors under the row. Below: a `role="status"` games line, “Looking for installed games” with a spinner until the first scan has finished, then “<n> installed game(s) found” with the number counting up over 600 ms (`useCountUp`, `src/renderer/src/lib/motion.ts:18-43`); the count is polled every 1 s from `artwork:status` and, when its revision moves after `scanned`, `library:list` (`SetupGuide.tsx:41-69`). Then the artwork sentence: with `artwork.autoFetch` on, “Covers are downloaded for you from Steam; only game names and store ids are sent, never anything about your accounts. Games Steam does not sell need a free SteamGridDB key (Settings › Artwork).”; off, “Cover art downloads are off. Settings › Artwork turns them on; only game names and store ids are ever sent, never anything about your accounts.” (`SetupGuideSteps.tsx:144-148`).
3. **How Play works** (`SetupGuideSteps.tsx:230-258`): heading “Play switches accounts for you”, lead “Press Play on a game, pick the account to play it on, and One Launch does the rest.” Three points: “The launcher is switched for you” (“One Launch closes it, signs it in as the account you picked, starts it again and launches the game.”), “Sign in once per account” (“The first time you play on an account you may have to sign in once, in the launcher's own window. One Launch waits for you and carries on.”), “Backed up before every switch” (“The launcher's sign-in files are saved first and put back if anything fails. Settings › Backups can restore one by hand.”). When `dryRun === 'true'`: “Preview mode is on, so Play only shows what a switch would do and changes nothing. Settings › Advanced turns it off.” Right column: `PhaseDemo`, the five phase labels for a demo (“Counter-Strike 2” as “Alt” on Steam) advancing one every 850 ms through pending → running → done and holding for three ticks; frozen at the all-done frame under reduced motion (`SetupGuideArt.tsx:50-100`; `useLoop`, `motion.ts:46-58`). Footer line “What you see when you press Play.”
4. **Your accounts** (`SetupGuideSteps.tsx:364-439`): heading “Add your accounts”, lead “Import the ones your launchers already remember, or add them by hand. Passwords are optional and stay on this PC.” For each installed launcher that keeps a list (Steam, Battle.net), the guide reads `launchers:rememberedAccounts { launcher }` once the status is in (and again after a Locate) and pre-ticks every entry not already linked and not yet imported (`SetupGuide.tsx:133-160`). A `role="status"` summary: “Checking your launchers…”, “Reading what your launchers remember…”, “Your launchers have not remembered any account on this PC yet.”, “Everything here is in One Launch.”, or “<ticked> of <selectable> selected”; beside it a primary “Import” / “Import <n> account(s)” / “Importing…” button (disabled with nothing ticked). One section per launcher, `aria-label="<launcher> remembered accounts"`, header “<launcher>” plus “reading…” or “remembers <n> account(s)”; body: “Could not read <launcher>'s remembered accounts: <message>”, a skeleton, “<launcher> has not remembered any account on this PC yet.”, or rows (checkbox, name or login, pill “Imported” (ok) / “Already in One Launch” (info) / “Signed in” (ok, `mostRecent`), the login; linked and imported rows are locked) (`SetupGuideSteps.tsx:264-334`). When no installed launcher keeps a list: “No launcher was found on this PC, so there is nothing to import yet. Accounts can still be added by hand.” or, naming the installed launchers without a list, “<Name> keeps no account list. Add it under Add by hand.” / “<A> and <B> keep no account list. Add those under Add by hand.” (`SetupGuideSteps.tsx:337-350`). Section “Add by hand”: “Pick a launcher; its account form opens when you finish the guide.” and four `aria-pressed` toggle chips, one per launcher, at most one on (`SetupGuideSteps.tsx:411-436`). Import (`SetupGuide.tsx:172-195`): one `launchers:importAccounts { launcher, launcherUids }` per launcher with ticked entries, marking them imported; flash “Imported <n> account(s)” or “Nothing new to import” (success, 4 s), or “Could not import: <message>” (error).
5. **Ready** (`SetupGuideSteps.tsx:462-487`): a check roundel; heading “You're all set” when at least one account exists (`accounts:list` is counted on entering this step, `SetupGuide.tsx:120`), else “Almost there”; lead “Pick a game in the Library, pick the account to play it on, and press Play.” / “Add your <launcher> account next. Then pick a game in the Library, pick the account, and press Play.” (a by-hand launcher is chosen) / “Add an account from the Accounts page whenever you like. Then pick a game in the Library, pick the account, and press Play.”; three stat tiles “Launchers found”, “Games found”, “Accounts” (counting up; “…” while unknown).

**Footer rules** (`SetupGuide.tsx:252-266`): the secondary is “Skip” on every step but Ready. Primary: “Get started” on Welcome; “Continue” elsewhere; on Your accounts, while ticked rows are not yet imported, “Import <n> and continue”, which imports first and only advances when every import succeeded (a failed import stays on the step with the error flash) (`SetupGuide.tsx:223-228`). On Ready: with a by-hand launcher chosen, primary “Add a <launcher> account” and secondary “Open Library”; otherwise primary “Open Library” and, only when there are no accounts, secondary “Go to Accounts”. Every button is disabled while importing or closing.

**Exits** (`finish`, `SetupGuide.tsx:198-212`): Skip, the X, Escape (ignored while busy, `SetupGuide.tsx:230-240`), Open Library, Go to Accounts, and Add a <launcher> account all write `settings:set { key: 'firstRun.dismissed', value: 'true' }`, bump `settingsRevision`, close the guide (even when the write failed; the guide then returns next start), and then `setView('library')`, `setView('accounts')`, or `newAccount(launcher)` as chosen. Back/Continue never leave the guide.

**Motion** (`index.css:171-267`): the card fades up over 240 ms; steps slide in over 240 ms; rows rise over 360 ms with per-row delays; check marks pop over 280 ms; the two loops (5 s name swap, 2.4 s ring). All of it is turned off under `prefers-reduced-motion`, which also hides the second swap name and the ring.

### What's New card

**Purpose.** Announces a release's notes once after an update (`src/renderer/src/components/WhatsNewDialog.tsx`). Notes live in `src/shared/whats-new-releases.ts`.

- **When it opens** (`WhatsNewHost`, `WhatsNewDialog.tsx:170-207`): at startup the host reads `settings:getAll`, `app:info` and `recovery:get`. With a pending recovery, nothing happens (the card returns next start). With `whatsNew.enabled === 'false'`, it only records `settings:set { key: 'whatsNew.lastSeen', value: <running version> }` when that differs, so turning the setting on later never resurfaces old notes. Otherwise `pickWhatsNew` (`src/shared/whats-new.ts:84-94`) chooses the newest entry at or below the running version and newer than `whatsNew.lastSeen` (an unset or malformed `lastSeen` counts as older than everything); if one exists the card opens 400 ms later, unless the setup guide is up by then. Settings › General › Updates › “Show” opens the newest entry at or below the running version regardless (`latestWhatsNew`, `UpdatesPanel.tsx:76, 137-147`).
- **Layout** (`WhatsNewDialog.tsx:87-161`): fixed overlay at `z-40`, backdrop `bg-bg/80` with blur; `role="dialog" aria-modal aria-labelledby="whats-new-title"`, up to 640 px wide (`max-w-[40rem]`), full height at most. Header: “What's New” and the release date formatted from its `YYYY-MM-DD` parts as a local long date (never shifted by time zone, `WhatsNewDialog.tsx:26-30`), and an X (`aria-label="Close"`, tooltip “Close (Esc)”). Body: a scrolling list (`tabIndex=-1`, takes the focus on open; permanent scrollbar gutter; thumb always visible via `sb-always`) holding the optional image full width, the optional intro, and the sections: an uppercase heading in the section's tone (accent by default; success, warning, danger, or a launcher colour) with a rule, then items as a glowing accent dot, the bold `lead` and the muted `detail`. Rows animate in with a 50 ms stagger capped at 700 ms.
- A wheel event anywhere in the window scrolls the list (line and page delta modes converted; smooth unless reduced motion); over the list itself the browser scrolls natively (`WhatsNewDialog.tsx:44-54`).
- **Every exit** (the X, Escape, the backdrop) writes `settings:set { key: 'whatsNew.lastSeen', value: <running version> }`, bumps `settingsRevision`, and closes even when the write failed (`WhatsNewDialog.tsx:57-77, 91`). A second close while one is in flight is ignored.
- **[dev-only]** Under `npm run dev`, saving the releases file swaps the open card's release for the new one (`src/renderer/src/components/WhatsNewDevPanel.tsx:12-19`).

### Settings view

**Purpose.** Every setting, grouped under five tabs (`src/renderer/src/views/SettingsView.tsx`).

**Layout.** `Page narrow` with a scroll ref. Sticky header: title “Settings” and, in the toolbar slot, a `TabList` (`aria-label="Settings sections"`) with tabs “General”, “Launchers”, “Artwork”, “Backups”, “Advanced” (`SettingsView.tsx:23-29, 152`). Below the header, an optional vault banner, then a `TabPanel` holding only the chosen tab's panels (so their polls stop off-tab). Choosing a tab scrolls the page to the top (`SettingsView.tsx:71-74`). The chosen tab is kept in the store for the session; `openSettings(tab)` from elsewhere lands on a tab (`store/ui.ts:81-82, 202`).

- **Tabs widget** (`src/renderer/src/components/Tabs.tsx`): `role="tablist"` of text-only `role="tab"` buttons with roving tabindex; Left/Right wrap, Home/End jump, and selection follows the focus; each tab controls a `role="tabpanel"`.
- **Data** (`SettingsView.tsx:59-62`): `app:info`, `vault:status`, `autostart:status`, and `settings:getAll` (re-read on every `settingsRevision` bump).
- **Saving a setting** (`setSetting`, `SettingsView.tsx:103-112`): `settings:set { key, value }` with `'true'` / `'false'` for toggles; success bumps `settingsRevision` (the sidebar and this view re-read); failure shows “Could not save the setting: <message>” as a `role="alert"` under that row (`src/renderer/src/components/Panel.tsx:42-48`) and reloads the settings so the control snaps back. Toggles are disabled while settings load. Boolean settings read as on unless stored `'false'`, except `dryRun` and `steam.autoLogin`, which read as on only when `'true'` (`SettingsView.tsx:93-101`).
- **Vault banner** (`SettingsView.tsx:155-183`), shown above the tabs when `vault:status` reports `readable === false`: danger-toned, “Your stored passwords and saved sign-ins can no longer be read. This happens when the app's data folder was copied from another PC or Windows user, or when its `chromium` folder was deleted. Clearing them makes One Launch usable again; you then enter passwords and save sign-ins afresh.” Button “Clear stored passwords and sign-ins…” (danger) → two-step “Yes, clear them” (“Clearing…”) + “Cancel”. Confirm calls `vault:reset` and shows “Removed <n> password(s), <n> saved sign-in(s), <n> other secret(s).” (singular/plural per count), then re-reads the vault status; a failure shows the message (`SettingsView.tsx:76-91`).

#### General tab

- **General panel** (`SettingsView.tsx:189-209`):
  - Row “Start with Windows”, hint “Starts One Launch when you sign in to Windows.” Toggle checked from `autostart:status.openAtLogin`; disabled while loading or when `available !== true` (development builds and custom data folders, `src/shared/models.ts:346-354`). Change: `autostart:set { openAtLogin }`, then the status is re-read; failure shows “Could not save the setting: <message>” under the row (`SettingsView.tsx:114-122`).
  - Row “Minimize One Launch to System Tray”, hint “Closing One Launch minimizes the app.” Toggle for `window.closeToTray`.
  - Row “Setup guide”, hint “The welcome steps: your launchers, how Play works, and adding accounts.” Button “Show again” → `openSetupGuide('settings')`.
- **Updates panel** (`src/renderer/src/components/UpdatesPanel.tsx`): polls `updates:status` every 5 s while mounted (1 s while checking or downloading) into the store (`UpdatesPanel.tsx:13, 68`).
  - Header action: when the state is `ready`, a primary “Restart to update” (disabled while restarting or while a switch job is running) calling `restartToUpdate()` (leave guard, then `updates:install`); failure shows “Could not restart to update: <message>”. Otherwise “Check for updates” (spinner while busy), disabled until the first status arrives, while busy, or when updates are `unavailable`; calls `updates:check` and stores the returned status; failure shows “Could not check for updates: <message>” (`UpdatesPanel.tsx:79-116`).
  - Row “Keep One Launch updated automatically”, hint “Downloads new versions in the background and installs them the next time One Launch restarts.” Toggle for `updates.auto`.
  - Row “Show what's new after an update”, hint “The first time you open One Launch after an update, a short list of what changed.” Toggle for `whatsNew.enabled`.
  - Row “Version”: “<version> · <state>” where state is “…” before the first answer, “Not checked yet” (idle), “Checking…”, “Up to date” (success), “Downloading <version> · <percent>%”, “<version> is ready” (success), “Check failed” / “Download failed” / “Not verified” (danger, by failed step), or “Off for this copy” (unavailable) (`UpdatesPanel.tsx:22-41`). Row hint: “Last checked at <time>.” when up to date; the unavailability reason: “Updates work only in the installed app.” (development), “Updates are off while One Launch runs from a custom data folder.”, “Updates work only in a copy installed with the One Launch installer.”, “Updates need a per-user install. Reinstall and choose "Only for me".”; when ready, “It installs the next time One Launch restarts.” (auto on) or “It installs when you choose Restart to update.” (`UpdatesPanel.tsx:15-20, 43-48`).
  - Row “What's new”, hint “The changes in the latest update.” Button “Show” (`aria-label="Show what's new"`), disabled when no entry is at or below the running version; opens the card.
  - A failed status adds a `role="alert"` line: “Could not check for updates: <message>.”, “Could not download the update: <message>.”, or “Could not verify the update, so it was not installed: <message>.” (`UpdatesPanel.tsx:50-54`).

#### Launchers tab

- **Launchers panel** (`src/renderer/src/components/LauncherStatusPanel.tsx`): description “Which launchers are installed and set up the way One Launch expects.” Action “Re-check” (spinner while loading) reloads `launchers:status`. The last answer is kept in the store and shown at once on later visits while a fresh one loads (`LauncherStatusPanel.tsx:146-153`); before any answer, four skeleton rows of the same height. Error: “Could not check launchers: <message>”.
  - **Row** (`LauncherStatusPanel.tsx:17-122`): a disclosure button (chevron; disabled, with a blank spacer, when the launcher is not `implemented`) with the launcher dot, display name and the install folder in monospace or “Not installed”. Right: a “Locate…” button when implemented, not installed and the row is collapsed; a “Needs attention” pill (fail tone) when `attention` is set; a health pill “Healthy” / “Warnings” / “Problems” / “Not installed” by `health.status` (`src/renderer/src/components/StatusPill.tsx:19-21`); a pill “Running” / “Not running” when `running` is known.
  - **Expanded** (`LauncherStatusPanel.tsx:85-119`): the attention message in danger text when set; a location line, “Checking…” while a re-check this row asked for is pending, else “You chose this folder.” (install source `user`), “<exe name> is no longer in the folder you chose.” (a chosen path that no longer holds the exe), “Found automatically.”, or “Not found. If it is installed, point One Launch at <exe name>.” (`LauncherStatusPanel.tsx:11-15`); beside it “Locate…” and, when a chosen path exists, “Reset” (ghost). Then the health checks, each a coloured dot (ok/warn/absent/fail), a 160 px label and the detail, and “Checked <local time>”.
  - **Locate…** calls `launchers:locate { launcher }` (main opens its file dialog, see Native dialogs); a cancelled picker (`install === null`) changes nothing; otherwise the row is marked stale and the whole status reloads. **Reset** calls `launchers:resetPath { launcher }` then reloads. Errors show under the row (`LauncherStatusPanel.tsx:31-44`).
- **Play panel** (`SettingsView.tsx:227-268`), description “What One Launch does for you when you press Play.”:
  - Row “Steam automatic sign-in”, hint “Passes a stored Steam password to Steam for you; other programs can see it while Steam runs.” Toggle for `steam.autoLogin` (on only when `'true'`).
  - Row “Start Riot games for you”, hint “Presses Play in the Riot Client for you. Off, it opens the game's page and you press Play.” Toggle for `riot.pressPlay`.
  - Row “Hide One Launch after Play”, hint “Sends the window to the tray as the game starts, when no account switch is needed.” while the tray setting is on, else “Needs Minimize One Launch to System Tray, in General.” Toggle for `play.hideToTray`, shown off and disabled while `window.closeToTray` is off (`SettingsView.tsx:252-267`).

#### Artwork tab

**Artwork panel** (`src/renderer/src/components/ArtworkPanel.tsx`), description “Covers come from Steam. Other games need a free SteamGridDB key.” Polls `artwork:status` every 3 s while mounted (`ArtworkPanel.tsx:43-47`). Header actions: “Rescan” (tooltip “Look for games added or removed since the last scan”) and “Refresh all” (tooltip “Fetch every cover again; images you chose yourself are kept”), both disabled while any action is busy.

- Row “SteamGridDB API key”. Without a key: hint “Free. Press Get a key, sign in, paste it here.”; a “Get a key” button (tooltip “Opens the SteamGridDB API page in your browser”; `app:openUrl { target: 'steamgriddb-api-key' }`; failure “Could not open the browser: <message>”) and a 224 px password-type field (`aria-label="SteamGridDB API key"`, placeholder “Paste your key”), disabled when the vault is unavailable. The field saves itself: a paste that looks like a key (20 to 64 letters and digits, `ArtworkPanel.tsx:24`) is saved at once without entering the field; Enter saves a key-shaped value or shows “That does not look like a SteamGridDB API key. Paste the key itself.”; leaving the field saves a key-shaped value silently (`ArtworkPanel.tsx:66-72, 174-184`). Saving calls `vault:setSecret { key: 'steamgriddb.apiKey', value }`, then `artwork:refresh { gameId: null, force: false }`, and shows “Key saved. Fetching artwork for <n> game(s).” or “Key saved.” (`ArtworkPanel.tsx:50-64`). With a key: hint “Remove it to paste a different one.”; a disabled masked field (“••••••••”) and an icon button “Remove key” (`aria-label` and tooltip) calling `vault:deleteSecret { key: 'steamgriddb.apiKey' }`, message “Key removed.”.
- Row “Fetch artwork automatically”, hint “Looks up missing covers after each scan; only game names and ids are sent.” Toggle for `artwork.autoFetch`; save errors under the row.
- Row “Queue”, hint “Background downloads since the app started.”: “<n> in progress · ” (when active) then “<completed> done” and “ · <failed> not found” when any failed; “…” before the first status.
- “Refresh all”: `artwork:refresh { gameId: null, force: true }` → “Re-fetching artwork for <n> game(s). Images you chose yourself are kept.” “Rescan”: `library:rescan` → “Library rescanned: <added> added, <removed> removed.” plus “ Not scanned: <launcher>: <error>; …” when any launcher failed. Error messages from any action replace the message line (`ArtworkPanel.tsx:97-126, 203`).

#### Backups tab

**Backups panel** (`src/renderer/src/components/BackupsPanel.tsx`), description “The launcher files One Launch changes, saved before every switch.” Header actions: “Refresh” (re-lists every launcher) and “Open folder” (`app:openPath { target: 'backups' }`; failure “Could not open the folder: <message>”).

- Row “Snapshots to keep”, hint “Per launcher. Older ones go as new ones are taken; Kept snapshots never count.” A `Dropdown` over 5, 10, 20, 50, 100 plus whatever value is stored (so a value set outside the app stays selectable); 20 is labelled “20 (default)” and used when the stored value is below 1 or missing. Reads `settings:get { key: 'backups.keep' }`, writes `settings:set { key: 'backups.keep', value }` and re-reads; failure shows “Could not save the setting: <message>” above the row (`BackupsPanel.tsx:12-13, 151-165, 210-219`).
- One collapsible section per launcher (`LauncherBackups`, `BackupsPanel.tsx:89-142`): header chevron, dot, name, “<n> snapshot(s)” or “…” while first loading; `backups:list { launcher }`, re-run on Refresh. Expanded: “Could not list backups: <message>”; or “No snapshots yet. The first one is taken automatically before the first real switch.”; or rows: the date and time, a “Kept” badge (warning tone, tooltip = the pin reason) when pinned, then “<pin reason> · ” (when pinned) “<reason or “Snapshot”> · <n> file(s) · <size B/KB/MB> · <id>” (id in monospace), and a “Restore” button.
- After a restore, a success box: “Restored snapshot <id>: <n> file(s)[, registry <names joined by “, ”>]. Start the launcher to check it signed in as expected.” (`BackupsPanel.tsx:203-208`).
- **Restore dialog** (`RestoreDialog`, `BackupsPanel.tsx:26-87`): fixed overlay at `z-30`, `role="alertdialog" aria-modal aria-labelledby="restore-title"`, up to 512 px wide, focus trap, backdrop click closes; there is no Escape handler. Title “Restore <launcher> files from <date time>?”, subtitle the snapshot reason or “Snapshot”. Body: “This overwrites the current copies with the ones in the snapshot:” then the snapshot's files in monospace and, for Steam, “HKCU\Software\Valve\Steam\AutoLoginUser and RememberPassword (as recorded)”; a warning box: Steam “Steam must be closed first. It rewrites loginusers.vdf when it exits, which would undo the restore.”, others “<launcher> must be closed first. It rewrites its settings file while it runs, which would undo the restore.”; errors in danger text. Footer: “Cancel” (ghost) and danger “Restore this snapshot” (“Restoring…”). Restore calls `backups:restore { launcher, id }`; success closes the dialog and shows the success box; failure keeps the dialog open with the message.

#### Advanced tab

- **Launching panel** (`SettingsView.tsx:286-308`), description “How Play switches and starts games.”:
  - Row “Preview mode”, hint “Play shows what a switch would do instead of doing it.” Toggle for `dryRun` (on only when `'true'`).
  - Row “Epic launch link”, hint “Change only if Epic games stop launching after an Epic update.” `Dropdown` over `epic.launchUri`: “Catalog link (default)” (`catalog`) and “App name link” (`appname`) (`src/shared/models.ts:282`).
  - Row “Battle.net launch”, hint “Change only if games stop launching after a Battle.net update.” `Dropdown` over `battlenet.launch`: “Launch command (default)” (`exec`) and “battlenet:// link” (`uri`) (`models.ts:270`).
- **Encryption panel** (`SettingsView.tsx:310-344`), description “Windows encrypts passwords and saved sign-ins for your account; nothing is in plain text.” Action “Run test” (“Testing…”), disabled while running or when the vault is unavailable.
  - Row “Status”: “Checking…” while loading; “Working” (success) when available and readable; “Key lost” (danger) when available but `readable === false`; “Unavailable · passwords and sign-ins cannot be stored” (danger) when not available.
  - Row “Last test”, hint “Stores a random value, reads it back, deletes it.”: “Not run”, “Running…”, “Passed in <n> ms” (success) or “Failed: <message>” (danger).
  - **Self-test** (`SettingsView.tsx:135-148`): `vault:setSecret { key: 'diagnostics.roundtrip', value: <random UUID> }`, `vault:getSecret { key: 'diagnostics.roundtrip' }`, `vault:deleteSecret { key: 'diagnostics.roundtrip' }`; a mismatch fails with “Decrypted value did not match what was stored”; the time is measured in the renderer.
- **Diagnostics panel** (`SettingsView.tsx:346-382`). Actions “Logs” and “Data folder” call `app:openPath { target: 'logs' | 'data' }`; failure shows “Could not open the folder: <message>” above the rows.
  - Row “Log level”, hint “Logs stay on this PC, with passwords, tokens, emails and account ids removed.” `Dropdown` over `logLevel`: “Debug”, “Info”, “Warn”, “Error” (`models.ts:294`).
  - Row “Runtime”: “Electron <version> · Node <version>” (“…” until `app:info` answers).
  - Row “Startup”, hint “From process start to the window, then to the interface.” with “; <n> ms before app code ran” inserted before the full stop when `processToNodeMs` is known: value “<windowShownMs> ms / <didFinishLoadMs> ms”, each “pending” when null.
  - Row “Data folder”: the data folder path in monospace, selectable.
  - Row “Build”: “Packaged” or “Development”.
- **[dev-only] Developer panel** (`src/renderer/src/components/WhatsNewDevPanel.tsx`; mounted lazily only when `import.meta.env.DEV`, `SettingsView.tsx:20-21, 384-388`): description “Only in a development build. Under npm run dev, saving the releases file updates the open card.” Row “Preview What's New”, hint “Any entry in whats-new-releases.ts, whatever the running version.”: a `Dropdown` of every release version (newest preselected) and a “Preview” button that opens the card for that entry with the running version from `app:info`.

### Native dialogs (main process)

- **Quit during a switch** (`src/main/index.ts:194-231`; triggered from `before-quit` while a job is busy, `index.ts:737-745`): if no job object exists yet the pending start is stopped and the window closes with no prompt. Otherwise the window is shown first (a tray quit arrives with it hidden) and a warning message box asks: title “A switch is still running”, message “One Launch is still switching to <account>.”, detail “Quitting stops the switch. If <launcher> has not been started again yet, its files are put back first; after that <launcher> carries on signing in on its own. Left alone, the switch finishes once the launcher reports the sign-in.”, buttons “Keep running” (default and cancel) and “Quit anyway”. Only one prompt is open at a time. “Quit anyway” stops the running switch, then closes the window. **[dev-only]** a fake scenario can pre-answer the prompt (`index.ts:207-209`).
- **Startup error** (`index.ts:715-726`): when bootstrap throws, an error box titled “One Launch could not start” with the message, then “Details are in the log:” and the log file path, then “Data folder:” and the data folder, after which the process exits with code 1.
- **Crash dialog** (`index.ts:777-805`): on an uncaught exception after startup, at most once per 60 s and only while a window exists, the window is shown and an error message box asks: title “One Launch hit an unexpected error”, message “Something went wrong inside One Launch.”, detail “<reason>\n\nThe details are in <log path>. Relaunching is the safe choice; keeping it running may leave things half-done.”, buttons “Relaunch” (default) and “Keep running” (cancel). Relaunch restarts the app without the start-hidden flag and quits through the normal path, with a hard exit 15 s later as a fallback (`QUIT_WAIT_MS` 10 s + 5 s, `index.ts:142, 802`).
- **File pickers** (`index.ts:642-660`): “Choose an image” (open file; filter “Images”: png, jpg, jpeg, webp) for the card menu's “Choose portrait image…”; “Locate <exe name>” (open file; filter named after the launcher, `exe` only) for Locate… in Settings and the setup guide. Both are owned by the main window when it exists and return null when cancelled.
- **Tray** (`src/main/tray.ts:13-25`): created when the tray setting is on; a 16 px icon scaled to the primary display; tooltip “One Launch” (or “One Launch (fake launchers)” on fake launchers, `index.ts:480`); context menu “Open One Launch”, a separator, “Quit One Launch”; single and double click open the window. If the icon cannot be read the tray is skipped and closing the window quits (`index.ts:185-187`).

### Global behaviours

**Shortcuts** (`src/renderer/src/lib/shortcuts.ts`; `App.tsx:37-69`). A shortcut is plain Ctrl plus the key (no Alt or Shift); none fires while any element with `aria-modal="true"` is in the document.

| Keys | Where | Fires from inside a field | Effect |
| --- | --- | --- | --- |
| Ctrl+1 / Ctrl+2 / Ctrl+3 | Anywhere | No | `setView('library' / 'accounts' / 'settings')` (`App.tsx:55-58`) |
| Ctrl+B | Anywhere | No | Toggle the sidebar (`App.tsx:46-50`) |
| Ctrl+F | Library | Yes | Focus and select the search field (`LibraryView.tsx:178-188`) |
| Ctrl+S | Account edit page | Yes | Save changes (no-op when clean or invalid) (`AccountDetailView.tsx:608-617`) |
| Escape | Dialogs | — | Closes the launch dialog (not while busy), the import dialog, the What's New card, the setup guide (not while busy), answers the confirm dialog no; clears the Library search text; cancels password editing. Not handled by the recovery or restore dialogs. |
| Enter | Launch dialog, pick stage | Not from a button or field | Starts the check (`LaunchDialog.tsx:364-375`) |
| Back gesture: mouse button 3 on `mouseup`, or Alt+Left | Account create/edit page only | Yes | `backToAccounts` through the leave guard; nothing elsewhere (`shortcuts.ts:21-30`; `App.tsx:38-62`) |

**Flash / status messages** (`src/renderer/src/lib/useFlash.ts`): a transient message in an always-mounted `role="status"` span that is `sr-only` while empty, so the next message is announced (`flashClassOf`). Default duration 3000 ms; 6000 ms for the error tone; callers may override (copy notes 4000 ms, import results 4000 ms, “Account created…” 8000 ms). Tones: info (muted), success, error (danger). Views with a flash: Library header, Accounts header, account edit header, setup guide footer.

**Error template**: “Could not <verb> <thing>: <message>” in danger text, `role="alert"` when it appears on its own; the message is the IPC error's `message`. The renderer branches on an error `code` in exactly one place: `CONFIRM_REQUIRED` in the launch dialog (`LaunchDialog.tsx:439`).

**Startup prefetch** (`src/renderer/src/lib/startup.ts`; `src/shared/ipc-contract.ts:249-257`). The preload sends `STARTUP_REQUESTS` when the document starts: `library:list`, `launch:recent { limit: 8 }`, `settings:getAll`, `accounts:list`, `app:info`, `recovery:get`, `updates:status`. `prefetchStartup` collects the answers and resolves when all are in or after 50 ms (`boot.ts:7`). While the window is open, `peekStartup(channel, req)` hands a settled, successful answer with the same request to a `useState` initialiser (used as `useIpcQuery`'s `seed`, which then skips the first fetch), and `ipc.invoke` for the same channel and request joins the in-flight startup call instead of asking main again (`ipc.ts:19-20`). `App`'s first effect closes the window (`App.tsx:33-35`); after that every mount and reload fetches fresh. Seeds are used by the title bar and sidebar (`app:info`, `settings:getAll`), the Library (`library:list`, `launch:recent`) and the recovery host (`recovery:get`). A failed startup answer is served as an `INTERNAL` error envelope and the caller fetches as usual (`startup.ts:29-32, 63-66`).

**Polling the renderer runs** (main never pushes):

| What | Channel | Interval | Where |
| --- | --- | --- | --- |
| Job progress | `launch:status` | 150 ms while a job is running | `LaunchHost.tsx:107` |
| Library revision and artwork queue | `artwork:status` | 500 ms while the queue is busy; 1000 ms until games are on screen; 4000 ms otherwise; 3000 ms after an error | `LibraryView.tsx:205-207` |
| Updater | `updates:status` | 1000 ms while checking/downloading; else 60 s (sidebar), 5 s (Settings › Updates) | `lib/updates.ts:6`, `Sidebar.tsx:21`, `UpdatesPanel.tsx:13` |
| Artwork queue | `artwork:status` | 3000 ms | `ArtworkPanel.tsx:45` |
| Installed-game count | `artwork:status`, then `library:list` when the revision moved | 1000 ms | `SetupGuide.tsx:41-69` |
| Relative-time captions | none (local clock) | 60 s | `LibraryView.tsx:97-100` |

None of these back off while the window is hidden (`docs/renderer.md:42`).

**localStorage keys** (per viewer, try/catch everywhere): `sidebar` = `expanded` | `collapsed` (`store/ui.ts:10-29`); `library.sort` = `name` | `lastPlayed` (`LibraryView.tsx:23, 39-45, 220-228`). Everything else is a setting in main.

**Accessibility conventions**:
- Dialogs: `role="dialog"` (launch, import, setup guide, What's New) or `role="alertdialog"` (confirm, recovery, restore), `aria-modal="true"`, `aria-labelledby`, a focus trap that keeps Tab/Shift+Tab inside, focuses the first control unless `autoFocus` already placed it, and restores focus on close (`src/renderer/src/lib/useFocusTrap.ts:10-41`). `useFocusWhenReady` moves focus to a primary action once it is enabled, unless the user has already moved it off the Close button (`useFocusTrap.ts:48-62`).
- Popups: `Dropdown` is `aria-haspopup="listbox"` over a `role="listbox"` with `aria-activedescendant`, `MenuButton` is `aria-haspopup="menu"` over `role="menu"` / `menuitem`, `CardMenu` is a `role="menu"`. All handle Down/Up (menus wrap and skip disabled items, the listbox clamps), Home, End, Enter, Space, Escape and Tab; return focus to the opener; close on outside pointer-down, window resize or any scroll; and are positioned fixed, flipped above the trigger when there is no room below, and clamped 8 px inside the viewport (`Dropdown.tsx:57-133`, `MenuButton.tsx:35-97`, `CardMenu.tsx:29-74`). Down on a closed trigger opens it.
- `Toggle` is `role="switch"` with `aria-checked` and `aria-label` (`Toggle.tsx`). Icon-only buttons carry `aria-label`; tooltips go in `title` and name the shortcut when there is one. Tabs have no `aria-label` so they can be clicked by text.
- Status: flashes and the launch dialog's one `sr-only` line are `role="status"`; standalone errors are `role="alert"`.
- `body` is `user-select: none`; inputs opt back in, and values the user may copy (paths, step details, the error boundary's message) carry `select-text` (`index.css:74-80`).

**Motion rules** (`docs/renderer.md:8`; `index.css`): interactive elements use 150 ms colour transitions; the sidebar width 200 ms ease-out; everything that moves carries `motion-reduce:transition-none`. The only entrance animations are the setup guide's and the What's New card's (opacity and transform only, 240 to 360 ms); the guide's two loops and the phase demo pause under reduced motion (`usePrefersReducedMotion`, `motion.ts:6-15`), and the CSS block at `index.css:252-267` turns the keyframes off. `useCountUp` jumps straight to the value under reduced motion (`motion.ts:24-28`).

**Design tokens** (`index.css:3-57`): colours as CSS variables exposed as Tailwind utilities: `bg` #0b0d12, `surface` #12151c, `surface-2` #1a1e28, `surface-3` #222736, `border` #262b38, `border-strong` #343a4b, `text` #e8eaf0, `text-strong` #ffffff, `muted` #8f97ab, `faint` #7f879b, `accent` #8b5cf6, `accent-hover` #9d75ff, `accent-soft` rgb(139 92 246 / 0.16), `success` #3ddc97, `warning` #f5b750, `danger` #f25f5c, launcher colours `steam` #66c0f4, `epic` #e6e6e6, `battlenet` #148eff, `riot` #ff4655. Fonts: `font-sans` Segoe UI Variable Text / Segoe UI (body 14 px, line height 1.45), `font-mono` Cascadia Code / Cascadia Mono / Consolas. Radii: `rounded-panel` 12 px; `shadow-panel` an inset 1 px highlight plus a 24 px drop shadow. Callout tone pattern: `border-<tone>/30 bg-<tone>/10 text-<tone>` (warning = needs the user, danger = failed, accent-soft = in progress, success = done). Focus ring: 2 px accent outline offset 2 px. Scrollbars: 12 px custom WebKit scrollbars, thumb visible only on the scroll container under the pointer (`.sb-hover`, set by `lib/scrollbarHover.ts`) or always with `.sb-always`. Selection colour is `accent-soft`.

**Button variants** (`src/renderer/src/components/Button.tsx:10-17`): `primary` (accent, white text), `secondary` (surface-2 with border; the default), `ghost` (muted text, no border), `danger` (danger border and tint); all `whitespace-nowrap`, 50 % opacity when disabled.

## 3. Data model

Everything the app stores or passes between its processes. Types are given as they appear on the wire (renderer-visible) or at rest (SQLite, JSON files); the "source" column cites where the type or the invariant is defined. Nothing in this section is development-only unless tagged.

### 3.1 Enumerations

#### Launcher ids

| Id | Display name | Client name (mid-sentence) | Executable Locate asks for | Source |
| --- | --- | --- | --- | --- |
| `steam` | “Steam” | “Steam” | `steam.exe` | `src/shared/models.ts:6-31` |
| `epic` | “Epic Games” | “Epic Games Launcher” | `EpicGamesLauncher.exe` | `src/shared/models.ts:6-31` |
| `battlenet` | “Battle.net” | “Battle.net” | `Battle.net.exe` (a shortcut's `Battle.net Launcher.exe` next to it is accepted too) | `src/shared/models.ts:25-31` |
| `riot` | “Riot Games” | “the Riot Client” | `RiotClientServices.exe` | `src/shared/models.ts:6-31` |

- Order everywhere a list is built: `steam, epic, battlenet, riot` (`src/shared/models.ts:8`).
- Any string not in this list read from the database is dropped at read time rather than rejected (`src/main/services/accounts.ts:82`, `src/main/services/library.ts:40`, `src/main/services/sessions.ts:19`, `src/main/services/launch.ts:128`).

#### Setting keys (19)

`dryRun`, `logLevel`, `artwork.autoFetch`, `backups.keep`, `epic.launchUri`, `battlenet.launch`, `steam.autoLogin`, `riot.pressPlay`, `play.hideToTray`, `steam.path`, `epic.path`, `battlenet.path`, `riot.path`, `window.bounds`, `window.closeToTray`, `firstRun.dismissed`, `updates.auto`, `whatsNew.enabled`, `whatsNew.lastSeen` (`src/shared/models.ts:206-247`).

- Main-only (renderer may read, never write): `window.bounds`, `steam.path`, `epic.path`, `battlenet.path`, `riot.path` (`src/shared/models.ts:262`).
- The per-launcher path keys are mapped from launcher id by `LAUNCHER_PATH_SETTINGS` (`src/shared/models.ts:254-259`).
- Full behaviour per key: section 9.

#### Secret keys

| Key | Who may set/delete | Who may read back | Source |
| --- | --- | --- | --- |
| `steamgriddb.apiKey` | renderer and main | main only (artwork service, at the moment of a request) | `src/shared/models.ts:188-195`, `src/main/index.ts:608-609` |
| `diagnostics.roundtrip` | renderer and main | renderer, through `vault:getSecret` (the only key that channel accepts) | `src/shared/models.ts:195`, `src/main/ipc/index.ts:170-174` |
| `vault.probe` | main only | main only | `src/shared/models.ts:192-193`, `src/main/services/vault-health.ts:8,41-45` |

#### Open-URL targets

`steamgriddb-api-key` → `https://www.steamgriddb.com/profile/preferences/api`; the only target (`src/shared/models.ts:198-199`, `src/main/ipc/index.ts:26-28`).

#### Log levels

`debug`, `info`, `warn`, `error` (`src/shared/models.ts:292-294`).

#### Launch forms

| Enum | Values | Default when unset or invalid | Source |
| --- | --- | --- | --- |
| `BattlenetLaunchForm` | `exec`, `uri` | `exec` | `src/shared/models.ts:268-274`, `src/main/index.ts:551-554` |
| `EpicLaunchUriForm` | `catalog`, `appname` | `catalog` | `src/shared/models.ts:280-286`, `src/main/index.ts:533-536` |

#### Artwork

| Enum | Values | Notes | Source |
| --- | --- | --- | --- |
| `ArtworkKind` | `grid`, `hero`, `logo` | one cached file per kind per game | `src/shared/models.ts:101-103` |
| `ArtworkSource` | `steamgriddb`, `steamcdn`, `manual`, `launcher` | `launcher` is the game's own icon standing in for a missing cover | `src/shared/models.ts:106` |
| `ArtworkExt` (file extension) | `jpg`, `png`, `webp`, `ico` | `ico` only ever for a `launcher` stand-in | `src/main/services/artwork-url.ts:14-15` |

#### Launch and switch

| Enum | Values | Source |
| --- | --- | --- |
| `LaunchResult` (history row) | `ok`, `failed`, `cancelled` | `src/shared/models.ts:146` |
| `SwitchStepKind` | `process`, `backup`, `file`, `registry`, `launch`, `wait` | `src/shared/models.ts:436` |
| `SwitchStrategy` | `remembered`, `credentials` | `src/shared/models.ts:461` |
| `JobStepStatus` | `pending`, `running`, `done`, `skipped`, `failed` | `src/shared/models.ts:542` |
| `LaunchJobState` | `running`, `done`, `failed`, `needs-sign-in`, `cancelled` | `src/shared/models.ts:557` |
| `LaunchAttention.kind` | `sign-in` (the only kind) | `src/shared/models.ts:564` |
| `SwitchRecoveryPhase` | `writing`, `signing-in`, `rollback-failed` | `src/shared/models.ts:617` |
| `RunningGame.source` | `launcher`, `process` | `src/shared/models.ts:522` |
| `LauncherInstall.source` | `user`, `registry-hkcu`, `registry-hklm`, `launcher-file`, `default-path` | `src/shared/models.ts:378` |
| `HealthStatus` | `ok`, `warn`, `fail`, `absent` (`absent` = not installed, not a problem) | `src/shared/models.ts:419-420` |

#### Step ids

The adapters report progress against fixed step ids; the renderer folds them into five phases and the journal classifies them as before or after the first write.

| Step id | Phase shown to the user | Counts as "after the writes" for recovery | Source |
| --- | --- | --- | --- |
| `close` | “Close <launcher>” | no | `src/renderer/src/lib/phases.ts:7`, `src/main/services/switch-journal.ts:31` |
| `capture` | “Switch to <account>” | no | `src/renderer/src/lib/phases.ts:8` |
| `backup` | “Switch to <account>” | no | `src/renderer/src/lib/phases.ts:9` |
| `file` | “Switch to <account>” | no | `src/renderer/src/lib/phases.ts:10` |
| `registry`, `registry-user`, `registry-remember` | “Switch to <account>” | no | `src/renderer/src/lib/phases.ts:11-13` |
| `start` | “Start <launcher>” | yes | `src/renderer/src/lib/phases.ts:14`, `src/main/services/switch-journal.ts:31` |
| `wait` | “Start <launcher>” | yes | `src/renderer/src/lib/phases.ts:15` |
| `verify` | “Sign in” | yes | `src/renderer/src/lib/phases.ts:16` |
| `login` | “Sign in” | yes | `src/renderer/src/lib/phases.ts:17` |
| `manual` | “Sign in” | yes | `src/renderer/src/lib/phases.ts:18` |
| `recapture` | “Sign in” | yes | `src/renderer/src/lib/phases.ts:19` |
| `launch` | “Launch <game>” (added by the launch service, never by an adapter) | not journaled (the journal is gone before the launch) | `src/renderer/src/lib/phases.ts:20`, `src/main/services/launch.ts:194,359-361` |

- An unknown step id falls into the “Switch to <account>” phase (`src/renderer/src/lib/phases.ts:25-27`).
- A journaled step id must match `^[a-z-]{1,32}$` or the whole journal entry is unreadable (`src/main/services/switch-journal.ts:29,43`).

#### Health check ids

Each adapter's health report is a list of checks with a stable id and label. Ids seen in the adapters (`add(id, label, …)` calls):

| Launcher | Check ids and labels | Source |
| --- | --- | --- |
| all | `chosen-folder` “Chosen folder” (warn: the chosen folder no longer holds the executable) | `src/main/launchers/chosen-folder.ts:19-24` |
| `steam` | `install` “Installation”, `process` “Process”, `loginusers` “Remembered accounts”, `registry` “Registry AutoLoginUser”, `most-recent-key` “Most-recent marker”, `libraries` “Library folders”, `games` “Installed games”, `backups` “Backups” | `src/main/launchers/steam/adapter.ts:1191-1270` |
| `epic` | `install` “Installation”, `process` “Process”, `settings` “GameUserSettings.ini”, `launcher-installed` “LauncherInstalled.dat”, `manifests` “Manifests”, `log` “Launcher log”, `backups` “Backups” | `src/main/launchers/epic/adapter.ts:1353-1430` |
| `battlenet` | `process` “Process”, `config` “Battle.net.config”, `token` “Session token”, `aggregate` “aggregate.json”, `product-db` “product.db”, `uninstall` “Installed products”, `log` “Client log”, `backups` “Backups” | `src/main/launchers/battlenet/adapter.ts:1624-1710` |
| `riot` | `install` “Installation”, `process` “Process”, `data` “Client data”, `products` “Installed products”, `log` “Client log”, `backups` “Backups” | `src/main/launchers/riot/adapter.ts:1739-1815` |

(The status, detail text and ordering of each check belong to the adapter sections.)

#### Update status

| State | Extra fields | Source |
| --- | --- | --- |
| `unavailable` | `reason`: `development`, `custom-data-folder`, `not-installed`, `all-users-install` | `src/shared/models.ts:356,361` |
| `idle` | | `src/shared/models.ts:362` |
| `checking` | | `src/shared/models.ts:363` |
| `up-to-date` | `checkedAt` ISO string | `src/shared/models.ts:364` |
| `downloading` | `version`, `percent` 0..100 integer | `src/shared/models.ts:365`, `src/main/updates/update-service.ts:247` |
| `ready` | `version` | `src/shared/models.ts:366` |
| `failed` | `step`: `check`, `download`, `verify`; `message` | `src/shared/models.ts:358,367` |

#### Other small enums

- `PlatformSource`: `real`, `fake`. **[dev-only]** `fake` marks a run against in-process fake launchers (`src/shared/models.ts:331`).
- `VaultStatus.backend`: `dpapi`, `none` (`src/shared/models.ts:303`).
- `WhatsNewTone`: `accent`, `success`, `warning`, `danger`, `steam`, `epic`, `battlenet`, `riot` (`src/shared/whats-new.ts:2`).
- `app:openPath` targets: `logs`, `data`, `backups` (`src/shared/ipc-contract.ts:44`).
- `app:reportError` sources: `render`, `window`, `promise` (`src/shared/ipc-contract.ts:48`).
- `DataDirSource`: `flag` (`--data-dir=`), `env` (**[dev-only]** `ONELAUNCH_DATA_DIR`), `default` (`src/main/platform/paths.ts:11,45-54`).

#### Error codes

Every failure crosses IPC as `{ code, message }`. Codes in the "refusal" set are expected outcomes the app explains to the user and are logged at WARN; every other code is logged at ERROR (`src/main/services/refusals.ts:1-21`, `src/main/ipc/handle.ts:53-57`).

| Code | Meaning | Refusal | Thrown by | Source |
| --- | --- | --- | --- | --- |
| `BAD_REQUEST` | the request payload failed shape or business validation | no | IPC helpers; account validation; a game of another launcher than the account | `src/main/ipc/handle.ts:67-107`, `src/main/services/accounts.ts:24-30`, `src/main/services/launch.ts:47-53` |
| `UNTRUSTED_SENDER` | the call did not come from the app's own renderer frame; message “Request rejected” | no | IPC registrar | `src/main/ipc/handle.ts:46-49` |
| `UNKNOWN_CHANNEL` | the channel is not on the preload allowlist, or is not a startup channel | no | preload | `src/preload/index.ts:12-14,32-37` |
| `INTERNAL` | an error without a code, or a non-Error throw (“Unknown error”); also a rejected startup request | no | IPC registrar; preload; renderer startup | `src/main/ipc/handle.ts:30-31`, `src/preload/index.ts:23-26`, `src/renderer/src/lib/startup.ts:29-32` |
| `OPEN_FAILED` | Windows could not open the requested folder; message is the shell's own text | no | `app:openPath` | `src/main/ipc/index.ts:130-131` |
| `NO_PASSWORD` | “No password is stored for this account” | no | `accounts:copy` with `field: 'password'` | `src/main/ipc/index.ts:319` |
| `NOT_A_LAUNCHER` | the picked file is not the launcher's executable in a complete installation | no | `launchers:locate` | `src/main/ipc/index.ts:377-382` |
| `NOT_FOUND` | account, game, job, snapshot or pending recovery does not exist | yes | accounts, library, launch, recovery services; `backups:restore` | `src/main/services/accounts.ts:16-22`, `src/main/services/library.ts:27-33`, `src/main/services/launch.ts:39-45`, `src/main/services/switch-journal.ts:139-145`, `src/main/ipc/index.ts:470` |
| `DUPLICATE` | another account on the same launcher already has this login or launcher id | yes | accounts service | `src/main/services/accounts.ts:36-49` |
| `BUSY` | “Another switch or launch is still running” | yes | launch service; every `requireIdle` handler | `src/main/services/launch.ts:23-29`, `src/main/ipc/index.ts:101-103` |
| `DRY_RUN` | “Preview mode is on. Turn it off in Settings, or from the preview, to switch accounts for real.” | yes | `launch:start` | `src/main/services/launch.ts:31-37` |
| `CONFIRM_REQUIRED` | the switch would close the running launcher and `confirmClose` was not set; message is the adapter's close sentence; carries `runningGame` | yes | launch service and adapters | `src/main/launchers/types.ts:227-236`, `src/main/services/launch.ts:185-187` |
| `GAME_RUNNING` | a game is running that the switch must not disturb (Riot) | yes | adapters | `src/main/launchers/types.ts:242-251` |
| `GAME_NOT_PLAYABLE` | the launcher says the game cannot start now (update, repair, already running) | yes | adapters (`launchGame`) | `src/main/launchers/types.ts:254-260` |
| `LAUNCHER_RUNNING` | the operation needs the launcher closed (restore) | yes | adapters (`restoreBackup`) | `src/main/launchers/types.ts:263-269` |
| `SWITCH_BLOCKED` | the plan cannot run (no password on the credentials path, nothing to capture, …) | yes | adapters | `src/main/launchers/types.ts:272-278` |
| `NOT_INSTALLED` | “<launcher id> is not installed on this PC” | yes | adapters | `src/main/launchers/types.ts:198-204` |
| `NOT_IMPLEMENTED` | no adapter for the launcher, or the adapter has no session capture | yes | `LauncherService` | `src/main/services/launchers.ts:41-47,339`, `src/main/launchers/types.ts:206-212` |
| `NOT_READY` | “No verified update has been downloaded yet” | yes | `updates:install` | `src/main/updates/update-service.ts:19-25` |
| `UNAVAILABLE` | “Start with Windows is not available for this copy of One Launch” | yes | `autostart:set` | `src/main/ipc/index.ts:555` |
| `PROCESS_ACCESS_DENIED` | Windows refused to close or kill a launcher process (it runs as administrator); message names the image and the two ways out | yes | process controller | `src/main/platform/processes.ts:44-51` |
| `LAUNCHER_FILE` | a launcher file exists but is not in the expected shape; message “<reason>: <path>” | no | adapters | `src/main/launchers/types.ts:215-224` |
| `ADAPTER_FAILED` | an adapter threw something without a code; message “<Launcher display name>: <reason>” | no | `LauncherService.guard` | `src/main/services/launchers.ts:26-39` |
| `VAULT_UNAVAILABLE` | “Secure storage is not available on this system. Secrets cannot be stored.” | no | vault | `src/main/services/vault.ts:4-10` |
| `VAULT_UNREADABLE` | ciphertext cannot be decrypted (key lost or another Windows user); long message pointing at Settings | no | vault | `src/main/services/vault.ts:17-25` |
| `BACKUP` | snapshot not found, wrong launcher, corrupt file, stray path, encrypted without vault | no | backup service | `src/main/services/backup.ts:65-71,226-229,264,275,282-284` |
| `TOO_LARGE` | “That image is larger than <N> MB” (manual artwork) | no | artwork service | `src/main/services/artwork.ts:300` |
| `BAD_IMAGE` | “That file is not a JPEG, PNG, or WebP image” | no | artwork service | `src/main/services/artwork.ts:303` |
| `UNAUTHORIZED`, `RATE_LIMITED` | SteamGridDB rejected the key / rate-limited; internal to a background job, surfaces only as text in `artworkError` | no | artwork service | `src/main/services/artwork.ts:574-576` |
| `ABORTED` | an aborted wait; converted to a cancelled result inside the adapter and never reaches the renderer | no | wait helper | `src/main/launchers/wait.ts:1-8` |
| `REGISTRY`, `CONFIG_FORMAT`, `MANIFEST_FORMAT`, `LOGINUSERS_FORMAT`, `VDF_PARSE` | parser and registry failures inside adapters; reach the renderer only wrapped by `AdapterError`, which keeps the inner code | no | platform and adapter parsers | `src/main/platform/registry.ts:62`, `src/main/launchers/battlenet/config.ts:21`, `src/main/launchers/epic/manifests.ts:10`, `src/main/launchers/steam/loginusers.ts:33`, `src/main/launchers/steam/manifests.ts:10`, `src/main/launchers/steam/vdf.ts:11` |
| `DB_CORRUPT`, `DB_UNAVAILABLE`, `DB_TOO_NEW` | database cannot be opened; startup fails with an error box, never reaches IPC | no | database open and migrate | `src/main/db/index.ts:14-43,115-127` |

### 3.2 Entities

Conventions for the field tables: "Nullable" means the JSON value may be `null`; "Default" is what a new record gets when the field is not supplied. Times are ISO-8601 strings in UTC, produced by `new Date().toISOString()`.

#### Account

The renderer-visible account. Never carries the password or its ciphertext (`src/shared/models.ts:37-52`, `src/main/services/accounts.ts:66-70`).

| Field | Type | Nullable | Default | Invariant | Source |
| --- | --- | --- | --- | --- | --- |
| `id` | string (UUID v4) | no | random UUID at create | primary key; never changes | `src/main/services/accounts.ts:235` |
| `launcher` | `LauncherId` | no | required | rows with an unknown launcher are dropped on read | `src/main/services/accounts.ts:82,224` |
| `displayName` | string | no | required | trimmed; non-empty; at most 120 chars (“Display name is required” / “Display name must be at most 120 characters”) | `src/shared/models.ts:56`, `src/main/services/accounts.ts:104-109,225` |
| `login` | string | no | required | trimmed; non-empty; at most 256 chars; unique per launcher, case-insensitive | `src/shared/models.ts:57`, `src/main/services/accounts.ts:226,254-258` |
| `hasSecret` | boolean | no | `false` | true when a password ciphertext is stored | `src/main/services/accounts.ts:68,89` |
| `hasSession` | boolean | no | `false` | true when a `launcher_sessions` row exists for the account | `src/main/services/accounts.ts:69,90` |
| `launcherUid` | string | yes | `null` | trimmed; empty becomes `null`; at most 64 chars (“Launcher ID must be at most 64 characters”); unique per launcher, case-insensitive | `src/main/services/accounts.ts:111-119,258-261` |
| `tags` | string[] | no | `[]` | each trimmed and cut to 40 chars, empties dropped, de-duplicated case-insensitively (first spelling wins), at most 32 kept; stored as JSON text; unparsable stored JSON reads as `[]` | `src/shared/models.ts:85-99`, `src/main/services/accounts.ts:72-79` |
| `notes` | string | no | `''` | at most 20,000 chars (“Notes must be at most 20000 characters”); not trimmed | `src/main/services/accounts.ts:121-126` |
| `lastUsedAt` | ISO time | yes | `null` | set when a job ends `done` or `cancelled` after the switch succeeded | `src/main/services/accounts.ts:294-296`, `src/main/services/launch.ts:326,387,401` |
| `createdAt` | ISO time | no | now at create | | `src/main/services/accounts.ts:236-237` |
| `updatedAt` | ISO time | no | same as `createdAt` | bumped by update, setPassword, clearPassword, vault reset; not by `touchLastUsed` | `src/main/services/accounts.ts:250,273,279,294`, `src/main/services/vault-health.ts:35` |

- Password (not a field of the DTO): non-empty, at most 1024 chars (“Password must not be empty” / “Password must be at most 1024 characters”); encrypted before the row is touched so a vault failure leaves no half-created row (`src/main/services/accounts.ts:128-134,230-233`).
- List order: by launcher, then display name case-insensitively (`src/main/services/accounts.ts:158`).
- `AccountCreateInput` = `{ launcher, displayName, login, password?, launcherUid?, tags?, notes? }`; `AccountPatch` = `{ displayName?, login?, launcherUid? (null clears), tags?, notes? }`; a patch never touches the password (`src/shared/models.ts:65-83`).
- `ACCOUNT_LIMITS`: displayName 120, login 256, launcherUid 64, password 1024, notes 20,000, tagCount 32, tagLength 40 (`src/shared/models.ts:55-63`).

#### AccountRef (main-process only)

What adapters receive instead of an `Account`: `{ id: string | null, displayName, login, launcherUid: string | null, hasSecret }`. `id` is `null` only for refs built from launcher data (previews, imports). Never carries the password; adapters call `getPassword()` at the moment of use (`src/main/launchers/types.ts:17-25`, `src/main/services/launchers.ts:58-66`).

#### Game

The library row joined with artwork, the last artwork miss and the last successful launch (`src/shared/models.ts:117-134`, `src/main/services/library.ts:78-91`).

| Field | Type | Nullable | Default | Invariant | Source |
| --- | --- | --- | --- | --- | --- |
| `id` | string (UUID v4) | no | random at first sync | stable across scans: a game seen again keeps its id | `src/main/services/library.ts:129,191-196` |
| `launcher` | `LauncherId` | no | | unknown launcher rows dropped on read | `src/main/services/library.ts:40` |
| `launcherGameId` | string | no | | unique together with `launcher` | `src/main/db/migrations/index.ts:150` |
| `name` | string | no | | overwritten by every scan | `src/main/services/library.ts:117` |
| `installPath` | string | yes | | overwritten by every scan | `src/main/services/library.ts:117` |
| `installed` | boolean | no | `true` | set `false` when a scan no longer lists the game; `library:list` returns installed games only; rows are never deleted | `src/main/services/library.ts:108,120,146-150` |
| `lastSeenAt` | ISO time | no | | time of the last scan that listed it | `src/main/services/library.ts:123,129` |
| `needsUpdate` | boolean | no | `false` | from the adapter | `src/main/services/library.ts:129` |
| `lastPlayedAt` | ISO time | yes | `null` | from the launcher's own files | `src/shared/models.ts:127-128` |
| `artwork` | `GameArtwork` | yes | `null` | `null` when there is no artwork row or its source is unknown | `src/main/services/library.ts:52-54` |
| `artworkError` | string | yes | `null` | the last recorded miss; cleared when artwork is committed without an error | `src/main/services/library.ts:55`, `src/main/services/artwork.ts:472-473` |
| `lastLaunch` | `{ accountId, accountName: string \| null, launchedAt }` | yes | `null` | the newest `launch_history` row with `result = 'ok'` for the game; `accountName` is `null` when that account was deleted | `src/main/services/library.ts:56-57,87-91` |

`GameArtwork` = `{ source: ArtworkSource, grid, hero, logo: string | null (renderer URLs), fetchedAt: ISO time | null }`. URLs are `onelaunch-art://art/<gameId>/<kind>.<ext>?v=<fetchedAt>` (`src/shared/models.ts:108-115`, `src/main/services/artwork-url.ts:8-30`).

- Library list order: by name, case-insensitive (`src/main/services/library.ts:108`).
- The library `revision` is an in-memory integer starting at 0, incremented by a scan that changed a row, any artwork commit or attempt, artwork pruning, and a launch-history insert; it is not persisted (`src/main/services/library.ts:104,155-162,197-201`, `src/main/services/artwork.ts:225,422,474`, `src/main/services/launch.ts:454`).
- `scanned` is an in-memory flag: false until the first scan of this run has finished (even a failed one) (`src/main/services/library.ts:105,164-171`, `src/main/services/library-scan.ts:23-25`).

#### InstalledGame (adapter output, also `launchers:games`)

`{ launcher, launcherGameId, name, installPath, libraryPath, sizeOnDiskBytes: number | null, lastPlayedAt: ISO | null, needsUpdate }`. Read straight from launcher files; not a library row (`src/shared/models.ts:381-393`).

#### LibraryScanResult

`{ launcher, added, updated, removed, installed, error: string | null }`. `installed` is false when the launcher was not found (its games are then synced as an empty set); `error` is set when the scan threw anything else, and that launcher's rows are left untouched (`src/shared/models.ts:136-144`, `src/main/services/library-scan.ts:29-45`).

#### LaunchHistoryEntry (row) and RecentLaunch

- Row: `{ id: UUID, gameId, accountId, launchedAt, result: LaunchResult }`; one inserted per job that named a game, whatever the outcome; no foreign keys, never pruned (`src/shared/models.ts:148-154`, `src/main/services/launch.ts:106-108,451-458`).
- `RecentLaunch` = `{ gameId, gameName, launcher, accountId, accountName, launchedAt }`: newest successful launch per (game, account) pair, newest first; pairs whose game is uninstalled or whose account is gone are left out (inner joins) (`src/shared/models.ts:156-164`, `src/main/services/launch.ts:109-139`).

#### ArtworkStatus

`{ queued, running, completed, failed, hasApiKey, revision, scanned }`. `completed`/`failed` count jobs since the app started; `queued` and `running` are the live queue sizes; `revision` and `scanned` mirror the library (`src/shared/models.ts:166-182`, `src/main/services/artwork.ts:231-241`).

#### Artwork row and attempt row (main-process only)

- `artwork`: `{ game_id, source, grid_path, hero_path, logo_path, fetched_at }`; paths are relative to the artwork folder, always `<gameId>/<kind>.<ext>` (`src/main/services/artwork.ts:54-60`, `src/main/services/artwork-url.ts:21-24`).
- `artwork_attempts`: `{ game_id, attempted_at, error }`; `error` is cut to 500 chars; one row per game, replaced on each miss; a miss is not retried automatically for 7 days (`src/main/services/artwork.ts:175,185-189,255,421,473`).
- Queue limits: concurrency 2, request timeout 15 s, download cap 10 MiB, manual image cap 20 MiB, JSON answer cap 1 MiB, at most 3 redirects, only between the artwork hosts (`src/main/services/artwork.ts:84-87,173-177`).

#### LauncherInstall

`{ launcher, installDir, exePath, source }` (`src/shared/models.ts:373-379`).

#### RememberedAccount

`{ launcher, launcherUid, login, personaName, rememberPassword, mostRecent, lastLoginAt: ISO | null, sessionMayBeExpired, linkedAccountId: string | null }`. `launcherUid` is the empty string for launchers that remember logins but not ids (Battle.net); the entry's key is then its login. `linkedAccountId` is the One Launch account matching by launcher uid first, then by login (`src/shared/models.ts:395-417`, `src/main/services/launchers.ts:224-237`).

#### HealthCheck and HealthReport

`HealthCheck` = `{ id, label, status: HealthStatus, detail }`; `HealthReport` = `{ launcher, status, checks[], checkedAt }` (`src/shared/models.ts:422-434`).

#### SwitchChange, SwitchPlanStep, SwitchPlan

- `SwitchChange` = `{ where, before, after }`; `where` is a line number or registry value name, empty when not applicable (`src/shared/models.ts:438-443`).
- `SwitchPlanStep` = `{ id (step id), kind: SwitchStepKind, title, target, detail, changes: SwitchChange[], skipped }` (`src/shared/models.ts:445-459`).
- `SwitchPlan` = `{ launcher, account: { id: string | null, displayName, login, launcherUid }, strategy, current: { mostRecentLogin, autoLoginUser, running }, steps[], warnings[], notes[], executable, generatedAt }`. `warnings` stop the dialog; `notes` are informational; `executable` false means the plan cannot run as-is (`src/shared/models.ts:463-483`).
- The preflight's plan has the “will be closed” warning filtered out; the dry-run plan keeps it (`src/main/launchers/types.ts:280-286`).

#### LauncherStatus

`{ launcher, displayName, implemented, install: LauncherInstall | null, chosenPath: string | null, running: boolean | null, health: HealthReport | null, attention: { message } | null }`. For a launcher without an adapter only `launcher`, `displayName` and `implemented: false` are filled. `running` is `null` when not installed. `attention` is set when the adapter threw during the status pass (`src/shared/models.ts:485-497`, `src/main/services/launchers.ts:185-213`).

#### BackupSnapshot and the manifest

Renderer view: `{ id, launcher, createdAt, reason, files: string[] (relative paths of files that existed), bytes (sum of original sizes), pinned: string | null, path }` (`src/shared/models.ts:499-510`, `src/main/services/backup.ts:309-320`).

On disk: `<backupsDir>\<launcher>\<id>\manifest.json` plus the copied files (`src/main/services/backup.ts:128-131`).

| Manifest field | Type | Invariant | Source |
| --- | --- | --- | --- |
| `version` | `1` | any other value makes the snapshot invisible | `src/main/services/backup.ts:56,293` |
| `launcher` | `LauncherId` | must equal the folder's launcher or loading fails with `BACKUP` | `src/main/services/backup.ts:229` |
| `createdAt` | ISO time | required string | `src/main/services/backup.ts:293` |
| `reason` | string | missing reads as `''` | `src/main/services/backup.ts:298` |
| `files[]` | `{ relative, source (absolute), existed, bytes, sha256: string \| null, encrypted? }` | every entry must have that shape; `sha256` is of the stored copy (ciphertext when encrypted); `encrypted` means the stored file is `<relative>.enc`, vault ciphertext of the base64 original | `src/main/services/backup.ts:42-53,94-104` |
| `meta` | `Record<string, string \| number \| boolean \| null>` | registry values and the like; missing reads as `{}` | `src/main/services/backup.ts:31,300` |
| `pinned` | string (why) | optional; empty string reads as absent | `src/main/services/backup.ts:39,301` |

- Snapshot id: the creation time as ISO with `:` replaced by `-` (`2026-01-02T03-04-05.678Z`), suffixed `-2`, `-3`, … when the folder exists; an id of any other shape is never joined to a path (`src/main/services/backup.ts:86-92,157-162,225-228`).
- `relative` must be non-empty, not absolute, with no `..` or empty segment (`src/main/services/backup.ts:80-84`).
- Retention per launcher: `backups.keep` ordinary snapshots (default 20, floor 1), plus at most 10 pinned ones; the id passed as `protect` is never pruned (`src/main/services/backup.ts:9-11,238-251`).
- A sensitive file whose vault is unavailable is stored in the clear with a WARN log (`src/main/services/backup.ts:172-173`).

#### RunningGame

`{ launcher, launcherGameId, name, source: 'launcher' | 'process' }` (`src/shared/models.ts:517-523`).

#### LaunchPreflight

`{ plan, dryRun, launcherRunning, runningGame, alreadyActive, needsConfirmation, closeMessage: string | null, blocked: string | null }`. `needsConfirmation` = launcher running and not already active; `closeMessage` is set only then; `blocked` is the plan's first warning (or “This switch cannot run.”) when the plan is not executable (`src/shared/models.ts:525-540`, `src/main/services/launch.ts:146-164`).

#### SwitchPreflight (main-process only)

`{ plan, launcherRunning, runningGame, alreadyActive, closeMessage }`; the adapter's answer the launch service builds `LaunchPreflight` from (`src/main/launchers/types.ts:103-115`).

#### JobStep and LaunchAttention

- `JobStep` = `{ id (step id or `launch`), title, status: JobStepStatus, detail }` (`src/shared/models.ts:544-550`).
- `LaunchAttention` = `{ kind: 'sign-in', title, message, steps?: string[], reason: string | null, canCopyPassword, until: ISO time }`; a checkbox the launcher must keep ticked is named in “ ” inside `steps` (`src/shared/models.ts:563-576`).

#### LaunchJob

| Field | Type | Nullable | Initial value | Invariant | Source |
| --- | --- | --- | --- | --- | --- |
| `id` | UUID | no | random | | `src/main/services/launch.ts:197` |
| `state` | `LaunchJobState` | no | `running` | leaves `running` exactly once | `src/main/services/launch.ts:198,328,369,385,403,409` |
| `launcher` | `LauncherId` | no | the account's | | `src/main/services/launch.ts:199` |
| `account` | `{ id, displayName, login }` | no | | copied at start; not updated if the account changes | `src/main/services/launch.ts:200` |
| `game` | `{ id, name, launcherGameId }` | yes | `null` for a plain switch | must be the account's launcher, else `BAD_REQUEST` “<game> is a <launcher> game; pick a <launcher> account” | `src/main/services/launch.ts:201,225-231` |
| `steps` | `JobStep[]` | no | `[launch]` with a game, `[]` without; a direct job starts with the plan's steps as `skipped` | replaced wholesale on every adapter progress report, launch step re-appended | `src/main/services/launch.ts:194-202,311-314` |
| `attention` | `LaunchAttention` | yes | `null` | set while the adapter waits for the user; cleared when the switch settles or the job fails | `src/main/services/launch.ts:348-350,363,405` |
| `direct` | boolean | no | | true only when a game is named, `confirmClose` is false, and the consumed preflight says already active and running | `src/main/services/launch.ts:193` |
| `switched` | boolean | no | `false` | true once the adapter reports `ok`, or at once for a direct job | `src/main/services/launch.ts:319,373` |
| `handOff` | string | yes | `null` | set from `launchGame` when the launcher only opened the game's page | `src/main/services/launch.ts:325,399` |
| `backupId` | string | yes | `null` | set as soon as the adapter reports the snapshot; final value from the result | `src/main/services/launch.ts:344-347,364` |
| `restored` | boolean | yes | `null` | from the result after a failure; `null` when nothing was modified | `src/main/services/launch.ts:365` |
| `error` | string | yes | `null` | the result's error, or “The switch failed”, or “Stopped before launching <game>.”, or the thrown error's text | `src/main/services/launch.ts:367,384,406` |
| `startedAt` | ISO time | no | now | | `src/main/services/launch.ts:210` |
| `finishedAt` | ISO time | yes | `null` | set in every end path | `src/main/services/launch.ts:413` |

In-memory limits: one running job at a time; the last 20 finished jobs are kept for `launch:status`; nothing about jobs is persisted except the history row (`src/main/services/launch.ts:65,88,416-418,435-439`). A reused preflight is accepted for 10 s; a direct job re-takes one older than 2 s (`src/main/services/launch.ts:67-70`).

#### RestoreResult

`{ launcher, id, files: string[] (absolute paths written back), registry: string[] (“name = value” or “name (removed)”) }` (`src/shared/models.ts:602-609`).

#### SwitchRecovery and the journal entry

Renderer view `SwitchRecovery` = `{ jobId, launcher, accountName: string | null (null when the account was deleted), startedAt, backupId, snapshotAvailable, phase }` (`src/shared/models.ts:619-630`, `src/main/services/switch-journal.ts:184-196`).

On disk, `<dataDir>\switch.json` (`src/main/platform/paths.ts:64`):

| Field | Type | Invariant | Source |
| --- | --- | --- | --- |
| `version` | `1` | anything else is unreadable | `src/main/services/switch-journal.ts:13,36` |
| `jobId` | string | `^[0-9a-f-]{1,64}$` case-insensitive | `src/main/services/switch-journal.ts:27,38` |
| `launcher` | `LauncherId` | | `src/main/services/switch-journal.ts:39` |
| `accountId` | string | same pattern as `jobId`; the account is named by id only | `src/main/services/switch-journal.ts:40` |
| `startedAt` | ISO time | must parse as a date | `src/main/services/switch-journal.ts:41` |
| `updatedAt` | ISO time | missing reads as `startedAt` | `src/main/services/switch-journal.ts:50` |
| `backupId` | string or `null` | `^[0-9A-Za-z.-]{1,64}$`; `null` until the snapshot exists | `src/main/services/switch-journal.ts:28,42` |
| `stepId` | string or `null` | `^[a-z-]{1,32}$`; the step running when last written | `src/main/services/switch-journal.ts:29,43` |
| `rollbackFailed` | boolean | anything but `true` reads as `false` | `src/main/services/switch-journal.ts:53` |

- Written with temp file, fsync, rename; every write failure is logged and swallowed (`src/main/services/switch-journal.ts:120-136`).
- An unreadable file is renamed `switch.json.unreadable` (`src/main/services/switch-journal.ts:102-118`).
- Phase derivation: `rollback-failed` when `rollbackFailed`; else `signing-in` when `stepId` is one of `start`, `wait`, `verify`, `login`, `manual`, `recapture`; else `writing` (`src/main/services/switch-journal.ts:31,194`).

#### LauncherSession and LauncherSessionState

- `LauncherSession` (renderer view) = `{ accountId, launcher, launcherUid: string | null, fingerprint (SHA-256 hex), capturedAt, verifiedAt: ISO | null, staleReason: string | null }`. Never the blob (`src/shared/models.ts:638-651`, `src/main/services/sessions.ts:15-29`).
- Save semantics: one row per account (replace); a replaced snapshot starts unverified and not stale; a `null` `launcherUid` on save keeps the previous one (`src/main/services/sessions.ts:66-73,96-103`).
- `LauncherSessionState` = `{ launcher, remembered, fingerprint: string | null, launcherUid: string | null, linkedAccountId: string | null, launcherRunning, lastSignIn: { ok, reason: string | null } | null }` (`src/shared/models.ts:653-667`).

#### Session blobs (vault-encrypted JSON, main-process only)

| Launcher | Blob shape | Fingerprint | Source |
| --- | --- | --- | --- |
| Epic | `{ v: 1, rememberMe: { enable: string, data: string }, offline: { data: string } \| null }`; from `[RememberMe]` and `[Offline]` of `GameUserSettings.ini`; parse requires `v === 1` and a non-empty `rememberMe.data`, `enable` missing reads as `True` | SHA-256 hex of `rememberMe.data` | `src/main/launchers/epic/session.ts:15-20,32-34,56-77` |
| Battle.net | `{ v: 1, unifiedAuth: { name: string, hex: string (lower-case, even length) } }`; the REG_BINARY value under `HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth`; parse also accepts a retired `v: 2` and normalises it to v1 | SHA-256 hex of the lower-cased hex | `src/main/launchers/battlenet/session.ts:12-29,36,43-56` |
| Riot | `{ v: 1, file: string }`: the whole `RiotGamesPrivateSettings.yaml` text | SHA-256 hex of the `refresh_token` under `psl › authorization › riot-client` | `src/main/launchers/riot/session.ts:13,40-48,203-213,239` |

- Riot's read of the file is fail-closed: it yields `session` (with `accountId` = lower-cased `sub` of the id token, which must be a UUID; `riotId` from the `acct` claim, each part at most 64 chars; `dpopBound`; `writeCount` up to 9 digits), `signed-out` (`riot-client: null`), or `unexpected` with a reason; nothing is guessed from an unexpected shape (`src/main/launchers/riot/session.ts:20-38,216-244`).
- Steam has no session blob (native account list) (`src/main/launchers/types.ts:186-195`).

#### SwitchResult and SwitchOptions (main-process only)

- `SwitchResult` = `{ launcher, account, strategy, ok, needsSignIn, cancelled, backupId: string | null, restored: boolean | null, error: string | null, steps: JobStep[], startedAt, finishedAt, readyAt: ISO | null, startedWithGame: string | null }` (`src/main/launchers/types.ts:36-67`).
- `SwitchOptions` = `{ dryRun, confirmClose?, onProgress?, onBackup?, onAttention?, signal?, getPassword?, game?: { launcherGameId, name } }` (`src/main/launchers/types.ts:72-101`).
- `KnownLauncherState` = `{ install, running: boolean | null }` handed from a status pass to the health check so nothing is read twice (`src/main/launchers/types.ts:117-121`).

#### LauncherAdapter contract (main-process only)

Required: `id`, `displayName`, `detectInstall`, `chosenInstallDir`, `installFromExe`, `listInstalledGames`, `listRememberedAccounts`, `isRunning`, `runningGame`, `shutdown`, `preflight`, `switchAccount` (dry run returns a plan, real returns a result), `launchGame` (returns `{ command, handOff? }`), `restoreBackup`, `healthCheck`. Optional: `gameIcon` (Riot only), `sessionState` and `captureSession` (Epic, Battle.net, Riot) (`src/main/launchers/types.ts:126-196`; implementations at `src/main/launchers/epic/adapter.ts:453,467`, `src/main/launchers/battlenet/adapter.ts:560,575`, `src/main/launchers/riot/adapter.ts:534,602,622`).

#### VaultStatus, VaultHealthStatus, VaultResetSummary

- `VaultStatus` = `{ available, backend }`; `VaultHealthStatus` adds `readable: boolean | null` (`null` until a probe has ever been stored, or when the vault is unavailable) (`src/shared/models.ts:300-309`, `src/main/services/vault-health.ts:47-57`).
- The probe is the constant `one-launch-vault-probe-v1` stored under `vault.probe` once and never overwritten; `readable` is false when it no longer decrypts to that constant (`src/main/services/vault-health.ts:8,40-45,52-55`).
- `VaultResetSummary` = `{ passwords, sessions, secrets }`: counts of account passwords cleared (rows keep their other fields, `updatedAt` bumped), session rows deleted, named secret rows deleted (the probe included, then re-stored) (`src/shared/models.ts:311-316`, `src/main/services/vault-health.ts:59-67`).

#### StartupMetrics and AppInfo

- `StartupMetrics` = `{ processToNodeMs, windowShownMs, readyToShowMs, didFinishLoadMs }`, each `number | null` (`src/shared/models.ts:318-328`).
- `AppInfo` = `{ version, electron, node, chrome, isPackaged, dataDir, logDir, dbPath, startup, platformSource }` (`src/shared/models.ts:333-344`, `src/main/ipc/index.ts:113-124`).

#### AutostartStatus

`{ available, openAtLogin }`. `available` is true only for a packaged per-user install running on the default data folder; `openAtLogin` reflects what Windows will do (false once disabled in Task Manager) (`src/shared/models.ts:351-354`, `src/main/platform/login-item.ts:14-17,30-35`, `src/main/updates/availability.ts:30-39`).

#### WhatsNewRelease

`{ version, date (YYYY-MM-DD), intro?, image?: { file, alt? }, sections: { heading, tone?, items: { lead, detail? }[] }[] }`. A version string is `^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$` and at most 64 chars; ordering is semver with prereleases before their release (`src/shared/whats-new.ts:5-38,57-78`). The release shown is the newest at or below the running version that is newer than `whatsNew.lastSeen`; a missing or invalid `lastSeen` counts as older than everything (`src/shared/whats-new.ts:80-99`).

#### IpcEnvelope and CopyResult

- `IpcEnvelope<T>` = `{ ok: true, data: T } | { ok: false, error: { code, message } }` (`src/shared/ipc-contract.ts:232-238`).
- `CopyResult` = `{ clearAfterMs: 30000, historyExcluded }` (`src/main/services/clipboard.ts:3,14-18`).

### 3.3 SQLite schema (as it stands at `user_version = 5`)

File `<dataDir>\onelaunch.db`, opened with `busy_timeout` 5000 ms, `journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON`, and a `quick_check` on open (`src/main/platform/paths.ts:60`, `src/main/db/index.ts:45,73-78`). Migrations are forward-only, contiguous from 1, each in its own transaction; a database with a higher version is refused (`src/main/db/index.ts:129-143,174-185`). A copy of the file is taken 250 ms after the first paint into `<backupsDir>\db\onelaunch-<timestamp>.db`, newest 5 kept (`src/main/index.ts:118,704-708`, `src/main/db/index.ts:98-109`).

#### `accounts` (rebuilt at v4; the v1 `CHECK (launcher IN (...))` is gone)

| Column | Type | Null | Default | Notes | Source |
| --- | --- | --- | --- | --- | --- |
| `id` | TEXT | no | | PRIMARY KEY | `src/main/db/migrations/index.ts:122` |
| `launcher` | TEXT | no | | no CHECK; filtered on read | `:123` |
| `display_name` | TEXT | no | | | `:124` |
| `login` | TEXT | no | | uniqueness per launcher enforced in code, not by index | `:125`, `src/main/services/accounts.ts:254-262` |
| `secret_enc` | BLOB | yes | NULL | DPAPI ciphertext of the password; never selected by a public read | `:126`, `src/main/services/accounts.ts:66-70` |
| `launcher_uid` | TEXT | yes | NULL | | `:127` |
| `tags` | TEXT | no | `'[]'` | JSON array | `:128` |
| `notes` | TEXT | no | `''` | | `:129` |
| `last_used_at` | TEXT | yes | NULL | | `:130` |
| `created_at` | TEXT | no | | | `:131` |
| `updated_at` | TEXT | no | | | `:132` |

Index: `idx_accounts_launcher (launcher)` (`src/main/db/migrations/index.ts:138`).

#### `games` (rebuilt at v4)

| Column | Type | Null | Default | Notes | Source |
| --- | --- | --- | --- | --- | --- |
| `id` | TEXT | no | | PRIMARY KEY | `:141` |
| `launcher` | TEXT | no | | no CHECK | `:142` |
| `launcher_game_id` | TEXT | no | | | `:143` |
| `name` | TEXT | no | | | `:144` |
| `install_path` | TEXT | yes | NULL | | `:145` |
| `installed` | INTEGER | no | 1 | 0 = no longer found; rows never deleted | `:146` |
| `last_seen_at` | TEXT | no | | | `:147` |
| `needs_update` | INTEGER | no | 0 | added at v2 | `:148`, `:84` |
| `last_played_at` | TEXT | yes | NULL | added at v2 | `:149`, `:85` |

Constraints: `UNIQUE (launcher, launcher_game_id)`; index `idx_games_launcher (launcher)` (`src/main/db/migrations/index.ts:150,156`).

#### `artwork` (rebuilt at v5)

| Column | Type | Null | Default | Notes | Source |
| --- | --- | --- | --- | --- | --- |
| `game_id` | TEXT | no | | PRIMARY KEY, `REFERENCES games(id) ON DELETE CASCADE` | `:182` |
| `source` | TEXT | no | | `CHECK (source IN ('steamgriddb', 'steamcdn', 'manual', 'launcher'))`: the one CHECK that survives | `:183` |
| `grid_path` | TEXT | yes | NULL | | `:184` |
| `hero_path` | TEXT | yes | NULL | | `:185` |
| `logo_path` | TEXT | yes | NULL | | `:186` |
| `fetched_at` | TEXT | yes | NULL | | `:187` |

No index beyond the primary key.

#### `artwork_attempts` (v2)

| Column | Type | Null | Notes | Source |
| --- | --- | --- | --- | --- |
| `game_id` | TEXT | no | PRIMARY KEY, `REFERENCES games(id) ON DELETE CASCADE` | `:89` |
| `attempted_at` | TEXT | no | | `:90` |
| `error` | TEXT | no | | `:91` |

#### `launch_history` (v1, unchanged)

| Column | Type | Null | Notes | Source |
| --- | --- | --- | --- | --- |
| `id` | TEXT | no | PRIMARY KEY | `:58` |
| `game_id` | TEXT | no | no foreign key | `:59` |
| `account_id` | TEXT | no | no foreign key | `:60` |
| `launched_at` | TEXT | no | | `:61` |
| `result` | TEXT | no | `CHECK (result IN ('ok', 'failed', 'cancelled'))` | `:62` |

Index: `idx_launch_history_launched_at (launched_at DESC)` (`:64`).

#### `settings` (v1)

| Column | Type | Null | Notes | Source |
| --- | --- | --- | --- | --- |
| `key` | TEXT | no | PRIMARY KEY | `:67` |
| `value` | TEXT | no | always a string; typed accessors parse | `:68` |

#### `secrets` (v1)

| Column | Type | Null | Notes | Source |
| --- | --- | --- | --- | --- |
| `key` | TEXT | no | PRIMARY KEY | `:74` |
| `value_enc` | BLOB | no | DPAPI ciphertext, never plaintext | `:75` |
| `updated_at` | TEXT | no | | `:76` |

#### `launcher_sessions` (v3, rebuilt at v4)

| Column | Type | Null | Default | Notes | Source |
| --- | --- | --- | --- | --- | --- |
| `account_id` | TEXT | no | | PRIMARY KEY, `REFERENCES accounts(id) ON DELETE CASCADE` | `:159` |
| `launcher` | TEXT | no | | no CHECK | `:160` |
| `launcher_uid` | TEXT | yes | NULL | | `:161` |
| `blob_enc` | BLOB | no | | vault ciphertext of the serialized blob; never selected by a public read | `:162`, `src/main/services/sessions.ts:15-16` |
| `fingerprint` | TEXT | no | | SHA-256 hex | `:163` |
| `captured_at` | TEXT | no | | | `:164` |
| `verified_at` | TEXT | yes | NULL | | `:165` |
| `stale_reason` | TEXT | yes | NULL | | `:166` |

Index: `idx_launcher_sessions_fingerprint (launcher, fingerprint)` (`:172`).

#### Migration history

| Version | Name | What it did | Source |
| --- | --- | --- | --- |
| 1 | `initial` | all tables above except `artwork_attempts` and `launcher_sessions`; `launcher` columns had `CHECK (launcher IN ('steam', 'epic', 'battlenet'))`; `artwork.source` CHECK without `launcher` | `src/main/db/migrations/index.ts:17-79` |
| 2 | `library_artwork` | `games.needs_update`, `games.last_played_at`, table `artwork_attempts` | `:80-94` |
| 3 | `launcher_sessions` | table `launcher_sessions` (with the launcher CHECK) | `:95-113` |
| 4 | `launcher_check_dropped` | rebuilt `accounts`, `games`, `launcher_sessions` without the launcher CHECK; runs with foreign keys off and a `foreign_key_check` before commit | `:114-174`, `src/main/db/index.ts:158-172` |
| 5 | `artwork_launcher_source` | rebuilt `artwork` with `launcher` added to the source CHECK | `:175-194` |

### 3.4 Files outside the database

| Path (under the data folder) | Content | Source |
| --- | --- | --- |
| `onelaunch.db` (+ WAL files) | the database | `src/main/platform/paths.ts:60` |
| `artwork\<gameId>\<kind>.<ext>` | cached images; one file per kind, stale variants of the same kind deleted on write | `src/main/platform/paths.ts:61`, `src/main/services/artwork.ts:608-624` |
| `backups\<launcher>\<snapshotId>\manifest.json` and files | launcher-file snapshots | `src/main/services/backup.ts:128-131` |
| `backups\db\onelaunch-<timestamp>.db` | database copies, newest 5 | `src/main/index.ts:118,704-708` |
| `logs\onelaunch.log` (rotated, 1 MiB, 5 kept) | the log | `src/main/platform/paths.ts:63`, `src/main/services/logger.ts:235-242` |
| `switch.json` | the switch journal, present only during a switch or after a run that died in one | `src/main/platform/paths.ts:64` |
| `chromium\` | Chromium profile, holding the DPAPI-wrapped vault key (`Local State`) | `src/main/platform/paths.ts:52`, `src/main/services/vault.ts:12-16` |

### 3.5 Cross-entity invariants

- **Uniqueness.** Within one launcher, `login` is unique case-insensitively and `launcher_uid` is unique case-insensitively; both are enforced in the accounts service on create and update (ignoring the row being edited), not by a database index. The error is `DUPLICATE` with the message “"<existing display name>" already uses this login on <Launcher>. Edit that account instead, or change its login first.” or “"<existing display name>" is already linked to this <Launcher> account id. Edit that account instead, or clear its id first.” (`src/main/services/accounts.ts:36-49,254-262`). The adapters' `setLauncherUid` goes through the same update and so fails closed (`src/main/services/launchers.ts:68-86`).
- **Games are unique per `(launcher, launcher_game_id)`**, keep their id across scans, and are never deleted; a game that disappears is marked `installed = 0` (`src/main/services/library.ts:113-120,146-150,191-196`).
- **Cascades.** Deleting an account deletes its `launcher_sessions` row (FK cascade). Nothing cascades to `launch_history`: rows stay and their account name reads as `null` in `Game.lastLaunch` and the pair drops out of `RecentLaunch` (`src/main/db/migrations/index.ts:159`, `src/main/services/library.ts:87-91`, `src/main/services/launch.ts:113-120`). Artwork rows cascade from games by FK, but games are never deleted, so in practice artwork rows and files are removed by the artwork prune, which drops any game that is not installed or does not exist and is not busy in the queue (`src/main/services/artwork.ts:194-229`).
- **What is never deleted.** `games` rows; `launch_history` rows (no pruning); the vault probe except through a reset that immediately re-stores it (`src/main/services/vault-health.ts:59-67`).
- **What never crosses IPC.** `accounts.secret_enc`, `launcher_sessions.blob_enc`, `secrets.value_enc` (public column lists exclude them: `src/main/services/accounts.ts:66-70`, `src/main/services/sessions.ts:15-16`); any decrypted password except through the clipboard in main (`src/main/ipc/index.ts:310-324`); any secret except `diagnostics.roundtrip` (`src/main/ipc/index.ts:170-174`); any filesystem path chosen by the renderer (image and executable pickers are main's dialogs: `src/main/ipc/index.ts:349-357,370-389`); the Riot refresh token or any session material (fingerprints only: `src/shared/models.ts:632-636`).
- **Secrets at rest.** `encrypt` throws `VAULT_UNAVAILABLE` rather than ever storing plaintext; passwords are encrypted before the row is written (`src/main/services/vault.ts:54-57`, `src/main/services/accounts.ts:232-233`).
- **The journal precedes the first write.** `backupId` reaches the journal before any launcher file or registry value changes; a journal with `backupId: null` at startup is discarded as "nothing was written" (`src/main/services/launch.ts:343-347`, `src/main/services/switch-journal.ts:164-172`).
- **One job at a time.** `BUSY` while a job is running or starting (`src/main/services/launch.ts:171,245-247`).
- **Revision, not push.** Every change to games, artwork or launch history bumps the in-memory library revision, which the renderer polls through `artwork:status` (`src/main/services/library.ts:93-96`).

## 4. Persistence

Everything the app itself owns on disk. Launcher-owned files and registry keys (what a switch writes) belong to the adapter sections; this section covers the data folder, the files outside it, the renderer's browser storage, and the one registry value the app writes for itself.

### 4.1 The data folder and how it is chosen

The data folder is Electron's `userData` path, redirected before `ready` (`src/main/platform/paths.ts:45-54`):

| Source | Rule | Citation |
| --- | --- | --- |
| `--data-dir=<folder>` on the command line | Wins over everything, in every build (packaged or not). Only the `=` form is read. The value is trimmed; it must be an absolute Windows path, is normalised, and trailing backslashes are dropped. | `src/main/platform/paths.ts:29-38` |
| `ONELAUNCH_DATA_DIR` environment variable | Honoured only when the app is not packaged; the packaged exe ignores it. Trimmed; empty means unset. | `src/main/platform/paths.ts:49-50` |
| Default | `%APPDATA%\OneLaunch` (Electron's `appData` plus the folder name `OneLaunch`). | `src/main/platform/paths.ts:6,50` |

The chosen source is remembered as `flag`, `env` or `default` (`src/main/platform/paths.ts:11,53`) and reported in the first log line (`src/main/index.ts:423-430`); `default` is one of the conditions for updates and Start with Windows (section 7 and 8).

Failure modes of the flag, each a thrown `DataDirFlagError` (`src/main/platform/paths.ts:13`):

- Given more than once with different values: “`--data-dir=` was given more than once with different folders” (`src/main/platform/paths.ts:32`).
- Not absolute: “`--data-dir=` needs an absolute folder path, got "<value>"” (`src/main/platform/paths.ts:34`).
- A drive root such as `C:\`: “`--data-dir=` must name a folder, not the drive "<folder>"” (`src/main/platform/paths.ts:36`).

Any of these (and a bad `ONELAUNCH_FAKE_LAUNCHERS` or `ONELAUNCH_UPDATE_FEED`/`ONELAUNCH_UPDATE_KEYS` value, both **[dev-only]**) writes one line `One Launch: <message>` to stderr and exits with code 2 before the log, the database or a window exists (`src/main/index.ts:79-87`).

Chromium's own profile (`sessionData`: caches, GPU state, `Local State`) is placed in the `chromium` subfolder of the data folder so the top level holds only the app's files (`src/main/platform/paths.ts:52`). The single-instance lock is Electron's and follows `userData`, so two copies on different data folders run side by side while a second copy on the same folder exits at once (`src/main/index.ts:92-94`).

At startup, before `ready`, the folders `dataDir`, `artwork`, `backups` and `logs` are created recursively (`src/main/platform/paths.ts:68-72`, `src/main/index.ts:102-103`).

### 4.2 Layout

| Path (under the data folder) | What | Section |
| --- | --- | --- |
| `onelaunch.db` (plus SQLite's `onelaunch.db-wal` and `onelaunch.db-shm` while open, from WAL mode) | The database | 4.3 |
| `switch.json` | The switch journal; present only during a switch or after a run that ended in one. `switch.json.<pid>.tmp` briefly while writing; `switch.json.unreadable` after a corrupt one is moved aside | 4.6 |
| `artwork\<game uuid>\<kind>.<ext>` | Cached images; `<kind>.<ext>.<pid>.tmp` while writing | 4.9 |
| `backups\<launcher>\<snapshot id>\manifest.json` + copied files | Launcher-file snapshots | 4.7 |
| `backups\db\onelaunch-<timestamp>.db` | Online copies of the database, newest 5 | 4.3.6 |
| `logs\onelaunch.log`, `logs\onelaunch.1.log` … `onelaunch.5.log` | Log with rotation | 4.8 |
| `chromium\` | Chromium profile; `chromium\Local State` holds the DPAPI-wrapped key every secret depends on | 4.5 |
| `dev-app-update.yml`, `update-cache\` | **[dev-only]** test-feed updater config and cache | 4.10 |
| `fake-platform.json` (and `.tmp`) | **[dev-only]** what the fake platform was asked to do | 4.12 |

Paths come from one place (`src/main/platform/paths.ts:56-66`).

### 4.3 The database

SQLite through better-sqlite3, one synchronous connection owned by the main process, opened in `bootstrap` right after the first log line and closed at `will-quit` (`src/main/index.ts:432-433,756-765`).

#### 4.3.1 Open sequence

1. Open the file (created if missing). An open error is classified (below) (`src/main/db/index.ts:66-71`).
2. Pragmas, in order: `busy_timeout` = 5000 ms (configurable by the caller, floored at 0; the app uses the default), `journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON` (`src/main/db/index.ts:45,73-76`).
3. `PRAGMA quick_check`; anything but `ok` is `DB_CORRUPT` with the check's text as detail (`src/main/db/index.ts:77-78`).
4. A probe write: `BEGIN IMMEDIATE`, set `user_version` to its current value, `ROLLBACK`. This proves the file is writable and not locked by another connection; a read-only WAL database only fails on a real write (`src/main/db/index.ts:79-89`).
5. On any error in steps 2 to 4 the connection is closed; a `DB_CORRUPT` passes through, anything else is classified (`src/main/db/index.ts:90-94`).
6. Migrations run (4.3.3) and, when any applied, one INFO line `database migrated` with `from`, `to` and the list applied (`src/main/index.ts:433-437`).

A failure at any of these throws out of `bootstrap`, which shows the native error box “One Launch could not start” with the error message, “Details are in the log:” followed by the log path, and “Data folder:” followed by the data folder, then exits with code 1 (`src/main/index.ts:715-726`).

#### 4.3.2 Error classification

| Code | When | Message (verbatim, `<detail>` is SQLite's text) |
| --- | --- | --- |
| `DB_CORRUPT` | SQLite error code starting `SQLITE_CORRUPT`, or `SQLITE_NOTADB`, or `quick_check` not `ok` | “The One Launch database is damaged (<detail>). Restore a copy from the backups\db folder in the data directory, or move the file aside to start over.” (`src/main/db/index.ts:15-23,61`) |
| `DB_UNAVAILABLE`, reason `locked` | Code starting `SQLITE_BUSY` or `SQLITE_LOCKED` | “The One Launch database is in use by another program (<detail>). Close anything that has it open, such as a second One Launch, a database viewer, or a backup tool, and try again.” (`src/main/db/index.ts:36,59`) |
| `DB_UNAVAILABLE`, reason `read-only` | Code starting `SQLITE_READONLY` | “The One Launch database is read-only (<detail>). Check the permissions on the data folder and the file, then try again.” (`src/main/db/index.ts:38,60`) |
| `DB_UNAVAILABLE`, reason `cannot-open` | Any other error | “The One Launch database could not be opened (<detail>). Check that the data folder exists and is writable.” (`src/main/db/index.ts:39,62`) |
| `DB_TOO_NEW` | `user_version` greater than the newest migration this build knows | “This database was created by a newer version of One Launch (schema <found>; this version supports up to <supported>). Update One Launch, or restore an older copy of the database.” (`src/main/db/index.ts:116-127,134`) |

#### 4.3.3 Versioning and migrations

- The schema version is `PRAGMA user_version`; a new file reads 0 (`src/main/db/index.ts:111-113`).
- Migrations are a contiguous list from version 1; the list is validated (contiguous, integer version, non-empty name and SQL) before anything runs (`src/main/db/index.ts:174-185`).
- Every migration newer than the current version runs in its own transaction, which also sets `user_version` to the migration's version before committing (`src/main/db/index.ts:136-151`).
- A migration flagged `foreignKeysOff` rebuilds a table other rows reference. For it, `foreign_keys` is switched OFF around the transaction (the pragma is a no-op inside one), the migration is refused if the pragma still reads on (“Migration <v>_<name> rebuilds tables, but foreign keys could not be switched off (is migrate() inside a transaction?)”), `PRAGMA foreign_key_check` runs before the commit and any orphan rolls the migration back (“Migration <v>_<name> left <n> row(s) pointing at a missing parent”), and the pragma is restored to ON afterwards (`src/main/db/index.ts:158-172`).
- The result reports `from`, `to` and the applied labels `<version>_<name>` (`src/main/db/index.ts:8-12,140-142`).
- A fresh database (`from === 0`) has one side effect: the running version is written to the setting `whatsNew.lastSeen` so a fresh install never shows What's New; an existing database is left as it is (`src/main/services/whats-new.ts:8-12`, `src/main/index.ts:440`).

| Version | Name | What changed | Citation |
| --- | --- | --- | --- |
| 1 | `initial` | Creates `accounts` (launcher CHECK `steam/epic/battlenet`), `games` (same CHECK, UNIQUE `(launcher, launcher_game_id)`), `artwork` (source CHECK `steamgriddb/steamcdn/manual`, cascades from `games`), `launch_history`, `settings`, `secrets`; indexes `idx_accounts_launcher`, `idx_games_launcher`, `idx_launch_history_launched_at` (`launched_at DESC`) | `src/main/db/migrations/index.ts:17-79` |
| 2 | `library_artwork` | Adds `games.needs_update INTEGER NOT NULL DEFAULT 0` and `games.last_played_at TEXT`; creates `artwork_attempts` (cascades from `games`) | `src/main/db/migrations/index.ts:80-94` |
| 3 | `launcher_sessions` | Creates `launcher_sessions` (launcher CHECK `steam/epic/battlenet`, cascades from `accounts`) and index `idx_launcher_sessions_fingerprint (launcher, fingerprint)` | `src/main/db/migrations/index.ts:95-113` |
| 4 | `launcher_check_dropped` (`foreignKeysOff`) | Rebuilds `accounts`, `games` and `launcher_sessions` without the launcher CHECK so a launcher (Riot) can be added without a migration; copies every row; recreates the three indexes. Reads filter unknown launcher ids instead | `src/main/db/migrations/index.ts:114-174` |
| 5 | `artwork_launcher_source` | Rebuilds `artwork` with source CHECK `steamgriddb/steamcdn/manual/launcher`; copies every row. Nothing references `artwork`, so foreign keys stay on | `src/main/db/migrations/index.ts:175-194` |

#### 4.3.4 Schema at version 5

All timestamps are ISO-8601 strings written by the app (`new Date().toISOString()`). Ids are UUIDv4 strings.

**`accounts`** (`src/main/db/migrations/index.ts:121-138`)

| Column | Type | Meaning | Written by |
| --- | --- | --- | --- |
| `id` | TEXT PK | One Launch's account id (UUID) | `AccountsService.create` (`src/main/services/accounts.ts:235-237`) |
| `launcher` | TEXT NOT NULL | Launcher id; unknown ids are skipped on read (`src/main/services/accounts.ts:82`) | create |
| `display_name` | TEXT NOT NULL | Name shown in the UI; trimmed, 1..`ACCOUNT_LIMITS.displayName` | create, update (`src/main/services/accounts.ts:241-251`) |
| `login` | TEXT NOT NULL | Launcher login; unique per launcher case-insensitively, enforced by the service, not an index (`src/main/services/accounts.ts:254-262`) | create, update |
| `secret_enc` | BLOB | DPAPI ciphertext of the password, or NULL. Never selected by any public read (`src/main/services/accounts.ts:66-70`) | create, `setPassword`, `clearPassword` (`src/main/services/accounts.ts:270-281`), `VaultHealth.reset` sets every one NULL (`src/main/services/vault-health.ts:35,61`) |
| `launcher_uid` | TEXT | The launcher's own account id (SteamID64, Epic/Battle.net/Riot account id) or NULL; unique per launcher case-insensitively | create, update, `accountDirectory.setLauncherUid` from adapters (`src/main/services/launchers.ts:82-84`), import (`src/main/services/launchers.ts:259-260`) |
| `tags` | TEXT NOT NULL DEFAULT `'[]'` | JSON array of strings; unparsable reads as `[]` (`src/main/services/accounts.ts:72-79`) | create, update |
| `notes` | TEXT NOT NULL DEFAULT `''` | Free text up to `ACCOUNT_LIMITS.notes` | create, update |
| `last_used_at` | TEXT | Last time a job for the account finished (ok, or cancelled after the switch succeeded) | `touchLastUsed` (`src/main/services/accounts.ts:294-296`) from `LaunchService` (`src/main/services/launch.ts:326,387,401`) |
| `created_at`, `updated_at` | TEXT NOT NULL | Row times; `updated_at` moves on update, password set/clear and vault reset | create, update, setSecret |

Read rows also carry two computed flags: `has_secret` (`secret_enc IS NOT NULL`) and `has_session` (a `launcher_sessions` row exists) (`src/main/services/accounts.ts:67-70`). Deleting an account cascades to its `launcher_sessions` row (FK) but not to `launch_history` (no FK); the recent-launch query joins accounts, so pairs whose account is gone drop out (`src/main/services/launch.ts:113-120`).

**`games`** (`src/main/db/migrations/index.ts:140-156`)

| Column | Meaning | Written by |
| --- | --- | --- |
| `id` TEXT PK | Stable UUID; a game keeps its id across scans so history and artwork stay attached | `LibraryService.sync` (`src/main/services/library.ts:129`) |
| `launcher` TEXT NOT NULL | Launcher id | sync |
| `launcher_game_id` TEXT NOT NULL | Steam appid, Epic AppName, Battle.net codename, Riot `<product>.<patchline>`; UNIQUE with `launcher` | sync |
| `name` TEXT NOT NULL | Display name from the launcher | sync |
| `install_path` TEXT | Install folder or NULL | sync |
| `installed` INTEGER NOT NULL DEFAULT 1 | 1 while the last scan of that launcher listed the game; a vanished game is set to 0, never deleted (`src/main/services/library.ts:120,146-150`) | sync |
| `last_seen_at` TEXT NOT NULL | Time of the last scan that listed it | sync |
| `needs_update` INTEGER NOT NULL DEFAULT 0 | The launcher reports an update pending | sync |
| `last_played_at` TEXT | The launcher's own last-played time, or NULL | sync |

`sync` is one transaction per launcher: upsert every game found (`installed = 1`), then mark the launcher's other rows `installed = 0`; the library `revision` moves only when a row changed (`src/main/services/library.ts:121-152,197-201`). Only `installed = 1` rows are listed (`src/main/services/library.ts:108`).

**`artwork`** (`src/main/db/migrations/index.ts:181-188`)

| Column | Meaning | Written by |
| --- | --- | --- |
| `game_id` TEXT PK, cascades from `games` | One row per game | `ArtworkService.commit` upsert (`src/main/services/artwork.ts:179-183,469-475`) |
| `source` TEXT NOT NULL, CHECK `steamgriddb/steamcdn/manual/launcher` | Where the grid came from (or the first stored kind when there is no grid) (`src/main/services/artwork.ts:439`) | commit |
| `grid_path`, `hero_path`, `logo_path` TEXT | Paths relative to the artwork folder, forward slashes, `<uuid>/<kind>.<ext>`, or NULL | commit |
| `fetched_at` TEXT | Time of the last commit; also the `?v=` cache-buster in artwork URLs (`src/main/services/library.ts:41`) | commit |

Rows are deleted by `prune()` for games no longer installed or unknown (`src/main/services/artwork.ts:190,219-222`).

**`artwork_attempts`** (`src/main/db/migrations/index.ts:88-92`)

| Column | Meaning | Written by |
| --- | --- | --- |
| `game_id` TEXT PK, cascades from `games` | One row per game with a recorded miss | `recordAttemptStmt` (`src/main/services/artwork.ts:185-188`) |
| `attempted_at` TEXT NOT NULL | When the lookup last failed; a game is not retried automatically within 7 days of it (`src/main/services/artwork.ts:175,253-256`) | record |
| `error` TEXT NOT NULL | The problems joined with `; ` or “No artwork found”, cut to 500 characters (`src/main/services/artwork.ts:417,421,473`) | record |

Cleared when a commit succeeds without a stand-in (`src/main/services/artwork.ts:472`) and by `prune()` (`src/main/services/artwork.ts:221`). A launcher-icon stand-in keeps the row so the sources online are asked again later (`src/main/services/artwork.ts:426-427`). Exposed as `artworkError` on a game (`src/main/services/library.ts:82,55`).

**`launch_history`** (`src/main/db/migrations/index.ts:57-64`)

| Column | Meaning | Written by |
| --- | --- | --- |
| `id` TEXT PK | UUID per row | `LaunchService.record` (`src/main/services/launch.ts:106-108,451-458`) |
| `game_id`, `account_id` TEXT NOT NULL | No foreign keys; rows survive deletions | record |
| `launched_at` TEXT NOT NULL | When the job finished | record |
| `result` TEXT NOT NULL, CHECK `ok/failed/cancelled` | The job's result | record |

One row per finished job that had a game (a plain switch writes none) (`src/main/services/launch.ts:416`). A failed insert is logged at WARN and swallowed (`src/main/services/launch.ts:455-457`). Never pruned. Read by the library join for a game's `lastLaunch` (latest `ok` per game) (`src/main/services/library.ts:87-91`) and by `launch:recent` (latest `ok` per game and account pair, installed games and existing accounts only, limit 1..50, default 10) (`src/main/services/launch.ts:113-121`, `src/main/ipc/index.ts:93,512-517`).

**`settings`** (`src/main/db/migrations/index.ts:66-69`): `key TEXT PK`, `value TEXT NOT NULL`. Values are strings; booleans read as true for `true` or `1`; integers are parsed base 10 with the fallback on failure (`src/main/services/settings.ts:38-49`). Rows with keys outside `SETTING_KEYS` are ignored by `getAll` (`src/main/services/settings.ts:30-36`).

| Key | Writer | Notes |
| --- | --- | --- |
| `dryRun`, `logLevel`, `artwork.autoFetch`, `backups.keep`, `epic.launchUri`, `battlenet.launch`, `steam.autoLogin`, `riot.pressPlay`, `play.hideToTray`, `window.closeToTray`, `firstRun.dismissed`, `updates.auto`, `whatsNew.enabled` | Renderer through `settings:set` | Validated per key: booleans must be `true`/`false`; `logLevel` one of debug/info/warn/error; `backups.keep` 1..200 digits only; `epic.launchUri` `catalog`/`appname`; `battlenet.launch` `uri`/`exec`; value at most 4096 characters (`src/main/ipc/index.ts:90,192-231`) |
| `whatsNew.lastSeen` | Main on a fresh database (`src/main/services/whats-new.ts:8-12`); renderer when the card closes or when the feature is off (`src/renderer/src/components/WhatsNewDialog.tsx:182-191`) | Must be a version string (`src/main/ipc/index.ts:203`) |
| `window.bounds` | Main only (`rememberBounds`): JSON `{x, y, width, height, maximized}` of the normal bounds, 500 ms after the last resize/move/maximize/unmaximize and on close; never while minimized (`src/main/index.ts:273-293`) | Refused from the renderer (`src/shared/models.ts:262`, `src/main/ipc/index.ts:196`) |
| `steam.path`, `epic.path`, `battlenet.path`, `riot.path` | Main only: written by `launchers:locate` from a file picked in main's own dialog, deleted by `launchers:resetPath` (`src/main/ipc/index.ts:385,395`) | Refused from the renderer |

**`secrets`** (`src/main/db/migrations/index.ts:73-77`): `key TEXT PK`, `value_enc BLOB NOT NULL` (DPAPI ciphertext), `updated_at TEXT NOT NULL`. Keys are `steamgriddb.apiKey`, `diagnostics.roundtrip`, `vault.probe` (`src/shared/models.ts:188-190`). Written by `SecretStore.set` (encrypts first) and deleted by `delete`; `has` checks existence without decrypting (`src/main/services/secrets.ts:28-45`). The renderer may set or delete only the first two (`src/shared/models.ts:195`, `src/main/ipc/index.ts:161-182`) and read back only `diagnostics.roundtrip` (`src/main/ipc/index.ts:170-174`). `vault.probe` is main's: the constant `one-launch-vault-probe-v1` stored once when the vault is available and absent, never overwritten; its failure to decrypt is how a lost key is detected (`src/main/services/vault-health.ts:8,41-57`). `VaultHealth.reset` deletes every secret row, then stores a fresh probe (`src/main/services/vault-health.ts:60-67`).

**`launcher_sessions`** (`src/main/db/migrations/index.ts:158-172`)

| Column | Meaning | Written by |
| --- | --- | --- |
| `account_id` TEXT PK, cascades from `accounts` | One saved sign-in per account | `LauncherSessionStore.save` upsert (`src/main/services/sessions.ts:66-73,97-103`) |
| `launcher` TEXT NOT NULL | Launcher id | save |
| `launcher_uid` TEXT | The launcher's account id; on replace, kept when the new value is NULL (`COALESCE`) | save |
| `blob_enc` BLOB NOT NULL | DPAPI ciphertext of the adapter-serialised session material; never selected by the public reads (`src/main/services/sessions.ts:15-16`) | save |
| `fingerprint` TEXT NOT NULL | SHA-256 of the blob, computed by the adapter | save |
| `captured_at` TEXT NOT NULL | When captured | save |
| `verified_at` TEXT | Last time the launcher signed in from it; NULL after a (re)capture | `markVerified` (`src/main/services/sessions.ts:111-113`) |
| `stale_reason` TEXT | Why the saved sign-in is believed expired; NULL after capture or verification | `markStale` (`src/main/services/sessions.ts:115-117`) |

Deleted by `delete` (Forget), by cascade when the account goes, and all rows by `VaultHealth.reset` (`src/main/services/sessions.ts:119-121`, `src/main/services/vault-health.ts:36,62`).

#### 4.3.5 What is read when

- Reads are prepared statements run synchronously inside IPC handlers; there is no cache layer other than the renderer's own queries (`src/main/services/*.ts` throughout).
- Every account read feeds its `login` and `display_name` to the log redactor (`src/main/services/accounts.ts:83`, section 4.8).
- The library join reads `games`, `artwork`, `artwork_attempts`, the latest `ok` launch per game and that account's name in one statement (`src/main/services/library.ts:78-91`).

#### 4.3.6 Online copies of the database

- 250 ms after the window's first paint (`ready-to-show`), skipped when a quit has begun or the database is already closed, main copies the live database with SQLite's online backup API to `backups\db\onelaunch-<ISO time with ':' replaced by '-'>.db` (`src/main/index.ts:694-709`, `src/main/db/index.ts:99-109`).
- After the copy, files matching `onelaunch-*.db` in that folder are sorted by name (chronological) and all but the newest 5 are deleted; the keep count is floored at 1 (`src/main/db/index.ts:103-107`, `src/main/index.ts:118`).
- Success logs `database copied` with the path at INFO; failure logs `database copy failed` at WARN and nothing else happens (`src/main/index.ts:706-708`).
- A copy holds ciphertext encrypted under this Windows user's key in `chromium\Local State`; restoring it elsewhere leaves the secrets unreadable (`src/main/services/vault.ts:12-16`).
- Restore is manual: the `DB_CORRUPT` message tells the user to copy a file back or move the database aside; the app has no restore button for the database (`src/main/db/index.ts:19`; no caller of a database restore exists, grep `backups\\db` and `backupDatabase` in `src/`).

### 4.4 Encryption (the vault) and where the key lives

- Every secret is encrypted with Electron `safeStorage`, which on Windows is DPAPI bound to the current Windows user; `encrypt` throws `VAULT_UNAVAILABLE` (“Secure storage is not available on this system. Secrets cannot be stored.”) rather than fall back to plaintext, and any decrypt failure is `VAULT_UNREADABLE` (`src/main/services/vault.ts:4-10,17-25,54-66`).
- The `VAULT_UNREADABLE` message, verbatim: “One Launch cannot decrypt its stored secrets: the encryption key in the app data folder (chromium\Local State) was lost or belongs to a different Windows user. In Settings, clear the stored passwords and sign-ins, then enter them again.” (`src/main/services/vault.ts:21`).
- The key itself is Chromium's `os_crypt` entry in `<data folder>\chromium\Local State`; the app never reads or writes that file directly (`src/main/services/vault.ts:13-15`; `docs/architecture.md` › Vault and secrets says Chromium writes it some seconds after first use, which the code does not control).
- Encrypted at rest: `accounts.secret_enc`, `secrets.value_enc`, `launcher_sessions.blob_enc`, sensitive backup files (`.enc`, 4.7) and Battle.net token bytes inside backup manifests (4.7).
- Vault status at startup is logged (`vault` INFO line, WARN when unavailable, ERROR when the probe no longer decrypts); startup continues either way (`src/main/index.ts:484-492`).

### 4.5 Renderer-side storage

The renderer uses `localStorage` for two per-viewer conveniences only; every read and write is wrapped so a failure is ignored:

| Key | Values | Behaviour | Citation |
| --- | --- | --- | --- |
| `sidebar` | `expanded`, `collapsed`, or absent | Absent: the sidebar follows the window width (collapsed under 1100 px). Toggling stores the choice only when it differs from what the width would pick; crossing the breakpoint clears it | `src/renderer/src/store/ui.ts:8-29,160,178-189` |
| `library.sort` | `name`, else anything reads as `lastPlayed` | The Library's sort order; written when the user changes it | `src/renderer/src/views/LibraryView.tsx:23,39-45,220-227` |

Nothing else in the renderer touches `localStorage`, `sessionStorage` or IndexedDB (grep `localStorage|sessionStorage` over `src/` returns only those two files). Chromium's own storage for the page lives under `chromium\`.

### 4.6 The switch journal (`switch.json`)

A crash-safe record of a switch in progress, kept as a file (not a table) so it survives a damaged or locked database (`src/main/services/switch-journal.ts:57-62`). Every failure to write or remove it is logged at WARN and swallowed; the journal never stops a switch (`src/main/services/switch-journal.ts:97-99,132-135`).

**Fields** (pretty-printed JSON, `src/main/services/switch-journal.ts:12-25`):

| Field | Type | Meaning |
| --- | --- | --- |
| `version` | literal `1` | Format version; anything else is unreadable |
| `jobId` | string, `^[0-9a-f-]{1,64}$` (case-insensitive) | The launch job |
| `launcher` | launcher id | Which launcher |
| `accountId` | string, same shape as `jobId` | One Launch's account id. No login, display name, launcher account id or step title is ever written |
| `startedAt` | ISO string, must parse as a date | When the job began |
| `updatedAt` | ISO string | Last write; a missing or non-string value reads as `startedAt` |
| `backupId` | snapshot id `^[0-9A-Za-z.-]{1,64}$` or `null` | Null until the snapshot exists, which is always before the first launcher write |
| `stepId` | `^[a-z-]{1,32}$` or `null` | The step running when last written |
| `rollbackFailed` | boolean | The switch failed and its rollback failed too; anything but `true` reads as false |

**Lifecycle** (`src/main/services/switch-journal.ts:71-100`, `src/main/services/launch.ts:332-361`):

- Written (`begin`) before the adapter is called for a non-direct job, with `backupId` and `stepId` null. A direct job (nothing to write) never journals (`src/main/services/launch.ts:318-332`).
- Updated when the adapter reports its snapshot (`backupId`) and whenever the running step changes (`stepId`); an update that changes nothing is not written (`src/main/services/switch-journal.ts:78-86`, `src/main/services/launch.ts:338-347`).
- When the adapter throws before any snapshot was reported, the file is removed; a throw after a snapshot leaves the file for the next start (`src/main/services/launch.ts:354-358`).
- When the switch settles: `rollbackFailed: true` is written if the adapter reports `restored === false`; otherwise the file is removed, before the game launch (`src/main/services/launch.ts:359-361`).
- `clear(jobId)` removes the file only if the entry on disk still belongs to that job; `clear()` with no id removes it regardless (`src/main/services/switch-journal.ts:89-100`).
- Atomicity: each write goes to `switch.json.<pid>.tmp`, is `fsync`ed, then renamed over `switch.json`; a failed write removes the temp file (`src/main/services/switch-journal.ts:121-136`).

**Reading and the `.unreadable` rename** (`src/main/services/switch-journal.ts:103-118`): a missing file reads as null. A file that is not JSON, or JSON that fails the shape above, is logged at WARN (“the switch journal has an unexpected shape; moving it aside” or “the switch journal could not be read; moving it aside”) and renamed to `switch.json.unreadable` (overwriting a previous one), so it is reported once rather than at every start.

**Recovery at startup** (`SwitchRecoveryService`, `src/main/services/switch-journal.ts:151-226`), loaded once in `bootstrap` before the launch service exists so a journal from this run is never mistaken for one to recover (`src/main/index.ts:576-579`):

- No file: nothing.
- `backupId` null: logged at INFO (“the last run ended during a switch before anything was written; nothing to recover”), file removed, no prompt.
- Otherwise the entry becomes the pending recovery, logged at WARN with its ids.
- `recovery:get` returns `jobId`, `launcher`, `accountName` (null if the account was deleted since), `startedAt`, `backupId`, `snapshotAvailable` (the snapshot folder still has a readable manifest) and `phase`: `rollback-failed` when `rollbackFailed`; `signing-in` when `stepId` is one of `start`, `wait`, `verify`, `login`, `manual`, `recapture`; else `writing` (`src/main/services/switch-journal.ts:31,184-196`).
- `recovery:restore` restores that snapshot through the launcher's normal restore path, logs at WARN, and clears the pending entry and the file; with nothing pending it throws `NOT_FOUND` “There is no interrupted switch to recover” (`src/main/services/switch-journal.ts:139-145,199-206`). A failed restore leaves the prompt pending.
- `recovery:dismiss` logs at INFO and clears entry and file (`src/main/services/switch-journal.ts:208-213`).
- A restore from Settings › Backups for the same launcher also settles a pending recovery and removes a journal for that launcher (`src/main/services/switch-journal.ts:216-220`, `src/main/ipc/index.ts:473`).

### 4.7 Backups of launcher files (`BackupService`)

**Folder layout**: `<data folder>\backups\<launcher>\<id>\` holding `manifest.json` and the copied files at their `relative` paths (an encrypted file at `<relative>.enc`) (`src/main/services/backup.ts:73-74,138-144,151-163`).

**Id format**: the creation time as ISO-8601 with `:` replaced by `-`, for example `2026-01-31T12-34-56.789Z`, so ids sort chronologically as strings; when that folder already exists the id gets `-2`, `-3`, … appended until free (`src/main/services/backup.ts:87-89,157-162`). Only an id matching `^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}\.\d{3}Z(-\d+)?$` is ever joined onto a path; any other id is “not found” (`src/main/services/backup.ts:92,225-228`).

**Manifest** (`manifest.json`, pretty JSON, `src/main/services/backup.ts:42-63,191-200`):

| Field | Type | Meaning |
| --- | --- | --- |
| `version` | literal `1` | Any other value makes the snapshot invisible to `list` |
| `launcher` | launcher id | Must equal the folder's launcher on load, else “Snapshot <id> was taken for <x>, not <launcher>” |
| `createdAt` | ISO string (required) | Creation time |
| `reason` | string | Shown in the UI; missing reads as `''`. Adapters write “Before switching to <account display name>” and “Before restoring <snapshot id>” (`src/main/launchers/steam/adapter.ts:837,1150`, `src/main/launchers/epic/adapter.ts:1036,1326`, `src/main/launchers/battlenet/adapter.ts:1240,1581`, `src/main/launchers/riot/adapter.ts:1401,1722`) |
| `files[]` | array (required) | One entry per requested file, each: `relative` (path inside the snapshot, forward or back slashes; no empty or `..` segment, not absolute), `source` (absolute path of the launcher file), `existed` (false when the source was missing at snapshot time), `bytes` (size of the original file; 0 when missing), `sha256` (hex of the stored copy, the ciphertext when encrypted; null when missing), optional `encrypted: true` |
| `meta` | object of string/number/boolean/null | Small values restored alongside the files; a missing or non-object value reads as `{}` |
| `pinned` | string, optional | Present only when the snapshot is pinned; the text is the reason |

**Encrypted storage**: a file flagged `sensitive` is read, its bytes base64-encoded, encrypted with the vault, and written to `<relative>.enc`; `bytes` is the plaintext size and `sha256` the ciphertext's. When the vault is unavailable the file is stored in the clear and a WARN “secure storage unavailable; sensitive backup file stored in the clear” is logged (`src/main/services/backup.ts:171-188`).

**What each adapter puts in a snapshot**:

| Launcher | `files` | `meta` | Pinned when |
| --- | --- | --- | --- |
| Steam | `config/loginusers.vdf` (the install's `loginusers.vdf`), in the clear | `AutoLoginUser` (string or null) and `RememberPassword` (number or null): the `HKCU\Software\Valve\Steam` values at snapshot time | never (`src/main/launchers/steam/adapter.ts:835-840,1148-1157`) |
| Epic | Every existing `GameUserSettings.ini` the adapter knows, at `Config/<folder>/GameUserSettings.ini` (`<folder>` is the platform folder, `Windows` by default), each `sensitive` | none | the file holds a remembered session no One Launch account owns, and the switch is not in place (`src/main/launchers/epic/adapter.ts:292-295,1031-1039,1324-1330`) |
| Battle.net | `Battle.net.config` (only when the file exists), in the clear | `unifiedAuth`: a JSON string, an array of `{name, enc}` where `enc` is the vault ciphertext (base64) of each `HKCU\…\Battle.net\UnifiedAuth` token value's hex bytes | the client remembers a sign-in no account owns, and not in place (`src/main/launchers/battlenet/adapter.ts:1238-1245,1553-1565,1579-1586`) |
| Riot | `Data/RiotGamesPrivateSettings.yaml` (the client's session file), `sensitive` | none | the file holds a sign-in no account owns, and not in place (`src/main/launchers/riot/adapter.ts:121-123,1399-1404,1720-1726`) |

The pinned text is the same for all three: “Holds a sign-in that no One Launch account owns” (`src/main/launchers/epic/adapter.ts:100`, `src/main/launchers/battlenet/adapter.ts:119`, `src/main/launchers/riot/adapter.ts:127`). A snapshot taken before a restore passes `protect: <id being restored>` so pruning cannot remove the snapshot about to be used (`src/main/services/backup.ts:32-33,203,245`).

**Retention** (`src/main/services/backup.ts:9-11,239-251`): after every snapshot, the launcher's snapshots are listed newest first (by id); ordinary ones beyond `backups.keep` (setting, default 20, read at the moment of the snapshot, floored at 1; the IPC validator allows 1..200) and pinned ones beyond 10 (`PINNED_BACKUPS_TO_KEEP`, not a setting) are deleted recursively, never the protected id. Removals log `pruned backups` at INFO. Pinned snapshots do not count against the ordinary quota.

**Listing** (`src/main/services/backup.ts:207-218,288-320`): every subfolder of `backups\<launcher>` with a valid manifest, sorted by id descending. A folder without a manifest is skipped; an unparsable manifest logs `unreadable backup manifest` at WARN. Each snapshot reports `id`, `launcher`, `createdAt`, `reason`, `files` (relative paths of files that existed), `bytes` (sum of original sizes), `pinned` (text or null) and `path`.

**Restore** (`src/main/services/backup.ts:260-269,279-286`):

1. Load by id (shape check, manifest present, launcher matches).
2. Take only files with `existed: true`. Refuse the whole restore if any `source` is not absolute or is not one of the target paths the adapter owns right now (compared case- and separator-insensitively): “Snapshot <id> names a file that is not one of <launcher>'s: <source>” (`src/main/services/backup.ts:262-264`, `src/main/platform/fs-path.ts:19-27`).
3. Read every stored copy, verify its SHA-256 against the manifest (“Snapshot file is corrupt: <relative>”), decrypt when encrypted (fails if no vault: “Snapshot file <relative> is encrypted and no vault is available to decrypt it”), all before the first write.
4. Write every file atomically: each goes to `<dest>.onelaunch-<pid>.tmp` in its own folder (created if needed); only after every temp file is written are they renamed over their destinations; leftover temp files are removed on any failure, so a full disk or locked folder fails with nothing replaced (`src/main/services/backup.ts:110-126`).
5. Return the restored paths and the manifest's `meta`; the adapter then puts the registry values back (Steam sets or deletes `AutoLoginUser`/`RememberPassword`, Battle.net writes each decrypted token value and deletes tokens not in the snapshot; Epic and Riot have none) (`src/main/launchers/steam/adapter.ts:1161-1187`, `src/main/launchers/battlenet/adapter.ts:1601-1621`, `src/main/launchers/epic/adapter.ts:1341-1349`, `src/main/launchers/riot/adapter.ts:1731-1735`).

Files with `existed: false` are left alone. A restore is logged at WARN (`restored launcher files from backup`). `readFile` gives one file's original bytes with the same hash and decrypt checks (`src/main/services/backup.ts:272-277`).

### 4.8 Logs

- File: `<data folder>\logs\onelaunch.log`; rotated files `onelaunch.1.log` … `onelaunch.5.log` (`src/main/services/logger.ts:170-174,231-237`).
- Rotation: before appending a line, if the current file is non-empty and the line would push it past 1 MiB, `onelaunch.5.log` is deleted, each `onelaunch.N.log` becomes `N+1`, and the live file becomes `onelaunch.1.log` (`src/main/services/logger.ts:177-201`). The size is read from disk once per run then tracked in memory.
- Appends are synchronous; the folder is created if missing; a write failure is swallowed and never crashes the app (`src/main/services/logger.ts:178-183,280-296`).
- Console mirror only in development (`console: isDev`), WARN and ERROR to stderr, the rest to stdout (`src/main/index.ts:104`, `src/main/services/logger.ts:289-292`).
- Level: threshold `debug` in development, else `info`, then the `logLevel` setting is applied at startup (non-dev only) and at once when changed (`src/main/index.ts:104,441-442`, `src/main/ipc/index.ts:197-200`). Order: debug 10, info 20, warn 30, error 40 (`src/main/services/logger.ts:208`).
- Line format: `<ISO timestamp> <LEVEL padded to 5> [<scope>] <message>` followed, when metadata is given, by one space and the metadata as JSON, then `\n`. Scopes nest with `:` (`main:platform`, `main:steam`, …) (`src/main/services/logger.ts:262-278,298-306`).
- One line per message: CR and LF in message and metadata are escaped to `\r` and `\n` (`src/main/services/logger.ts:258-260`).
- Clipping: message and serialised metadata are each cut to 8 KiB **after** redaction (cutting first could leave a secret's start too short to match), with `… [<n> more characters]` appended (`src/main/services/logger.ts:251-256`).
- If redaction itself throws, the line is replaced by `<prefix>[log line dropped: redaction failed]`; metadata that cannot be serialised becomes `"[unserializable]"` (`src/main/services/logger.ts:268-277`).

**Redaction rules** (every message and every metadata string passes `redactString`; metadata objects pass `redactValue` first) (`src/main/services/logger.ts:107-150`):

| Rule | Purpose | Replacement |
| --- | --- | --- |
| Sensitive object keys | Any metadata property named like a password, passphrase, secret, token, API key, authorization, auth, cookie, session (id/blob/data), refresh/access token, credential(s), private key, remember-me, `secret_enc`, `value_enc`, or `data`, regardless of its value | `[REDACTED]` (`src/main/services/logger.ts:10-11,147`) |
| Key=value in free text | `password=…`, `token: …`, `"apiKey": "…"` and the like inside a string | value becomes `[REDACTED]`, key and separator kept (`:14-15,109`) |
| INI `Data=` | Epic's `[RememberMe]` blob line | `Data=[REDACTED]` (`:18,110`) |
| Steam command line | The password after `-login <user>` | `[REDACTED]` (`:21,111`) |
| JWT | Three dot-separated base64url parts starting `eyJ` | `[REDACTED]` (`:26,112`) |
| Email addresses | Epic and Battle.net logins | `[EMAIL]` (`:29,113`) |
| SteamID64 | 17 digits starting `7656119` | `[STEAMID]` (`:32,114`) |
| UUID | Riot account ids; also One Launch's own job and account ids | `[UUID…<last 4>]` so lines can still be matched up (`:35,115`) |
| Profile path | The Windows user name in `<drive>:\Users\<name>` with raw, escaped or forward slashes | `<drive>:\Users\[USER]` (`:38,116`) |
| Long opaque runs | 48+ base64/hex/token characters containing at least one digit (a long word or path segment is left alone) | `[REDACTED]` (`:41-42,117`) |
| Account names | Every login and display name read from the database or a launcher file is registered (`toAccount`, Steam's `loginusers.vdf` reader); whole-word, case-insensitive matches are masked. Names under 3 or over 256 characters, a fixed list of ordinary words (`steam`, `epic`, `main`, `alt`, `account`, `default`, `test`, `user`, …) and anything past 2000 names are ignored | `[ACCOUNT]` (`:52-105`, `src/main/services/accounts.ts:83`) |
| Buffers | Any byte array in metadata | `<bytes N>` (`:128`) |
| Errors | Name, redacted message, redacted stack, and a string `code` | object (`:129-137`) |
| Depth and cycles | Metadata deeper than 6 levels or circular | `[depth]`, `[circular]` (`:120,139-141`) |

Every pattern is required to stay linear on a long run of one kind of character because redaction runs synchronously on the main thread over text a launcher file or the update feed can supply (`src/main/services/logger.ts:23`).

**Never logged, by construction** (beyond the rules above): the Riot lockfile password and anything the Riot Client answers (only an error code or name is kept, `src/main/launchers/riot/local-launch.ts:86-95`); registry write data and command lines (failures report exit code and stderr, never Node's message, and stderr is dropped when it echoes the data, `src/main/platform/registry.ts:149-157`); PowerShell command lines (same rule, `src/main/platform/processes.ts:456-463`); the SteamGridDB key (read from the vault per request, sent only as a header, `src/main/services/artwork.ts:140-141,396,534`); session blobs and passwords (decrypted at the moment of use only, `src/main/services/sessions.ts:105-109`, `src/main/services/accounts.ts:283-292`). The journal carries no login or display name (4.6).

### 4.9 Artwork cache

- Folder: `<data folder>\artwork\<game uuid>\` with at most one file per kind: `grid.<ext>`, `hero.<ext>`, `logo.<ext>`, `<ext>` one of `jpg`, `png`, `webp`, or `ico` (ico only for a launcher's own icon standing in as the grid) (`src/main/services/artwork-url.ts:12-24`).
- Write: the bytes go to `<kind>.<ext>.<pid>.tmp` in the game's folder (created if needed), then rename into place; the temp file is removed on failure. Afterwards every other file in the folder named `<kind>.<other ext>` (not ending `.tmp`) is deleted, so a kind changing extension leaves one file (`src/main/services/artwork.ts:609-624`).
- The database row stores the relative path `<uuid>/<kind>.<ext>` with forward slashes (`src/main/services/artwork-url.ts:21-24`). A file deleted from disk counts as never fetched (`src/main/services/artwork.ts:358-360,369-370`).
- Manual images: the user's file (at most 20 MiB, must sniff as JPEG, PNG or WebP) is copied into the cache the same way with `source = 'manual'`; errors `TOO_LARGE` “That image is larger than 20 MB” and `BAD_IMAGE` “That file is not a JPEG, PNG, or WebP image” (`src/main/services/artwork.ts:177,297-309`).
- Prune, after every library scan: every folder under `artwork` whose name is not an installed game's id and not currently queued or running is deleted recursively; `artwork` and `artwork_attempts` rows for the same set are deleted too (a row can outlive its folder); the library revision bumps and `artwork pruned` is logged when anything went (`src/main/services/artwork.ts:198-229`, `src/main/index.ts:634-640`).

**The `onelaunch-art:` scheme** (how the renderer reads the cache):

- Registered before `ready` as a privileged scheme: standard, secure, streaming (`src/main/index.ts:97`).
- URL grammar: `onelaunch-art://art/<game uuid>/<kind>.<ext>[?v=<fetched_at>]`; `<game uuid>` must match the lowercase UUID shape, the file `^(grid|hero|logo)\.(jpg|png|webp|ico)$`, exactly two path segments, host `art`, nothing decoded, dotted or absolute. The resolved path must stay inside the artwork folder (`src/main/services/artwork-url.ts:8-12,36-52`).
- The handler answers 404 (empty body) for any URL that fails the grammar and for a file that does not exist; otherwise 200 with the file's content type (or `application/octet-stream`) and `cache-control: max-age=31536000, immutable`. The `?v=` value is the row's `fetched_at`, so a refreshed image gets a new URL and the renderer may cache each version forever (`src/main/index.ts:464-475`, `src/main/services/library.ts:41`, `src/main/services/artwork-url.ts:27-30`).
- The renderer's CSP allows images from `'self' data: blob: onelaunch-art:` only (`electron.vite.config.ts:17`).

### 4.10 Outside the data folder

| Location | What | Written when | Citation |
| --- | --- | --- | --- |
| `%TEMP%\one-launch-compile-cache` (Electron's `temp` path) | Node's compile cache: V8's compiled form of the main bundle and better-sqlite3's modules. Switched on by the entry module before the main bundle is imported, so the data folder (known only after the command line is read) cannot host it. Shared by every copy of the app; compiled code only | Every start, by Node | `src/main/boot.ts:1-9` |
| `%LOCALAPPDATA%\one-launch-updater` | electron-updater's download cache for the real GitHub feed (the library's default folder, named after the package name `one-launch`); holds the downloaded installer until it runs | Only when an update downloads, which only an installed per-user copy on the default data folder does | Default of electron-updater, not set in app code (`src/main/updates/engine.ts:61-62`); the path is from `docs/architecture.md` › Data directory, see Open questions |
| `.updaterId` (electron-updater's per-install id file) | **Never written**: the updater is given a fixed staging id instead | n/a | `src/main/updates/engine.ts:12-17` |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `OneLaunch` | Start with Windows: the exe path (Electron quotes it) followed by ` --start-hidden`, written through Electron's login-item API with `name: OneLaunch`; turning it off passes `openAtLogin: false` with the same path, args and name, and Electron removes the value (library behaviour, not app code). Only a per-user installed copy on the default data folder may write it | `autostart:set` | `src/main/platform/login-item.ts:6-7,37-42`, `src/main/ipc/index.ts:551-559` |
| `HKCU\…\Explorer\StartupApproved\Run`, value `OneLaunch` | Windows' own enabled/disabled flag for the Run entry. The app only reads it indirectly (`executableWillLaunchAtLogin`); the installer deletes it together with the Run value | Installer only | `src/main/platform/login-item.ts:30-35`, `build/installer.nsh:44-45,54-55` |
| `HKCU\Software\37275e3e-77ba-54aa-bc32-31551d28dc0d`, value `InstallLocation` | electron-builder's per-user install record; read to decide whether updates and Start with Windows are available. Never written by the app | Installer only | `src/main/updates/availability.ts:6-9,36-37`, `src/main/index.ts:445-451` |
| `resources\app-update.yml` beside the exe | electron-builder's feed description, read by electron-updater | Build time | `electron-builder.yml:48-53` |
| `<data folder>\dev-app-update.yml` and `<data folder>\update-cache\` | **[dev-only]** With `ONELAUNCH_UPDATE_FEED` set (unpackaged only), a generic-provider config is written at every engine creation and the updater's cache root is the data folder | Each run with a test feed | `src/main/updates/engine.ts:53-59` |

### 4.11 Developer-only files

- **[dev-only]** `<data folder>\fake-platform.json`: with `ONELAUNCH_FAKE_LAUNCHERS` set (unpackaged only), everything the fake platform was asked to do is rewritten on every change via `fake-platform.json.tmp` and rename: `registryWrites[{op, path, value?}]`, `processEvents[{kind: start|close|kill, exe, args}]`, `opened[]` (URLs handed to the shell), `fetched[]` (URLs fetched), `clipboardWrites` (count), `riotLaunchRequests[]` (`<product>.<patchline>`). Attached to the data folder when the scenario is materialised (`src/main/launchers/sim/report.ts:4-66`, `src/main/launchers/sim/scenario.ts:90`). Read by `scripts/app-driver.mjs` only.

## 5. IPC & main-process logic

The renderer is sandboxed and reaches privileged code only through typed request/response channels. There are no push channels: background progress is discovered by polling (`artwork:status`, `launch:status`, `updates:status`). This section gives the transport rules, then every channel, then the launcher gateway every adapter call goes through.

### 5.1 Transport

#### Envelope

- Every call is a request/response pair over Electron's `invoke`/`handle`. The handler never throws across the bridge: the answer is `{ ok: true, data }` or `{ ok: false, error: { code, message } }` (`src/shared/ipc-contract.ts:4-6,232-238`, `src/main/ipc/handle.ts:45-59`).
- Error mapping in main: an `IpcRequestError` keeps its code; any thrown object with a string `code` keeps that code and its message; any other `Error` becomes `INTERNAL` with its message; anything else becomes `INTERNAL` “Unknown error” (`src/main/ipc/handle.ts:25-32`).
- Logging: a failure whose code is in the refusal set (section 3.1) logs `ipc <channel> refused` at WARN; every other failure logs `ipc <channel> failed` at ERROR, with code and message only (`src/main/ipc/handle.ts:53-57`).
- A channel may be registered once; a second registration is a programming error that throws at startup (`src/main/ipc/handle.ts:39-43`).

#### Sender check

Before a handler runs, the sender frame's URL must be the app's own renderer: the packaged `index.html` file URL (exactly, or with `#`/`?` appended), or in development the dev server's origin and nothing else. Otherwise the call is answered `{ ok: false, error: { code: 'UNTRUSTED_SENDER', message: 'Request rejected' } }` and logged at WARN with the channel and URL (`src/main/ipc/handle.ts:46-49`, `src/main/index.ts:238-244`, `src/main/window-guard.ts:4-18`).

#### Allowlist

- The preload exposes exactly one object, `window.oneLaunch`, with `invoke(channel, req?)` and `startup(channel)`. `invoke` forwards only channels in `IPC_CHANNELS` (49 names) and answers any other name with `{ ok: false, error: { code: 'UNKNOWN_CHANNEL', message: 'Unknown IPC channel: <name>' } }` without touching main (`src/preload/index.ts:9-16,30-40`, `src/shared/ipc-contract.ts:173-225`).
- The channel list and the contract interface are tied together at compile time: a channel in one but not the other is a type error (`src/shared/ipc-contract.ts:227-230`).
- Main registers a handler for every key of the contract, in `registerIpcHandlers` (`src/main/ipc/index.ts:97`).

#### Validation helpers and limits

All request validation throws `BAD_REQUEST` with these messages (`src/main/ipc/handle.ts:67-107`):

| Helper | Accepts | Error message on failure |
| --- | --- | --- |
| `expectObject` | a non-null, non-array object | “<channel>: expected an object payload” |
| `expectString(value, name, max)` | a string of at most `max` chars | “<name> must be a string” / “<name> exceeds <max> characters” |
| `expectOneOf(value, name, allowed)` | one of the listed strings | “<name> is not an allowed value” |
| `expectOptionalString` | `undefined`, or as `expectString` | as above |
| `expectNullableString` | `undefined`, `null`, or as `expectString` | as above |
| `expectStringArray(value, name, maxItems, maxLength)` | an array of at most `maxItems` strings each at most `maxLength` | “<name> must be an array” / “<name> has more than <maxItems> items” / per item “<name>[<i>] must be a string” |
| `expectOptionalStringArray` | `undefined`, or as above | as above |

Limits (`src/main/ipc/index.ts:89-95`, `src/shared/models.ts:55-63`):

| Constant | Value | Used for |
| --- | --- | --- |
| `MAX_SECRET_LENGTH` | 4096 | `vault:setSecret` value |
| `MAX_SETTING_LENGTH` | 4096 | `settings:set` value |
| `MAX_ID_LENGTH` | 64 | every account, game, job id |
| `MAX_IMPORT_BATCH` | 64 | `launchers:importAccounts` keys |
| `MAX_RECENT` | 50 | `launch:recent` limit cap |
| `MAX_ERROR_TEXT` | 8192 | `app:reportError` message and stack |
| `MAX_BACKUPS_TO_KEEP` | 200 | `backups.keep` upper bound |
| `ACCOUNT_LIMITS` | displayName 120, login 256, launcherUid 64, password 1024, notes 20,000, 32 tags of 40 chars | account payloads |
| snapshot id | 128 | `backups:restore` id |
| ping message | 1000 | `app:ping` |

#### `requireIdle`

Handlers that change accounts, sessions, launcher folders, or launcher files first check that no switch-and-launch job is running or starting; if one is, they answer `BUSY` “Another switch or launch is still running” before reading the payload (`src/main/ipc/index.ts:100-103`, `src/main/services/launch.ts:23-29,245-247`). The channels that do this are marked in the table below. `launch:start` has its own busy check (`src/main/services/launch.ts:171`).

#### Startup prefetch

- The moment the renderer document starts, the preload sends seven requests, before the page's own scripts are fetched: `library:list`, `launch:recent` with `{ limit: 8 }`, `settings:getAll`, `accounts:list`, `app:info`, `recovery:get`, `updates:status`. Each settles on its own; a rejected call becomes that channel's `INTERNAL` error envelope (`src/shared/ipc-contract.ts:243-259`, `src/preload/index.ts:18-28`).
- `window.oneLaunch.startup(channel)` returns the same promise every time for those seven channels and an `UNKNOWN_CHANNEL` “Not a startup channel: <name>” envelope for any other (`src/preload/index.ts:30-38`).
- The renderer gathers those answers and renders once they are all in or after a cap (50 ms), whichever comes first; while this "startup window" is open a mount may read a settled answer as its first data (`peekStartup`) and an `invoke` for the same channel and request joins the in-flight startup call instead of asking main again; the window closes in the app's first effect, after which every mount and reload asks main afresh (`src/renderer/src/lib/startup.ts:36-78`, `src/renderer/src/lib/ipc.ts:16-23`; the 50 ms cap is passed in `src/renderer/src/boot.ts:7`).

#### Renderer client

- `ipc.invoke(channel, req)` unwraps the envelope: `ok` returns `data`; otherwise it throws `IpcError` with the envelope's `code` and `message` (`src/renderer/src/lib/ipc.ts:5-26`).
- `useIpcQuery(fetcher, deps, seed?)` is the renderer's query hook: `data`, `error` (message text), `loading`, `reload()`. A new run clears the previous error; stale results are ignored; a seeded startup answer skips the first fetch (`src/renderer/src/lib/ipc.ts:28-74`).

### 5.2 Channel table

Direction is always renderer → main (request) and main → renderer (response). "Startup" marks the seven prefetched channels. Callers are the renderer files that name the channel (grep of `src/renderer/src`); "none" means grep found no renderer caller.

| Channel | Request | Response | Renderer callers | `requireIdle` |
| --- | --- | --- | --- | --- |
| `app:ping` | `{ message: string ≤1000 }` | `{ echo, receivedAt }` | none | no |
| `app:info` | none | `AppInfo` | Sidebar, TitleBar, SettingsView, WhatsNewDialog, **[dev-only]** WhatsNewDevPanel; startup | no |
| `app:openPath` | `{ target: 'logs' \| 'data' \| 'backups' }` | `{ ok: true }` | SettingsView, BackupsPanel, ErrorBoundary | no |
| `app:openUrl` | `{ target: 'steamgriddb-api-key' }` | `{ ok: true }` | ArtworkPanel | no |
| `app:reportError` | `{ source: 'render' \| 'window' \| 'promise', message ≤8192, stack: string ≤8192 \| null }` | `{ ok: true }` | ErrorBoundary, main.tsx | no |
| `vault:status` | none | `VaultHealthStatus` | SettingsView | no |
| `vault:reset` | none | `VaultResetSummary` | SettingsView | no |
| `vault:setSecret` | `{ key: RendererSecretKey, value ≤4096 }` | `{ ok: true }` | ArtworkPanel, SettingsView | no |
| `vault:getSecret` | `{ key: 'diagnostics.roundtrip' }` | `{ value: string \| null }` | SettingsView | no |
| `vault:deleteSecret` | `{ key: RendererSecretKey }` | `{ ok: true }` | ArtworkPanel, SettingsView | no |
| `settings:get` | `{ key: SettingKey }` | `{ value: string \| null }` | BackupsPanel | no |
| `settings:set` | `{ key: SettingKey, value ≤4096 }` | `{ ok: true }` | SettingsView, BackupsPanel, LaunchDialog, SetupGuide, WhatsNewDialog | no |
| `settings:getAll` | none | `{ settings: Partial<Record<SettingKey, string>> }` | SettingsView, Sidebar, SetupGuide, WhatsNewDialog; startup | no |
| `accounts:list` | none | `{ accounts: Account[] }` | AccountsView, LaunchDialog, SetupGuide; startup | no |
| `accounts:get` | `{ id ≤64 }` | `{ account }` | AccountDetailView | no |
| `accounts:create` | `AccountCreateInput` | `{ account }` | AccountDetailView | no |
| `accounts:update` | `{ id ≤64, patch: AccountPatch }` | `{ account }` | AccountDetailView | yes |
| `accounts:delete` | `{ id ≤64 }` | `{ ok: true }` | AccountDetailView | yes |
| `accounts:setPassword` | `{ id ≤64, password ≤1024 }` | `{ account }` | AccountDetailView | yes |
| `accounts:clearPassword` | `{ id ≤64 }` | `{ account }` | AccountDetailView | yes |
| `accounts:copy` | `{ id ≤64, field: 'login' \| 'password' }` | `{ clearAfterMs, historyExcluded }` | AccountDetailView, LaunchDialog | no |
| `library:list` | none | `{ games: Game[], revision }` | LibraryView, SetupGuide; startup | no |
| `library:rescan` | none | `{ games, scanned: LibraryScanResult[] }` | LibraryView, ArtworkPanel | no |
| `artwork:refresh` | `{ gameId: string ≤64 \| null, force: boolean }` | `{ queued }` | LibraryView, ArtworkPanel | no |
| `artwork:status` | none | `{ status: ArtworkStatus }` | LibraryView, ArtworkPanel, SetupGuide | no |
| `artwork:setManual` | `{ gameId ≤64, kind: ArtworkKind }` | `{ game: Game \| null }` | LibraryView | no |
| `launchers:status` | none | `{ launchers: LauncherStatus[] }` | LauncherStatusPanel, LibraryView, SetupGuide | no |
| `launchers:locate` | `{ launcher: LauncherId }` | `{ install: LauncherInstall \| null }` | LauncherStatusPanel, SetupGuideSteps | yes (twice) |
| `launchers:resetPath` | `{ launcher }` | `{ ok: true }` | LauncherStatusPanel | yes |
| `launchers:games` | `{ launcher }` | `{ games: InstalledGame[] }` | none | no |
| `launchers:rememberedAccounts` | `{ launcher }` | `{ accounts: RememberedAccount[] }` | ImportAccountsDialog, SetupGuide | no |
| `launchers:importAccounts` | `{ launcher, launcherUids: string[] ≤64 items × ≤256 }` | `{ accounts: Account[], skipped: { launcherUid, reason }[] }` | ImportAccountsDialog, SetupGuide | no |
| `launchers:switchPlan` | `{ accountId ≤64 }` | `{ plan: SwitchPlan }` | AccountDetailView | no |
| `sessions:get` | `{ accountId ≤64 }` | `{ session, state, stateError }` | SessionPanel | no |
| `sessions:capture` | `{ accountId ≤64 }` | `{ session, account }` | SessionPanel | yes |
| `sessions:forget` | `{ accountId ≤64 }` | `{ ok: true }` | SessionPanel | yes |
| `backups:list` | `{ launcher }` | `{ snapshots: BackupSnapshot[] }` | BackupsPanel | no |
| `backups:restore` | `{ launcher, id ≤128 }` | `{ result: RestoreResult }` | BackupsPanel | yes |
| `launch:preflight` | `{ accountId ≤64, gameId?: string ≤64 \| null }` | `{ preflight: LaunchPreflight }` | LaunchDialog | no |
| `launch:start` | `{ accountId ≤64, gameId: string ≤64 \| null, confirmClose: boolean, instant?: boolean }` | `{ job: LaunchJob }` | LaunchDialog | own busy check |
| `launch:status` | `{ jobId ≤64 }` | `{ job }` | LaunchHost (every 150 ms while a job runs) | no |
| `launch:cancel` | `{ jobId ≤64 }` | `{ job }` | LaunchDialog | no |
| `launch:recent` | `{ limit: number }` | `{ recent: RecentLaunch[] }` | LibraryView; startup (`limit: 8`) | no |
| `recovery:get` | none | `{ recovery: SwitchRecovery \| null }` | RecoveryDialog, WhatsNewDialog; startup | no |
| `recovery:restore` | none | `{ result: RestoreResult }` | RecoveryDialog | yes |
| `recovery:dismiss` | none | `{ ok: true }` | RecoveryDialog | no |
| `autostart:status` | none | `AutostartStatus` | SettingsView | no |
| `autostart:set` | `{ openAtLogin: boolean }` | `AutostartStatus` | SettingsView | no |
| `updates:status` | none | `UpdateStatus` | lib/updates.ts via Sidebar (every 60 s) and UpdatesPanel (every 5 s; 1 s while checking or downloading); startup | no |
| `updates:check` | none | `UpdateStatus` | UpdatesPanel | no |
| `updates:install` | none | `{ ok: true }` | lib/updates.ts (`restartToUpdate`) | yes |

Sources for the table: `src/shared/ipc-contract.ts:41-163`, `src/main/ipc/index.ts:107-559`, caller grep over `src/renderer/src` (files named by basename; e.g. `src/renderer/src/components/LaunchHost.tsx:97,107`, `src/renderer/src/lib/updates.ts:21,42`, `src/renderer/src/components/Sidebar.tsx:21,45`, `src/renderer/src/components/UpdatesPanel.tsx:13,68`).

### 5.3 Channels in detail

#### `app:ping`

- Validates an object payload and a `message` string of at most 1000 chars.
- Answers `{ echo: <message>, receivedAt: <now ISO> }`. No side effects (`src/main/ipc/index.ts:107-111`).
- Errors: `BAD_REQUEST` only.

#### `app:info`

- No payload. Answers the app version, Electron, Node and Chrome versions (“unknown” when missing), whether the build is packaged, the data folder, log folder and database path, the startup timing marks collected so far, and `platformSource` (`real`, or **[dev-only]** `fake`) (`src/main/ipc/index.ts:113-124`).
- No errors beyond the envelope's generic ones.

#### `app:openPath`

- `target` must be `logs`, `data` or `backups`; it maps to the logs folder, the data folder, or the backups folder respectively (`src/main/ipc/index.ts:126-129`).
- Opens the folder with the system shell. When the shell reports a problem the handler fails with `OPEN_FAILED` and the shell's own text as the message (`src/main/ipc/index.ts:130-132`).

#### `app:openUrl`

- `target` must be `steamgriddb-api-key`; the renderer never names a URL. Main opens `https://www.steamgriddb.com/profile/preferences/api` in the system browser and answers `{ ok: true }` (`src/main/ipc/index.ts:26-28,135-140`).

#### `app:reportError`

- `source` is one of `render`, `window`, `promise`; `message` is a string of at most 8192 chars; `stack` is a string of at most 8192 chars, `null`, or absent (read as `null`).
- Writes one ERROR line `renderer error` with those three fields to the main log (which redacts secrets and account names) and answers `{ ok: true }` (`src/main/ipc/index.ts:142-149`).

#### `vault:status`

- Answers `{ available, backend, readable }`: whether DPAPI storage is usable, `dpapi` or `none`, and whether the stored probe still decrypts (`null` when the vault is unavailable or no probe has ever been stored; `false` when decryption fails) (`src/main/ipc/index.ts:153`, `src/main/services/vault-health.ts:47-57`).
- An unexpected error other than an unreadable vault propagates as its own code.

#### `vault:reset`

- No confirmation in main (the renderer asks). Clears every account password (rows kept, `updatedAt` bumped), deletes every session snapshot and every named secret, then stores a fresh probe. Answers the three counts and logs `vault reset by user` at WARN with them (`src/main/ipc/index.ts:155-159`, `src/main/services/vault-health.ts:59-67`).
- Encrypted files inside backup snapshots are untouched and stay unreadable.

#### `vault:setSecret`

- `key` must be `steamgriddb.apiKey` or `diagnostics.roundtrip` (never `vault.probe`); `value` a string of at most 4096 chars.
- Encrypts and upserts the value with the current time; logs `secret stored` with the key only (`src/main/ipc/index.ts:161-168`, `src/main/services/secrets.ts:28-31`).
- Errors: `BAD_REQUEST`; `VAULT_UNAVAILABLE` when encryption is not available.

#### `vault:getSecret`

- `key` must be exactly `diagnostics.roundtrip`. Answers `{ value }` decrypted, or `null` when none is stored (`src/main/ipc/index.ts:170-174`, `src/main/services/secrets.ts:33-37`).
- Errors: `BAD_REQUEST`; `VAULT_UNAVAILABLE`; `VAULT_UNREADABLE`.

#### `vault:deleteSecret`

- `key` as for `vault:setSecret`. Deletes the row (no error when absent), logs `secret deleted` with the key, answers `{ ok: true }` (`src/main/ipc/index.ts:176-182`).

#### `settings:get`

- `key` must be one of the 19 setting keys. Answers the stored string or `null` (`src/main/ipc/index.ts:186-190`). Main-only keys may be read.

#### `settings:set`

Validation, in order (`src/main/ipc/index.ts:192-221`); every failure is `BAD_REQUEST`:

1. object payload; `key` one of the 19; `value` a string of at most 4096 chars.
2. `key` in the main-only set (`window.bounds`, `steam.path`, `epic.path`, `battlenet.path`, `riot.path`): “<key> is not user-settable”.
3. `logLevel`: must be `debug`, `info`, `warn` or `error` (“logLevel must be debug, info, warn, or error”); when valid the live log threshold changes at once, before the value is stored.
4. `epic.launchUri`: `catalog` or `appname` (“epic.launchUri must be catalog or appname”).
5. `battlenet.launch`: `uri` or `exec` (“battlenet.launch must be uri or exec”).
6. `whatsNew.lastSeen`: a version string (“whatsNew.lastSeen must be a version”).
7. `dryRun`, `artwork.autoFetch`, `steam.autoLogin`, `riot.pressPlay`, `play.hideToTray`, `firstRun.dismissed`, `window.closeToTray`, `updates.auto`, `whatsNew.enabled`: exactly `true` or `false` (“<key> must be true or false”).
8. `backups.keep`: one to three digits, value 1..200 (“backups.keep must be a whole number from 1 to 200”).

Then: the value is stored (upsert); `setting updated` is logged at INFO with key and value; main's `onSettingChanged` hook runs (`window.closeToTray` creates or removes the tray icon at once; `updates.auto` turns the updater's automatic checks on or off at once, cancelling a background download when turned off); and when the key is `artwork.autoFetch`, the new value is `true` and the previously stored value was exactly `false`, every installed game without a cover is queued for artwork (respecting the 7-day miss back-off), with `artwork queued` logged when the count is above zero (`src/main/ipc/index.ts:222-230`, `src/main/index.ts:685-688`, `src/main/services/artwork.ts:262-265`). Answers `{ ok: true }`.

- Note the asymmetry: turning auto-fetch on from an unset (never stored) state does not queue, because the previous value is `null`, not `'false'` (`src/main/ipc/index.ts:222`).

#### `settings:getAll`

- Answers every row of the settings table whose key is a known setting key, main-only keys included (`src/main/ipc/index.ts:233`, `src/main/services/settings.ts:30-36`).

#### `accounts:list`

- Answers all accounts ordered by launcher then display name (case-insensitive), without secrets; rows with an unknown launcher are left out (`src/main/ipc/index.ts:239`, `src/main/services/accounts.ts:158,184-189`).

#### `accounts:get`

- `id` string ≤64. Answers the account or `NOT_FOUND` “Account <id> does not exist” (`src/main/ipc/index.ts:241-245`, `src/main/services/accounts.ts:16-22,204-208`).

#### `accounts:create`

- Shape: `launcher` one of the four ids; `displayName` ≤120; `login` ≤256; `password` string ≤1024, `null` or absent; `launcherUid` string ≤64, `null` or absent; `tags` array of ≤32 strings of ≤40 chars or absent (read as `[]`); `notes` string ≤20,000 or absent (read as `''`) (`src/main/ipc/index.ts:247-256`).
- Business rules (all `BAD_REQUEST` unless stated): display name and login are trimmed and must be non-empty; launcher uid is trimmed, empty becomes `null`; tags are normalised (section 3.2); an empty-string password counts as none, a non-empty one is checked and encrypted before any row is written; then uniqueness of login and launcher uid within the launcher (`DUPLICATE`) (`src/main/services/accounts.ts:223-239`).
- Side effects: inserts the row with a new UUID and `createdAt = updatedAt = now`; logs `account created` with id, launcher and whether a password was stored. Answers the stored account (`src/main/ipc/index.ts:257-258`).
- Does not require idle.
- Other errors: `VAULT_UNAVAILABLE` when a password is given and encryption is unavailable.

#### `accounts:update`

- Requires idle. `id` ≤64; `patch` must be an object (“accounts:update.patch: expected an object payload”); each present field is validated as for create; `launcherUid: null` clears the id; absent fields are left as they are (`src/main/ipc/index.ts:261-276`, `src/main/services/accounts.ts:241-252`).
- Uniqueness is re-checked ignoring this row (`DUPLICATE`). `updatedAt` is set to now. Logs `account updated` with the id and the list of patched field names. Answers the stored account.
- Errors: `BUSY`, `BAD_REQUEST`, `NOT_FOUND`, `DUPLICATE`.

#### `accounts:delete`

- Requires idle. Deletes the row; `NOT_FOUND` when nothing was deleted. The account's session snapshot goes with it (database cascade); its launch history rows stay. Logs `account deleted`. Answers `{ ok: true }` (`src/main/ipc/index.ts:282-289`, `src/main/services/accounts.ts:264-267`).

#### `accounts:setPassword`

- Requires idle. `id` ≤64, `password` string ≤1024; the service additionally refuses an empty string (“Password must not be empty”). Encrypts and overwrites the ciphertext, bumps `updatedAt`, logs `account password updated` (id only), answers the account. The launcher is never touched (`src/main/ipc/index.ts:291-299`, `src/main/services/accounts.ts:269-275`).
- Errors: `BUSY`, `BAD_REQUEST`, `NOT_FOUND`, `VAULT_UNAVAILABLE`.

#### `accounts:clearPassword`

- Requires idle. Sets the ciphertext to null, bumps `updatedAt`, logs `account password removed`, answers the account (`src/main/ipc/index.ts:301-308`, `src/main/services/accounts.ts:277-281`).

#### `accounts:copy`

- `id` ≤64; `field` is `login` or `password`. For `login` the text is the account's login; for `password` it is decrypted in main, and `NO_PASSWORD` “No password is stored for this account” is returned when none is stored (`src/main/ipc/index.ts:310-319`).
- The text is written to the clipboard by main. First a private write that keeps the value out of Windows clipboard history and cloud sync is tried and read back; if it fails or the read-back differs, the plain clipboard write is used and the fallback reason is logged at WARN (`clipboard history exclusion unavailable; copied with the plain clipboard`). A timer then clears the clipboard after 30,000 ms, but only if the clipboard still holds exactly what was copied (compared by SHA-256; only the hash is kept in memory). A new copy cancels the previous timer. At quit the pending value is cleared the same way (`src/main/services/clipboard.ts:3,37-70`, `src/main/index.ts:494-498,630-631`).
- Answers `{ clearAfterMs: 30000, historyExcluded }` and logs `copied to clipboard` with id, field and delay (never the text) (`src/main/ipc/index.ts:321-323`).
- Errors: `BAD_REQUEST`, `NOT_FOUND`, `NO_PASSWORD`, `VAULT_UNAVAILABLE`, `VAULT_UNREADABLE`.

#### `library:list`

- Answers installed games ordered by name (case-insensitive), each joined with its artwork, last artwork miss and last successful launch, plus the current library revision (`src/main/ipc/index.ts:328`, `src/main/services/library.ts:78-91,108,173-178`).

#### `library:rescan`

- Runs the shared rescan: for every launcher that has an adapter, in id order, read its installed games and sync the games table (new rows added, seen rows updated, unseen rows marked uninstalled; row ids kept). A launcher that is not installed syncs as having no games (`installed: false` in its result); any other failure leaves that launcher's rows untouched and is reported as `error` on its entry; neither stops the others. The library is marked scanned even when the loop throws (`src/main/services/library-scan.ts:17-45`, `src/main/services/library.ts:121-152,197-201`).
- After the scan: artwork folders and rows for games that are not installed are pruned, and when auto-fetch is on every installed game without a real cover is queued (7-day back-off on misses) (`src/main/index.ts:634-640`, `src/main/services/artwork.ts:198-229,248-265`).
- Answers the installed games after the scan and the per-launcher results (`src/main/ipc/index.ts:330-333`).
- Not guarded by `requireIdle`; a scan reads launcher files only.
- Logs: `library rescanned` (INFO, per launcher), `launcher not installed; library scan skipped` (INFO), `library scan failed for launcher` (WARN), `artwork pruned`, `artwork queued`.

#### `artwork:refresh`

- `gameId` string ≤64, `null` or absent (read as `null`); `force` is true only when the value is exactly boolean `true`.
- With a `gameId`: the game must exist (`NOT_FOUND` “Game <id> does not exist”) and is queued, forced or not. A forced single-game refresh replaces a manual image (`src/main/services/artwork.ts:271-274,366`).
- With `null` and `force` false: every installed game without a real cover is queued, ignoring the 7-day back-off. With `null` and `force` true: every installed game whose artwork source is not `manual` is queued for a re-download (`src/main/services/artwork.ts:276-281`).
- Queue rules: a game already running is not queued again unless the new request is forced and the running one is not, in which case it runs once more afterwards; a pending entry becomes forced if either request was forced; a stopped service (quitting) queues nothing (`src/main/services/artwork.ts:311-329`).
- Answers `{ queued }` = number of games newly queued; logs `artwork refresh queued` (`src/main/ipc/index.ts:338-345`).

#### `artwork:status`

- Answers the queue counters, whether a SteamGridDB key is stored (existence check, no decryption), the library revision and whether a scan has finished this run (`src/main/ipc/index.ts:347`, `src/main/services/artwork.ts:231-241`).

#### `artwork:setManual`

- `gameId` ≤64 must exist (`NOT_FOUND`); `kind` is `grid`, `hero` or `logo`.
- Main opens its own file picker titled “Choose an image” for `png`, `jpg`, `jpeg`, `webp`. Cancel answers `{ game: null }` (`src/main/ipc/index.ts:349-356`, `src/main/index.ts:642-650`).
- The chosen file must be at most 20 MiB (`TOO_LARGE` “That image is larger than 20 MB”) and sniff as JPEG, PNG or WebP by magic bytes (`BAD_IMAGE` “That file is not a JPEG, PNG, or WebP image”). It is copied into the cache as `<gameId>/<kind>.<ext>` (temp then rename; other files of that kind removed), the artwork row is upserted with source `manual` and the other kinds kept, any recorded miss is cleared, the revision bumps, `manual artwork set` is logged. A fetch for the same game that was in flight drops its result when it finishes (`src/main/services/artwork.ts:296-309,411-415,469-475`).
- Answers the game as it now reads.

#### `launchers:status`

- For each of the four launcher ids, in order and in parallel: a launcher without an adapter answers `implemented: false` with everything else null. Otherwise the chosen folder, the detected install, whether it is running (only when installed, else `null`) and the health check (handed the install and running state so nothing is read twice) are collected. A throw anywhere in that pass becomes `attention: { message }` on that launcher's row only; the call never fails as a whole. A pass over 2000 ms logs `launcher status was slow` at WARN (`src/main/ipc/index.ts:362`, `src/main/services/launchers.ts:49-50,181-213`).

#### `launchers:locate`

- Requires idle at entry. `launcher` one of the four.
- Main opens its own picker titled “Locate <exe name>” filtered to `.exe` with the launcher's display name as the filter label. Cancel answers `{ install: null }` (`src/main/ipc/index.ts:374-375`, `src/main/index.ts:652-660`).
- The adapter checks the file (read-only) and answers the installation it belongs to, or nothing. Nothing: `NOT_A_LAUNCHER` “That file is not <Launcher>. Choose <exe name> in the folder <Launcher> is installed in.” (`src/main/ipc/index.ts:376-382`).
- Requires idle again (the dialog may have stayed open for minutes). Then the install folder is stored under `<launcher>.path`, `launcher folder chosen by user` is logged, and a rescan starts in the background (not awaited; a failure logs `rescan after a launcher folder change failed` at WARN). Answers `{ install }` (`src/main/ipc/index.ts:364-389`).
- The path never comes from the renderer.

#### `launchers:resetPath`

- Requires idle. Deletes `<launcher>.path`, logs `launcher folder reset to automatic detection`, starts a background rescan, answers `{ ok: true }` (`src/main/ipc/index.ts:391-399`).

#### `launchers:games`

- `launcher` one of the four. Answers the installed games read straight from the launcher's files through the gateway, not the library table (`src/main/ipc/index.ts:401-405`).
- Errors: `NOT_IMPLEMENTED`, `NOT_INSTALLED`, `LAUNCHER_FILE`, `ADAPTER_FAILED`.

#### `launchers:rememberedAccounts`

- `launcher` one of the four. Reads the launcher's own remembered accounts (names and ids only) and attaches `linkedAccountId`: the One Launch account with the same launcher uid, else the one with the same login (case-insensitive), else `null` (`src/main/ipc/index.ts:407-411`, `src/main/services/launchers.ts:224-232`).
- Errors as for `launchers:games`.

#### `launchers:importAccounts`

- `launcher` one of the four; `launcherUids` an array of at most 64 strings of at most 256 chars. Each string is a remembered entry's key: its launcher uid, or its login when the launcher records no id (`src/main/ipc/index.ts:413-417`, `src/main/services/launchers.ts:234-237`).
- The remembered list is read once; duplicate keys are collapsed; for each key, in order (`src/main/services/launchers.ts:244-279`):
  - no remembered entry matches: skipped with reason “not remembered by the launcher”;
  - the entry has no login and no linked account: skipped with “the launcher does not record the login for this account”;
  - the entry is already linked to an account: if that account has no launcher uid and the entry has one, the account gets the uid (through the normal update, so `DUPLICATE` can propagate) and is returned in `accounts`; otherwise skipped with “already in One Launch”;
  - otherwise a new account is created with display name = persona name or, when empty, the login; login; launcher uid or `null`; no password, no tags, no notes.
- Logs `imported launcher accounts` with counts. Answers `{ accounts, skipped }`.
- Not guarded by `requireIdle` although it creates and updates accounts.
- Errors: `BAD_REQUEST`, `NOT_IMPLEMENTED`, `NOT_INSTALLED`, `DUPLICATE`, `ADAPTER_FAILED`.

#### `launchers:switchPlan`

- `accountId` ≤64 must exist (`NOT_FOUND`). Runs the account's adapter in dry-run mode and answers the full plan including the “will be closed” warning; nothing is changed. Logs `switch plan generated` with account id, launcher, strategy and step count (`src/main/ipc/index.ts:423-429`, `src/main/services/launchers.ts:281-285`).
- Errors: `NOT_IMPLEMENTED`, `NOT_INSTALLED`, `LAUNCHER_FILE`, `SWITCH_BLOCKED`, `ADAPTER_FAILED`.

#### `sessions:get`

- `accountId` ≤64 must exist (`NOT_FOUND`). Answers the stored snapshot (or `null`), and, when the launcher's adapter reads session state, what the launcher's files say now; a failing state read never hides the snapshot: `state` is `null` and `stateError` carries the message. For a launcher without session state (Steam) `state` and `stateError` are both `null` (`src/main/ipc/index.ts:434-438`, `src/main/services/launchers.ts:322-333`).

#### `sessions:capture`

- Requires idle. `accountId` ≤64 must exist. The adapter must support capture (`NOT_IMPLEMENTED` “The <Launcher> adapter is not available yet” otherwise). The adapter reads the launcher's current remembered session, refuses with `SWITCH_BLOCKED` when there is none or it is known to belong to another account, stores it as this account's snapshot (replacing any previous one, unverified and not stale) and links the launcher's account id. Logs `session captured` and `session captured by user`. Answers the snapshot and the account re-read (`src/main/ipc/index.ts:440-447`, `src/main/services/launchers.ts:335-343`, `src/main/launchers/types.ts:186-195`).
- Reads a launcher file; writes none.

#### `sessions:forget`

- Requires idle. `accountId` ≤64 must exist. Deletes the snapshot row if any; logs `session forgotten` only when one was removed. Answers `{ ok: true }` either way (`src/main/ipc/index.ts:449-455`, `src/main/services/launchers.ts:345-350`).

#### `backups:list`

- `launcher` one of the four. Lists `<backupsDir>\<launcher>\*` folders with a readable manifest, newest id first (ids sort chronologically as strings). Folders with no or an unreadable manifest are skipped (the latter logs `unreadable backup manifest` at WARN) (`src/main/ipc/index.ts:459-463`, `src/main/services/backup.ts:207-218,288-307`).

#### `backups:restore`

- Requires idle. `launcher` one of the four; `id` string ≤128. The snapshot must be listed (`NOT_FOUND` “That snapshot no longer exists”) (`src/main/ipc/index.ts:465-470`).
- The adapter restores it: refuses with `LAUNCHER_RUNNING` while the launcher runs; takes a protective snapshot first; then copies every file back (each stored copy verified against its recorded hash and decrypted before the first write, every path checked against the adapter's own file list, all writes atomic) and puts recorded registry values back (`src/main/services/backup.ts:253-269`; the refusal and protective snapshot are stated in `docs/switching.md:27` and belong to the adapter sections).
- Logs `backup restored by user` at WARN with counts, and settles any pending crash recovery for that launcher (clears the journal if it names the launcher). Answers `{ result }` (`src/main/ipc/index.ts:471-474`, `src/main/services/switch-journal.ts:215-220`).
- Errors: `BUSY`, `BAD_REQUEST`, `NOT_FOUND`, `NOT_IMPLEMENTED`, `LAUNCHER_RUNNING`, `BACKUP`, `ADAPTER_FAILED`.

#### `launch:preflight`

- `accountId` ≤64 must exist (`NOT_FOUND`); `gameId` string ≤64, `null` or absent. A game must exist and be of the account's launcher (`BAD_REQUEST` “<game> is a <launcher> game; pick a <launcher> account”) (`src/main/ipc/index.ts:482-486`, `src/main/services/launch.ts:146-148,225-231`).
- Runs the adapter's read-only preflight (with the game, so the plan's start step is the real one) and remembers it with the time, for the start that follows. Answers the plan (without the close warning), the preview-mode flag, whether the launcher runs, the running game, `alreadyActive`, `needsConfirmation` (running and not already active), `closeMessage` (only then), and `blocked` (the plan's first warning or “This switch cannot run.” when the plan is not executable) (`src/main/services/launch.ts:149-163`).
- Errors: `NOT_IMPLEMENTED`, `NOT_INSTALLED`, `LAUNCHER_FILE`, `ADAPTER_FAILED`.

#### `launch:start`

- `accountId` ≤64; `gameId` string ≤64, `null` or absent; `confirmClose` and `instant` are true only when exactly boolean `true` (`src/main/ipc/index.ts:489-494`).
- Refusals, in order (`src/main/services/launch.ts:170-189`):
  1. a job is running or starting, or the app is quitting: `BUSY`;
  2. preview mode is on: `DRY_RUN`;
  3. the account must exist, the game must exist and match the launcher;
  4. when `confirmClose` is false: the preflight the dialog took for this account is reused if it is at most 10 s old (and is consumed: each preflight answers one start), else a new one is taken. If a game is named and the reused preflight says already active and running but is older than 2 s, a fresh preflight is taken anyway. If the launcher is running and not already active: `CONFIRM_REQUIRED` with the adapter's close sentence and the running game. If quitting began meanwhile: `BUSY`.
- The job is created (section 3.2, `LaunchJob`) and returned at once; the work continues in the background. `direct` is true when a game is named, `confirmClose` was false and the preflight said already active and running: the plan's steps go on the job as `skipped` and the job goes straight to the game launch with nothing written, snapshotted or journaled (`src/main/services/launch.ts:191-217`).
- Logs `launch job started` and `launch job accepted` (job id, account id, game id, flags, direct) (`src/main/services/launch.ts:215`, `src/main/ipc/index.ts:496`).
- Background run (`src/main/services/launch.ts:289-427`):
  - direct: the launch step runs, the adapter launches the game, the step's detail becomes “Sent <command>”, `handOff` is recorded, `lastUsedAt` is touched, the job ends `done`;
  - otherwise the journal is begun, then the adapter switch runs with progress mirrored into the job's steps (the running step id goes to the journal), the snapshot id recorded (job and journal) the moment it exists, attention mirrored, and the password decrypted only if the adapter asks;
  - an adapter throw before a snapshot clears the journal and fails the job; a throw after a snapshot leaves the journal for the next start;
  - when the switch settles: a result with `restored: false` marks the journal `rollbackFailed`, anything else clears it (before the game launch, so a crash during the launch never offers to undo a finished switch); steps, attention (`null`), `backupId`, `restored` are copied;
  - a switch that is not `ok` ends the job `cancelled`, `needs-sign-in` or `failed` with the result's error (or “The switch failed”), and the launch step `skipped`;
  - `ok` sets `switched`; with a game, the launch step waits until the result's `readyAt` if that is in the future; an abort by then ends the job `cancelled` with “Stopped before launching <game>.”; otherwise the game is launched (`startedBySwitch` when the result's `startedWithGame` is this game), detail “Sent <command>”, `handOff` recorded;
  - `lastUsedAt` is touched, the job ends `done`;
  - any throw: attention cleared, error text recorded, a running launch step marked `failed` (a pending one `skipped`), job `failed`; refusals log `launch job refused` at WARN, others `launch job failed` at ERROR;
  - always: `finishedAt` set, per-step durations logged (`launch step finished`), a `launch_history` row written when a game was named (`ok`, `failed` or `cancelled`), the revision bumped, the job pushed onto the 20-deep finished list, `launch job finished` logged, idle waiters released, and main's finished hook run (which may hide the window after an instant direct job that ended `done` with no `handOff`, when the tray is present and both `window.closeToTray` and `play.hideToTray` are on) (`src/main/index.ts:588-603`, `src/main/window-guard.ts:38-48`).
- Errors returned by the call: `BUSY`, `DRY_RUN`, `BAD_REQUEST`, `NOT_FOUND`, `CONFIRM_REQUIRED`, `NOT_IMPLEMENTED`, `NOT_INSTALLED`, `ADAPTER_FAILED`.

#### `launch:status`

- `jobId` ≤64. Answers a copy of the running job or of one of the last 20 finished jobs; `NOT_FOUND` “No launch job <id>” otherwise (`src/main/ipc/index.ts:500-504`, `src/main/services/launch.ts:435-449`).

#### `launch:cancel`

- `jobId` ≤64 must be known (`NOT_FOUND`). If it is the running job, its abort signal fires and `launch job cancel requested` is logged; whatever wait the adapter is in ends. Before the launcher has been started again the adapter stops and puts back what it wrote; after that the writes stand and the launcher finishes signing in on its own; a rollback is never interrupted. Finished jobs are returned unchanged. Answers the job as it reads at that moment (the state may still be `running`) (`src/main/ipc/index.ts:506-510`, `src/main/services/launch.ts:270-278`, `src/main/launchers/types.ts:83-89`).

#### `launch:recent`

- `limit`: a positive integer is capped at 50; anything else (missing, zero, negative, non-integer, non-number) becomes 10. Never `BAD_REQUEST` for the limit (`src/main/ipc/index.ts:512-517`).
- Answers successful (game, account) pairs, newest first, one per pair, only for games still installed and accounts still present (`src/main/services/launch.ts:124-139`).

#### `recovery:get`

- Answers the interrupted switch loaded at startup, or `null`. The answer carries the account's display name as it is now (`null` when deleted), whether the snapshot folder still exists, and the phase (`src/main/ipc/index.ts:522`, `src/main/services/switch-journal.ts:184-196`).
- Only a journal left by a previous run is ever offered: the service loads once at startup, before any job can start; an entry without a snapshot id is dropped at load (nothing was written) with an INFO log, and the file removed (`src/main/services/switch-journal.ts:147-182`).

#### `recovery:restore`

- Requires idle. `NOT_FOUND` “There is no interrupted switch to recover” when nothing is pending. Otherwise the journal's snapshot is restored through the same adapter path as `backups:restore` (so `LAUNCHER_RUNNING` while the launcher runs, and the prompt stays pending). On success logs `interrupted switch recovered from its snapshot` at WARN, forgets the pending entry and removes the journal file. Answers `{ result }` (`src/main/ipc/index.ts:524-528`, `src/main/services/switch-journal.ts:198-206`).

#### `recovery:dismiss`

- Forgets the pending entry (if any) and removes the journal file (only when the file still belongs to that job); logs `interrupted switch left as it is`. Answers `{ ok: true }` even when nothing was pending (`src/main/ipc/index.ts:530-533`, `src/main/services/switch-journal.ts:88-100,208-213`).

#### `autostart:status`

- Answers `{ available, openAtLogin }`. Availability is worked out once per run: packaged build, default data folder, an uninstaller beside the exe, and a per-user install location equal to the exe's folder; otherwise `{ available: false, openAtLogin: false }`. When available, `openAtLogin` is what Windows reports for the quoted exe path (`src/main/ipc/index.ts:549`, `src/main/platform/login-item.ts:30-35,44-46`, `src/main/updates/availability.ts:30-39`). **[dev-only]** A fake-launcher run uses an in-memory login item that is always available (`src/main/platform/login-item.ts:49-60`).

#### `autostart:set`

- `openAtLogin` must be a boolean (“openAtLogin must be a boolean”). When the status says unavailable: `UNAVAILABLE` “Start with Windows is not available for this copy of One Launch”. Otherwise the login item `OneLaunch` (exe plus `--start-hidden`) is registered or removed and the new status answered; logs `autostart updated` (`src/main/ipc/index.ts:551-559`, `src/main/platform/login-item.ts:5-7,37-42`).

#### `updates:status`

- Answers a copy of the updater's current state (section 3.1, update status) (`src/main/ipc/index.ts:537`, `src/main/updates/update-service.ts:91-93`).

#### `updates:check`

- Starts a manual check unless the service is stopped, unavailable, already holds a verified download, or a check is already running; in those cases nothing starts. Answers the status at once (usually `checking`); the renderer polls (`src/main/ipc/index.ts:539`, `src/main/updates/update-service.ts:101-105`).
- A manual check that finds a newer version verifies its manifest signature and downloads it even when automatic updates are off; only background checks stop at `idle` when the setting is off (`src/main/updates/update-service.ts:232-235`).

#### `updates:install`

- Requires idle. `NOT_READY` “No verified update has been downloaded yet” without a verified download. Otherwise the install is requested, `update install requested` logged, and the app quits; the installer runs at quit (re-hashing the file first) and relaunches the app (`src/main/ipc/index.ts:541-545`, `src/main/updates/update-service.ts:121-126,143-172`).

### 5.4 The launcher gateway (`LauncherService`)

Every call into a launcher adapter goes through one object that owns the four adapters (`src/main/services/launchers.ts:93-106`).

- **Implemented launchers.** `implemented()` is the launcher ids, in canonical order, that have an adapter; the library scan iterates it (`src/main/services/launchers.ts:112-114`). A call for a launcher without an adapter fails with `NOT_IMPLEMENTED` “The <Launcher> adapter is not available yet” (`:41-47,116-120`). In the shipped wiring all four have adapters (`src/main/index.ts:581`).
- **Guard and tracking.** Each operation runs inside `guard(launcher, operation, fn)`: the in-flight counter goes up, the adapter runs, and any throw is rewrapped as an `AdapterError` whose message is “<Launcher display name>: <reason>” and whose code is the inner error's string code or `ADAPTER_FAILED`. Logging by outcome: `NOT_INSTALLED` at DEBUG (`launcher not installed`); a refusal code at WARN (`adapter call refused`); anything else at ERROR (`adapter call failed`), or at DEBUG once quitting has begun (`… after quit began`), because the database closes under calls still in flight (`:26-39,122-135,176-179`).
- **Idle for quit.** `closing()` marks quitting; `isBusy()` is "any adapter call in flight"; `whenIdle(timeoutMs)` resolves true when the count reaches zero or false when the wait runs out. At quit main waits up to 1500 ms (`:137-174`, `src/main/index.ts:144,625-628`).
- **`status()` isolation.** Does not use `guard`. Every launcher's pass is tracked and try/caught on its own; a throw becomes `attention: { message }` on that row and `adapter status failed` is logged (ERROR, or DEBUG when quitting). Over 2000 ms logs `launcher status was slow` (`:181-213`).
- **`installFromExe`, `installedGames`, `runningGame`, `launchGame`, `restoreBackup`**: direct pass-throughs under `guard` (`:215-221,299-305,352-358`).
- **`rememberedAccounts`** attaches `linkedAccountId` by launcher uid, then login (`:223-232`); **`rememberedKey`** is the launcher uid or, when empty, the login (`:234-237`).
- **`importAccounts`** rules: section 5.3 `launchers:importAccounts` (`:239-279`).
- **`switchPlan(accountId)`** requires the account (`NOT_FOUND`) and runs `switchAccount` with `dryRun: true` under `guard` (`:281-285`).
- **`preflight(account, forGame?)`** and **`switch(account, opts)`** run the adapter's preflight and real switch under `guard`; `switch` always passes `dryRun: false`. Mid-switch failures come back as results, not throws; refusals before anything is touched throw (`:287-297`).
- **`gameIcon`** answers `null` for an adapter without the optional method (`:307-313`).
- **Sessions.** `session(accountId)`: account required; snapshot read; for an adapter with session state, the state is read under `guard` and a failure is folded into `stateError`. `captureSession`: adapter must have the method (`NOT_IMPLEMENTED`); the result's snapshot is returned with the account re-read (its `hasSession` and `launcherUid` may have changed); logs `session captured`. `forgetSession`: account required; deletes the row; logs only when one existed. Without a session store these throw a plain error “Session snapshots are not available” (`:315-350`).
- **Account directory for adapters.** Adapters see accounts through `byLauncherUid`, `byId` (only if the account is this launcher's) and `setLauncherUid`, which runs the normal account update so a duplicate uid fails closed (`:68-86`).

### 5.5 Launcher adapters

The four adapters behind `LauncherService`. Each follows the pipeline in `docs/switching.md` (section 6 gives the job lifecycle); this subsection is what each one does inside it: the launcher-owned files and registry values it reads and writes (the app-owned files are in section 4), detection, the library scan, running state, remembered accounts, the plan, the switch step by step with every timing budget, cancellation, rollback and restore, the game launch, health checks, session capture, error codes and log parsing. Timing constants are the adapter's production defaults; the **[dev-only]** fake platform overrides them with `SIM_TIMING` (Appendix A.2).

#### Adapter: Steam

Launcher id `steam`, display name "Steam" (`src/main/launchers/steam/adapter.ts:248-249`). Steam keeps its own account list, so this adapter never stores or reads a Steam session: it tells Steam which remembered account to sign in as, or starts Steam on its sign-in form. There is no `sessionState` and no `captureSession` (`src/main/launchers/types.ts:186-195`; neither is implemented in `adapter.ts`).

Shared helpers used throughout: `readLauncherText` refuses a file larger than a cap before reading it (`LAUNCHER_FILE`, "Too large to be a launcher file (<n> bytes)"), and Steam's VDF files use the tighter 1 MiB cap `MAX_VDF_BYTES` (`src/main/launchers/read-file.ts:4-23`). Registry reads go through `reg.exe` under code page 65001 so non-ASCII values survive; exit code 1 means the key or value is absent and reads as null (`src/main/platform/registry.ts:93-96`, `registry.ts:206-216`). Registry writes are `reg add ... /f` and `reg delete ... /f` (`registry.ts:268-303`). Process checks read the kernel process table by image name through PowerShell, never WMI; an empty process list is an error, not "nothing runs" (`src/main/platform/processes.ts:526-532`). A forced close kills the image name and, by default, its child tree (`processes.ts:551-557`). A launcher Windows refuses to close or kill raises `PROCESS_ACCESS_DENIED`: "Windows would not let One Launch close <image>. It is most likely running as administrator: close it yourself, or start One Launch as administrator too, then try again." (`processes.ts:44-52`).

Every wait uses `waitUntil` / `sleep` from `src/main/launchers/wait.ts`: `waitUntil` polls until the condition holds or the budget passes, and throws `SwitchAbortedError` as soon as the abort signal fires (`wait.ts:41-50`). `mergeTiming` applies overrides; an override that only shortens `pollMs` also pulls `logPollMs` down to it (`wait.ts:31-39`).

##### 1. Files and registry it reads; what it writes

Reads. `<install>` is the detected Steam folder.

| Path or value | Format | What is read | Cap | On failure |
| --- | --- | --- | --- | --- |
| `HKCU\Software\Valve\Steam\SteamPath` (REG_SZ) | forward-slash, lowercase path | detection candidate 2 (`adapter.ts:277`) | n/a | absent = skipped |
| `HKCU\Software\Valve\Steam\SteamExe` (REG_SZ) | path | refines `exePath` when the file exists (`adapter.ts:279-283`) | n/a | absent or missing file = `<install>\steam.exe` kept |
| `HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath` (REG_SZ) | path | detection candidate 3 (`adapter.ts:286`) | n/a | absent = skipped |
| `HKCU\Software\Valve\Steam\AutoLoginUser` (REG_SZ) | login name | plan, health, snapshot meta (`adapter.ts:509`, `1029`, `1153`, `1253`) | n/a | absent = null |
| `HKCU\Software\Valve\Steam\RememberPassword` (REG_DWORD) | 0/1 | plan, snapshot meta (`adapter.ts:510`, `1039`, `1154`) | n/a | absent = null |
| `HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser` (REG_DWORD) | SteamID3 account id (SteamID64 minus 76561197960265728) of the signed-in account; not cleared when Steam exits | sign-in confirmation and already-active check (`adapter.ts:756-758`, `109-114`) | n/a | absent = null |
| `HKCU\Software\Valve\Steam\RunningAppID` (REG_DWORD) | app id, 0 when idle | running game (`adapter.ts:428`) | n/a | absent = null |
| `HKCU\Software\Valve\Steam\Apps\<appid>\Name` (REG_SZ) | game name | name of the running game (`adapter.ts:430`) | n/a | absent = "Steam app <appid>" |
| `<install>\config\loginusers.vdf` | Valve KeyValues text ("VDF"): `"users" { "<steamid64>" { "AccountName" "..." "PersonaName" "..." "RememberPassword" "1" "WantsOfflineMode" "0" "SkipOfflineModeWarning" "0" "AutoLogin" "1" "Timestamp" "<unix s>" } ... }`; current clients write `AutoLogin`, older ones `MostRecent`, treated as synonyms (`loginusers.ts:1-17`) | remembered accounts, target lookup, rewrite planning | 1 MiB | missing: `LAUNCHER_FILE` "loginusers.vdf not found" for the account list (`adapter.ts:389`), a plan warning in a switch plan (`adapter.ts:525`); unparsable: `LAUNCHER_FILE` "Could not parse loginusers.vdf (<reason>)" for the account list (`adapter.ts:393`), a plan warning in a switch plan (`adapter.ts:522`); too large: `LAUNCHER_FILE` thrown out of the plan (`adapter.ts:517` is outside the try) |
| `<install>\steamapps\libraryfolders.vdf` | VDF `"libraryfolders" { "0" { "path" "D:\\Games\\Steam" ... } }` (block form) or `"1" "D:\\Games\\Steam"` (pre-2021 flat form); backslashes escaped (`manifests.ts:17-37`) | extra library folders | 1 MiB | missing: main folder only, no warning (`adapter.ts:324`); unparsable or too large: warning logged, main folder only, health `libraries` row fails (`adapter.ts:332-335`, `1281-1283`) |
| `<library>\steamapps\appmanifest_<appid>.acf` | VDF `"AppState" { "appid" "name" "StateFlags" "installdir" "LastPlayed" "SizeOnDisk" "buildid" "LastOwner" ... }` (`manifests.ts:55-77`) | installed games | 1 MiB each | unreadable or unparsable: that file skipped with a warning log (`adapter.ts:364-367`) |
| `<install>\logs\steamui_login.txt` | plain text, `[<time>] [ <phase> ] SetLoginState: <State> - <reason>` (`login-log.ts:1-12`) | sign-in outcome | tail of 512 KiB per read | missing or unreadable = empty text (`login-log.ts:80-88`) |
| `<install>\logs\bootstrap_log.txt` | plain text updater log (`bootstrap-log.ts:1-32`) | self-update detection during the sign-in wait | tail of 512 KiB per read | missing = empty text |
| Windows process list | image names (and paths for the running-game fallback) | `isRunning`, `runningGame`, every wait | n/a | a failed listing throws; `runningGame` logs a warning and answers null (`adapter.ts:434-440`) |

Never read: `%LOCALAPPDATA%\Steam\local.vdf` (the token store) and manifest `LastOwner` (parsed over but not surfaced) (`docs/adapters/steam.md:7`, `manifests.ts:55-58`). `parseLoginUsers` registers every account and persona name with the logger's redaction list, so they appear as `[ACCOUNT]` in log files (`loginusers.ts:64-65`).

Writes (only inside `runSwitch` and `applyRestore`):

| Target | What changes | How |
| --- | --- | --- |
| `<install>\config\loginusers.vdf` | every `AutoLogin` or `MostRecent` line set to `"1"` in the target's block and `"0"` in every other block; a block with neither key gets an `"AutoLogin"` line inserted before its closing brace, using the indentation and key/value separator of the last key seen in that block (default two tabs each) (`loginusers.ts:106-177`) | whole text rewritten line by line; the original line ending (CRLF if any CRLF is present, else LF), ordering, indentation and unknown keys are preserved byte for byte; written to `<file>.onelaunch-<pid>.tmp` in the same folder then renamed over the original (`src/main/services/backup.ts:110-125`); skipped when nothing would change (`adapter.ts:1020-1026`) |
| `HKCU\Software\Valve\Steam\AutoLoginUser` | REG_SZ = target `AccountName` as written in loginusers.vdf | `reg add`, only when the current value differs case-insensitively (`adapter.ts:1028-1036`) |
| `HKCU\Software\Valve\Steam\RememberPassword` | REG_DWORD = 1 | `reg add`, only when the current value is not already 1 (`adapter.ts:1038-1045`) |
| On restore: `loginusers.vdf` of the current install, and the two values above set or deleted to what the snapshot recorded | see section 9 | `backups.restore` writes the file atomically after verifying its hash; registry set or `reg delete` (`adapter.ts:1161-1187`) |

The password, when used, is never written: it is passed only as a `steam.exe` command-line argument (`adapter.ts:917-927`).

##### 2. Detection

`detectInstall` takes the first candidate whose folder contains `steam.exe` (`adapter.ts:273-289`, `300-309`):

1. The folder from the `steam.path` setting (Locate in Settings), trimmed and normalised. `source: 'user'`. If `steam.exe` is no longer in it the ladder continues and the health check warns (section 11).
2. `HKCU\Software\Valve\Steam\SteamPath`. `source: 'registry-hkcu'`. `SteamExe` from the same key replaces `exePath` when that file exists.
3. `HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath`. `source: 'registry-hklm'`.
4. `C:\Program Files (x86)\Steam`. `source: 'default-path'`.

Paths are normalised (`\\` unescaped, separators unified, trailing separator dropped) (`src/main/platform/fs-path.ts:8-18`). A missing candidate is logged at debug and skipped. `chosenInstallDir()` returns the normalised setting or null when blank (`adapter.ts:291-293`, `src/main/launchers/chosen-folder.ts:6-9`). `installFromExe(path)` accepts only a file named `steam.exe` (case-insensitive) and answers the install whose folder is the file's folder, `source: 'user'`, or null when that folder has no `steam.exe` (`adapter.ts:295-298`). Any operation that needs Steam throws `NOT_INSTALLED` ("steam is not installed on this PC", lowercase launcher id) when detection finds nothing (`adapter.ts:311-315`, `types.ts:198-204`).

A chosen folder only moves where files are looked up; the HKCU values belong to whichever Steam ran last (`docs/adapters/steam.md:24`).

##### 3. Installed games scan

`listInstalledGames` (`adapter.ts:346-383`):

- Library folders: the install folder first, then every path in `steamapps\libraryfolders.vdf` whose key is all digits, normalised and deduplicated case-insensitively against folders already seen (`adapter.ts:320-338`, `manifests.ts:21-37`). A folder with no `steamapps` subfolder is logged and skipped (`adapter.ts:354-357`).
- Files: every entry in `<library>\steamapps` matching `appmanifest_<digits>.acf` (case-insensitive) (`manifests.ts:87`).
- A manifest counts when `StateFlags` has bit 4 set (fully installed; the observed value 6 is installed plus update required), its app id has not been seen in an earlier library, and the app id is not one of the tool apps 228980 (Steamworks Common Redistributables), 1070560, 1391110, 1628350 (Steam Linux Runtimes) (`adapter.ts:62-67`, `368`; `manifests.ts:39-41`, `79-81`).
- Fields: `launcherGameId` = `appid` (digits only, else the manifest is rejected); `name` = `name` trimmed, or "App <appid>" when absent; `installPath` = `<library>\steamapps\common\<installdir>` resolved, and clamped to `<library>\steamapps\common` itself if the resolved path would escape that folder; `libraryPath` = the library folder; `sizeOnDiskBytes` = `SizeOnDisk` as an integer or null; `lastPlayedAt` = `LastPlayed` (unix seconds, > 0) as ISO or null; `needsUpdate` = `StateFlags` bit 2 (`adapter.ts:340-344`, `370-379`; `manifests.ts:59-85`).
- Rejected manifests (no `AppState`, non-numeric `appid`, no `installdir`, unparsable VDF, over 1 MiB) are skipped with a warning log (`adapter.ts:361-367`).
- Result sorted by name, case-insensitive (`adapter.ts:382`).

A game whose update Steam has deferred gets the update bit only once Steam is running and has asked its backend, so a scan taken while Steam is closed cannot show the badge (`docs/adapters/steam.md:101`).

##### 4. Running state

- `isRunning`: the image name `steam.exe` is in the process list (`adapter.ts:415-417`).
- `runningGame` (`adapter.ts:423-444`): null when Steam is not installed. If `HKCU\Software\Valve\Steam\RunningAppID` is > 0: `{ launcherGameId: "<appid>", name: Apps\<appid>\Name or "Steam app <appid>", source: 'launcher' }`. Otherwise the process list is taken (a failure is logged and answers null; an empty list answers null) and the first process whose executable path lies under an installed game's `installPath` (as recorded, or with junctions and subst drives resolved; drive-letter paths only; case-insensitive prefix with a trailing separator) answers `source: 'process'` (`src/main/launchers/running-game.ts:6-37`).

##### 5. Remembered accounts

`listRememberedAccounts` (`adapter.ts:397-411`) throws `NOT_INSTALLED` or `LAUNCHER_FILE` (missing or unparsable `loginusers.vdf`), else one entry per block of `loginusers.vdf` whose key matches `^\d{15,20}$` and has a non-blank `AccountName` (other blocks are skipped, never guessed at) (`loginusers.ts:14`, `47-63`):

| `RememberedAccount` field | Source |
| --- | --- |
| `launcherUid` | the block key (SteamID64) |
| `login` | `AccountName`, trimmed |
| `personaName` | `PersonaName`, trimmed, or "" |
| `rememberPassword` | `RememberPassword` is `"1"` |
| `mostRecent` | `AutoLogin` or `MostRecent` is `"1"` |
| `lastLoginAt` | `Timestamp` (unix seconds) as ISO, or null when absent or non-numeric |
| `sessionMayBeExpired` | true when `Timestamp` is absent, or the whole days since it are >= 60 (`STALE_SESSION_DAYS`, `adapter.ts:107`, `116-118`, `408`) |
| `linkedAccountId` | added outside the adapter: the One Launch Steam account with this `launcherUid`, else the one with this login (`src/main/services/launchers.ts:225-231`) |

Sorted most-recent first, then by `lastLoginAt` descending (`adapter.ts:410`). Every entry has a login, so every entry can be imported; the adapter never sets `launcherUid` on a hand-made account, the plan only warns on a mismatch (section 6). What cannot be imported: the session itself (Steam's token store is never read) and the password.

##### 6. Preflight and plan

`preflight(account)` ignores the `forGame` argument (its signature takes only the account, `adapter.ts:482`). It builds the plan, then: `launcherRunning` = `steam.exe` running; `runningGame` = section 4's answer, asked only when Steam runs and is not already active, else null; `alreadyActive` from the plan context; `closeMessage` = "Steam is running <game>. Switching closes Steam and the game; unsaved progress in the game is lost." when a game was found, else "Steam is running. Switching closes it." (`adapter.ts:476-492`). The plan handed to preflight has the close warning (below) removed; the dry-run plan keeps it (`types.ts:284-286`).

`buildSwitchPlan(account)` (`adapter.ts:503-754`) reads: install (throws `NOT_INSTALLED`), running state, `AutoLoginUser`, `RememberPassword`, and `loginusers.vdf`. Derived values:

- `target`: the block whose SteamID64 equals the account's `launcherUid`, else the block whose `AccountName` equals the account's login case-insensitively, else null (`loginusers.ts:69-76`).
- `strategy`: `remembered` when the target exists and has `RememberPassword=1`, else `credentials` (`adapter.ts:530`).
- `withPassword`: the `steam.autoLogin` setting is on (default off, `src/main/index.ts:514`) and the account has a stored password (`adapter.ts:531`; the real switch also needs a `getPassword` callback, `adapter.ts:787`).
- `rewrite`: the planned `loginusers.vdf` rewrite (remembered only); `noop` when the target is already the only most-recent account.
- `sameUser`: `AutoLoginUser` equals the target's `AccountName` case-insensitively.
- `pointsAtTarget` = rewrite is a no-op and `sameUser`.
- `signedInElsewhere` = `pointsAtTarget` and Steam runs and `signedInAs(target)` is false, where `signedInAs` = `ActiveUser` equals the target's SteamID3 and the whole login log's last decisive line is not `WaitingForCredentials` (`adapter.ts:764-767`).
- `alreadyActive` = `pointsAtTarget` and not `signedInElsewhere` (`adapter.ts:540`). Note: `alreadyActive` can be true with Steam closed; the real switch then only starts Steam.
- `stale` = remembered strategy and (no `Timestamp` or age >= 60 days).
- `executable` is always true (`adapter.ts:750`).
- `plan.account.launcherUid` = the target's SteamID64 when found, else the account's own; `plan.current` = `{ mostRecentLogin: the most-recent block's AccountName or null, autoLoginUser, running }` (`adapter.ts:744-746`).

Plan steps, in order. "Skipped" is the plan's `skipped` flag; the runtime detail strings are in section 7.

Remembered strategy:

| id | kind | title | target | skipped when | detail |
| --- | --- | --- | --- | --- | --- |
| `close` | process | "Close Steam" | `steam.exe` | not running, or already active | already active: "Skipped: Steam is already signed in as this account, so it is left running."; running: "Run \"<exePath>\" -shutdown and wait up to 30 s for steam.exe to exit. If it is still running after that, force-close steam.exe and every process it started. Any game running under Steam closes with it."; else "Skipped: steam.exe is not running." (`adapter.ts:551-562`) |
| `backup` | backup | "Back up Steam files" | `<dataDir>\backups\steam\<timestamp>` | already active | "Skipped: nothing will be modified." or "Copy <loginusers path> into a new snapshot folder and record the current HKCU\Software\Valve\Steam\AutoLoginUser and RememberPassword values in its manifest.json, so a failed switch can be rolled back. The newest <backups.keep> snapshots are kept." (`adapter.ts:564-573`) |
| `file` | file | "Mark the account as most recent" | loginusers.vdf path | rewrite is a no-op | no-op: "No change: <AccountName> is already the most recent account in loginusers.vdf."; else "Set <AutoLogin and/or MostRecent, or 'AutoLogin (added, because the block has neither AutoLogin nor MostRecent)'> to 1 for <AccountName> (<steamid64>) and to 0 for the other <n> remembered account(s). The new content is written to a temp file and renamed over the original. Nothing else in the file changes."; `changes` = one `{ where: "line <n>", before, after }` per changed line, trimmed (`adapter.ts:575-589`) |
| `registry-user` | registry | "Point Steam at the account" | `HKCU\Software\Valve\Steam\AutoLoginUser` | `sameUser` | "No change: AutoLoginUser is already <AccountName>." or "REG_SZ. Steam reads this at startup to decide which remembered account to sign in as."; change `{ where: "AutoLoginUser", before: current or "(absent)", after: AccountName }` (`adapter.ts:591-601`) |
| `registry-remember` | registry | "Keep the session remembered" | `HKCU\Software\Valve\Steam\RememberPassword` | value already 1, or already active | "No change: RememberPassword is already 1." / "Skipped: no switch is needed, so the value is left as it is." / "REG_DWORD set to 1. On this Steam build the value is normally absent and the per-account RememberPassword in loginusers.vdf does the work; writing it is harmless and matches the documented procedure."; change `{ where: "RememberPassword", before: value or "(absent)", after: "1" }` (`adapter.ts:603-617`) |
| `start` | launch | "Start Steam" | `"<exePath>"` | already active and running | "Skipped: Steam is already running as this account." or "Start Steam normally. It signs in as <AccountName> without prompting because the session is remembered." (`adapter.ts:619-629`) |
| `wait` | wait | "Wait for Steam to come up" | `steam.exe` | already active and running | "Poll the process list until steam.exe is running, up to 60 s." (`adapter.ts:654-661`) |
| `verify` | wait | "Confirm Steam signed in" | `HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser` | already active and running | skipped: "Skipped: Steam is already signed in as this account."; else "Poll until Steam reports the signed-in account id <SteamID3> (the SteamID3 of <AccountName>), up to 60 s. Meanwhile <login log path> is read (Steam's own sign-in state log, no credentials in it) to learn at once whether Steam is instead asking for a password or a Steam Guard code, and <bootstrap log path> (its updater log) to see whether Steam is updating itself first; then the wait starts over when the update ends, up to 15 min in all. A game launch (steam://rungameid/<appid>) happens only after the sign-in is confirmed." (`adapter.ts:663-677`) |
| `login` | launch | "Fallback: sign in with the stored password" | `"<exePath>" -login <login> <password>` | always | `withPassword`: "Runs only if Steam asks for credentials instead of signing in (an expired session): close Steam, start it again with -login using the password from the vault, and confirm the sign-in again."; password stored but setting off: "Not used: Steam automatic sign-in is off in Settings. If Steam asks for credentials, One Launch waits for you to sign in by hand (next step) and offers to copy the login and password."; no password: "Not available: no password is stored for this account. If Steam asks for credentials, One Launch waits for you to sign in by hand (next step)." (`adapter.ts:679-692`) |
| `manual` | wait | "Wait for you to sign in" | `...\ActiveProcess\ActiveUser` | always | "Runs only if Steam still asks for something One Launch cannot supply: a Steam Guard code for a PC this account has not used before, an expired session with no stored password, or a session Steam simply refuses. The Steam window stays open on its sign-in form; you sign in there once with \"Remember me\" ticked<' (the password can be copied from One Launch)' when a password is stored> and One Launch continues with the game as soon as Steam reports the sign-in, up to 10 min. A wrong stored password ends the switch instead." (`adapter.ts:694-703`) |

Credentials strategy: `close` and `backup` as above (backup is never skipped here since `alreadyActive` needs a target), then:

| id | kind | title | target | skipped | detail |
| --- | --- | --- | --- | --- | --- |
| `login` | launch | "Start Steam with credentials" (`withPassword`) or "Start Steam on its sign-in form" | `"<exePath>" -login <login> <password>` or `"<exePath>" -login <login>` | never | `<why>` is "Steam remembers <AccountName> but \"Remember me\" is off for it" when the target exists, else "Steam has never signed in as <login> on this PC". With password: "<why>, so Steam is started with -login. The password is decrypted from the vault at that moment and passed only as a process argument; it is never written anywhere by One Launch, but it is visible on the command line to other processes on this PC while Steam runs. Afterwards Steam adds the account to loginusers.vdf and later switches use the remembered path."; without: "<why>, so Steam is started with -login and only the account name, which makes it open its sign-in form instead of signing in as the previous account. You enter the password there; nothing secret goes on the command line. Afterwards Steam adds the account to loginusers.vdf and later switches use the remembered path." (`adapter.ts:631-640`) |
| `wait` | wait | as above | | already active and running (never, here) | as above |
| `verify` | wait | as above | | never | with a target (Remember me off): the SteamID3 form with "up to 120 s"; with no target: "Poll until Steam reports a signed-in account, up to 120 s. Meanwhile ..." (`adapter.ts:672-676`) |
| `manual` | wait | as above | | always (in the plan; the real switch runs it) | as above |

Warnings (the dialog stops on these) in the order they are added (`adapter.ts:522-525`, `641-651`, `705-740`):

- "loginusers.vdf could not be parsed (<reason>). Steam is treated as remembering no accounts."
- "loginusers.vdf does not exist yet. Steam has not remembered any account on this PC."
- Credentials with password: "If this PC has never signed in as <login>, Steam Guard will ask for an emailed code once. One Launch then waits up to 10 min while you sign in in the Steam window, and continues on its own."
- Credentials without password: "Steam will open its sign-in form for <login>. One Launch waits up to 10 min while you sign in there with \"Remember me\" ticked and offers to copy the login and password; later switches are automatic." (or "... and offers to copy the login; ..." when no password is stored).
- Close warning (removed from the preflight plan, kept in the dry-run plan). `signedInElsewhere`: "Steam's files already name <AccountName>, but the Steam that is running is signed in as <other AccountName>. It will be closed, along with any game running under it, and started again as <AccountName>." (or "... is not signed in as <AccountName>. ..." when `ActiveUser` matches no other block). Otherwise, when running and not already active: "Steam is running. It will be closed, along with any game running under it."
- Stale: "<AccountName> last signed in here <n days | an unknown time> ago. Steam sessions expire, so Steam may ask for credentials instead of signing in. One Launch will then sign in with the stored password." (or "... One Launch will then wait while you sign in in the Steam window, and continues on its own.").
- Uid mismatch: "The SteamID64 stored on this account (<uid>) does not match Steam's entry for <AccountName> (<steamid64>). Matched by login instead; check the ID on the account page."
- "Registry AutoLoginUser (<value>) and the most recent account in loginusers.vdf (<AccountName>) disagree. The switch sets both, which resolves it."

Notes (never a reason to stop): when already active, "Steam is already set to sign in as <AccountName>. The real switch would change nothing and would leave Steam running." (the "and would leave Steam running" part only when Steam runs) (`adapter.ts:706-709`).

##### 7. Switch (`runSwitch`)

`switchAccount` with `dryRun: true` returns the plan and changes nothing; with `dryRun: false` it runs the switch (`adapter.ts:494-500`). The switch starts by building the plan again (`adapter.ts:784`). Refusals, thrown before anything is touched: `SWITCH_BLOCKED` with the first warning when `executable` is false (never for Steam, `adapter.ts:789`); `CONFIRM_REQUIRED` with the `closeMessage` and the running game when Steam runs, is not already active and `confirmClose` is not set (`adapter.ts:790-793`). Job steps start as the plan's steps: skipped ones keep their plan detail, others are `pending` with an empty detail; every status change calls `onProgress` with a copy of the full list (`adapter.ts:795-809`). `readyAt` and `startedWithGame` are always null for Steam (`adapter.ts:818-819`). `login` for messages is the target's `AccountName` when found, else the account's login (`adapter.ts:786`).

Timing constants (`DEFAULT_TIMING`, `adapter.ts:83-100`; overridable through `deps.timing`):

| Name | Default | Used for |
| --- | --- | --- |
| `pollMs` | 500 ms | process-list polls (shutdown, kill grace, startup) |
| `logPollMs` | 250 ms | polls that read the registry and logs (sign-in wait, library-ready grace) |
| `shutdownMs` | 30 s | wait for `steam.exe` to exit after `-shutdown` |
| `killGraceMs` | 5 s | wait after the forced kill |
| `startupMs` | 60 s | wait for `steam.exe` to appear after a start |
| `signInMs` | 60 s | sign-in budget after a plain start (remembered) |
| `guardMs` | 120 s | sign-in budget after a `-login` start |
| `updateMs` | 15 min | ceiling of the whole automatic wait while Steam updates itself |
| `manualSignInMs` | 10 min | hand-over wait |
| `readyGraceMs` | 5 s | wait for the log's `Success` after `ActiveUser` confirmed the account |

**[dev-only]** The fake platform overrides these with `SIM_TIMING`: `pollMs` 100, `logPollMs` 100, `shutdownMs` 3 s, `killGraceMs` 1 s, `startupMs` 5 s, `signInMs` 8 s, `guardMs` 8 s, `updateMs` 20 s, `manualSignInMs` 30 s, `readyGraceMs` 300 ms (plus keys other adapters use) (`src/main/launchers/sim/scenario.ts:22-40`, `280`).

Sequence (`adapter.ts:1001-1107`):

1. Abort check (`stopIfAborted`).
2. **Close** (only when Steam runs and is not already active): step `close` running; `shutdown({ signal })` (`adapter.ts:451-463`): if `steam.exe` is not running, nothing; else spawn `"<exePath>" -shutdown` with the install folder as working directory, poll every `pollMs` up to `shutdownMs` (30 s) for `steam.exe` to be gone; if still running, log a warning and force-kill `steam.exe` with its process tree, then poll up to `killGraceMs` (5 s); if still running throw "steam.exe is still running after -shutdown and a forced close". An elevated Steam makes the kill raise `PROCESS_ACCESS_DENIED`. There is no restart watch for Steam. On success: detail "Steam closed.".
3. Abort check. **Started-meanwhile check**: if not already active and `steam.exe` is running now, fail with "Steam was started by something else while One Launch was switching accounts, so the switch stopped before changing anything. Try again." (the `close` step is marked failed) (`adapter.ts:1010`, `869-874`).
4. **Remembered strategy with a target** (`adapter.ts:1012-1083`):
   - If not already active: **snapshot** (step `backup`): one file, `loginusers.vdf` as `config/loginusers.vdf`, not sensitive (stored in the clear), with meta `{ AutoLoginUser, RememberPassword }` as read at plan time (null when absent); reason "Before switching to <displayName>"; never pinned; detail = the snapshot folder path (`adapter.ts:833-843`). Then the **writes**: the file is re-read fresh (Steam has just exited and may have rewritten it; this read uses no size cap, `adapter.ts:1017`), the rewrite planned again, step `file` running; no-op → `skipped` "Already marked as most recent."; else written atomically, detail "<n> line(s) changed.". Step `registry-user`: `AutoLoginUser` re-read; equal (case-insensitive) → `skipped` "AutoLoginUser is already <AccountName>."; else set, detail "AutoLoginUser = <AccountName>". Step `registry-remember`: re-read; 1 → `skipped` "RememberPassword is already 1."; else set to 1, detail "RememberPassword = 1". `onBackup(backupId)` fires once, right before the first of these writes, so a switch that writes nothing never puts a snapshot id in the journal and leaves no recovery prompt (`adapter.ts:845-848`, `docs/switching.md:10`).
   - `previous` = `ActiveUser` now (0 when absent). **Post-write running check**: if `steam.exe` runs now and the switch is not already active → "Steam was started by something else while One Launch was switching accounts, so the switch was stopped. Try again." (step `start` marked failed; rolled back below). If already active and running: `start` and `wait` get `skipped` "Steam is already running.". Else **start**: abort check, log marks taken, `"<exePath>"` spawned with no arguments and the install folder as working directory, detail "Steam started."; then step `wait` polls every `pollMs` up to `startupMs` (60 s) for `steam.exe`, failing with "steam.exe did not appear within 60 s"; detail "steam.exe is running." (`adapter.ts:849-867`, `1048-1061`).
   - If Steam was not started by this switch and `signedInAs(target)` holds: `verify` → `skipped` "Steam is already signed in as this account.". Otherwise `confirmSignIn(previous, signInMs)` (section "Sign-in wait"). Outcomes: `cancelled` → cancelled result. Not signed in and `retryWithPassword` (= `withPassword` and Steam was started by this switch and the cause is not one of `guard-email`, `guard-mobile`, `rate-limit` and it is not a timeout while Steam updates) → `startSignIn()` (step `login`: fetch the password through `getPassword` now; if Steam runs, `shutdown({ signal })`; `previous` = `ActiveUser`; start `"<exePath>" -login <account.login> <password>` (or without the password when `getPassword` returned null); detail "Steam started with -login." or "Steam started on its sign-in form."; wait for the process as above) then `confirmSignIn(previous, guardMs)`; `cancelled` → cancelled result; `credentials` with cause `password` → throw "Steam rejected the stored password for <login>. Update it on the account page and try again." (rolled back below). Still not signed in → `waitForManualSignIn` (section "Hand-over"): `cancelled` → cancelled result; not signed in → needs-sign-in result (`adapter.ts:1063-1083`, `1071`, `917-928`).
5. **Credentials strategy** (`adapter.ts:1084-1102`): snapshot as above (so `backupId` is set although nothing is written; `onBackup` never fires); `startSignIn()` (closes a running Steam first; `-login <account.login>` with the password only when `withPassword`); with password: `confirmSignIn(previous, guardMs)`, `cancelled` → cancelled result, `password` cause → the rejected-password error above; without password: `verify` → `skipped` "Handed over to you." and the hand-over begins at once with cause `none`. Not signed in → `waitForManualSignIn`; `cancelled` → cancelled result; timeout → needs-sign-in result. Then `verifyIdentity()`: only when the target was unknown (no SteamID3 expected): `loginusers.vdf` is re-read (no size cap, `adapter.ts:982`; a parse failure skips the check) and the block whose `AccountName` equals the login is looked up; if it exists and `ActiveUser` is a different id, throw "Steam signed in as <other AccountName | a different account> instead of <login>." (nothing was written, so no rollback) (`adapter.ts:977-991`). When the target was known (Remember me off), identity is not re-checked.
6. **Library-ready grace**: if this switch started Steam, wait up to `readyGraceMs` (5 s), polling every `logPollMs`, for a `Success` line in the login log since the last start mark; an abort here is swallowed and the result is still ok (`adapter.ts:966-972`, `1104`).
7. Result `ok: true`, `restored: null`, `backupId` (possibly non-null with nothing written) (`adapter.ts:1107`).

Sign-in wait (`waitForSignIn(previous, timeoutMs, manual)`, `adapter.ts:883-906`), polled every `logPollMs` (250 ms):

- Abort → `cancelled`.
- `v` = `ActiveUser`. The login log since the mark taken at the last start is parsed for its last decisive line unless in manual mode and `v` has changed. Signed in when `v` is non-null, non-zero and (with a known target) equals the target's SteamID3, or (unknown target) differs from `previous`; and additionally, when `v` equals `previous`, only if the log reports `Success` since the mark (`ActiveUser` survives an exit, so an unchanged value proves nothing by itself).
- Not manual: a `WaitingForCredentials` outcome ends the wait at once as `credentials` with Steam's reason and its cause (section 14).
- Not manual: the bootstrap log since its mark decides `updating`. While updating and `steam.exe` runs, the deadline moves to now + the budget, capped at the ceiling start + max(budget, `updateMs` 15 min); a dead `steam.exe` stops extending. Transitions set the `verify` detail to "Steam is updating itself first; the sign-in follows when it has finished." and "Steam finished updating itself.".
- Deadline passed → `timeout` with the `updating` flag.

`confirmSignIn` marks `verify` running, then "Steam signed in as <login>." (done), "Steam is asking for credentials: <reason | no reason given>." (failed), "Steam is still updating itself." or "Steam did not report a sign-in in time." (failed), or "Cancelled." (`adapter.ts:907-915`).

Hand-over (`waitForManualSignIn`, `adapter.ts:930-946`): `until` = now + 10 min; the attention is built by `attentionFor` (`adapter.ts:188-245`) with `canCopyPassword` = a password is stored and `getPassword` exists; step `manual` running with the attention title as detail; `onAttention(attention)`; the wait runs in manual mode for `manualSignInMs` (a `WaitingForCredentials` line is not final here); then `onAttention(null)`. Step detail: "Steam signed in as <login>." / "Stopped waiting." / "No sign-in after 10 min.". Attention texts (`kind: 'sign-in'`, `reason` = Steam's reason text or null):

| Cause | title | message | steps |
| --- | --- | --- | --- |
| timeout while updating | "Steam is still updating itself" | "Steam has been updating itself for longer than One Launch waits on its own, so it was left to finish." | ["If Steam shows its sign-in form afterwards, sign in as <login> with “Remember me” ticked. One Launch continues on its own as soon as Steam reports the sign-in, and later switches are automatic."] |
| `guard-email` | "Steam needs a one-time sign-in on this PC" | "Steam accepted the sign-in for <login>, but this PC is not yet authorized for the account, so it wants the code it just emailed you." | ["In the Steam window, enter the account name <login> and the password.", "Keep “Remember me” ticked.", "Enter the code Steam emailed you. One Launch continues on its own as soon as Steam reports the sign-in, and later switches are automatic. Later switches need none of this."] |
| `guard-mobile` | "Steam Guard needs your approval" | "Steam wants the sign-in for <login> approved." | ["Approve the sign-in in the Steam Mobile app, or enter its code in the Steam window.", "Keep “Remember me” ticked. One Launch continues on its own as soon as Steam reports the sign-in, and later switches are automatic."] |
| `rate-limit` | "Steam is rate-limiting sign-ins" | "Steam refused the sign-in for <login> because of too many recent attempts." | ["Wait a few minutes.", "Sign in as <login> in the Steam window with “Remember me” ticked. One Launch continues on its own as soon as Steam reports the sign-in, and later switches are automatic."] |
| everything else (`expired`, `password`, `none`, `other`, a plain timeout) | "Steam is asking you to sign in as <login>" | `expired`: "Its remembered session has expired."; a reason present: "Steam did not complete the sign-in on its own (<reason>)."; else "Steam is waiting on its sign-in form." | ["In the Steam window, enter the account name <login> and your password.", "Keep “Remember me” ticked.", "Sign in. If Steam Guard asks for a code, enter it. One Launch continues on its own as soon as Steam reports the sign-in, and later switches are automatic."] |

Outcomes:

- **ok**: as above.
- **needsSignIn** (`adapter.ts:957-964`): `ok: false`, `needsSignIn: true`, `restored: null`, error "One Launch waited 10 min for the sign-in as <login> and gave up. Steam is still on its sign-in form: sign in there with \"Remember me\" ticked, and later switches are automatic." Nothing is rolled back.
- **cancelled** after Steam was started (`adapter.ts:947-956`, `1115`): `restored: null`; error "Stopped waiting for the sign-in as <login>. Steam was left on its sign-in form; signing in there with \"Remember me\" ticked finishes the switch, and later switches are automatic." when the hand-over had begun, else "Stopped while Steam was starting as <login>. Nothing was rolled back: Steam carries on signing in on its own.".
- **cancelled** before any write (`adapter.ts:1116-1118`): `restored: null`, error "Stopped before Steam was started as <login>. Nothing had been changed."; the running step gets detail "Stopped.".
- **cancelled** after a write, before the start: rollback as for failed, `cancelled: true`, `restored: true`, error "Stopped before Steam was started as <login>." (`adapter.ts:1121-1126`).
- **failed** with nothing modified: `restored: null`, the error message; logged at WARN when it is an expected refusal code, else ERROR (`adapter.ts:1113-1118`).
- **failed** after a write (`adapter.ts:1119-1136`): no snapshot id → `restored: false`, "<message> (no backup was available to restore)" (cannot happen in practice: the snapshot precedes every write); else Steam is shut down if running (no signal, a failure only logged), `applyRestore(backupId)` → `restored: true` with the original message; a restore failure → `restored: false`, "<message>. Restoring backup <id> also failed: <reason>".

##### 8. Cancellation

The signal is checked before the close, after the close, and right before each spawn; every wait takes it (`adapter.ts:829-831`, `862`, `1002`, `1008`):

- During the close or before the first write: `SwitchAbortedError` → cancelled, `restored: null`, "Stopped before Steam was started as <login>. Nothing had been changed.".
- After a write and before the start (the abort check in `startSteam`): rollback, cancelled, `restored: true`.
- During the process wait, the sign-in wait, the password retry's shutdown, or the hand-over: cancelled, `restored: null`, the writes stand, Steam keeps signing in (section 7 messages).
- During the library-ready grace: swallowed; `ok: true`.
- The rollback's own `shutdown` takes no signal and cannot be interrupted (`adapter.ts:1123`).

##### 9. Rollback and `applyRestore`

- `restoreBackup(id)` (`adapter.ts:1142-1159`): requires an install; refuses with `LAUNCHER_RUNNING` "Close Steam before restoring: Steam rewrites loginusers.vdf when it exits, which would undo the restore." while `steam.exe` runs; takes a snapshot "Before restoring <id>" of the current `loginusers.vdf` plus the current `AutoLoginUser` and `RememberPassword` values, with `protect: id` so pruning cannot remove the snapshot about to be restored; then `applyRestore(id)`.
- `applyRestore(id)` (`adapter.ts:1161-1187`): `backups.restore('steam', id, [current install's loginusers.vdf path])` verifies every stored hash, refuses a snapshot whose recorded source path is not that file (a moved install), and writes atomically (`src/main/services/backup.ts:260-270`); then for each of `AutoLoginUser` and `RememberPassword` present in the manifest's meta: a string/number value is set back, a null is deleted (an already-absent value counts as deleted). Result `{ files: [paths written], registry: ["AutoLoginUser = <v>" | "AutoLoginUser (removed)", "RememberPassword = <n>" | "RememberPassword (removed)"] }`.
- A switch rollback (section 7) calls `applyRestore` directly after closing Steam, without the before-restore snapshot.

##### 10. Launch (`launchGame`)

`launchGame(game)` (`adapter.ts:465-471`): the id must match `^\d+$`, else a plain Error "Not a Steam app id: <id>" (no code). It opens `steam://rungameid/<appid>` through the shell and returns `{ command: "steam://rungameid/<appid>" }`. Nothing is confirmed afterwards, no `handOff` is returned, and the `signal` and `startedBySwitch` options are ignored (the signature takes only the game). The adapter never throws `GAME_NOT_PLAYABLE` or `GAME_RUNNING`; a game update Steam deferred runs first inside Steam, as its own Play button would (`docs/adapters/steam.md:101`).

##### 11. Health check

`healthCheck(known?)` (`adapter.ts:1191-1307`) reuses the install and running state passed in. Report status = worst row status (`fail` > `warn` > `ok`), or `absent` when not installed.

| id | label | status | detail |
| --- | --- | --- | --- |
| `install` | "Installation" | `absent` when not installed (report ends after `chosen-folder`) | "Steam is not installed on this PC. Checked HKCU\Software\Valve\Steam\SteamPath, HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath, and C:\Program Files (x86)\Steam." |
| `install` | "Installation" | `ok` | "<installDir> (from <the folder chosen in Settings | HKCU\...\SteamPath | HKLM\...\InstallPath | a launcher file | default location>)" |
| `chosen-folder` | "Chosen folder" | `warn`, only when a folder is chosen and the install in use is not `source: 'user'` | "steam.exe is no longer in <chosen>, the folder chosen in Settings, so <the installation found automatically is used | nothing is used>. Use Locate to choose it again, or Reset to go back to automatic detection." (`chosen-folder.ts:18-27`) |
| `process` | "Process" | `ok`; `warn` when the process list fails | "steam.exe is running" / "steam.exe is not running" / "Could not query the process list: <reason>" |
| `loginusers` | "Remembered accounts" | `warn` missing; `ok`; `fail` unparsable | "<path> not found. Steam has not signed in on this PC yet, so only the credentials switch path is available." / "<n> account(s) in loginusers.vdf, <m> with a remembered session" / "Could not parse <path>: <reason>" |
| `most-recent-key` | "Most-recent marker" | `warn` when accounts exist but no block has either key; `warn` when more than one block is marked; else `ok`; absent when the file is missing, empty or unparsable | "Neither AutoLogin nor MostRecent appears in loginusers.vdf. A switch will add AutoLogin." / "File uses <AutoLogin and/or MostRecent>; <n> account(s) marked most recent<' (expected one)' when > 1>" |
| `registry` | "Registry AutoLoginUser" | `warn` absent; `ok` agrees (or no block is marked most recent); `warn` disagrees; `warn` on a read error | "HKCU\Software\Valve\Steam\AutoLoginUser is absent. Steam will ask which account to use on next start." / "Present and matches the most recent account in loginusers.vdf" / "Present but names a different account than loginusers.vdf marks as most recent" / "Could not read the registry: <reason>" |
| `libraries` | "Library folders" | `warn` missing file; `fail` unparsable; `warn` when a folder has no `steamapps`; else `ok` | "<path> not found. Only the main Steam folder will be scanned." / "Could not parse <path>: <reason>" / "<n> library folder(s)<'; missing steamapps in: <list>'>" |
| `games` | "Installed games" | `ok`; `fail` on a scan error | "<n> installed game(s)<' (<m> waiting for an update)'>" / "Could not scan the library: <reason>" |
| `backups` | "Backups" | `ok` | "No snapshots yet" / "<n> snapshot(s) available for restore" |

##### 12. Session capture / sessionState

Not applicable: Steam has no session capture and no `sessionState` (section intro).

##### 13. Error codes

| Code | Where | Message |
| --- | --- | --- |
| `NOT_INSTALLED` | any operation needing Steam | "steam is not installed on this PC" (`types.ts:198-204`) |
| `LAUNCHER_FILE` | `listRememberedAccounts`; a too-large VDF anywhere | "loginusers.vdf not found: <path>", "Could not parse loginusers.vdf (<reason>): <path>", "Too large to be a launcher file (<n> bytes): <path>" (`adapter.ts:389`, `393`; `read-file.ts:9-11`; `types.ts:214-224`) |
| `CONFIRM_REQUIRED` | `runSwitch` | the `closeMessage` (section 6), with the running game attached (`adapter.ts:790-793`) |
| `SWITCH_BLOCKED` | `runSwitch` | the plan's first warning or "This switch cannot run." (unreachable for Steam: `executable` is always true) (`adapter.ts:789`) |
| `LAUNCHER_RUNNING` | `restoreBackup` | "Close Steam before restoring: Steam rewrites loginusers.vdf when it exits, which would undo the restore." (`adapter.ts:1145`) |
| `PROCESS_ACCESS_DENIED` | shutdown's close or kill | see intro (`processes.ts:44-52`) |
| `REGISTRY` | a failing `reg.exe` call | "reg query failed for <hive>\<key>\<name>: <reason>", "reg add failed for ...", "reg delete failed for ...", "Refusing to touch a registry path with unexpected characters", "Refusing to write unexpected characters to <name>" (`registry.ts:61-67`, `215`, `272`, `302`) |
| `BACKUP` | snapshot or restore | "Snapshot <id> not found for steam", "Snapshot <id> was taken for <other>, not steam", "Snapshot <id> names a file that is not one of steam's: <path>", "Snapshot file is corrupt: <relative>" (`backup.ts:225-229`, `264`, `282`) |
| `ABORTED` | internal | "The switch was stopped"; turned into a cancelled result, never shown (`wait.ts:1-8`) |
| `VDF_PARSE`, `LOGINUSERS_FORMAT`, `MANIFEST_FORMAT` | parsers | "<message> (line <n>)", "loginusers.vdf has no \"users\" block", "SteamID64 <id> is not in loginusers.vdf", "libraryfolders.vdf has no \"libraryfolders\" block", "appmanifest has no \"AppState\" block", "appmanifest has no numeric appid", "appmanifest <id> has no installdir"; always wrapped into a `LAUNCHER_FILE`, a plan warning, a skipped manifest, or a health row (`vdf.ts:10-19`, `loginusers.ts:32-44`, `109`, `manifests.ts:9-15`, `24`, `62-66`) |
| plain `Error` (no code) | `runSwitch`, `launchGame`, `shutdown` | "steam.exe is still running after -shutdown and a forced close", "steam.exe did not appear within 60 s", the started-meanwhile messages, "Steam rejected the stored password for <login>. Update it on the account page and try again.", "Steam signed in as <other> instead of <login>.", "Not a Steam app id: <id>" |

Outside the adapter, `LauncherService.guard` rethrows everything as `AdapterError` "Steam: <reason>" keeping the inner code (`src/main/services/launchers.ts:26-34`).

##### 14. Log parsing

`steamui_login.txt` (`login-log.ts`):

- Line pattern: `SetLoginState:\s*<State>\s*-\s*<reason>` anywhere in the line; `State` is letters only; lines longer than 16 KiB (`MAX_LOG_LINE_LENGTH`) are skipped (`login-log.ts:29-39`, `log-read.ts:3-4`).
- Decisive states, scanning from the newest line: `Success` → success; `WaitingForCredentials` → credentials with its reason; any other state is ignored, and no decisive line means "still working" (null) (`login-log.ts:58-65`).
- Reason classification (`classifyReason`, `login-log.ts:46-55`), on the lowercased reason: "" or "ok" → `none`; contains "two factor", "twofactor" or "mobile" → `guard-mobile`; "logon denied", "login denied" or "email" → `guard-email`; "password" → `password`; "expired" → `expired`; "rate limit" or "throttle" → `rate-limit`; else `other`. Observed wordings: "Success - OK", "WaitingForCredentials - OK", "Account Logon Denied", "License expired" (`login-log.ts:9-11`).
- Marks: the file size when the switch began and again right before each spawn (`adapter.ts:859-864`). A read covers the bytes appended since the mark, at most the last 512 KiB (`MAX_TAIL_BYTES`, Steam rotates to `steamui_login.previous.txt` near that size); a file smaller than its mark (rotated) is read from 0; a clipped read drops its first, partial line (`login-log.ts:76-88`, `log-read.ts:18-27`). Only `signedInAs` reads from mark 0 (the whole current file) (`adapter.ts:766`).

`bootstrap_log.txt` (`bootstrap-log.ts`):

- Read since the mark taken with the login log's, same tail rules.
- `Downloaded new manifest`, `Downloading update`, `Found pending update`, `Installing update`, `Extracting package`, `Update complete, launching Steam` → updating; `Verification complete` → settled; after a `Background update loop` line, update lines are ignored until the next `Startup - updater built` line (a background download applies at the next start and holds nothing up) (`bootstrap-log.ts:43-59`). The result is "an update is in progress" when the last transition left `updating` set.

Deliberately ignored: every `SetLoginState` other than `Success` and `WaitingForCredentials`; bootstrap lines in a background-update stretch.

#### Adapter: Epic Games

Launcher id `epic`, display name "Epic Games" (`src/main/launchers/epic/adapter.ts:249-250`); user-facing text says "Epic Games Launcher". The launcher has no account list and no sign-in flag: it signs in from an opaque, Epic-encrypted blob in `[RememberMe] Data` of `GameUserSettings.ini`. One Launch stores one such blob per account ("saved sign-in"), a switch swaps it in, restarts the launcher, and confirms the sign-in from the launcher's own log (`adapter.ts:1-9`). This adapter implements `sessionState` and `captureSession` (section 12).

Shared helpers: `readLauncherBytes`/`readLauncherText` refuse a file over 4 MiB (`MAX_LAUNCHER_FILE_BYTES`) with `LAUNCHER_FILE` "Too large to be a launcher file (<n> bytes)" (`src/main/launchers/read-file.ts:4-23`). Registry reads use `reg.exe` under code page 65001; exit 1 means absent, read as null (`src/main/platform/registry.ts:93-96`, `218-230`). This adapter needs only a `RegistryReader`; it never writes the registry (`adapter.ts:147`). Process checks, forced kills (image plus tree by default) and `PROCESS_ACCESS_DENIED` ("Windows would not let One Launch close <image>. It is most likely running as administrator: close it yourself, or start One Launch as administrator too, then try again.") are as in the Steam section (`src/main/platform/processes.ts:44-52`, `521-555`). A window close posts WM_CLOSE to the process's top-level window; a process with no window gets nothing and the kill follows (`processes.ts:185-188`, `540-546`). Waits use `waitUntil`/`sleep` from `src/main/launchers/wait.ts`.

Folders: `%LOCALAPPDATA%` (fallback `%USERPROFILE%\AppData\Local`, then `C:\AppData\Local`) and `%ProgramData%` (fallback `C:\ProgramData`) (`adapter.ts:260-261`).

##### 1. Files and registry it reads; what it writes

Reads:

| Path or value | Format | What is read | Cap | On failure |
| --- | --- | --- | --- | --- |
| `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\{AEB35C6C-B6D4-4AA0-8452-DE699737B5F6}` `InstallLocation` (REG_SZ) | path | detection candidate 2, with `\Launcher` appended (`adapter.ts:318-322`) | n/a | absent or blank = skipped |
| `HKLM\SOFTWARE\Classes\com.epicgames.launcher\shell\open\command` default value | `"<exe>" ...` | detection candidate 3: the first quoted token, four folders up (`adapter.ts:323-328`) | n/a | absent or no leading quoted token = skipped |
| `HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher` `AppDataPath` (REG_SZ) | path | manifests folder = `<AppDataPath>\Manifests` (`adapter.ts:305-309`) | n/a | absent = `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests` |
| `<Manifests>\<32 hex>.item` | JSON object; keys used: `AppName`, `DisplayName`, `CatalogNamespace`, `CatalogItemId`, `InstallLocation` (mixes `\` and `/`), `LaunchExecutable`, `InstallSize`, `bIsIncompleteInstall`, `MainGameAppName`, `AppCategories`, `InstallationGuid`; an optional UTF-8 BOM is tolerated (`manifests.ts:48-76`) | installed games, launch URI ids | 4 MiB each | not JSON, not an object, no `AppName`, no `InstallLocation`: that file is skipped, counted as unreadable, warning logged (`adapter.ts:367-376`); missing folder: warning logged, no games (`adapter.ts:363-366`) |
| `%ProgramData%\Epic\UnrealEngineLauncher\LauncherInstalled.dat` | JSON `{ "InstallationList": [ { InstallLocation, NamespaceId, ItemId, ArtifactId, AppVersion, AppName } ] }` (`manifests.ts:96-116`) | health cross-check only | 4 MiB | missing or unparsable: health `launcher-installed` warns |
| `%LOCALAPPDATA%\EpicGamesLauncher\Saved\Config\Windows\GameUserSettings.ini` and `...\Config\WindowsEditor\GameUserSettings.ini` | Unreal INI: `[Section]` headers, `key=value` lines, CRLF or LF, optional UTF-8 BOM on the first line; sections of interest `[RememberMe]` (`Enable`, `Data`), `[Offline]` (`Data`), and per-account `[<32 hex>_<Name>]` sections (`ini.ts:1-5`, `25-27`; `session.ts:12-13`; `adapter.ts:142`) | the remembered session, the ids of accounts that signed in here | 4 MiB | missing: `readIni` answers null (plan not executable, account list empty); UTF-16 BOM or any NUL byte: `LAUNCHER_FILE` "GameUserSettings.ini is not UTF-8 (UTF-16 or binary content); One Launch will not rewrite it"; other read error: `LAUNCHER_FILE` "Could not read GameUserSettings.ini (<reason>)" (`adapter.ts:408-422`) |
| `%LOCALAPPDATA%\EpicGamesLauncher\Saved\Logs\EpicGamesLauncher.log` | plain text; first line `Log file open, <date>`; lines prefixed `[YYYY.MM.DD-HH.MM.SS:mmm][<frame>]` in UTC (`launcher-log.ts:1-20`, `43-44`) | sign-in outcome, restart announcements, startup finished | tail of 8 MiB per read (`MAX_TAIL_BYTES`) | missing or unreadable = empty text (`launcher-log.ts:154-164`) |
| mtime of the settings file in use | | `diskCurrent` (section 12) | | unreadable = null |
| Windows process list | image names (and paths for `runningGame`) | `isRunning`, `runningGame`, every wait | n/a | a failed listing throws; `runningGame` logs and answers null |

Which settings file: `settingsPaths()` lists the files that exist in `Windows` and `WindowsEditor`, newest mtime first; when two were written within 2 s of each other the later folder (`WindowsEditor`) wins (`adapter.ts:273-285`). `settingsPath()` is the first of those, or the classic `Windows` path when none exists (`adapter.ts:288-290`). The newest file is read; every existing one is backed up and written (`adapter.ts:1037`, `1047`).

Never read for the sign-in: `webcache_*` and `Saved\Data` (`docs/adapters/epic.md:48`).

Writes (only inside `runSwitch` and `applyRestore`):

| Target | What changes | How |
| --- | --- | --- |
| every existing `GameUserSettings.ini` (both folders) | `[RememberMe]`: `Enable` set to `True` and `Data` set to the stored blob (remembered strategy) or removed (credentials strategy); `[Offline]`: `Data` set to the stored offline blob or removed when the snapshot has none or on sign-out. `Enable=False` is never written (`session.ts:94-111`) | `rewriteSection` replaces an existing key line in place (duplicates of the key are dropped), appends a missing key after the section's last entry before its trailing blank lines, and creates a missing section at the end of the file; removing a key deletes its lines; every other line, the line-ending style, the trailing-newline state, the BOM and section order are kept byte for byte (`ini.ts:73-129`). Each file is written to `<file>.onelaunch-<pid>.tmp` and renamed over the original (`src/main/services/backup.ts:110-125`); a file whose rewrite changes nothing is left untouched (`adapter.ts:1062-1068`) |
| on restore: the files recorded in the snapshot, written back to their recorded paths | see section 9 | atomic, after hash verification and vault decryption |

The settings file is never created: a file with only the two sections loses every per-account setting and the launcher does not sign in from it (`adapter.ts:1048-1054`, `docs/adapters/epic.md:11`). The blob is decrypted only inside the write step and never logged or shown; plans and the UI show a SHA-256 fingerprint (first 8 hex) and a length (`session.ts:1-5`, `79-84`).

##### 2. Detection

`detectInstall` takes the first candidate whose folder contains `Portal\Binaries\Win64\EpicGamesLauncher.exe` (`adapter.ts:313-330`, `342-349`, `90`):

1. The `epic.path` setting (Locate), normalised. `source: 'user'`. If the exe is gone, the ladder continues and health warns (`chosen-folder`).
2. Uninstall key `InstallLocation` + `\Launcher`. `source: 'registry-hklm'`.
3. The URI handler command's first quoted token, resolved four folders up. `source: 'registry-hklm'`.
4. `C:\Program Files (x86)\Epic Games\Launcher`. `source: 'default-path'`.

Only the 32-bit registry view is read; per-user and Store installs must be located by the user (`docs/adapters/epic.md:25`). `installFromExe(path)` accepts only a file named `EpicGamesLauncher.exe` (case-insensitive) and takes the folder four levels up as the install dir, so a `Win32` pick resolves to the `Win64` exe; null when that folder has no Win64 exe (`adapter.ts:336-340`). Operations that need the launcher throw `NOT_INSTALLED` ("epic is not installed on this PC") (`adapter.ts:351-355`). A chosen folder only changes where the exe starts from, not where settings, log or manifests are read (`docs/adapters/epic.md:27`).

##### 3. Installed games scan

`listInstalledGames` (`adapter.ts:380-400`) requires an install, reads every `<32 hex>.item` in the manifests folder (section 1), and keeps a manifest when it is a library game and its `AppName` has not been seen: not `bIsIncompleteInstall`, not a DLC or add-on (`MainGameAppName` present and different from `AppName`), and `AppCategories` empty or containing `games` (Unreal Engine installs carry `engines`) (`manifests.ts:78-87`).

| `InstalledGame` field | Source |
| --- | --- |
| `launcherGameId` | `AppName` |
| `name` | `DisplayName`, or `AppName` when absent |
| `installPath` | `InstallLocation`, normalised |
| `libraryPath` | the parent folder of `installPath` |
| `sizeOnDiskBytes` | `InstallSize` when a finite number >= 0, else null |
| `lastPlayedAt` | always null (Epic records no time) |
| `needsUpdate` | always false |

Sorted by name, case-insensitive. Unreadable manifests are skipped and counted for the health row. `steam-ids.ts` maps `AppName` → Steam app id for games delisted on Steam (only `Sugar` → 252950, Rocket League) for artwork lookups elsewhere (`steam-ids.ts:1-8`).

##### 4. Running state

- `isRunning`: `EpicGamesLauncher.exe` in the process list (`adapter.ts:563-565`).
- `runningGame` (`adapter.ts:568-581`): the launcher keeps no running-app flag. Null when not installed; the process list is taken (failure logged, null; empty list, null) and the first process whose executable lies under an installed game's `installPath` (recorded path or its resolved real path, drive-letter paths only, case-insensitive prefix) answers `{ launcherGameId: AppName, name, source: 'process' }` (`src/main/launchers/running-game.ts:6-37`).

##### 5. Remembered accounts

`listRememberedAccounts` (`adapter.ts:533-559`): requires an install; an absent settings file gives an empty list; a UTF-16 file throws `LAUNCHER_FILE`. Every distinct 32-hex id that prefixes a section name `[<id>_...]` (lowercased, file order) is an account that has signed in on this PC. The "active" id is, when the file remembers a session, the current session's owner uid or else the log's last signed-in id (section 12).

| `RememberedAccount` field | Value |
| --- | --- |
| `launcherUid` | the 32-hex id, lowercase |
| `login` | the linked One Launch account's login, or "" (the launcher records no login name) |
| `personaName` | the linked account's display name, or "" |
| `rememberPassword` | id equals the active id |
| `mostRecent` | id equals the active id |
| `lastLoginAt` | null |
| `sessionMayBeExpired` | false |
| `linkedAccountId` | added outside the adapter: the One Launch Epic account with this uid, else by login (`src/main/services/launchers.ts:225-231`) |

Sorted most-recent first. What cannot be imported: an id with no linked account and no login is skipped by the import with reason "the launcher does not record the login for this account" (`launchers.ts:253-256`); the session blob is captured separately (section 12), never by import.

##### 6. Preflight and plan

`preflight(account)` ignores `forGame` (its signature takes only the account, `adapter.ts:646`). `launcherRunning` = `ctx.running`; `runningGame` only when running and not already active; `closeMessage` = "Epic Games Launcher is running <game>. Switching closes the launcher and the game; unsaved progress in the game is lost." or "Epic Games Launcher is running. Switching closes it." (`adapter.ts:640-656`). The close warning is removed from the preflight plan and kept in the dry-run plan.

`buildSwitchPlan(account)` (`adapter.ts:667-930`) reads: install, running state, the newest settings file (`readIni`, may throw `LAUNCHER_FILE`), the current session (section 12), and the account's stored session row (never the blob). Derived:

- `inPlace` = the current session's owner is this account and (the launcher runs, or the file remembers a session) (`adapter.ts:684`).
- `strategy` = `remembered` when `inPlace`, or a stored snapshot exists with no `staleReason`; else `credentials` (`adapter.ts:685`).
- `alreadyActive` = `inPlace` and running (`adapter.ts:686`).
- `usesStored` = remembered and not in place.
- `expectedUid` = the stored snapshot's `launcherUid`, else the account's `launcherUid`, else null (`adapter.ts:689`); `plan.account.launcherUid` = `expectedUid`.
- `executable` = the settings file exists (`adapter.ts:926`). When it does not, the first warning is the "blocked" reason the dialog shows.
- `plan.current` = `{ mostRecentLogin: owner's login or null, autoLoginUser: null, running }`.
- `outgoing` = the current owner; `outgoingChanged` = owner exists and their stored fingerprint differs from the file's; `captureNeeded` = owner exists and not already active and (running, or (file remembers a session and (not in place or changed))) (`adapter.ts:709-717`).

Plan steps, always these nine in this order:

| id | kind | title | target | skipped when | detail |
| --- | --- | --- | --- | --- | --- |
| `close` | process | "Close Epic Games Launcher" | `EpicGamesLauncher.exe` | not running, or already active | "Skipped: the launcher is already signed in as this account, so it is left running." / "Send EpicGamesLauncher.exe a window close (the launcher exits unless \"minimise to tray\" is on) and wait up to 20 s. If it is still running after that, force-close EpicGamesLauncher.exe and every process it started. Any game running under the launcher closes with it." / "Skipped: EpicGamesLauncher.exe is not running." (`adapter.ts:696-707`) |
| `capture` | backup | "Save this account's session" (in place) or "Save the outgoing session" | settings path | not `captureNeeded` | already active: "Skipped: the launcher stays running, so its file is not re-read."; no owner and a remembered session: "Skipped: the launcher remembers a session (fingerprint <8 hex>) that no One Launch account owns, so it is left alone and not saved. To keep it, capture it from that account's page before switching."; no owner, nothing remembered: "Skipped: the launcher is not remembering any session."; owner but not needed: "Skipped: <owner>'s stored snapshot already matches what the launcher remembers."; running: "After the launcher has closed (it writes the file on exit), read [RememberMe] and, if it holds <owner>'s session and it changed, store it, encrypted, as the account's snapshot. Nothing in the launcher's files changes."; in place: "[RememberMe] holds <owner>'s session but it differs from the stored snapshot (<Epic rotated the token | no snapshot yet>), so store it, encrypted, as the account's snapshot. Nothing in the launcher's files changes."; else "[RememberMe] holds <owner>'s session<' and it changed since it was last saved (Epic rotates tokens)'>; store it, encrypted, as the account's snapshot if it changed. Nothing in the launcher's files changes." (`adapter.ts:718-737`) |
| `backup` | backup | "Back up GameUserSettings.ini" | `<dataDir>\backups\epic\<timestamp>` | already active or in place | "Skipped: the file will not be modified." / "Copy <path[ and path]> into a new snapshot folder so a failed switch can be rolled back. The newest <backups.keep> snapshots are kept." (`adapter.ts:739-749`) |
| `file` | file | "Put the account's session in place" | settings path(s) | in place (always skipped) | in place: "No change: [RememberMe] already holds <displayName>'s session (fingerprint <8 hex>)."; remembered from the store: "Decrypt <displayName>'s session snapshot from the vault and write it into [RememberMe] (Enable=True and Data) and [Offline] (Data) of <both settings files (the launcher reads one of them; writing both is harmless) | GameUserSettings.ini>. The new content is written to a temp file and renamed over the original. Every other line, including the per-account [<id>_*] sections, stays byte for byte. The blob itself is never shown or logged." with changes `[RememberMe] Enable: <current | (absent)> → True`, `[RememberMe] Data: <(absent) | (<n> characters, fingerprint <8 hex>)> → (stored snapshot, fingerprint <8 hex>, captured <ISO>)`, `[Offline] Data: <...> → (from the same snapshot, or removed when it has none)` (`adapter.ts:751-779`) |
| `file` (credentials) | file | "Sign the launcher out" | settings path(s) | already signed out (`Data` absent in both sections and `Enable` is `True`) | "No change: the launcher is not remembering any session, so it will open on its sign-in page." / "Remove Data from [RememberMe] and [Offline] and keep Enable=True, so the launcher opens on its sign-in page instead of signing in as <owner | the previous account> and remembers the next sign-in. Everything else in <both files | the file> is preserved." with changes Enable → True, `[RememberMe] Data` → (removed), `[Offline] Data` → (removed) (`adapter.ts:780-800`) |
| `start` | launch | "Start Epic Games Launcher" | `"<exePath>"` | already active and running | "Skipped: the launcher is already running as this account." / remembered: "Start the launcher normally (no arguments, as the com.epicgames.launcher:// handler does). It signs in as <login> on its own from [RememberMe]." / credentials: "Start the launcher normally. With no remembered session it opens on its sign-in page." (`adapter.ts:803-815`) |
| `wait` | wait | "Wait for the launcher to come up" | `EpicGamesLauncher.exe` | already active and running | "Poll the process list until EpicGamesLauncher.exe is running, up to 60 s." (`adapter.ts:817-824`) |
| `verify` | wait | "Confirm the launcher signed in" | log path | already active and running | "Skipped: the launcher is already signed in as this account." / remembered: "Read what the launcher appends to its own log (no tokens in it) until FAccountService::OnLoginComplete reports TRUE with <the Epic account id …<last 6> | an account id, which is then recorded on the account>, up to 45 s. A FALSE line means the launcher did not sign in and the next step hands over to you; when the reason is a refused token (for example TOKEN_EXPIRED) the snapshot is also marked stale." / credentials: "Read what the launcher appends to its own log until FAccountService::OnLoginComplete reports TRUE<' with the Epic account id …<last 6>'>. The launcher will not sign in on its own, so this goes straight to the next step." (`adapter.ts:826-840`) |
| `manual` | wait | "Wait for you to sign in" | log path | remembered strategy | remembered: "Runs only if the launcher rejects the stored session (expired token) or does not sign in within 45 s. The launcher stays open on its sign-in page; you sign in there once as <login><' (the password can be copied from One Launch)'> and One Launch continues with the game as soon as the log reports the sign-in, up to 10 min." / credentials: "The launcher opens on its sign-in page. Sign in there as <login><' (the password can be copied from One Launch)'>. One Launch continues with the game as soon as the log reports the sign-in, up to 10 min. Signing in as a different Epic account than the one linked to <displayName> ends the switch and rolls the file back." (`adapter.ts:842-856`) |
| `recapture` | backup | "Save the new session" | settings path | remembered strategy | remembered: "Runs only after a manual sign-in: the fresh [RememberMe] replaces the stale snapshot." / credentials: "Once the sign-in is reported, wait up to 15 s for the launcher to write the new [RememberMe], then store it, encrypted, as <displayName>'s session snapshot and record the Epic account id on the account. If the launcher writes it later, it is saved at the next switch instead. Later switches use the remembered path." (`adapter.ts:858-868`) |

Warnings, in order (`adapter.ts:870-916`):

- Settings file missing (also the blocked reason): "<path> does not exist. One Launch never creates it: restore the newest Epic Games backup from Settings, or start the launcher once by hand so it writes its settings file."
- Else when `Enable` is present and not `True`: "[RememberMe] Enable is <value> in <path>, so the launcher does not sign in on its own at start. The switch sets it back to True."
- Close warning (removed from the preflight plan): when running and not already active and not in place: "Epic Games Launcher is running< as <owner displayName> | as an account not linked to One Launch | on its sign-in page>. It will be closed, along with any game running under it."
- Running, not in place, the log names an id no account owns: "The launcher's current sign-in (Epic account …<last 6>) is not linked to any One Launch account, so it will not be saved."
- Stored snapshot stale and not in place: "The stored session for <displayName> was rejected by the launcher (<staleReason>). You will sign in once by hand and One Launch will store the fresh session."
- Credentials, no snapshot, not in place: "No session snapshot is stored for <displayName> yet. The launcher will open on its sign-in page; sign in once and One Launch captures the session for later switches.<' No password is stored in One Launch, so have it ready.' when no password>"
- Not in place, the file remembers a session nobody owns: "The launcher currently remembers a session that no One Launch account owns (fingerprint <8 hex>). It will be replaced. The backup taken before this switch keeps a copy and is left out of the usual clean-up, so restoring it from Settings › Backups brings that sign-in back."
- The account's uid and the snapshot's uid differ: "The Epic account id on this account (…<last 6>) differs from the one recorded with its session snapshot (…<last 6>). The snapshot's id is used to confirm the sign-in."

Notes: already active: "Epic Games Launcher is running and signed in as <displayName>. The real switch would change nothing and would leave the launcher running."; in place but closed: "Epic Games Launcher already remembers <displayName>'s session, so it is only started." (`adapter.ts:880-885`).

##### 7. Switch (`runSwitch`)

`switchAccount` with `dryRun: true` returns the plan; `dryRun: false` runs the switch (`adapter.ts:658-664`). The switch rebuilds the plan, then refuses: `SWITCH_BLOCKED` with the plan's first warning (the missing settings file) when not executable; `CONFIRM_REQUIRED` with the `closeMessage` and the running game when the launcher runs, is not already active, and `confirmClose` is unset (`adapter.ts:945-949`). Job steps start from the plan (skipped ones keep their detail), `onProgress` gets a copy on every change (`adapter.ts:951-965`). `startedWithGame` is always null; `readyAt` is set on success when a sign-in was confirmed (`adapter.ts:977-978`, `1277`).

Timing (`DEFAULT_TIMING`, `adapter.ts:115-140`):

| Name | Default | Used for |
| --- | --- | --- |
| `pollMs` | 500 ms | process-list polls |
| `logPollMs` | 250 ms | log and settings-file polls (sign-in wait, recapture) |
| `shutdownMs` | 20 s | wait for exit after the window close |
| `killGraceMs` | 10 s | wait after the forced kill |
| `restartWatchMs` | 8 s | watch for a self-restart after an exit the log did not explain |
| `cleanExitWatchMs` | 1.5 s | the same watch after a clean, unannounced exit in the log |
| `restartSettleMs` | 45 s | the watch after an announced restart, and how long a restarted launcher gets to finish starting |
| `startupMs` | 60 s | wait for the process after a start |
| `signInMs` | 45 s | auto sign-in budget (remembered) |
| `manualSignInMs` | 10 min | hand-over wait |
| `captureMs` | 15 s | wait for the fresh `[RememberMe]` after a manual sign-in |
| `readyGraceMs` | 2 s | `readyAt` offset after the confirmed sign-in |

**[dev-only]** `SIM_TIMING` overrides for the fake platform: `pollMs` 100, `logPollMs` 100, `processPollMs` 100, `shutdownMs` 3 s, `killGraceMs` 1 s, `restartWatchMs` 500 ms, `cleanExitWatchMs` 200 ms, `restartSettleMs` 2 s, `startupMs` 5 s, `signInMs` 8 s, `manualSignInMs` 30 s, `captureMs` 3 s, `readyGraceMs` 300 ms (`src/main/launchers/sim/scenario.ts:22-40`, `281`).

Sequence (`adapter.ts:1227-1281`):

1. Abort check.
2. **Close** (running and not already active): step `close` running; `shutdown({ signal })` (`adapter.ts:594-625`), up to 3 attempts: take a log mark (size plus first line); post a window close to `EpicGamesLauncher.exe`; poll every `pollMs` up to `shutdownMs` (20 s) for the process to be gone; else warn, force-kill the image and its tree, poll up to `killGraceMs` (10 s), else throw "EpicGamesLauncher.exe is still running after a window close and a forced close". Then read the log events since the mark: if a restart was announced (`Application: Restarting ... EpicGamesUpdater.exe`), watch for the process to reappear for `restartSettleMs` (45 s); if the log shows an exit line and no restart, watch `cleanExitWatchMs` (1.5 s); otherwise `restartWatchMs` (8 s). Not back → done. Back and this was attempt 3 → throw "EpicGamesLauncher.exe keeps restarting itself after being closed". Otherwise wait up to `restartSettleMs` for a new log file (different first line) whose events include `LogInit: AllCheckComplete`, then close again. An elevated launcher makes the close raise `PROCESS_ACCESS_DENIED` at once. Detail on success: "Launcher closed.".
3. Abort check. **Started-meanwhile check**: not already active and the launcher runs now → "Epic Games Launcher was started by something else while One Launch was switching accounts, so the switch stopped before changing anything. Try again." (step `close` marked failed) (`adapter.ts:1236`, `1081-1086`).
4. **Outgoing session capture** (`saveOutgoing`, `adapter.ts:996-1029`), only when the plan did not skip `capture`. Best effort, every failure is caught: the settings file is re-read (the launcher has just written it on exit), the current session recomputed as "closed", and: no remembered session → "The launcher left no remembered session behind (signed out)."; owner unknown → "The remembered session belongs to no One Launch account; left alone."; file older than the log's last sign-in → "The file predates the launcher's last sign-in, so it is not trusted as <owner>'s session."; owner's stored fingerprint equals the file's → "<owner>'s stored session is already current."; else `storeSession` (links the uid first, then saves the encrypted blob; the uid is the owner's or the log's) → "Saved <owner>'s session (fingerprint <8 hex>)."; an error → "Not saved: <reason>" and a warning log. All of these end the step `done`.
5. **Snapshot and writes** (not already active and not in place): if no settings file exists now → `SWITCH_BLOCKED` "<path> is missing. Restore the newest Epic Games backup from Settings, or start the launcher once by hand, then try again." (`adapter.ts:1240-1244`). `takeSnapshot` (`adapter.ts:1030-1043`): every existing settings file as `Config/<folder>/GameUserSettings.ini`, `sensitive: true` (stored vault-encrypted as `<relative>.enc`, or in the clear with a warning when the vault is unavailable, `backup.ts:172-173`), reason "Before switching to <displayName>", **pinned** with "Holds a sign-in that no One Launch account owns" when the file now remembers a session whose owner is no One Launch account (decided after the close, since the launcher rewrites its session on exit); `onBackup(id)` fires here, before any write; detail = snapshot folder. Then `writeSession` (`adapter.ts:1045-1079`): step `file` running; remembered strategy → the blob is decrypted now (`SWITCH_BLOCKED` "No session snapshot is stored for <displayName>." when none; a malformed blob throws "Stored Epic session has an unexpected shape", `session.ts:66-77`); credentials → null blob (sign-out). Each existing settings file is rewritten as in section 1; untouched when nothing changes. Nothing written → `skipped` "Already in place." or "Already signed out."; else "[RememberMe] now holds the snapshot (fingerprint <8 hex>) in <n> file(s)." or "[RememberMe] cleared in <n> file(s).". `modified` becomes true at the first file written.
6. **Post-write running check**: the launcher runs now and the switch is not already active → "Epic Games Launcher was started by something else while One Launch was switching accounts, so the switch was stopped. Try again." (step `start` failed; rolled back). Already active → `start` and `wait` `skipped` "The launcher is already running.". Else **start** (`adapter.ts:1088-1100`): step `start` running, abort check, log mark taken, `"<exePath>"` spawned with no arguments and the exe's folder as working directory, "Launcher started."; step `wait` polls up to `startupMs` (60 s), failing with "EpicGamesLauncher.exe did not appear within 60 s"; "EpicGamesLauncher.exe is running.".
7. Already active and running → `verify` `skipped` "The launcher is already signed in as this account.". Otherwise `confirmSignIn()` (`adapter.ts:1139-1149`): the sign-in wait with budget `signInMs` (45 s) for remembered, or 1 ms for credentials (one or two polls, then straight to the hand-over). Results: `cancelled` → cancelled result; `wrong-account` → throw (section "Identity"); `signed-in` on the remembered path: in place → `captureFresh(0)` (one look for a rotated blob), else `markVerified` (clears `staleReason`, sets `verifiedAt`); `failed` whose reason is a session rejection (section 14) on the remembered path → `markStale(accountId, reason)`; then any non-`signed-in` → `waitForManualSignIn` → `cancelled` → cancelled result; `wrong-account` → throw; timeout → needs-sign-in result; `signed-in` → `captureFresh(captureMs)`. Finally `readyAt` = now + `readyGraceMs` (2 s); `LaunchService` waits for it only when a game follows (`adapter.ts:1260-1278`).
8. Result `ok: true`, `restored: null`, `readyAt`.

Sign-in wait (`waitForSignIn(timeoutMs, manual)`, `adapter.ts:1102-1122`), polled every `logPollMs`: abort → `cancelled`; the log since the start mark is parsed for its last decisive sign-in event. A TRUE line: with no expected uid, an id another One Launch account holds → `wrong-account`, any other id (or none) → `signed-in`; with an expected uid, equal → `signed-in`, a different non-empty id → `wrong-account`, an empty id → keep waiting. A FALSE line → `failed` with the masked reason, but only when not manual (a failed attempt during the hand-over is not final). Deadline → `timeout`. `confirmSignIn` details: "Signed in as <login>.<' Linked to Epic Games account …<last 4>.'>", "Signed in as a different account.", "The launcher rejected the session: <reason | no reason given>.", "The launcher did not report a sign-in in time." (remembered) or "Waiting for you." (credentials), "Cancelled.".

Identity, wrong account and adoption (`adapter.ts:1123-1138`): the expected uid is the snapshot's, else the account's. A first confirmed sign-in with no expected uid is **adopted**: `setLauncherUid(accountId, id)` (fails closed with `DUPLICATE` when another account holds it), and the step detail gains " Linked to Epic Games account …<last 4>.". A `wrong-account` result throws, with no expected uid: "Epic Games Launcher signed in as <owner displayName> (account …<last 6>), which is another One Launch account, instead of <displayName>. Sign in to the launcher as <login>."; with one: "Epic Games Launcher signed in as <<owner displayName> (account …<last 6>) | account …<last 6>> instead of <displayName> (…<last 6>). Sign in to the launcher as <login>, or fix the launcher account ID on the account page." The throw fails the switch and rolls back (section 9).

Hand-over (`waitForManualSignIn`, `adapter.ts:1150-1168`, `230-246`): `until` = now + 10 min; cause `new` on the credentials strategy, `expired` when the failure reason contains "expired" (case-insensitive), else `other`; `canCopyPassword` = a password is stored and `getPassword` exists; `reason` = the masked failure reason or null. Attention `kind: 'sign-in'`: title "Sign in to Epic Games Launcher as <login>" (`new`) or "Epic Games Launcher is asking you to sign in as <login>"; message "No session is stored for this account yet." (`new`), "Its remembered session has expired." (`expired`), or "The launcher did not sign in on its own<' (<reason>)'>."; steps: ["Sign in as <login> in the Epic Games Launcher window. One Launch saves the sign-in and continues on its own, and later switches are automatic."]. Step `manual` detail: the title while running; then "Signed in as <login>.<linked>", "Signed in as a different account.", "Stopped waiting.", or "No sign-in after 10 min.". `onAttention(null)` on exit.

Recapture (`captureFresh(waitMs)`, `adapter.ts:1174-1199`): polls the settings file every `logPollMs` for a remembered session whose fingerprint differs from what the switch wrote (`writtenFingerprint`: the stored blob's, null after a sign-out, or the pre-switch fingerprint when nothing was written); found → `storeSession` with the expected uid and `markVerified`, "Stored <displayName>'s session (fingerprint <8 hex>)."; at the deadline or on abort: the file's fingerprint equals the stored one → `markVerified`, "The stored session is current."; else "The launcher has not written the new session yet; it is saved at the next switch.". Always ends `done`.

Outcomes:

- **ok**: above.
- **needsSignIn** (`adapter.ts:1210-1217`): `restored: null`, "One Launch waited 10 min for the sign-in as <login> and gave up. Epic Games Launcher is still on its sign-in page: sign in there, and the session is saved at the next switch."
- **cancelled** after the launcher was started (`adapter.ts:1200-1209`, `1289`): `restored: null`; "Stopped waiting for the sign-in as <login>. Epic Games Launcher was left on its sign-in page; signing in there finishes the switch, and the session is saved at the next one." after the hand-over began, else "Stopped while Epic Games Launcher was starting as <login>. Nothing was rolled back: the launcher carries on signing in on its own.".
- **cancelled** before any write: `restored: null`, "Stopped before Epic Games Launcher was started as <login>. Nothing had been changed."; the running step gets "Stopped." (`adapter.ts:1284-1291`).
- **cancelled** after a write, before the start: rollback, `cancelled: true`, `restored: true`, "Stopped before Epic Games Launcher was started as <login>.".
- **failed**, nothing modified: `restored: null` with the message (a refusal code logs at WARN, else ERROR). Note: the snapshot may already exist and have reached the journal (`onBackup` fires in `takeSnapshot`), so `backupId` can be non-null with `restored: null` (`docs/switching.md:119`).
- **failed** after a write (`adapter.ts:1293-1310`): no snapshot → `restored: false`, "<message> (no backup was available to restore)"; else the launcher is shut down if running (no signal; failure only logged), `applyRestore` → `restored: true`; a restore failure → `restored: false`, "<message>. Restoring backup <id> also failed: <reason>".

##### 8. Cancellation

Abort checks sit before the close, after it, and at the top of the start step; every wait takes the signal (`adapter.ts:988-990`, `1090`, `1228`, `1234`):

- During the close (including its restart watches) or before the writes: cancelled, `restored: null`, "... Nothing had been changed.".
- After a write, before the spawn: rollback, cancelled, `restored: true`.
- During the process wait, sign-in wait or hand-over: cancelled, `restored: null`, the file stands and the launcher keeps signing in.
- During `captureFresh`: the loop ends at once (storing if the fresh blob is already there), `readyAt` is set and the result is `ok: true`; `LaunchService` then skips the game launch (`docs/switching.md:75`).
- The rollback's `shutdown` has no signal and cannot be interrupted.

##### 9. Rollback and `applyRestore`

- `restoreBackup(id)` (`adapter.ts:1316-1332`): requires an install; refuses with `LAUNCHER_RUNNING` "Close Epic Games Launcher before restoring: it rewrites GameUserSettings.ini while it runs, which would undo the restore." while the launcher runs; snapshots every existing settings file ("Before restoring <id>", `sensitive`, `protect: id`, pinned with "Holds a sign-in that no One Launch account owns" when the file now remembers an unowned session); then `applyRestore(id)`.
- `applyRestore(id)` (`adapter.ts:1341-1349`): `backups.restore('epic', id, [Windows path, WindowsEditor path])`: refuses a snapshot whose recorded source is neither of those two paths, verifies hashes, decrypts `.enc` copies (a missing vault fails with "Snapshot file <relative> is encrypted and no vault is available to decrypt it"), writes all files atomically; files recorded as not existing are left alone. Result `{ files: [paths written], registry: [] }`. No registry values exist for Epic.
- A switch rollback closes the launcher (no signal) and calls `applyRestore` directly, without a before-restore snapshot (`adapter.ts:1297-1298`).

##### 10. Launch (`launchGame`)

`launchGame(game)` (`adapter.ts:627-635`): the id must match `^[A-Za-z0-9_.-]{1,128}$`, else a plain Error "Not an Epic app name: <id>". The manifests are read to find the game's `CatalogNamespace` and `CatalogItemId` (unknown when no manifest matches). The URI depends on the `epic.launchUri` setting, `catalog` unless stored as `appname` (`src/main/index.ts:533-536`, `manifests.ts:123-129`):

- `catalog` with both ids known: `com.epicgames.launcher://apps/<CatalogNamespace>%3A<CatalogItemId>%3A<AppName percent-encoded>?action=launch&silent=true` (the form the launcher writes into its own shortcuts).
- `appname`, or `catalog` with an id missing: `com.epicgames.launcher://apps/<AppName percent-encoded>?action=launch&silent=true`.

Only `AppName` is percent-encoded; the catalog ids go in as read. The URI is opened through the shell and returned as `command`. Nothing is confirmed afterwards, `handOff` is never returned, `signal`/`startedBySwitch` are ignored, and the adapter never throws `GAME_NOT_PLAYABLE` or `GAME_RUNNING`.

##### 11. Health check

`healthCheck(known?)` (`adapter.ts:1353-1482`) reuses the given install and running state. Status = worst row, or `absent`.

| id | label | status | detail |
| --- | --- | --- | --- |
| `install` | "Installation" | `absent` (report ends after `chosen-folder`) | "Epic Games Launcher is not installed on this PC. Checked the Uninstall key, the com.epicgames.launcher URI handler, and C:\Program Files (x86)\Epic Games\Launcher." |
| `install` | "Installation" | `ok` | "<installDir> (from <the folder chosen in Settings | default location | HKLM>)" |
| `chosen-folder` | "Chosen folder" | `warn` when a chosen folder is not the install in use | "EpicGamesLauncher.exe is no longer in <chosen>, the folder chosen in Settings, so <the installation found automatically is used | nothing is used>. Use Locate to choose it again, or Reset to go back to automatic detection." |
| `process` | "Process" | `ok` / `warn` | "EpicGamesLauncher.exe is running" / "EpicGamesLauncher.exe is not running" / "Could not query the process list: <reason>" |
| `manifests` | "Manifests" | `warn` folder missing; `warn` when any manifest is unreadable; `ok`; `fail` on error | "<dir> not found. No Epic games are installed, or the launcher has never installed one." / "<n> .item manifest(s) in <dir><', <m> unreadable'>; <k> installed game(s)" / "Could not scan the manifests: <reason>" |
| `launcher-installed` | "LauncherInstalled.dat" | `warn` missing; `warn` when an installed game is not listed; `warn` unparsable; else `ok` | "<path> not found; the cross-check is skipped." / "<n> entry(s)<'; <m> installed game(s) not listed there' | ', agrees with the manifests'>" / "Could not parse <path>: <reason>" |
| `settings` | "GameUserSettings.ini" | `warn` none exists; `ok` when `Enable` is `True` or absent; `warn` otherwise; `fail` unreadable | "<path> not found. The launcher has not run for this Windows user yet." / "<'<n> settings files, reading the newest (Windows|WindowsEditor); '><k> account(s) have signed in on this PC; <remembered session belongs to <owner> | remembered session not linked to any One Launch account | no remembered session (signed out)><'; [RememberMe] Enable=<value>, so the launcher does not sign in on its own until a switch sets it back'>" / the `LAUNCHER_FILE` message, or "Could not read <path>: <reason>" |
| `log` | "Launcher log" | `warn` missing; `ok` no attempt or success; `warn` last failed | "<path> not found. Sign-ins cannot be confirmed until the launcher has run once." / "Readable; no sign-in attempt in the current log" / "Readable; the last run signed in" / "Readable; the last sign-in failed (<reason>)" |
| `sessions` | "Session snapshots" | `warn` when any is stale, else `ok` | "<n> snapshot(s) stored<', <m> marked stale'>" |
| `backups` | "Backups" | `ok` | "No snapshots yet" / "<n> snapshot(s) available for restore" |

Note: "entry(s)" comes from `plural(n, 'entry')`, which appends "s" ("entrys") for n != 1 (`adapter.ts:182-184`, `1412`).

##### 12. Session capture and `sessionState`

Blob format (`session.ts:15-20`, `56-77`): JSON `{ "v": 1, "rememberMe": { "enable": "<Enable value, default True>", "data": "<[RememberMe] Data>" }, "offline": { "data": "<[Offline] Data>" } | null }`, vault-encrypted by the session store; a blob with `v != 1` or an empty `rememberMe.data` is rejected on parse. Fingerprint = SHA-256 hex of `rememberMe.data`; the UI shows its first 8 hex (`session.ts:32-39`). The file "remembers" a session when `[RememberMe] Enable` is `true` (case-insensitive) and `Data` is non-blank (`session.ts:41-54`). A `Data` value is described to the user as "(<n> characters, fingerprint <8 hex>)" when it looks like base64 of 16+ characters, else as "(<n> characters)" or the value itself when 8 characters or shorter (`session.ts:79-84`).

Who owns the session on disk (`currentSession`, `adapter.ts:433-451`): `loggedUid` = the current log's last successful sign-in id; `diskCurrent` = the settings file exists and (the log has no timed sign-in, or the file's mtime is at or after it); `owner`: while the launcher runs, the One Launch account whose `launcherUid` is `loggedUid` (the file can hold a session the running launcher is not using); while closed and the file remembers a session, the account whose stored snapshot has the file's fingerprint, else (if `diskCurrent`) the account linked to `loggedUid`; otherwise null.

`sessionState()` (`adapter.ts:453-465`): `{ launcher: 'epic', remembered, fingerprint (when remembered), launcherUid: running ? loggedUid : remembered ? owner's uid or loggedUid : null, linkedAccountId: owner's id or null, launcherRunning, lastSignIn: { ok, reason (null on success) } | null }`. `lastSignIn` is the current log file's last `OnLoginComplete` line, whole file.

`captureSession(account)` (`adapter.ts:467-504`) stores the launcher's current session as the account's snapshot and returns `{ session, launcherUid }`. Refusals, all `SWITCH_BLOCKED`:

- Unsaved account: "Save the account first, then capture its session."
- No remembered session in the file: "Epic Games Launcher is not remembering a session right now. Sign in to it as <login>, exit it once so it saves the session, then save again."
- Running, no successful sign-in in this run: "Epic Games Launcher is running but has not signed in during this run<' (<reason>)'>. Sign in to it as <login>, then save again."
- Running, file older than the sign-in: "Epic Games Launcher has not saved its current sign-in to disk yet. Exit the launcher (it writes the session on exit), then save again."
- Owned by another account: "The session Epic Games Launcher currently remembers belongs to <owner displayName>. Sign in to the launcher as <login> first."
- The uid (log's when running, else owner's or log's) differs from the account's linked uid: "The launcher's last sign-in was Epic account …<last 6>, but <displayName> is linked to …<last 6>. Check the launcher account ID on the account page."

`storeSession` (`adapter.ts:511-528`) links the uid to the account first (a duplicate throws and nothing is stored), then saves the encrypted blob with its fingerprint; a replaced snapshot starts unverified and not stale (`src/main/services/sessions.ts:97-104`). Stale marking: only a FALSE line whose reason matches the session-rejection allow-list (section 14) marks the snapshot stale, with that masked reason; a service or network failure does not (`adapter.ts:1268-1270`). `markVerified` clears `staleReason` and sets `verifiedAt` (`sessions.ts:111-113`).

##### 13. Error codes

| Code | Where | Message |
| --- | --- | --- |
| `NOT_INSTALLED` | anything needing the launcher | "epic is not installed on this PC" |
| `LAUNCHER_FILE` | `readIni`, oversize reads | "GameUserSettings.ini is not UTF-8 (UTF-16 or binary content); One Launch will not rewrite it: <path>", "Could not read GameUserSettings.ini (<reason>): <path>", "Too large to be a launcher file (<n> bytes): <path>" (`adapter.ts:416-419`, `read-file.ts:9-11`) |
| `SWITCH_BLOCKED` | `runSwitch`, `writeSession`, `captureSession` | the missing-file warning (section 6); "<path> is missing. Restore the newest Epic Games backup from Settings, or start the launcher once by hand, then try again."; "No session snapshot is stored for <displayName>."; the six capture refusals (section 12) |
| `CONFIRM_REQUIRED` | `runSwitch` | the `closeMessage` (section 6) |
| `LAUNCHER_RUNNING` | `restoreBackup` | "Close Epic Games Launcher before restoring: it rewrites GameUserSettings.ini while it runs, which would undo the restore." |
| `DUPLICATE` | `setLauncherUid` during adoption or capture | propagates from the accounts service (`src/main/services/launchers.ts:72-85`) |
| `PROCESS_ACCESS_DENIED` | shutdown's close or kill | see intro |
| `REGISTRY` | failing `reg.exe` reads | "reg query failed for <hive>\<key>\<name>: <reason>" |
| `BACKUP` | snapshot or restore | "Snapshot <id> not found for epic", "Snapshot <id> was taken for <other>, not epic", "Snapshot <id> names a file that is not one of epic's: <path>", "Snapshot file is corrupt: <relative>", "Snapshot file <relative> is encrypted and no vault is available to decrypt it" (`backup.ts:225-229`, `264`, `282-284`) |
| `MANIFEST_FORMAT` (`EpicManifestError`) | manifest parsing | "not valid JSON (<reason>)", "not a JSON object", "manifest has no AppName", "manifest for <AppName> has no InstallLocation", "LauncherInstalled.dat is not valid JSON (<reason>)", "LauncherInstalled.dat has no InstallationList"; surfaced only as a skipped manifest or a health detail (`manifests.ts:9-15`, `48-60`, `97-105`) |
| `ABORTED` | internal | "The switch was stopped"; never shown |
| plain `Error` | `runSwitch`, `shutdown`, `launchGame`, `parseBlob` | "EpicGamesLauncher.exe is still running after a window close and a forced close", "EpicGamesLauncher.exe keeps restarting itself after being closed", "EpicGamesLauncher.exe did not appear within 60 s", the two started-meanwhile messages, the two wrong-account messages, "Not an Epic app name: <id>", "Stored Epic session has an unexpected shape" |

`LauncherService.guard` wraps everything as "Epic Games: <reason>" with the inner code kept (`src/main/services/launchers.ts:26-34`).

##### 14. Log parsing

`EpicGamesLauncher.log` (`launcher-log.ts`). Lines longer than 16 KiB are skipped (`launcher-log.ts:63`, `log-read.ts:3-4`). Recognised, in match priority per line:

| Contains | Event |
| --- | --- |
| `FAccountService::AutoSignIn` | auto-sign-in (informational) |
| `FAccountService::SignIn` (seen as `FAccountService::SignIn (exchange code)`) | manual-sign-in (informational) |
| `FAccountService::OnLoginComplete - <n> TRUE <id>` | signed-in with the first token after TRUE as the account id when it is 32 hex (lowercased), else ""; `at` = the line's UTC prefix as ISO or null |
| `FAccountService::OnLoginComplete - <n> FALSE <text>` | sign-in-failed with the masked reason: a leading `errormessage=` dropped, everything from `errorpath=` on dropped, every 32-hex value replaced by "…", trimmed (`launcher-log.ts:53-58`). Observed: "Sorry the refresh token '…' is invalid. Reason: TOKEN_EXPIRED errorcode=18036" |
| `LogInit: AllCheckComplete` | ready (startup finished) |
| `Application: Restarting` and `EpicGamesUpdater.exe` on the same line | restart announced |
| `LogCore: Engine exit requested`, or a line starting `Log file closed` | exit |

Decisive sign-in = the newest signed-in or sign-in-failed event (`launcher-log.ts:98-105`). Session-rejection allow-list (marks stale): the reason matches `TOKEN_EXPIRED`, `refresh token`, `invalid_grant`, or `token (is|was|has)? (invalid|expired|revoked)`, case-insensitive (`launcher-log.ts:111-113`); only `TOKEN_EXPIRED` has been observed.

Marks and reads: a mark is the file size plus its first line (`Log file open, <date>` identifies a run; read from the first 256 bytes). A read since a mark covers the appended bytes when the first line is unchanged and the file has not shrunk, else the whole file (the launcher starts a new log on every start and renames the old one `EpicGamesLauncher-backup-*.log`); at most the last 8 MiB, with a clipped read's first partial line dropped (`launcher-log.ts:125-168`, `log-read.ts:18-27`). `lastSignInInLog` reads the whole current file (`launcher-log.ts:171-173`). The switch takes its mark before the close and again right before the start (`adapter.ts:1087`, `1091`); `shutdown` takes one per attempt (`adapter.ts:601`).

Deliberately ignored: `FAccountService::SignOut` (logged on every normal shutdown, says nothing about the remembered session); `OnLoginChanged`; everything else (`launcher-log.ts:13-15`).

#### Adapter: Battle.net

Launcher id `battlenet`, display name `Battle.net` (`src/main/launchers/battlenet/adapter.ts:117,299-300`). The client signs in from one cached token, a `REG_BINARY` DPAPI blob under `HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth`, which it rewrites at every sign-in and deletes at logout; the token is accepted only for the login that is first in `Client.SavedAccountNames` of `Battle.net.config`. A switch swaps that value for the target's stored copy (or removes it so the login page opens), puts the target login first in the list, restarts the client and confirms the sign-in from the client's own log (`adapter.ts:1-11`). The adapter is constructed with `appData`, `localAppData`, `programData`, a default install folder, a `battlenet.path` reader, a `battlenet.launch` reader, `shell.openExternal`, the vault, the registry, the process controller, the backup and session stores and a timing table (`adapter.ts:165-189,309-317`).

Timing table (`DEFAULT_TIMING`, `adapter.ts:136-159`), every value in milliseconds, merged with per-platform overrides through `mergeTiming` (an override that shortens `pollMs` also caps every `*PollMs` to it, `src/main/launchers/wait.ts:31-39`):

| Name | Default | Used for |
| --- | --- | --- |
| `pollMs` | 500 | process-list waits |
| `logPollMs` | 250 | waits that read the log or a registry value |
| `shutdownMs` | 30 000 | window close before a forced kill |
| `killGraceMs` | 10 000 | wait after a forced kill |
| `restartWatchMs` | 5 000 | watch for a self-restart after exit |
| `cleanExitWatchMs` | 1 500 | the same watch when the log showed the client shutting itself down |
| `restartSettleMs` | 45 000 | a restarted client's time to show its main window before it is closed again |
| `startupMs` | 90 000 | wait for `Battle.net.exe` after the start |
| `signInMs` | 60 000 | remembered-path sign-in wait |
| `manualSignInMs` | 600 000 | hand-over wait |
| `captureMs` | 10 000 | wait for the fresh token after a manual sign-in |
| `readyGraceMs` | 5 000 | `readyAt` offset after a confirmed sign-in |
| `launchConfirmMs` | 15 000 | watch for the game start after a launch command |

**[dev-only]** The fake launchers use `SIM_TIMING` (`pollMs` 100, `logPollMs` 100, `shutdownMs` 3 000, `killGraceMs` 1 000, `restartWatchMs` 500, `cleanExitWatchMs` 200, `restartSettleMs` 2 000, `startupMs` 5 000, `signInMs` 8 000, `manualSignInMs` 30 000, `captureMs` 3 000, `readyGraceMs` 300, `launchConfirmMs` 2 000; `src/main/launchers/sim/scenario.ts:22-39`).

##### 1. Files and registry it reads; what it writes

Reads (every file goes through `readLauncherText` / `readLauncherBytes`, which refuse a file over 4 MiB with a `LAUNCHER_FILE` error before reading it, `src/main/launchers/read-file.ts:5,13-23`):

| Path | Format | What is read | Cap | On failure |
| --- | --- | --- | --- | --- |
| `%APPDATA%\Battle.net\Battle.net.config` (`adapter.ts:321-327`) | Qt JSON (4-space indent, CRLF, trailing newline, `\/` escapes; optional BOM) (`config.ts:1-6,28-37`) | `Client.AutoLogin` (string as written), `Client.SavedAccountNames` (comma-separated, trimmed, empties dropped), `Client.Version.Release.LastBuildVersion`; `Games.<codename>.LastPlayed` / `LastInstalled` (Unix seconds, 9 to 11 digits, else null) and `ServerUid` (`config.ts:84-136`); whether the whole file round-trips byte for byte (`config.ts:60-67`) | 4 MiB | missing file: an empty state (no error). Unreadable: `LAUNCHER_FILE` “Could not read Battle.net.config (<reason>)”. Not JSON / not an object: kept as `error` text, config treated as unusable, the file step skipped and a plan warning added (`adapter.ts:406-423,944-950,1040-1050`). A UTF-16 file fails parsing this way. |
| `%APPDATA%\Battle.net\<1-8 hex>.config` (per-account file; the current account id's file, else the newest by mtime) (`adapter.ts:163,426-448`) | same JSON | `User.Client.HideOnClose`, `User.Client.DisplayExitPrompt` as booleans (`config.ts:144-152`) | 4 MiB | read failure logged at WARN and treated as no preferences |
| `%LOCALAPPDATA%\Battle.net\Logs\battle.net-<UTC stamp>.log` (`client-log.ts:15-19`) | text log, one file per run | sign-in, sign-out, product-state, game events (section 14) | tail of 8 MiB (`client-log.ts:206`), lines over 16 KiB skipped (`src/main/launchers/log-read.ts:4`) | missing folder or unreadable file: no events |
| `%ProgramData%\Battle.net\Agent\product.db` (`adapter.ts:333-335`) | protobuf, scraped for printable strings | (codename, TACT code, install path) triples; at most 512 (`products.ts:100-137`) | 4 MiB | missing: empty list; unreadable: WARN, empty list (`adapter.ts:469-478`) |
| `%ProgramData%\Battle.net\Agent\aggregate.json` (`adapter.ts:337-339`) | JSON `{installed:[{product_id,name,launch_uri,icon_path,last_played_timestamp}]}` | health row only (`adapter.ts:1699-1712`, `products.ts:148-173`) | 4 MiB | unparsable: health `warn` |
| `HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth` (`session.ts:12-13`) | registry values | every `REG_BINARY` value whose data is an even run of hex digits, name plus lower-cased hex; other value types ignored (`session.ts:39-41`, `adapter.ts:515-517`) | none | a listing failure surfaces as a health `warn` “Could not read HKCU\…\UnifiedAuth: <reason>” (`adapter.ts:1758`) and otherwise propagates |
| `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Battle.net` › `InstallLocation` | registry string | detection (`adapter.ts:359-363`) | | absent: next candidate |
| `HKLM\SOFTWARE\Classes\battlenet\shell\open\command` › default value | registry string, first quoted token | detection (`adapter.ts:364-369`) | | absent: next candidate |
| `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall` subtree, filtered | registry search, then one value listing per hit | installed products (section 3) | | an unreadable product key is skipped at debug (`adapter.ts:453-467`) |

Writes. Only two launcher-owned things are ever written, only in `runSwitch` and `applyRestore`, after a snapshot:

- The `UnifiedAuth` values: a `REG_BINARY` value set under the name recorded in the stored blob, and every other value under the key deleted (`adapter.ts:1250-1284`, `1601-1620`). The token bytes are decrypted from the vault at that moment and nowhere else except the restore (`adapter.ts:1249,1596-1603`). `Client.AutoLogin` is never written; `Identity`, `EncryptionKey`, `Launch Options`, the `<hex>.config` files, `CachedData.db`, `BrowserCaches\` and `%ProgramData%\Battle.net` are never touched (`adapter.ts:923`, `docs/adapters/battlenet.md` › Rules).
- `Battle.net.config`, rewritten with the target login first in `Client.SavedAccountNames` through `writeFileAtomic` (temp file in the same folder, then rename) (`adapter.ts:1314`). The rewrite is allowed only when the unmodified document serialises back to the exact bytes it was read from: the layout (EOL, indent width from the first indented key, trailing newline, `\/` escaping, BOM) is detected and reproduced, and `planSavedAccountNames` throws `CONFIG_FORMAT` “Battle.net.config does not round-trip through the JSON writer, so One Launch will not rewrite it” otherwise, or “Battle.net.config has no Client section” (`config.ts:28-57,168-186`). The file is never created (`adapter.ts:945,1292-1294`).

Snapshot contents: `Battle.net.config` as a plain file (when it exists) plus `meta.unifiedAuth`, a JSON string `[{name, enc}]` where `enc` is the vault-encrypted hex, base64 (`adapter.ts:1238-1244,1553-1555`).

##### 2. Detection

Ordered ladder; the first candidate folder that contains `Battle.net.exe` wins (`adapter.ts:354-371,383-390`):

1. The folder from the `battlenet.path` setting, trimmed and normalised (`source: 'user'`) (`adapter.ts:356-358,373-375`; `src/main/launchers/chosen-folder.ts:6-9`). When the exe is gone from it, detection continues and health adds a `chosen-folder` warning (section 11).
2. `HKLM\…\Uninstall\Battle.net` › `InstallLocation` (`source: 'registry-hklm'`).
3. The first quoted token of `HKLM\SOFTWARE\Classes\battlenet\shell\open\command`, its folder (`source: 'registry-hklm'`).
4. `C:\Program Files (x86)\Battle.net` (`source: 'default-path'`) (`adapter.ts:116`).

Locate validation (`installFromExe`): the picked file must be named `Battle.net.exe` or `Battle.net Launcher.exe` (case-insensitive), and its folder must contain `Battle.net.exe`; the result has `source: 'user'` (`adapter.ts:378-381`; `chosen-folder.ts:12-15`).

Which exe is started: `Battle.net Launcher.exe` from the install folder when it exists (what the shortcuts run; it applies pending updates and starts the client with `--from-launcher`), else `Battle.net.exe` (`adapter.ts:399-402`). `LauncherInstall.exePath` is always `Battle.net.exe`; the launch command (section 10) uses `exePath`, the switch uses `startExe`.

Every public operation that needs the install calls `requireInstall`, which throws `NOT_INSTALLED` (“battlenet is not installed on this PC”) when the ladder finds nothing (`adapter.ts:392-396`; `src/main/launchers/types.ts:198-204`).

##### 3. Installed games scan

- Registry search: `searchTree('HKLM', 'SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall', 'Blizzard Uninstaller')`, which runs `reg query <key> /s /f "Blizzard Uninstaller" /d` under `chcp 65001` (UTF-8 output), then one `listSubkeyValues` per hit, in parallel, with the subkey name passed exactly as reg.exe printed it (`adapter.ts:453-467`; `src/main/platform/registry.ts:27-32,90-95,257-262`). A full `/s` dump of the Uninstall tree is refused by design: it is about 165 K characters and takes tens of seconds under load, stalling the status pass (`docs/adapters/battlenet.md` › Library; `docs/architecture.md:38`).
- An entry counts when it is a direct subkey (no `\` in its relative name) and its `UninstallString` matches `Blizzard Uninstaller.exe` (case-insensitive). Codename = the `--uid=` argument (quoted or bare); `battle.net` and an empty uid are dropped. Name = `DisplayName` trimmed, else the product table's name, else the codename. Folder = `InstallLocation` normalised, or null (`products.ts:58-92`).
- A product with no folder or a folder that does not exist is skipped (`adapter.ts:494-497`).
- `InstalledGame`: `launcherGameId` = codename; `installPath` = folder; `libraryPath` = its parent; `sizeOnDiskBytes` null; `lastPlayedAt` from `Games.<codename>.LastPlayed` in `Battle.net.config` (Unix seconds, or null); `needsUpdate` from the client log (below). Sorted by name, case-insensitive (`adapter.ts:489-511`).
- `needsUpdate`: the newest client run's last `InstallState (<codename>): currentOperation=…` line decides; `Update` (case-insensitive) means the Play button reads Update. With no log every product reads up to date (`product-state.ts:18-27`; `adapter.ts:491-492`).
- Product table (`PRODUCTS`, `products.ts:25-48`). Each product has three names: the codename (adapter id, `--uid=`, `Games` key), the program code (`--exec=launch <code>`), the TACT code (`battlenet://game/<tact>`, `product.db`). Only `prometheus` and `hs_beta` are verified; the rest are table values marked `verified: false`:

| codename | program code | TACT code | name | Steam appid |
| --- | --- | --- | --- | --- |
| `prometheus` | `Pro` | `pro` | Overwatch | 2357570 |
| `hs_beta` | `WTCG` | `hsb` | Hearthstone | |
| `wow` | `WoW` | `wow` | World of Warcraft | |
| `wow_classic` | `WoWC` | `wow_classic` | World of Warcraft Classic | |
| `wow_classic_era` | `WoWC` | `wow_classic_era` | World of Warcraft Classic Era | |
| `diablo3` | `D3` | `d3` | Diablo III | |
| `fenris` | `Fen` | `fenris` | Diablo IV | 2344520 |
| `osi` | `OSI` | `osi` | Diablo II: Resurrected | |
| `anbs` | `ANBS` | `anbs` | Diablo Immortal | |
| `s2` | `S2` | `s2` | StarCraft II | |
| `s1` | `S1` | `s1` | StarCraft | |
| `heroes` | `Hero` | `hero` | Heroes of the Storm | |
| `w3` | `W3` | `w3` | Warcraft III: Reforged | |
| `odin` | `ODIN` | `odin` | Call of Duty: Modern Warfare | |
| `zeus` | `ZEUS` | `zeus` | Call of Duty: Black Ops Cold War | 1985810 |
| `fore` | `FORE` | `fore` | Call of Duty: Vanguard | 1985820 |
| `lazr` | `LAZR` | `lazr` | Call of Duty: Modern Warfare II | 1938090 |
| `auks` | `AUKS` | `auks` | Call of Duty | 1938090 |
| `viper` | `VIPR` | `viper` | Call of Duty: Black Ops 4 | |
| `rtro` | `RTRO` | `rtro` | Blizzard Arcade Collection | |
| `wlby` | `WLBY` | `wlby` | Warcraft III | |
| `gryphon` | `GRY` | `gryphon` | Warcraft Rumble | |

- `product.db` fills gaps: when it names a TACT code for a codename the table lacks, the program code is guessed as that TACT code upper-cased (`verified: false`, name = codename); when the table and the db disagree on the TACT code, the db's wins (`products.ts:51-56`; `adapter.ts:480-482`). The scrape walks printable runs of 2+ characters, accepts a triple when the codename matches `^[a-z][a-z0-9_.]{1,31}$`, the TACT code `^[a-z][a-z0-9_]{1,15}$` and the third string ends in a drive path; `agent` and `battle.net` and duplicates are skipped; at most 512 entries (`products.ts:100-137`).

##### 4. Running state

- `isRunning`: the process controller's image-name check for `Battle.net.exe` (`adapter.ts:662-664`).
- `runningGame`: requires an install (else null), lists processes with paths; a listing failure is logged at WARN and read as no game; an empty list is no game. Otherwise the installed games are listed (a registry search, section 3) and the first process whose real path lies under a game's install folder (the folder as recorded and with junctions and links resolved) names the game, `source: 'process'` (`adapter.ts:667-680`; `src/main/launchers/running-game.ts:22-37`).
- The log's view of a game (used by `launchGame`, not by `runningGame`): `gameRunning(events, codename)` is true when the run's last game event for that codename is `game-launching` or `game-running` and no `game-exited` followed (`product-state.ts:39-47`).

##### 5. Remembered accounts

`listRememberedAccounts` returns one entry per login in `Client.SavedAccountNames`, in file order, with `launcherUid: ''` (the client records no ids there; the id is learnt at the first confirmed sign-in), `personaName: ''`, `rememberPassword: true`, `lastLoginAt: null`, `sessionMayBeExpired: false`, and `mostRecent` true only for the login equal (trimmed, case-insensitive) to the current token's owner login (`adapter.ts:637-658`). An empty list when the config is missing, unparsable or has no names. The config `SavedAccountNames` is also the plan's `current.autoLoginUser` (its first entry) (`adapter.ts:1114`).

##### 6. Preflight and plan

`currentSession(running)` (`adapter.ts:520-558`) decides who owns the token:

- `tokens` = every `UnifiedAuth` value; `token` = the first whose fingerprint (SHA-256 of the lower-case hex, `session.ts:27-29`) matches a stored snapshot, else the first value; `remembered` = a token exists.
- `loggedUid` = the account id of the current run's last successful sign-in; `signedOut` = the run's last sign-in event is a sign-out.
- `owner`: while running, the account linked to `loggedUid` when the last sign-in succeeded, else null. When closed and a token exists: the stored snapshot's owner, else the account linked to `loggedUid` unless the run ended signed out. When closed and no token: null.

Plan context (`adapter.ts:832-837`):

- `inPlace` = the owner is the target account and (the client runs or a token exists).
- `strategy` = `remembered` when `inPlace` or a non-stale snapshot exists for the account, else `credentials`.
- `alreadyActive` = `inPlace` and running.
- `expectedUid` = the snapshot's `launcherUid`, else the account's `launcherUid`, else null; it goes on `plan.account.launcherUid`.
- `executable` is always true (`adapter.ts:1118`).
- `current` = `{ mostRecentLogin: owner login or null, autoLoginUser: first SavedAccountNames entry or null, running }`.

`preflight(account)` builds the plan, drops the close warning from it (`withoutCloseWarning`), reads `runningGame()` only when the client runs and is not already active, and answers `closeMessage`: “Battle.net is running <game>. Switching closes Battle.net; the game keeps running.” with a game, else “Battle.net is running. Switching closes it.” (`adapter.ts:794-808`). The `forGame` argument is ignored: Battle.net starts the client bare.

Plan steps, in order (`adapter.ts:846-1038`). `H` = `HideOnClose` true, `P` = `DisplayExitPrompt` true in the per-account file:

| id | kind | title | target | skipped when | detail |
| --- | --- | --- | --- | --- | --- |
| `close` | process | “Close Battle.net” | `Battle.net.exe` | not running, or alreadyActive | alreadyActive: “Skipped: the client is already signed in as this account, so it is left running.” Running: “Send Battle.net.exe a window close (which exits the client[, except that this account has "close to system tray" on][ after the exit prompt]) and wait up to 30 s for every Battle.net.exe process to be gone, since the client writes its config files at the end of its shutdown. If it is still running after that, force-close every Battle.net.exe process and nothing it started. Agent.exe is left alone. A game already running keeps running.” Else “Skipped: Battle.net.exe is not running.” |
| `capture` | backup | inPlace: “Save this account's session”, else “Save the outgoing session” | `HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth` | not `captureNeeded` = outgoing owner exists, not alreadyActive, and (running, or a token exists and (not inPlace or its fingerprint differs from the owner's stored one)) | alreadyActive: “Skipped: the client stays running, so its token is not re-read.” No owner, token: “Skipped: the client remembers a session (fingerprint <8 hex>) that no One Launch account owns, so it is left alone and not saved. To keep it, capture it from that account's page before switching.” No owner, no token: “Skipped: the client is not remembering any session.” Owner but not needed: “Skipped: <owner>'s stored snapshot already matches what the client remembers.” Running: “After the client has closed, read the UnifiedAuth value and, if the last run's log names <owner> and the value changed, store it, encrypted, as the account's snapshot. Nothing of the client's changes.” inPlace: “The UnifiedAuth value holds <owner>'s session but differs from the stored snapshot (the client rotates the token at every sign-in \| no snapshot yet), so store it, encrypted, as the account's snapshot. Nothing of the client's changes.” Else: “The UnifiedAuth value holds <owner>'s session[ and it changed since it was last saved (the client rotates it at every sign-in)]; store it, encrypted, as the account's snapshot if it changed. Nothing of the client's changes.” |
| `backup` | backup | “Back up Battle.net.config and the UnifiedAuth value” | the backups preview path | alreadyActive or inPlace | “Skipped: nothing will be modified.” or “Copy <config path \| (no Battle.net.config to copy)> into a new snapshot folder and record <n> UnifiedAuth value(s) in it, vault-encrypted, so a failed switch can be rolled back. The newest <backups.keep> snapshots are kept.” |
| `registry` | registry | inPlace or stored: “Put the account's session token in place”; credentials: “Sign the client out” | the UnifiedAuth key | inPlace: always; stored: never; credentials: when there is no token | inPlace: “No change: UnifiedAuth already holds <account>'s token (fingerprint <8 hex>).” Stored: “Decrypt <account>'s session snapshot from the vault and write it as the REG_BINARY value under UnifiedAuth (reg add /t REG_BINARY). Any other value there (the outgoing token, saved in the step above when it belongs to a One Launch account) is removed, the way the client's own logout removes it. The bytes are a DPAPI blob bound to this Windows user and are never shown or logged. Identity, EncryptionKey, Launch Options, and every file under the client's folders stay untouched.” with changes `UnifiedAuth\<stored name>`: before `describeToken(existing)` → after “(stored snapshot, fingerprint <8 hex>, captured <ISO>)”, plus `UnifiedAuth\<other>` → “(removed)” for every other value. Credentials, no token: “No change: the client has no remembered session, so it will open on its login page.” Credentials with token(s): “Remove the UnifiedAuth value (reg delete), which is what the client's own logout does, so it opens on its login page instead of signing in as <owner \| the previous account>. "Keep me logged in" (Client.AutoLogin) is not touched, so the next sign-in is remembered.” with one “(removed)” change per value. |
| `file` | file | “Put the login first in the saved-accounts list” | the config path | alreadyActive, or any skip reason | alreadyActive: “Skipped: the client stays running as this account.” Skip reasons: “Skipped: <path> does not exist. One Launch never creates it; the client writes it at its next exit.”, “Skipped: <path> could not be parsed (<error>), so it is left alone.”, “Skipped: <path> does not round-trip through One Launch's JSON writer byte for byte, so it is left alone rather than rewritten with a different layout.”, “No change: <login> is already first in Client.SavedAccountNames.”, “Skipped: <planning error>”. Otherwise: “Move <login> to the front of Client.SavedAccountNames[ (it is not in the list yet, so it is added)], rewriting the file with the same layout (4-space indent, CRLF, "\/" escapes) via a temp file and rename; every other key stays byte for byte. The client moves the signed-in login to the front at every sign-in and at start only accepts the UnifiedAuth token of the login that is first (2026-09-13: every switch with another login first was asked for a password; the one with the target first signed in silently).” with one change `Client.SavedAccountNames`: before → after, every login other than the target masked to its first character plus `…` and the `@domain` (`config.ts:155-159,184-185`). |
| `start` | launch | “Start Battle.net” | `"<startExe>"` | alreadyActive | “Skipped: the client is already running as this account.” Else “Start Battle.net Launcher.exe with no arguments, as the Start Menu shortcut does (it applies any pending update, then starts the client with --from-launcher).” or “Start Battle.net.exe with no arguments.” followed by remembered: “The client signs in as <login> on its own from the UnifiedAuth token.”; credentials: “With no token the client opens on its login page. The "Welcome back" email there comes from the login page's own web session (a cookie One Launch does not touch), so it may name the previous account; "Switch account" on that page lists the saved logins.” |
| `wait` | wait | “Wait for the client to come up” | `Battle.net.exe` | alreadyActive | “Poll the process list until Battle.net.exe is running, up to 90 s.” |
| `verify` | wait | “Confirm the client signed in” | the log folder | alreadyActive | remembered: “Read the new run's log (battle.net-<timestamp>.log; no tokens in it) until "Logged into Battle.net successfully" names <the account id …1234 \| an account id, which is then recorded on the account>, up to 60 s. A "Login failed" line means the token was rejected: the snapshot is marked stale and the next step hands over to you.” Credentials: “Skipped on this path: with no token the client waits on its login page, so the switch hands over to you at once (next step).” AlreadyActive: “Skipped: the client is already signed in as this account.” |
| `manual` | wait | “Wait for you to sign in” | the log folder | strategy remembered | remembered: “Runs only if the client rejects the stored token or does not sign in within 60 s. The client stays on its login page; you sign in there once as <login>[ (the password can be copied from One Launch)] with "Keep me logged in" on, and One Launch continues with the game as soon as the log reports the sign-in, up to 10 min.” Credentials: “The client opens on its login page, showing the last account it signed in as. Click "Switch account" there, pick or type <login>, and sign in[ (the password can be copied from One Launch)] with "Keep me logged in" on. One Launch continues with the game as soon as the log reports the sign-in, up to 10 min. Signing in as a different Battle.net account than the one linked to <account> ends the switch and rolls everything back.” |
| `recapture` | backup | “Save the new session” | the UnifiedAuth key | strategy remembered | remembered: “Runs only after a manual sign-in: the fresh UnifiedAuth value replaces the stale snapshot.” Credentials: “Once the sign-in is reported, read the UnifiedAuth value the client wrote at that sign-in (up to 10 s), store it, encrypted, as <account>'s session snapshot, and record the account id on the account. Later switches use the remembered path.” |

Plan warnings, each added when its condition holds (`adapter.ts:1040-1108`):

- config missing: “<path> does not exist. One Launch never creates it; the saved-accounts step is skipped and the client will write the file at its next exit.”
- config unparsable: “<path> could not be parsed (<error>). The saved-accounts step is skipped; the rest of the switch does not depend on it.”
- config does not round-trip: “<path> has a layout One Launch cannot reproduce exactly, so the saved-accounts step is skipped rather than rewriting the file differently.”
- `Client.AutoLogin` present and not `true` (case-insensitive): “"Keep me logged in" (Client.AutoLogin) is <value> in Battle.net.config, so the client will not sign in on its own from a token. One Launch never changes that setting: turn it on on the client's login page, and the next sign-in is remembered.”
- H: “The current account's Battle.net preferences close to the system tray instead of exiting (<prefs path \| per-account config>: HideOnClose). The close step will wait 30 s and then force-close the client.”
- P: “The current account's Battle.net preferences show an exit prompt on close (DisplayExitPrompt). Confirm it in the client window, or the close step force-closes the client after 30 s.”
- stored path and the file step cannot run and not inPlace: “<login> cannot be put first in Client.SavedAccountNames (see the saved-accounts step), and the client only accepts the token of the login that is first, so it will ask for the password.”
- more than one token: “UnifiedAuth holds <n> values; the client is expected to keep one. The value matching a stored snapshot is treated as the session, the others as leftovers.”
- running and not inPlace (the close warning, removed from the preflight's plan): “Battle.net is running[ as <owner> \| as an account not linked to One Launch \| on its login page]. It will be closed.”
- running, not inPlace, a logged uid with no owner: “The client's current sign-in (Battle.net account …1234) is not linked to any One Launch account, so its session will not be saved.”
- snapshot stale and not inPlace: “The stored session for <account> was rejected by the client (<stale reason>). You will sign in once by hand and One Launch will store the fresh session.”
- credentials with no snapshot and not inPlace: “No session snapshot is stored for <account> yet. The client will open on its login page; sign in once with "Keep me logged in" on and One Launch captures the session for later switches.[ No password is stored in One Launch, so have it ready.]”
- not inPlace, a token exists, no owner: “The client currently remembers a session that no One Launch account owns (fingerprint <8 hex>). It will be removed. The backup taken before this switch keeps a copy and is left out of the usual clean-up, so restoring it from Settings › Backups brings that sign-in back.”
- snapshot id differs from the account's id: “The account id on this account (…1234) differs from the one recorded with its session snapshot (…5678). The snapshot's id is used to confirm the sign-in.”

Plan notes: alreadyActive: “Battle.net is running and signed in as <account>. The real switch would change nothing and would leave the client running.”; inPlace and not running: “Battle.net already remembers <account>'s session, so it is only started.” (`adapter.ts:1074-1077`).

##### 7. Switch (`runSwitch`)

Refusals before anything is touched, in order (`adapter.ts:1152-1156`): a non-executable plan throws `SWITCH_BLOCKED` with the first warning (unreachable here, `executable` is always true); the client running and not alreadyActive without `confirmClose` throws `CONFIRM_REQUIRED` with the `closeMessage` and the running game.

Sequence (`adapter.ts:1468-1517`):

1. Abort check. When running and not alreadyActive: step `close` runs `shutdown` (below); done detail “Client closed.”.
2. Abort check. Then, unless alreadyActive, a running `Battle.net.exe` fails the switch with “Battle.net was started by something else while One Launch was switching accounts, so the switch stopped before changing anything. Try again.” (the `close` step is marked failed) (`adapter.ts:1319-1324,1477`).
3. `saveOutgoing` (step `capture`, only when the plan did not skip it; best effort, every failure becomes a done detail “Not saved: <error>” and a WARN log) (`adapter.ts:1205-1232`): re-reads the session as closed. No token: “The client left no remembered session behind (signed out).”. No owner with an id: “The remembered session belongs to no One Launch account; left alone.”. Owner's stored fingerprint equals the token's: “<owner>'s stored session is already current.”. Else store `{v:1, unifiedAuth:{name, hex}}` for the owner with uid = owner's linked uid or the logged uid, and “Saved <owner>'s session (fingerprint <8 hex>).”.
4. When neither alreadyActive nor inPlace:
   - `takeSnapshot` (step `backup`): reads the tokens, snapshots `Battle.net.config` (when it existed at plan time) with `meta.unifiedAuth` encrypted, reason “Before switching to <account>”, **pinned** with “Holds a sign-in that no One Launch account owns” when the closed client remembers a token no One Launch account owns (`holdsUnownedSession`, `adapter.ts:1591-1594`). `onBackup(id)` fires here. Done detail = the snapshot path (`adapter.ts:1233-1248`).
   - `writeRegistry` (step `registry`) (`adapter.ts:1250-1284`): on `remembered` the account's blob is decrypted (no blob: `SWITCH_BLOCKED` “No session snapshot is stored for <account>.”). The value with the blob's name is set only when absent or different; every other value is deleted (on `credentials`: every value). `writtenFingerprint` = the blob's fingerprint, or null. No change: step skipped “Already in place.” / “Already signed out.”. Else done “UnifiedAuth now holds the snapshot (fingerprint <8 hex>).” / “UnifiedAuth cleared.”. Any write sets `modified`.
   - `writeFile` (step `file`) (`adapter.ts:1285-1317`): when the plan skipped it, `listReady` = the plan found the login first already. Otherwise the file is re-read (the client rewrote it at exit), the move planned again; missing file: skipped “Battle.net.config is missing; One Launch never creates it.”; a `CONFIG_FORMAT` error: skipped “Left alone: <message>”; already first: skipped “<login> is already first in Client.SavedAccountNames.” with `listReady`; else atomic write, `modified`, `listReady`, done “<login> is now first of <n> saved account(s).”.
5. Post-write running check: a running client now fails with “…so the switch was stopped. Try again.” (rolled back, step `start` marked failed) unless alreadyActive, in which case `start` and `wait` are skipped “The client is already running.” (`adapter.ts:1486-1489`).
6. `startClient` (`adapter.ts:1326-1338`): abort check (`stopIfAborted`), a log mark is taken, `launcherStarted` set, `startExe` spawned with no arguments and the install folder as cwd; `start` done “Client started.”; `wait` polls `isRunning` every `pollMs` up to `startupMs` (90 s); timeout throws “Battle.net.exe did not appear within 90 s”; done “Battle.net.exe is running.”.
7. Sign-in confirmation (skipped “The client is already signed in as this account.” when alreadyActive) (`adapter.ts:1377-1394`):
   - `credentials`: one poll (1 ms budget). Signed in: `verify` done “Signed in as <login>.[ Linked to Battle.net account …1234.]”; wrong account: failed “Signed in as a different account.”; anything else: `verify` skipped “Handed over to you: the client is on its login page.” and a `failed` verdict is treated as a timeout.
   - `remembered`: poll every `logPollMs` (250 ms) up to `signInMs` (60 s). Done / failed details: “Signed in as <login>.…”, “Signed in as a different account.”, “The client rejected the session: <reason \| no reason given>.”, “The client did not report a sign-in in time.”, “Cancelled.”.
   - The poll (`waitForSignIn`, `adapter.ts:1340-1360`) reads the events appended since the start mark and takes `lastSignIn`. A success with `expectedUid` null: an id another One Launch account holds is `wrong-account`, any other id (an empty one included) is `signed-in`. With `expectedUid`: equal is `signed-in`, a different non-empty id is `wrong-account`, an empty id keeps polling. A failure verdict ends the wait (`failed`) only outside the manual wait.
   - Identity (`adapter.ts:1361-1376`): `linkIfNew` records the logged id on the account when `expectedUid` was null and the id is non-empty, and appends “ Linked to Battle.net account …1234.” to the step detail. `wrong-account` throws (rollback): with `expectedUid` null “Battle.net signed in as <owner> (account …1234) \| account …1234, which is another One Launch account, instead of <account>. Sign in to the client as <login>.”; otherwise “Battle.net signed in as <who> instead of <account> (…5678). Sign in to the client as <login>, or fix the account ID on the account page.”.
   - A cancelled verdict returns the cancelled result (section 8).
   - On `remembered` after a confirmed sign-in, `captureFresh(0)` (one read, below) keeps the snapshot current because the client rotates the token at every sign-in (`adapter.ts:1500-1503`).
8. Hand-over (`adapter.ts:1395-1413,1504-1512`) when the verdict is not `signed-in`: on `remembered`, a `failed` verdict with `listReady` marks the snapshot stale with the reason (or “rejected by the client”); a rejection while the list could not be fixed does not. Then `waitForManualSignIn`: `manual` running with the attention title as detail, `onAttention` called with (`adapter.ts:276-296`):
   - title: `credentials`: “Sign in to Battle.net as <login>”; else “Battle.net is asking you to sign in as <login>”.
   - message: cause `new` (credentials): “No session is stored for this account yet.”; `expired` (a failed verdict): “The client rejected its stored session.”; `other` (timeout): “The client did not sign in on its own[ (<reason>)].”; each followed by “ Its login page opens on the account it last signed in as.”.
   - steps: “In the Battle.net window, click “Switch account”.”, “Pick or type <login> and enter the password.”, “Leave “Keep me logged in” on and sign in. One Launch continues on its own, and later switches are automatic.”.
   - `reason` = the failure reason or null; `canCopyPassword` = the account has a secret and a password reader was supplied; `until` = now + `manualSignInMs` (10 min).
   - The poll runs up to 10 min; failed attempts are not final (only `signed-in`, `wrong-account`, cancel or the deadline end it). Details: “Signed in as <login>.…”, “Signed in as a different account.”, “Stopped waiting.”, “No sign-in after 10 min.”. `onAttention(null)` always follows.
   - Outcomes: cancelled → cancelled result; wrong account → throw (rollback); timeout → `needsSignIn` result; signed in → `captureFresh(captureMs)` (10 s).
9. `captureFresh(waitMs)` (step `recapture`, `adapter.ts:1415-1440`): polls the tokens every 250 ms for one whose fingerprint differs from `writtenFingerprint`; stores it as `{v:1, unifiedAuth}` with uid `expectedUid`, marks the session verified, done “Stored <account>'s session (fingerprint <8 hex>).”. At the deadline (or abort): if the first token's fingerprint equals the account's stored one, mark verified, “The stored session is current.”; else “The client has not written a new token yet; it is saved at the next switch.”. Runs only for a saved account (`account.id`).
10. `readyAt` = now + `readyGraceMs` (5 s) whenever a sign-in was confirmed in this run (not when alreadyActive): the client binds game families about a second after the sign-in and drops earlier launch requests; only a game launch waits for it (`adapter.ts:1513`; `types.ts:56-60`). `startedWithGame` is always null.
11. Result `ok: true`, `restored: null`, `backupId` (or null) (`adapter.ts:1517`).

`shutdown` (`adapter.ts:692-723`): no-op when `Battle.net.exe` is not running. Up to 3 attempts: take a log mark; window close to `Battle.net.exe`; wait up to `shutdownMs` (30 s, `pollMs`) for every such process to be gone; if not, forced kill **without the tree** (a running game is the client's child) and wait `killGraceMs` (10 s), then “Battle.net.exe is still running after a window close and a forced close”. Then the restart watch: `cleanExitWatchMs` (1.5 s) when the log gained an `exit` event since the mark, else `restartWatchMs` (5 s); a client that does not come back ends the shutdown. One that does: after the third attempt “Battle.net.exe keeps restarting itself after being closed”; otherwise wait up to `restartSettleMs` (45 s) for a new log file that has a `main-window` event, and close again. An elevated client's window refuses the close and the platform raises `PROCESS_ACCESS_DENIED` at once (`docs/architecture.md:42`).

Storing a session (`storeSession`, `adapter.ts:614-632`): the uid is kept only when it matches `^[0-9a-f]{1,16}$` (lower-cased); when it differs from the account's current uid, `setLauncherUid` runs first (and throws `DuplicateAccountError` when another account holds it, so nothing is stored under it); then the blob is encrypted and saved with its fingerprint.

##### 8. Cancellation

The abort signal is checked at the top, after the close, before the start (`stopIfAborted`, `adapter.ts:1195-1197,1469,1475,1328`) and inside every wait (`waitUntil` throws `ABORTED`; the sign-in polls return `cancelled`) (`adapter.ts:1343,1358`; `wait.ts:42-50`).

- Before any write (including during the close, which aborts its waits): result `ok:false, cancelled:true, restored:null`, error “Stopped before Battle.net was started as <login>. Nothing had been changed.”; the running step is marked failed “Stopped.” (`adapter.ts:1519-1527`).
- After a write and before the start: the catch rolls back (client closed first, no signal; snapshot restored): `cancelled:true, restored:true`, error “Stopped before Battle.net was started as <login>.”; `restored:false` with “…(no backup was available to restore)” or “…. Restoring backup <id> also failed: <error>” when it cannot (`adapter.ts:1529-1546`).
- After the client was started (`launcherStarted`): no rollback, `cancelled:true, restored:null`, error “Stopped while Battle.net was starting as <login>. Nothing was rolled back: the client carries on signing in on its own.”; during the hand-over instead “Stopped waiting for the sign-in as <login>. Battle.net was left on its login page; signing in there finishes the switch, and the session is saved at the next one.” (`adapter.ts:1441-1450,1498,1508,1525`).
- During `captureFresh` an abort only ends the token wait; the result is still `ok: true` (`adapter.ts:1428`).

##### 9. Rollback and `applyRestore`

- Failure handling (`adapter.ts:1518-1547`): a refusal thrown before any write is logged at WARN, anything else at ERROR. Not modified: `restored: null` with the error message. Modified without a snapshot id: `restored: false`, “<message> (no backup was available to restore)”. Otherwise: if the client runs it is closed first (`shutdown()` without a signal, failures only logged), then `applyRestore(backupId)`; `restored: true` with the original error, or `restored: false` with “<message>. Restoring backup <id> also failed: <restore error>”.
- `applyRestore(id)` (`adapter.ts:1601-1620`): the snapshot's `meta.unifiedAuth` is decrypted first (a vault failure or a malformed meta, “Snapshot meta has an unexpected shape”, stops the restore before anything is touched); then the backup service puts back only `Battle.net.config` (target list = the current config path); then every recorded value is set under its name and every other existing value deleted. Result `registry` lines: “UnifiedAuth\<name> = (<n> bytes, fingerprint <8 hex>)” and “UnifiedAuth\<name> (removed)”. A snapshot without `unifiedAuth` meta restores the file only. After a lost DPAPI key no Battle.net snapshot restores (`docs/adapters/battlenet.md` › Switch).
- `restoreBackup(id)` (`adapter.ts:1570-1588`): `NOT_INSTALLED` without an install; `LAUNCHER_RUNNING` “Close Battle.net before restoring: it rewrites its token at sign-in and Battle.net.config at exit, which would undo the restore.” while running; then a “Before restoring <id>” snapshot of the config (when present) and the current tokens, `protect: id`, pinned with the unowned-session reason when the closed client remembers an unowned token; then `applyRestore`.

##### 10. Launch (`launchGame`)

(`adapter.ts:736-789`; `startedBySwitch` is accepted and ignored.)

1. The codename must match `^[A-Za-z0-9_.-]{1,64}$`, else a plain error “Not a Battle.net product codename: <id>”.
2. `NOT_INSTALLED` without an install. Codes come from the table plus `product.db`; none: plain error “No launch code is known for <game> (<codename>); it is not in One Launch's product table or in product.db.”.
3. Pre-send refusals from the current run's log, all `GAME_NOT_PLAYABLE` (a refusal code: the job fails with the switch kept, logged at WARN):
   - operation `Update`: “<game> needs an update before it can start. Update it in the Battle.net window, then press Play again.”
   - `playable=0` with another operation: “<game> is not playable right now (<operation \| unknown state>[, <status lower-cased>]). Finish that in the Battle.net window, then press Play again.”
   - the log shows the game launching or running and not exited since (`gameRunning`): “<game> is already running. Close it, then press Play again.” (the client silently drops `--exec` for a game it sees running).
4. The form comes from the `battlenet.launch` setting, `exec` unless stored as `uri` (`src/main/index.ts:551-554`; `products.ts:178-183`):
   - `exec`: `"<installDir>\Battle.net.exe" --exec=launch <ProgramCode>` started as one argv token with the install folder as cwd; a running client receives it through the forwarder.
   - `uri`: `battlenet://game/<tact>` through `shell.openExternal`; in a running client this only selects the game page.
5. Post-send watch: a log mark is taken before sending; the log is polled every `logPollMs` (250 ms) for up to `launchConfirmMs` (15 s) for `gameStarted`: a `game-launching` or `game-running` event for the codename, or a pre-2.52.12 `launch-request` for the program code (`product-state.ts:30-36`).
6. Results (`command` strings, verbatim annotations):
   - abort during the watch: “<command> (stopped watching for the game start)”.
   - confirmed: “<command> (Battle.net started the game)”.
   - `uri` form, nothing logged: “<command> (this link only opens the game page: press Play there, or switch the Battle.net launch setting to the launch command)”.
   - `exec` form, nothing logged: throws “<game> did not start within 15 s of the launch command. Look at the Battle.net window: the game may need an update or a repair, or the client may be busy updating itself.”.
   - No `handOff` is ever returned.

##### 11. Health check

`healthCheck(known?)` takes the install and running state from the caller when given, else detects and checks itself (`adapter.ts:1624-1791`). Overall status = worst row (`fail` > `warn` > `ok`), or `absent`.

| id | label | status | detail |
| --- | --- | --- | --- |
| `install` | Installation | `absent` (report ends here, status `absent`) / `ok` | “Battle.net is not installed on this PC. Checked the Uninstall key, the battlenet:// URI handler, and <default dir>.” / “<installDir> (from the folder chosen in Settings \| HKLM \| default location); starts Battle.net.exe \| Battle.net Launcher.exe” |
| `chosen-folder` | Chosen folder | `warn`, only when a folder is chosen and the install's source is not `user` | “Battle.net.exe is no longer in <chosen>, the folder chosen in Settings, so the installation found automatically is used \| nothing is used. Use Locate to choose it again, or Reset to go back to automatic detection.” (`chosen-folder.ts:18-28`) |
| `process` | Process | `ok` / `warn` | “Battle.net.exe is running” / “Battle.net.exe is not running” / “Could not query the process list: <error>” |
| `uninstall` | Installed products | `warn` when none, `fail` on a read error, else `ok` | “No Blizzard products in the Uninstall key. No Battle.net games are installed, or the Agent has never installed one.” / “<n> product(s) in the Uninstall key: <name> (<codename>), …[; <m> without an install folder]” / “Could not read the Uninstall key: <error>” |
| `product-db` | product.db | `warn` when missing or a game has no code, else `ok` | “<path> not found; launch codes come from the built-in table only.” / “<n> install record(s); launch codes: <codename> → <Program> / <tact> \| unknown, … \| none needed[; no code known for <names>][ (<names>: table value, not verified on this PC)]” |
| `aggregate` | aggregate.json | `ok` / `warn`; row absent when the file is missing | “<n> entry\|entries: <name> → <launch_uri \| tact>, … \| none” / “Could not parse <path>: <error>” |
| `config` | Battle.net.config | `warn` missing; `fail` unparsable or no Client section; `ok` only when AutoLogin is absent or `true` and the layout round-trips, else `warn` | “<path> not found. The client has not run for this Windows user yet.” / “Could not parse <path>: <error \| no Client section>” / “build <version \| ?>; <n> saved account name(s); AutoLogin=<value \| (absent)>[ (the client will not sign in on its own; turn "Keep me logged in" on in the client)]; layout reproducible \| layout not reproducible, so One Launch will not rewrite it” |
| `token` | Session token | `warn` when more than one value or unreadable, else `ok` | “<n> value(s) under HKCU\…\UnifiedAuth; belongs to <owner> \| left over from a run that ended with a logout \| not linked to any One Launch account \| no remembered session (signed out)[ (<n> bytes, fingerprint <8 hex>)]” / “Could not read HKCU\…\UnifiedAuth: <error>” |
| `prefs` | Close behaviour | `warn` when HideOnClose or DisplayExitPrompt is true; row absent when no per-account file | “<path>: HideOnClose=<value \| (absent)>, DisplayExitPrompt=<value \| (absent)>[ (a window close hides the client; the switch will have to force-close it)]” |
| `log` | Client log | `warn` when the folder is missing or the last sign-in failed | “<logDir> not found. Sign-ins cannot be confirmed until the client has run once.” / “Readable; no sign-in in the current log” / “Readable; the last run signed in as account …1234” / “Readable; the last sign-in failed (<reason>)” |
| `sessions` | Session snapshots | `warn` when any is stale | “<n> snapshot(s) stored[, <m> marked stale]” |
| `backups` | Backups | `ok` | “No snapshots yet” / “<n> snapshot(s) available for restore” |

The Uninstall search runs once here and feeds `gamesFrom` directly (`adapter.ts:1663-1664`).

##### 12. Session capture and `sessionState`

- Blob: `{"v":1,"unifiedAuth":{"name":"<value name>","hex":"<lower-case hex>"}}`, JSON, vault-encrypted by the session store. `parseBlob` accepts `v` 1 or 2 (a short-lived v2 carried extras that are ignored) and requires a non-empty name and even-length hex; anything else: “Stored Battle.net session has an unexpected shape” (`session.ts:21-25,43-56`).
- Fingerprint: SHA-256 hex of the lower-cased token hex; `shortFingerprint` = its first 8 digits, “(none)” for null; `describeToken` = “(<bytes> bytes, fingerprint <8 hex>)” or “(absent)” (`session.ts:27-34,59-62`).
- Owner rules: section 6 (`currentSession`).
- `sessionState()` (`adapter.ts:560-573`): `NOT_INSTALLED` without an install; `remembered` = a token exists; `fingerprint` of the chosen token; `launcherUid`: while running the logged uid; when closed with a token the owner's linked uid, else (unless the run ended signed out) the logged uid; else null; `linkedAccountId` = owner id; `lastSignIn` = `{ok, reason}` from the current run's last verdict (reason null on success, “signed out” after a logout), or null when the run has no verdict.
- `captureSession(account)` refusals, all `SWITCH_BLOCKED`, in order (`adapter.ts:575-607`):
  1. unsaved account: “Save the account first, then capture its session.”
  2. (`NOT_INSTALLED` without an install)
  3. no token: “Battle.net has no remembered session right now (nothing under HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth). Sign in to the client as <login> with "Keep me logged in" on, then save again.”
  4. running without a successful sign-in this run: “Battle.net is running but has not signed in during this run[ (<failure reason>)]. Sign in to it as <login>, then save again.”
  5. closed and the last run ended with a logout: “The last Battle.net run ended with a logout, so the remembered session cannot be attributed to an account. Sign in to the client, exit it, then save again.”
  6. another account owns the token: “The session Battle.net currently remembers belongs to <owner>. Sign in to the client as <login> first.”
  7. uid (running: the logged uid; closed: the owner's linked uid, else the logged uid) differs from the account's linked uid: “The client's last sign-in was Battle.net account …1234, but <account> is linked to …5678. Check the account ID on the account page.”
  Then `storeSession` with that uid (or the account's own), which links the uid before saving.
- Stale marking: only in the switch, only on the remembered path, only on a `failed` verdict with the target login first in the list (`listReady`); the reason is the log's failure text (masked) or “rejected by the client” (`adapter.ts:1505-1506`). `markVerified` after every confirmed recapture or when the stored token is found current (`adapter.ts:1424,1431`).

##### 13. Error codes and user-facing messages

| Code | Thrown by | Message |
| --- | --- | --- |
| `NOT_INSTALLED` | every operation needing the install (`adapter.ts:392-396`) | “battlenet is not installed on this PC” |
| `LAUNCHER_FILE` | config read failure or over 4 MiB (`adapter.ts:411-416`; `read-file.ts:9-11`) | “Could not read Battle.net.config (<reason>): <path>” / “Too large to be a launcher file (<n> bytes): <path>” |
| `CONFIRM_REQUIRED` | `runSwitch` (`adapter.ts:1153-1156`) | the `closeMessage` sentences (section 6) |
| `SWITCH_BLOCKED` | `captureSession`, `writeRegistry` | section 12; “No session snapshot is stored for <account>.” |
| `GAME_NOT_PLAYABLE` | `launchGame` | section 10 |
| `LAUNCHER_RUNNING` | `restoreBackup` | “Close Battle.net before restoring: it rewrites its token at sign-in and Battle.net.config at exit, which would undo the restore.” |
| `CONFIG_FORMAT` (internal, `BattlenetConfigError`) | config parsing and the file step; never leaves the adapter as a code, its text appears in step details and warnings (`config.ts:20-26`) | “not valid JSON (<reason>)”, “not a JSON object”, “Battle.net.config does not round-trip through the JSON writer, so One Launch will not rewrite it”, “Battle.net.config has no Client section” |
| `ABORTED` (internal, `SwitchAbortedError`) | waits; turned into a cancelled result (`wait.ts:2-8`) | “The switch was stopped” |
| `PROCESS_ACCESS_DENIED` | the platform, on a refused close or kill (elevated client) | (platform message) |
| plain `Error` (no code) | `shutdown`, `startClient`, `launchGame`, wrong account, started meanwhile | “Battle.net.exe is still running after a window close and a forced close”, “Battle.net.exe keeps restarting itself after being closed”, “Battle.net.exe did not appear within 90 s”, “Not a Battle.net product codename: <id>”, “No launch code is known for …”, “<game> did not start within 15 s …”, the wrong-account and started-meanwhile sentences (section 7) |

Messages thrown out of the adapter reach the dialog prefixed “Battle.net: ” by `LauncherService` (`adapter.ts:745`; `docs/switching.md` › Isolation).

##### 14. Log parsing

- Discovery (`client-log.ts:15,31-45`): files in `%LOCALAPPDATA%\Battle.net\Logs` matching `^battle\.net-(\d{8}T\d{6}(?:\.\d+)?)\.log$` (case-insensitive), sorted by name descending (the name is the run's UTC start). The newest file whose first 4 096 bytes do not contain `IPC ShMem mode=client` or `Leaving because another instance` is the current run; files that do are forwarder logs (a second `Battle.net.exe --uri=`/`--exec=` handing its command to the running client) and are skipped. An unreadable file is skipped too.
- Marks and tails (`client-log.ts:200-237`): a `logMark` is `{file, size}` of the current log. `eventsSince(mark)` reads from `mark.size` when the newest log is the same file and has not shrunk, else the whole new file (a new run); at most the last 8 MiB, and a clipped read drops its first partial line (`log-read.ts:19-27`). `currentRunEvents` = everything of the current log (the newest 8 MiB). Any read error yields no events.
- Line grammar: `<level letter> <YYYY-MM-DD HH:MM:SS.ffffff> [<component>] {<thread>} <text>`; the timestamp is UTC and is kept as ISO on the signed-in event (`client-log.ts:63,72-76`). Lines longer than 16 KiB are skipped (`client-log.ts:90`). Matching is by substring or regex on the whole line, first match wins, in this order (`client-log.ts:87-175`):

| Event | Distinguishing text | Fields |
| --- | --- | --- |
| `started` | `Logging started for Battle.net build` | build = the token after `build ` |
| `signed-in` | `Logged into Battle.net successfully` | accountId = the hex in `\|bnet=<n>:<n>:<1-16 hex>\|`, lower-cased ('' when absent); `at` = the line time |
| `sign-in-attempt` | `Login triggered.` with `entityId=<n>:<n>:<hex>`, or `Login_QueueUpdate` | |
| `sign-in-failed` | `DeleteToken(): Deleting registry token` | reason “the client discarded the token at start (its login page remembers a different account)” |
| `sign-in-failed` | `Login failed.`, `OnLoginFailed()`, `External Challenge was canceled by user` | reason = the text after `error=`, else the text after the `}` thread marker; masked: emails → `…`, hex runs of 16+ → `…`, trimmed, at most 200 characters (`client-log.ts:79-85`) |
| `signed-out` | `Logout() : calling DeleteToken`, `DeleteToken() because !enabled` | |
| `main-window` | `Attempting to show main window` | |
| `game-launching` / `game-running` / `game-exited` / `product-state` | `[ProductState] {Main} InstallState (<codename>): <fields>` where `GameLaunching=1` → launching; `gameRunning=1` → running; `gameRunning=0` → exited; else `currentOperation=` present → product-state with `operation` = `currentOperation=`, `status` = `currentOperationStatus=`, `playable` = `playable=1`/`0` or null when absent (the regional `playable:false` entries use a colon and do not match). An empty codename never matches the pattern. |
| `game-launching` / `game-running` / `game-exited` | `Operation status changed: opType=Execute oldStatus=<x> newStatus=<Pending \| On \| Off> agentUid=<codename>` | |
| `game-launching` | `[GameLaunchController] {Main} LaunchBinary: uid=<codename>` | |
| `game-running` | `[InstallManager] {Main} Game is running: <codename>` | |
| `launch-request` | `Launch request <code>[.] isRunning=` (client builds before 2.52.12) | code |
| `close-requested` | `Frontend is requesting to close window` | |
| `exit` | `Shutting down battle.net interface` or `signaling application exit` | |

- `lastSignIn(events)`: scanning backwards, the first `signed-in` (`{ok:true, accountId, at}`), `sign-in-failed` (`{ok:false, reason}`) or `signed-out` (`{ok:false, reason:'signed out'}`), else null (`client-log.ts:180-188`). `endedSignedOut`: scanning backwards, a `signed-out` before any `signed-in` (`client-log.ts:191-198`).
- Nothing in the log is deliberately skipped beyond long lines; tokens never appear in it, and only failure text can carry a login, which `maskReason` strips (`client-log.ts:7-8`).

#### Adapter: Riot Games

Launcher id `riot`, display name `Riot Games`; the client is called “the Riot Client” in every sentence (`src/main/launchers/riot/adapter.ts:119,128,332-333`). The client signs in from one plain-text file, `RiotGamesPrivateSettings.yaml`; a switch stores the sign-in that file holds for its owner, snapshots the file, puts the target account's saved copy of the whole file in place (or the client's own signed-out state), starts the client the way its shortcut does and confirms the sign-in from the client log plus the account id the file then names (`adapter.ts:1-16`). The rules of `docs/adapters/riot.md` › Rules bind the adapter and this section: game processes and Vanguard are never opened, queried by path, signalled or killed (image names only, from a listing that opens no process); no switch or restore while a Riot game runs; no traffic to Riot's servers except the one loopback launch request; no credential automation; one file written; the device cookie kept; an old token never presented (store before every overwrite); sessions never leave the PC; the client started the way its shortcut does; the file treated as a bearer credential.

The adapter is constructed with `localAppData`, `programData`, a default install folder, a `riot.path` reader, a `riot.pressPlay` reader (default true, `src/main/index.ts:570`), a local-launch channel, the registry reader, the process controller, the backup and session stores and a timing table (`adapter.ts:176-197,341-348`).

Timing table (`DEFAULT_TIMING`, `adapter.ts:143-173`), milliseconds, merged through `mergeTiming` (`src/main/launchers/wait.ts:31-39`):

| Name | Default | Used for |
| --- | --- | --- |
| `pollMs` | 500 | process-list waits |
| `logPollMs` | 250 | log and session-file polls |
| `processPollMs` | 1 000 | how often a launch watch asks the process list for the game |
| `shutdownMs` | 3 000 | polite close of the core before the forced close |
| `killGraceMs` | 10 000 | wait after a forced close (core and window processes) |
| `windowCloseMs` | 10 000 | polite close of the window processes |
| `restartWatchMs` | 5 000 | watch for the core coming back |
| `cleanExitWatchMs` | 1 500 | the same watch after a logged clean exit or a forced close |
| `restartSettleMs` | 45 000 | a restarted core's time to reach a sign-in verdict before it is closed again |
| `startupMs` | 60 000 | wait for the core after the start |
| `signInMs` | 45 000 | remembered-path sign-in wait |
| `manualSignInMs` | 600 000 | hand-over wait |
| `captureMs` | 15 000 | wait for the client to write the file (after a manual sign-in, and for the account id after a logged sign-in) |
| `readyGraceMs` | 2 000 | `readyAt` offset after a bare start |
| `launchConfirmMs` | 90 000 | watch after handing the client a game |
| `playConfirmMs` | 30 000 | watch after One Launch's own Play |

**[dev-only]** `SIM_TIMING` for the fake client: `pollMs` 100, `logPollMs` 100, `processPollMs` 100, `shutdownMs` 3 000, `killGraceMs` 1 000, `restartWatchMs` 500, `cleanExitWatchMs` 200, `restartSettleMs` 2 000, `startupMs` 5 000, `signInMs` 8 000, `manualSignInMs` 30 000, `captureMs` 3 000, `readyGraceMs` 300, `launchConfirmMs` 2 000, `playConfirmMs` 2 000 (`src/main/launchers/sim/scenario.ts:22-39`). `windowCloseMs` has no sim override.

##### 1. Files and registry it reads; what it writes

Reads (through `readLauncherText`, 4 MiB cap, `LAUNCHER_FILE` when over; `src/main/launchers/read-file.ts:5,21-23`, unless noted). Paths in Riot's files use forward slashes and are normalised (`normalizeLauncherPath`):

| Path | Format | What is read | Cap | On failure |
| --- | --- | --- | --- | --- |
| `%LOCALAPPDATA%\Riot Games\Riot Client\Data\RiotGamesPrivateSettings.yaml` (`adapter.ts:353-368`) | YAML written by the client: space-indented maps, one scalar per line, one-line list items, CRLF, optional BOM (`session.ts:1-8,148-151`) | only the `psl` › `authorization` › `riot-client` block's direct scalars: `id_token` (JWT; its payload's `sub`, UUID-shaped, lower-cased, and `acct.game_name` / `acct.tag_line` as a display name), `refresh_token` (fingerprint = SHA-256 hex), `is_dpop_bound`, `refresh_token_write_count` (`session.ts:216-244`). `riot-client: null` (also `~`, `Null`, `NULL`) is the signed-out state. The device cookie `rso-authenticator.tdid` and the pre-116 `riot-login.persist` block are never looked at. | 4 MiB | missing: no file (not an error). Unreadable or too large: `LAUNCHER_FILE` “Could not read RiotGamesPrivateSettings.yaml (<reason>)”. Any shape the reader does not know is `unexpected` and becomes `LAUNCHER_FILE` “RiotGamesPrivateSettings.yaml is not in the shape One Launch knows (<reason>)”, never “nobody is signed in” (`adapter.ts:574-600`). Reasons: “no psl block” / “no psl.authorization block” / “no psl.authorization.riot-client block”, “riot-client is neither null nor a block”, “riot-client is empty”, “riot-client has no id_token”, “riot-client has no refresh_token”, “id_token is not a readable token”, “id_token names no account id” (`session.ts:154-164,216-231`). |
| `%LOCALAPPDATA%\Riot Games\Riot Client\Config\RiotClientSettings.yaml` (`adapter.ts:370-372`) | YAML | only the `last-session-timestamp:` block: `<product>.<patchline>: <digits>` (quoted or bare, 9 to 13 digits; 12+ digits read as milliseconds, else seconds), keys lower-cased, until the first line at the block's indent or less (`products.ts:136-162`) | 4 MiB | missing: none; unreadable: WARN, none (`adapter.ts:560-569`) |
| `%LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile` (`adapter.ts:361-364`) | one line `name:pid:port:password:protocol` | port and password, for the one launch request only; accepted only when name is `Riot Client`, protocol `https`, pid digits, password non-empty, port 1 to 65535 (`local-launch.ts:24-40`) | none (plain `readFileSync`, `local-launch.ts:104`) | missing: “the Riot Client has no lockfile to answer on”; wrong shape: “the lockfile is not in the shape One Launch knows” (both make the Play press `declined`, section 10). Its presence while the client runs is otherwise never read (`docs/adapters/riot.md:126`). |
| `%LOCALAPPDATA%\Riot Games\Riot Client\Logs\Riot Client Logs\<local time>_<pid>_Riot Client.log` (`client-log.ts:18-22`) | text log, one per run | sign-in states, patch states, launch pipeline (section 14) | tail of 8 MiB (`client-log.ts:425`); lines over 16 KiB and telemetry lines skipped | missing folder or unreadable: no events |
| `%ProgramData%\Riot Games\RiotClientInstalls.json` (`adapter.ts:378-380`) | JSON | `rc_default`, `rc_live` (full exe paths), `associated_client` (game folder → exe) (`products.ts:116-134`) | 4 MiB | missing: null; unreadable: WARN, null; malformed JSON: everything null/empty (`adapter.ts:435-444`) |
| `%ProgramData%\Riot Games\Metadata\<product>.<patchline>\` (`adapter.ts:374-376`) | folder per product | folder names matching `^([a-z0-9_]+)\.([a-z0-9_]+)$` (`Riot Client` and `<p>.<pl>.game_patch` do not match); inside: `<name>.product_settings.yaml` top-level scalars `product_install_full_path`, `product_install_root`, `product_dependency` (nested blocks skipped; `products.ts:77-99`); `<name>.ok` existence; `<name>.ico` existence (`adapter.ts:449-483,534-547`) | 4 MiB per settings file | folder missing: no products; unlistable: WARN, none; an unreadable settings file: WARN, product without a path |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Riot Game Riot_Client.` › `InstallLocation` (key name ends in a dot) (`adapter.ts:114-115,389-393`) | registry string | detection | | absent: next candidate |
| `HKCU\…\Uninstall\Riot Game <product>.<patchline>` › `DisplayName`, `DisplayIcon` (`adapter.ts:539-558`) | registry strings | the product's name; its icon when it is an `.ico` (a trailing `,<index>` dropped; an exe is not taken) | | read error: debug log, null |
| `HKLM` then `HKCU` `SOFTWARE\Classes\riotclient\shell\open\command` › default (`adapter.ts:116,400-406`) | registry string, first quoted token | detection | | absent: next candidate |

Writes. One launcher file, `RiotGamesPrivateSettings.yaml`, through `writeFileAtomic` (temp file in the same folder, then rename), only in `runSwitch` (`adapter.ts:1429,1441`) and in `applyRestore` through the backup service (`adapter.ts:1731-1735`), never created (`adapter.ts:1413-1416`):

- Remembered path: the account's stored copy of the **whole file** is written over the file as the client left it (sign-in, token write count and device cookie together); the swap confirmed by hand on the real client (`adapter.ts:1176,1417-1432`). Nothing is spliced.
- Credentials path: the `riot-client` block (its line through the last deeper line) is replaced by one line `riot-client: null` at the block's indent; every other line, the line endings and a BOM stay as they are (`session.ts:183-200`). A file already signed out is a no-op.

Snapshot contents: the file alone, at relative path `Data/RiotGamesPrivateSettings.yaml`, `sensitive: true` (vault ciphertext in the backup) (`adapter.ts:123,1399-1404`). No registry meta.

##### 2. Detection

First candidate folder containing `RiotClientServices.exe` wins (`adapter.ts:384-408,419-426`):

1. The `riot.path` setting folder (`source: 'user'`); when the exe is gone, the ladder continues and health adds a `chosen-folder` warning.
2. `HKCU\…\Uninstall\Riot Game Riot_Client.` › `InstallLocation` (`source: 'registry-hkcu'`).
3. `RiotClientInstalls.json` › `rc_default`, then `rc_live`: the exe's folder (`source: 'launcher-file'`).
4. `HKLM` then `HKCU` `SOFTWARE\Classes\riotclient\shell\open\command`: the first quoted token's folder (`source: 'registry-hklm'` / `'registry-hkcu'`).
5. `C:\Riot Games\Riot Client` (`source: 'default-path'`) (`adapter.ts:118`).

Locate validation (`installFromExe`): only a file named `RiotClientServices.exe` (case-insensitive) whose folder holds that exe; `source: 'user'` (`adapter.ts:414-417`). The exe started, for switches and launches alike, is `RiotClientServices.exe` from the install folder with the install folder as cwd (`adapter.ts:909,1457`). A chosen folder changes only where the exe starts from; the data folder is per Windows user (`docs/adapters/riot.md:36`). `requireInstall` throws `NOT_INSTALLED` (“riot is not installed on this PC”) (`adapter.ts:428-432`).

##### 3. Installed games scan

- One record per `<product>.<patchline>` folder under `%ProgramData%\Riot Games\Metadata`, sorted by id; a product is installed when its `<id>.ok` marker exists and its `product_install_full_path` exists on disk; a settings file alone also exists for products never installed (`adapter.ts:449-487`).
- `InstalledGame`: `launcherGameId` = `<product>.<patchline>` (the folder name; the launch arguments are built from it); `name` = the Uninstall key's `DisplayName`, else the table name with “ (<PATCHLINE>)” appended when the patchline is not `live`; `installPath`; `libraryPath` = `product_install_root` or the install path's parent; `sizeOnDiskBytes` null; `lastPlayedAt` from `last-session-timestamp`; `needsUpdate` from the log. Sorted by name (`adapter.ts:504-527`).
- Product table (`RIOT_PRODUCTS`, `products.ts:13-25`); patchlines seen are `live` and `pbe`:

| product | name | game image names (matched by name only) |
| --- | --- | --- |
| `valorant` | VALORANT | `VALORANT.exe`, `VALORANT-Win64-Shipping.exe` |
| `league_of_legends` | League of Legends | `LeagueClient.exe`, `LeagueClientUx.exe`, `LeagueClientUxRender.exe`, `League of Legends.exe` |
| `teamfighttactics` | Teamfight Tactics | the same four as League (they share one client) |
| `bacon` | Legends of Runeterra | `LoR.exe` |
| `lion` | 2XKO | `2XKO.exe`, `2XKO-Win64-Shipping.exe` (unverified) |

- `needsUpdate`: the newest run log's latest patch state per `<product>.<patchline>` says `OutOfDate` (case-insensitive) or, on build 116, `launchable: 0`. The client logs every product's state at each start and when it changes, so the newest run's last word stands until the next run. `<product>.<patchline>.update-status.json` is not read (`adapter.ts:494-502`; `client-log.ts:338-356`).
- `gameIcon(game)`: `<Metadata>\<id>\<id>.ico` first, else the Uninstall key's `DisplayIcon` when it is an `.ico` file; the first that exists, or null (`adapter.ts:534-547`).

##### 4. Running state

- `isRunning`: image-name check for `RiotClientServices.exe` (`adapter.ts:727-729`).
- `findRunningGame(names?)` (`adapter.ts:745-755`): from one `runningNames()` listing (image names, no process opened), the first product in table order with one of its images running (`.exe` optional, case-insensitive; `products.ts:27-39`); then the installed game whose id starts with `<product>.` names it, else `launcherGameId` = `<product>.live` and the table name; `source: 'process'`. **A listing that fails throws**, so a switch, a launch or a restore refuses rather than go on past a game it could not see.
- `runningGame()` (the status): the same, but a failed listing is logged at WARN and read as no game (`adapter.ts:732-739`).
- During a launch watch, the process list is asked for the product's images at most once per `processPollMs` (1 s), false in between, while the log is read every poll (`adapter.ts:768-780`).

##### 5. Remembered accounts

`listRememberedAccounts` lists the one account the session file names, or nothing: `launcherUid` = the account id (`sub`), `login` = the owner's login or `''`, `personaName` = the owner's display name, else the Riot ID as `<gameName>#<tagLine>`, else `''`; `rememberPassword: true`, `mostRecent: true`, `lastLoginAt: null`, `sessionMayBeExpired: false` (`adapter.ts:707-723`). The file records no login and the client keeps no list of past sign-ins, so this is never an import source. The Riot ID is registered with the logger's redaction the moment it is read (`session.ts:134-144`).

##### 6. Preflight and plan

`currentSession()` (`adapter.ts:588-600`): the file's text, the session it holds (or null when signed out or absent), and the owner: the account whose stored snapshot has the same fingerprint, else the account linked to the file's account id. A file in an unknown shape throws `LAUNCHER_FILE` (so a plan or preflight fails with it).

Plan context (`adapter.ts:1073-1090`):

- `running` and `game` come from one `runningNames()` listing: `running` = `RiotClientServices.exe` present; `game` = `findRunningGame(names)`.
- `inPlace` = the file holds a session and its owner is the target account.
- `alreadyActive` = `inPlace`, the client running, and the newest run log's last verdict is a sign-in.
- `strategy` = `remembered` when `inPlace` or a non-stale snapshot exists, else `credentials`.
- `expectedUid` = the snapshot's `launcherUid`, else the account's `launcherUid` lower-cased, else null.
- `executable` = the session file exists (`adapter.ts:1316`). Without it, the first warning is the blocked reason.
- `current` = `{ mostRecentLogin: owner login or null, autoLoginUser: null, running }`.

`preflight(account, forGame?)` (`adapter.ts:1042-1052`) builds the plan with the game a launch would follow with, **throws `GAME_RUNNING`** when a Riot game runs (“<game> is running. Close it, then switch: One Launch never closes a Riot game.”, `adapter.ts:783-785`), even when the client is already active as the account, and otherwise answers the plan without its close warning, `runningGame: null` always, `alreadyActive`, and `closeMessage` “The Riot Client is running. Switching closes it.” (`adapter.ts:1038-1040`).

Plan steps (`adapter.ts:1097-1266`):

| id | kind | title | target | skipped when | detail |
| --- | --- | --- | --- | --- | --- |
| `close` | process | “Close the Riot Client” | `RiotClientServices.exe` | not running, or alreadyActive | alreadyActive: “Skipped: the client is already signed in as this account, so it is left running.” Running: “Send RiotClientServices.exe a window close and wait up to 3 s; a process that ignores the close is ended on its own, never with its children: a running game would refuse the switch before this step. Then the same for the window processes it leaves behind (Riot Client.exe).” Else “Skipped: RiotClientServices.exe is not running.” |
| `capture` | backup | inPlace: “Save this account's sign-in”, else “Save the outgoing sign-in” | the session file path | not `captureNeeded` = an owner exists, not alreadyActive, and (running, or the file holds a session and (not inPlace or its fingerprint differs from the owner's stored one)) | alreadyActive: “Skipped: the client stays running, so its file is not re-read.” No owner, session: “Skipped: the client holds a sign-in (Riot account …1234) that no One Launch account owns, so it is left alone and not saved. To keep it, save it from that account's page before switching.” No owner, no session: “Skipped: the client holds no saved sign-in.” Not needed: “Skipped: <owner>'s saved sign-in already matches what the client holds.” Running: “After the client has closed, read RiotGamesPrivateSettings.yaml and, if it holds <owner>'s sign-in and it changed, store the file, encrypted, as the account's saved sign-in. Nothing in the client's files changes.” inPlace: “RiotGamesPrivateSettings.yaml holds <owner>'s sign-in but it differs from the saved copy (the client rotated the token \| no copy yet), so store the file, encrypted, as the account's saved sign-in. Nothing in the client's files changes.” Else: “RiotGamesPrivateSettings.yaml holds <owner>'s sign-in[ and it changed since it was last saved (the client rotates the token at every start)]; store the file, encrypted, as the account's saved sign-in if it changed. Nothing in the client's files changes.” |
| `backup` | backup | “Back up RiotGamesPrivateSettings.yaml” | the backups preview path | alreadyActive or inPlace | “Skipped: the file will not be modified.” / “Copy <path>, encrypted, into a new snapshot folder so a failed switch can be rolled back. The newest <backups.keep> snapshots are kept.” |
| `file` | file | inPlace or stored: “Put the account's sign-in in place”; credentials: “Sign the client out” | the session file path | inPlace: always; stored: never; credentials: when the file is absent or already signed out | inPlace: “No change: RiotGamesPrivateSettings.yaml already holds <account>'s sign-in (fingerprint <8 hex>).” Stored: “Decrypt <account>'s saved copy of RiotGamesPrivateSettings.yaml from the vault and write it over the file, whole, as the client itself left it (its sign-in, token write count and device cookie together): the swap confirmed by hand on the real client. The new content is written to a temp file and renamed over the original. The copy is never shown or logged.” with change `psl.authorization.riot-client`: before “(absent)” \| “(saved sign-in for Riot account …1234, fingerprint <8 hex>)” \| “null (signed out)” → after “(saved copy for <account>, fingerprint <8 hex>, saved <ISO>)”. Credentials: skipped “No change: there is no RiotGamesPrivateSettings.yaml, so the client opens on its login page.” / “No change: the client holds no saved sign-in, so it opens on its login page.”; else “Set psl.authorization.riot-client to null, the state the client itself writes at a sign-out, and leave every other line (the device cookie included) as it is, so the client opens on its login page instead of signing in as <owner \| Riot account …1234>.” with change before → “null (signed out)”. |
| `start` | launch | “Start the Riot Client” | `"<exePath>"[ --launch-product=<p> --launch-patchline=<pl>]` | alreadyActive | “Skipped: the client is already running as this account.” With a game, remembered: “Start the client with <game>'s launch arguments, as the game's own shortcut does. It signs in as <login> on its own from the saved sign-in and opens the game's page.”; credentials: “Start the client with <game>'s launch arguments, as the game's own shortcut does. With no saved sign-in it opens on its login page, and the game's page follows the sign-in.” Without: “Start the client with no arguments, as its own shortcut does. It signs in as <login> on its own from the saved sign-in.” / “Start the client with no arguments. With no saved sign-in it opens on its login page.” |
| `wait` | wait | “Wait for the client to come up” | `RiotClientServices.exe` | alreadyActive | “Poll the process list until RiotClientServices.exe is running, up to 60 s.” |
| `verify` | wait | “Confirm the client signed in” | the log folder | alreadyActive | “Skipped: the client is already signed in as this account.” Remembered: “Read what the client appends to its new run log (no tokens in it) until it reports a sign-in verdict, up to 45 s: the signed-in state (PendingProductContext), after which RiotGamesPrivateSettings.yaml names the Riot account id, which must be <…1234 \| an id no other account holds, which is then recorded on the account>; or the login page (PendingLoginStrategy), which hands over to you in the next step. Only an invalid_grant refusal marks the saved copy stale.” Credentials: “Read what the client appends to its new run log until it reports a sign-in verdict. With no saved sign-in it opens on its login page, so this goes straight to the next step.” |
| `manual` | wait | “Wait for you to sign in” | the log folder | strategy remembered | remembered: “Runs only if the client refuses the saved sign-in or does not sign in within 45 s. The client stays on its login page; you sign in there once as <login> with “Stay signed in” ticked[ (the password can be copied from One Launch)] and One Launch continues as soon as the log reports the sign-in, up to 10 min.” Credentials: “The client opens on its login page. Sign in there as <login> with “Stay signed in” ticked[ (the password can be copied from One Launch)]. One Launch continues as soon as the log reports the sign-in, up to 10 min. Signing in as a different Riot account than the one linked to <account> ends the switch and puts the file back.” |
| `recapture` | backup | “Save the new sign-in” | the session file path | strategy remembered | remembered: “Runs only after a manual sign-in: the fresh sign-in replaces the stale saved copy.” Credentials: “Once the sign-in is reported, wait up to 15 s for the client to write RiotGamesPrivateSettings.yaml, then store the file, encrypted, as <account>'s saved sign-in and record the Riot account id on the account. If the client writes it later, it is saved at the next switch instead. Later switches use the saved sign-in.” |

Plan warnings (`adapter.ts:1268-1306`):

- a Riot game running: “<game> is running. Close it first: One Launch never closes a Riot game, and no switch runs while one does.”
- no session file: “<path> does not exist. One Launch never creates it: start the Riot Client once by hand (it writes the file at its first start), then try again.” (this is also the `SWITCH_BLOCKED` reason, the plan not being executable)
- running and not alreadyActive (the close warning, left out of the preflight's plan): “The Riot Client is running[ as <owner> \| with a sign-in no One Launch account owns \| on its login page]. It will be closed.”
- stale snapshot, not inPlace: “<account>'s saved sign-in no longer works: <stale reason>. You will sign in once by hand, with “Stay signed in” ticked, and One Launch stores the fresh sign-in.”
- credentials without a snapshot, not inPlace: “No sign-in is saved for <account> yet. The client will open on its login page; sign in once with “Stay signed in” ticked and One Launch saves it for later switches.[ No password is stored in One Launch, so have it ready.]”
- not inPlace, a session nobody owns: “The Riot Client holds a sign-in that no One Launch account owns (Riot account …1234). It will be replaced. The backup taken before this switch keeps a copy and is left out of the usual clean-up, so restoring it from Settings › Backups brings that sign-in back.”
- snapshot id differs from the account's: “The Riot account id on this account (…1234) differs from the one recorded with its saved sign-in (…5678). The saved sign-in's id is used to confirm the sign-in.”

Notes: alreadyActive: “The Riot Client is running and signed in as <account>. The real switch would change nothing and would leave the client running.”; inPlace and not running: “The Riot Client already holds <account>'s sign-in, so it is only started.” (`adapter.ts:1275-1284`).

##### 7. Switch (`runSwitch`)

Refusals before anything is touched, in order (`adapter.ts:1337-1339`): a Riot game running → `GAME_RUNNING` (“<game> is running. Close it, then switch: One Launch never closes a Riot game.”); no session file → `SWITCH_BLOCKED` with the first plan warning (or “This switch cannot run.”); running, not alreadyActive, no `confirmClose` → `CONFIRM_REQUIRED` “The Riot Client is running. Switching closes it.” (no running game attached).

Sequence (`adapter.ts:1624-1673`):

1. Abort check. When running and not alreadyActive: step `close` runs `shutdown` (below), done “Client closed.”.
2. Abort check. Unless alreadyActive, a running core fails the switch: “The Riot Client was started by something else while One Launch was switching accounts, so the switch stopped before changing anything. Try again.” (`adapter.ts:1445-1450,1633`).
3. `saveOutgoing` (step `capture`, only when not skipped by the plan): `storeOnDiskSession` (`adapter.ts:678-692`), rule 8's store-before-overwrite, best effort. Details: “The client left no saved sign-in behind (signed out).”, “The saved sign-in belongs to no One Launch account; left alone.”, “<owner>'s saved sign-in is already current.”, “Saved <owner>'s sign-in (fingerprint <8 hex>).”, or “Not saved: <error>” (logged at WARN).
4. When neither alreadyActive nor inPlace:
   - `takeSnapshot` (step `backup`): the file at `Data/RiotGamesPrivateSettings.yaml`, `sensitive: true`, reason “Before switching to <account>”, pinned “Holds a sign-in that no One Launch account owns” when the closed client's file holds a session nobody owns (`holdsUnownedSession`, which reads false when the file cannot be read; `adapter.ts:695-702,1395-1408`). `onBackup(id)` fires. Done detail = the snapshot path.
   - `writeSession` (step `file`, `adapter.ts:1410-1443`): the file is re-read; missing → `SWITCH_BLOCKED` “<path> is missing. Start the Riot Client once by hand so it writes its settings file, then try again.”. Remembered: no account id or no stored blob → `SWITCH_BLOCKED` “No sign-in is saved for <account>.”; the blob is parsed (“the stored Riot sign-in is not readable” / “…is not in a known shape”, `session.ts:203-213`) and must hold a session (“the stored Riot sign-in holds no session (<reason \| signed out>)”, `adapter.ts:271-274`); identical text → skipped “Already in place.”; else atomic whole-file write, `modified`, done “RiotGamesPrivateSettings.yaml now holds the saved copy (fingerprint <8 hex>).”. Credentials: `planSignOut` (an unknown shape throws “<reason>; the file is left alone”); already signed out → skipped “Already signed out.”; else the splice, `modified`, done “RiotGamesPrivateSettings.yaml now holds the client's signed-out state; everything else in it is as it was.”. `writtenFingerprint` = the written session's fingerprint, or null after a sign-out.
5. Post-write running check: a running core fails with “…so the switch was stopped. Try again.” (rolled back) unless alreadyActive, where `start` and `wait` are skipped “The client is already running.” (`adapter.ts:1641-1644`).
6. `startLauncher` (`adapter.ts:1452-1469`): abort check, log mark, `launcherStarted`; `RiotClientServices.exe` started from the install folder **with `--launch-product=<product> --launch-patchline=<patchline>` when `SwitchOptions.game` names a game** (then `startedWithGame` = that `launcherGameId`, `start` done “Client started with <game>'s launch arguments.”), else with no arguments (“Client started.”). `wait` polls `isRunning` every 500 ms up to 60 s; timeout throws “RiotClientServices.exe did not appear within 60 s”; done “RiotClientServices.exe is running.”.
7. Sign-in confirmation (skipped “The client is already signed in as this account.” when alreadyActive) (`adapter.ts:1532-1542`): `verify` running; `waitForSignIn(signInMs, false)` on `remembered` (45 s), one poll (1 ms) on `credentials`. Details: “Signed in as <login>.[ Linked to Riot account …1234.]” or “Signed in as <login> (the client has not written RiotGamesPrivateSettings.yaml yet, so the account id is unconfirmed).”; “Signed in as a different account.”; “The client ended on its login page: <detail>.”; timeout: remembered “The client did not report a sign-in in time.”, credentials “Waiting for you.”; “Cancelled.”.
   - The poll (`waitForSignIn`, `adapter.ts:1484-1511`): every 250 ms, the log's last verdict since the start mark. On a sign-in verdict the file is read for the account id (the log never names the account; the client writes the file before logging the signed-in state): with `expectedUid` null, an id another account holds is `wrong-account`, any other is `signed-in`; with `expectedUid`, equal is `signed-in`, different is `wrong-account`. When the file holds no session yet, the poll waits up to `captureMs` (15 s) from the first such read and then returns `signed-in` with a null id. A verdict that is not a sign-in ends the wait (`failed`) only outside the manual wait.
   - `linkIfNew` records the id on the account when `expectedUid` was null (“ Linked to Riot account …1234.”). `wrong-account` throws (rollback): `expectedUid` null: “The Riot Client signed in as <owner> (Riot account …1234) \| Riot account …1234, which is another One Launch account, instead of <account>. Sign in to the client as <login>.”; else “The Riot Client signed in as <who> instead of <account> (…5678). Sign in to the client as <login>, or fix the Riot account id on the account page.” (`adapter.ts:1512-1527`).
   - Cancelled → cancelled result. On `remembered`, a confirmed sign-in with a stored snapshot marks it verified; the rotated file is **not** recaptured here, it is stored at the next overwrite (`adapter.ts:1655-1657`).
8. Hand-over when the verdict is not a sign-in (`adapter.ts:1543-1562,1658-1668`): on `remembered`, a `failed` verdict with reason `rejected` (an `invalid_grant`) marks the snapshot stale with its detail; a service failure, a sign-out or “no saved sign-in” does not. Then `onAttention` (`adapter.ts:309-329`):
   - title: `credentials`: “Sign in to the Riot Client as <login>”; else “The Riot Client is asking you to sign in as <login>”.
   - message: cause `new`: “No sign-in is saved for this account yet.”; `expired` (a `rejected` verdict): “The client refused its saved sign-in.”; `other`: “The client did not sign in on its own[ (<detail>)].” (the detail is omitted for the `no-session` reason).
   - steps: “In the Riot Client window, enter the login <login> and your password.”, “Tick “Stay signed in” before you sign in, or the client forgets the account when it closes.”, “Press Sign in. One Launch continues on its own, and later switches are automatic.”.
   - `reason`, `canCopyPassword` (secret stored and a password reader supplied), `until` = now + 10 min.
   - `manual` running with the title; the poll runs up to 10 min and a login-page verdict or a sign-out is not final. Details: “Signed in as <login>.…”, “Signed in as a different account.”, “Stopped waiting.”, “No sign-in after 10 min.”. `onAttention(null)` always follows.
   - Outcomes: cancelled → cancelled result; wrong account → throw (rollback); timeout → `needsSignIn`; signed in → `captureFresh(captureMs)`.
9. `captureFresh(15 s)` (step `recapture`, `adapter.ts:1568-1596`): polls the file every 250 ms (a read error propagates) for a session whose fingerprint differs from `writtenFingerprint`; one naming another account than `expectedUid`: done “Not saved: RiotGamesPrivateSettings.yaml names Riot account …1234, not this account's.”; else stored (`storeSession`, which links the id first) and marked verified: “Stored <account>'s sign-in (fingerprint <8 hex>).”. At the deadline or abort: the file's session equals the stored fingerprint → verified, “The saved sign-in is current.”; else “The client has not written the new sign-in yet; it is saved at the next switch.”.
10. `readyAt` = now + `readyGraceMs` (2 s) only when the client was started bare (no `startedWithGame`) and a sign-in was confirmed in this run; with the game's arguments `readyAt` stays null, nothing being sent afterwards (`adapter.ts:1669`).
11. Result `ok: true`, `restored: null`, `backupId`, `readyAt`, `startedWithGame` (`adapter.ts:1673`).

`shutdown` (`adapter.ts:802-861`), touching only the client's own processes by image name:

- Core not running: only the window processes are closed (below), then return.
- Up to 3 attempts: log mark; window close to `RiotClientServices.exe` (its one window, the tray icon's, swallows it on the current build); wait up to `shutdownMs` (3 s); if still running, forced close **without the tree** (a running game is the core's child) and wait `killGraceMs` (10 s), else “RiotClientServices.exe is still running after a window close and a forced close”. Then `closeWindows`: when `Riot Client.exe` runs, a window close (the processes with a window get it, the helpers exit with their parent), wait `windowCloseMs` (10 s), then a forced close without the tree and `killGraceMs`, else “Riot Client.exe is still running after a window close and a forced close”. Then the restart watch (`watchForRestart`): poll every `pollMs`; the budget is `restartWatchMs` (5 s), cut to `cleanExitWatchMs` (1.5 s) from the start after a forced close or the moment the log gains the lock-file removal (`exit` event). A core that stays down ends the shutdown; one that comes back after the third attempt throws “RiotClientServices.exe keeps restarting itself after being closed”; otherwise wait up to `restartSettleMs` (45 s) for a new log file whose run has reached a sign-in verdict or the terms page (`startupFinished`), and close again.

Storing a session (`storeSession`, `adapter.ts:653-670`): the account id is linked first when it differs from the account's (`setLauncherUid`, which throws when another account holds it), then the whole file text is saved as `{v:1, file}` with the session's fingerprint. The log line carries only the account id, the short fingerprint, the write count and the dpop flag.

##### 8. Cancellation

Abort checks sit at the top, after the close and before the start (`adapter.ts:1381-1383,1454,1625,1631`); every wait honours the signal (`waitUntil` throws `ABORTED`, the sign-in polls return `cancelled`, `captureFresh` ends its wait).

- Before any write (close included): `ok:false, cancelled:true, restored:null`, error “Stopped before the Riot Client was started as <login>. Nothing had been changed.”; the running step is marked failed “Stopped.” (`adapter.ts:1675-1683`).
- After the write, before the start: rollback (section 9): `cancelled:true, restored:true` with “Stopped before the Riot Client was started as <login>.”, or `restored:false` with the restore error appended.
- After the start: no rollback, `cancelled:true, restored:null`, “Stopped while the Riot Client was starting as <login>. Nothing was rolled back: the client carries on signing in on its own.”; after the hand-over began: “Stopped waiting for the sign-in as <login>. The Riot Client was left on its login page; signing in there with “Stay signed in” ticked finishes the switch, and the sign-in is saved at the next one.” (`adapter.ts:1597-1606,1653,1664,1681`).
- During `shutdown` an abort throws out of the wait at once; a forced close already issued is not undone.
- `launchGame`'s watches: an abort ends them with the “(stopped watching for the game start)” annotation, never an error (section 10).

##### 9. Rollback and `applyRestore`

- Failure after a write (`adapter.ts:1674-1705`): a refusal thrown before any write is logged at WARN, else ERROR. Not modified: `restored: null`. Modified without a snapshot: `restored: false`, “<message> (no backup was available to restore)”. Otherwise: close the client when it runs (no signal; failures only logged), **store what the file holds for its owner** (`storeOnDiskSession`, rule 8: a sign-in the client rotated or one made by hand), then `applyRestore`; `restored: true`, or `restored: false` with “<message>. Restoring backup <id> also failed: <error>”.
- `applyRestore(id)`: the backup service decrypts and verifies the one file and writes it atomically to the current session file path; result `files` = the restored paths, `registry` empty (`adapter.ts:1731-1735`). There is no registry meta.
- `restoreBackup(id)` (`adapter.ts:1710-1729`): `NOT_INSTALLED`; running → `LAUNCHER_RUNNING` “Exit the Riot Client from its tray icon before restoring: it rewrites RiotGamesPrivateSettings.yaml while it runs, which would undo the restore.”; a Riot game running (a failed listing throws too) → `GAME_RUNNING` “<game> is running. Close it, then restore: One Launch never closes a Riot game.”; then a “Before restoring <id>” snapshot of the file (`sensitive`, `protect: id`, pinned when the file holds a sign-in nobody owns); then the on-disk sign-in is stored for its owner; then `applyRestore`.

##### 10. Launch (`launchGame`)

(`adapter.ts:880-981`)

1. The id must parse as `<product>.<patchline>` (`^([a-z0-9_]+)\.([a-z0-9_]+)$`), else a plain error “Not a Riot product id: <id>” before the install is looked up.
2. `NOT_INSTALLED` without an install.
3. One `runningNames()` listing (a failure throws its own error, not a refusal code). A running Riot game refuses with `GAME_RUNNING`: the same product → “<game name> is already running.”; another product → “<game> is running. Close it, then press Play again: One Launch never closes a Riot game.”.
4. The newest run log reports the product out of date → `GAME_NOT_PLAYABLE` “<game> needs an update before it can start. Update it in the Riot Client window, then press Play again.”.
5. Command: `"<installDir>\RiotClientServices.exe" --launch-product=<product> --launch-patchline=<patchline>` (`products.ts:66-68`). `underway` = `startedBySwitch` and the client running and the run's log shows the product's context or launch start (`launchAccepted`). When underway nothing is sent and the whole current run is watched from its start (prefix `how` = “the switch started the client with these arguments; ” on every annotation); otherwise the exe is started with the arguments from the install folder (handed to the running singleton, or starting the client), and the log is watched from a mark taken just before.
6. The watch: every 250 ms up to `launchConfirmMs` (90 s), in this order, the first hit ends it:
   1. the product's latest patch state says out of date → `outOfDate`;
   2. `gameStarted` (the pipeline reached `WaitForSessionExit` while the product's lines are the latest to name a product, or the session-ended line; `client-log.ts:407-417`) or the game's image names in the process list (at most once per second) → started;
   3. `launchPageShown` (the pipeline reached `WaitForLaunch`, or the client said why it is not launching, after the product's lines; `client-log.ts:390-399`) → `pageShown`;
   4. a sign-in verdict that is not a sign-in → `loginPage`.
   An abort → `stopped`.
7. After the watch: `accepted` = `launchAccepted`, `declined` = the reason of the last refusal after the product's lines (`client-log.ts:373-383`). When the page is shown and `riot.pressPlay` is on and a local-launch channel exists (`pressPlayOn`, `adapter.ts:984-986`), One Launch presses Play (below).
8. Results, in precedence order:
   - out of date (from the watch or the press) → throws the `GAME_NOT_PLAYABLE` update message.
   - login page → throws “The Riot Client opened on its login page instead of signing in (<detail>), so <game> was not started. Sign in there with “Stay signed in” ticked and press Play; One Launch saves the sign-in at the next switch.” (the saved copy is not marked stale here).
   - press started the game → `{ command: "<command> (<how>One Launch pressed Play and the Riot Client started the game)" }`.
   - stopped (watch or press) → `{ command: "<command> (<how>stopped watching for the game start)" }`.
   - page shown → `{ command: "<command> (<how>the Riot Client opened the game's page[: <declined reason>][; <press reason>])", handOff: "Press Play in the Riot Client window to start <game>; it is open on the game's page." }`.
   - started without the page → `{ command: "<command> (<how>the Riot Client started the game)" }`.
   - silence, context set → throws “<game> did not start within 90 s. Look at the Riot Client window: the game may need an update or a repair, or the client may be waiting for you.”.
   - silence, nothing logged → throws “The Riot Client did not act on the launch within 90 s. Look at the Riot Client window: it may be updating itself or asking you to sign in.”.

Pressing Play (`pressPlay`, `adapter.ts:994-1033`; `local-launch.ts`), rule 4's one exception, sent only after the page is up:

- `WindowsRiotLocalLaunch.launch` validates product and patchline against `^[a-z0-9_]+$`, reads the lockfile at call time (plain read, no size cap), parses it (section 1), then sends over `node:https` alone (no proxy, no redirect, a fresh agent that accepts the client's self-signed certificate, `keepAlive: false`): `POST https://127.0.0.1:<port>/product-launcher/v1/products/<product>/patchlines/<patchline>`, headers `authorization: Basic base64("riot:<password>")`, `accept: application/json`, `content-length: 0`, empty body, timeout 10 s (`LOCAL_LAUNCH_TIMEOUT_MS`), the abort signal passed through. The response body is drained and dropped; only the status counts. A status outside 200 to 299 → “the Riot Client answered the launch request with HTTP <status>”; a transport error → “the launch request did not reach the Riot Client (<code \| name \| timed out \| error>)” (never the message). The password is never kept, logged or put in an error (`local-launch.ts:3-8,86-126`).
- A thrown request: with the signal aborted → `stopped`; else WARN and `declined` with reason “the launch request was not taken: <error>”. Then the log (the same reader as the watch) and the process list are watched every 250 ms up to `playConfirmMs` (30 s): out of date → `out-of-date`; `gameStarted` or the game's image → `started`; abort → `stopped`; else `silent` with reason “the Riot Client took the launch request but did not start the game within 30 s”.
- A refused or silent press never fails the launch: the hand-over stands with the reason appended to the command (`adapter.ts:971`).
- **[dev-only]** the fake platform routes the request to the simulator's `pressPlay` and records it in the fake report (`riotLaunchRequests`); `pressPlayFails` makes it refuse (`docs/adapters/riot.md:137`).

##### 11. Health check

`healthCheck(known?)` (`adapter.ts:1739-1835`); overall = worst row, or `absent`.

| id | label | status | detail |
| --- | --- | --- | --- |
| `install` | Installation | `absent` (report ends, status `absent`) / `ok` | “Riot Games is not installed on this PC. Checked the Uninstall key, <installs path>, the riotclient:// URI handler, and <default dir>.” / “<installDir> (from the folder chosen in Settings \| HKCU \| HKLM \| RiotClientInstalls.json \| default location)” |
| `chosen-folder` | Chosen folder | `warn` when a chosen folder is not the install in use | “RiotClientServices.exe is no longer in <chosen>, the folder chosen in Settings, so the installation found automatically is used \| nothing is used. Use Locate to choose it again, or Reset to go back to automatic detection.” |
| `process` | Process | `ok` / `warn` | “RiotClientServices.exe is running” / “RiotClientServices.exe is not running” / “Could not query the process list: <error>” |
| `installs` | RiotClientInstalls.json | `ok` / `warn` | “rc_default = <path \| (absent)>; <n> game folder(s) associated with a client” / “<path> not found or unreadable; the client has not registered an install on this PC.” |
| `products` | Installed products | `warn` when the Metadata folder is missing or no product is installed | “<Metadata> not found. No Riot game has been installed on this PC.” / “<n> product folder(s) in Metadata[: <id> (installed \| installed, needs an update \| install folder missing \| not installed), …]” |
| `data` | Client data | `warn` when the data folder is missing, the file is unreadable, in an unknown shape or device-bound; else `ok` | “<dataDir> not found. The client has not run for this Windows user yet.” / “<dataDir>; RiotGamesPrivateSettings.yaml absent (the client has not saved a sign-in yet)” / “…present, signed out (no saved sign-in)” / “…is not in the shape One Launch knows (<reason>)” / “…holds a saved sign-in for Riot account …1234[, bound to a device key (is_dpop_bound), which a saved copy may not carry]” / “…could not be read: <error>” (`adapter.ts:1838-1859`) |
| `log` | Client log | `warn` when the folder is missing or the newest run's verdict is `rejected` | “<logDir> not found. Sign-ins cannot be confirmed until the client has run once.” / “<n> run log(s) in <logDir>; the newest run[ (build <build>)] reached no sign-in verdict \| ended on the login page (<detail>) \| signed in from the saved sign-in \| signed in by hand \| signed in” |
| `sessions` | Session snapshots | `warn` when any is stale | “<n> snapshot(s) stored[, <m> marked stale]” |
| `backups` | Backups | `ok` | “No snapshots yet” / “<n> snapshot(s) available for restore” |

##### 12. Session capture and `sessionState`

- Blob: `{"v":1,"file":"<the whole file text>"}`, vault-encrypted; `parseBlob` requires `v: 1` and a string `file` (`session.ts:40-44,203-213`). Keeping the whole file keeps both write strategies open (whole-file swap and splice).
- Fingerprint: SHA-256 hex of the `refresh_token`; it changes at every client start, so a file the fingerprint does not match is attributed by the account id instead (`session.ts:25,46-48`; `adapter.ts:596-597`).
- Owner rules: by stored fingerprint, then by the account id the file names, whether or not the client runs (`adapter.ts:588-600`).
- `sessionState()` (`adapter.ts:602-615`): `remembered` = the file holds a session; `fingerprint`; `launcherUid` = the file's account id; `linkedAccountId` = owner id; `launcherRunning`; `lastSignIn` from the newest run log (`sessionLastSignIn`, `adapter.ts:246-255`): null when the run has no verdict or its reason is `no-session` (a start with no saved sign-in is not an attempt); `{ok:true, reason:null}` for a sign-in, except that a closed client whose last run signed in **by hand** while the file holds no session gives `{ok:false, reason: "the last sign-in was not kept because “Stay signed in” was not ticked. Sign in to the Riot Client again with it ticked."}`; `{ok:false, reason: <detail>}` for `rejected`, `signed-out` or `error`.
- `captureSession(account)` refusals, all `SWITCH_BLOCKED`, in order (`adapter.ts:622-646`):
  1. unsaved account: “Save the account first, then capture its session.”
  2. (`NOT_INSTALLED`)
  3. the client running: “The Riot Client is running. Right-click its tray icon, choose Exit, then save again.” (the file of a running client says nothing about what the next start will find)
  4. no session in the file (absent or `null`): “The Riot Client has no saved sign-in. Sign in to it as <login> with “Stay signed in” ticked, exit the client, then save again.”
  5. another account owns it: “The sign-in the Riot Client has saved belongs to <owner>. Sign in to the Riot Client as <login> first.”
  6. the file's id differs from the account's linked id: “The Riot Client's saved sign-in is Riot account …1234, but <account> is linked to …5678. Check the Riot account id on the account page.”
  Then `storeSession` with the file's text and id. A file in an unknown shape throws `LAUNCHER_FILE` from the read.
- Stale marking: only a `rejected` verdict (an `invalid_grant`) on the remembered path marks the snapshot stale, with the log's detail “the client refused the saved sign-in (<status> invalid_grant)” or, on builds up to 116, “the client could not authenticate with the saved sign-in” (`adapter.ts:1659-1662`; `client-log.ts:178,219`). `markVerified` on a confirmed remembered sign-in with a stored snapshot, on a stored recapture, and when the file is found current (`adapter.ts:1656,1581,1587`).

##### 13. Error codes and user-facing messages

| Code | Thrown by | Message |
| --- | --- | --- |
| `NOT_INSTALLED` | every operation needing the install | “riot is not installed on this PC” |
| `LAUNCHER_FILE` | session file read or shape (`adapter.ts:580-583,590-592`) | “Could not read RiotGamesPrivateSettings.yaml (<reason>): <path>”, “RiotGamesPrivateSettings.yaml is not in the shape One Launch knows (<reason>): <path>”, “Too large to be a launcher file (<n> bytes): <path>” |
| `GAME_RUNNING` | preflight, `runSwitch`, `restoreBackup`, `launchGame` | “<game> is running. Close it, then switch \| restore \| press Play again: One Launch never closes a Riot game.”; “<game> is already running.” |
| `CONFIRM_REQUIRED` | `runSwitch` | “The Riot Client is running. Switching closes it.” |
| `SWITCH_BLOCKED` | `runSwitch`, `writeSession`, `captureSession` | the missing-file warning; “<path> is missing. Start the Riot Client once by hand so it writes its settings file, then try again.”; “No sign-in is saved for <account>.”; section 12 |
| `GAME_NOT_PLAYABLE` | `launchGame` | “<game> needs an update before it can start. Update it in the Riot Client window, then press Play again.” |
| `LAUNCHER_RUNNING` | `restoreBackup` | “Exit the Riot Client from its tray icon before restoring: it rewrites RiotGamesPrivateSettings.yaml while it runs, which would undo the restore.” |
| `ABORTED` (internal) | waits | “The switch was stopped” |
| `PROCESS_ACCESS_DENIED` | the platform on a refused close or kill | (platform message) |
| plain `Error` | shutdown, start, launch, wrong account, started meanwhile, blob checks, local launch | “RiotClientServices.exe is still running after a window close and a forced close”, “Riot Client.exe is still running after a window close and a forced close”, “RiotClientServices.exe keeps restarting itself after being closed”, “RiotClientServices.exe did not appear within 60 s”, “Not a Riot product id: <id>”, the launch failures and the wrong-account and started-meanwhile sentences (sections 7 and 10), “the stored Riot sign-in is not readable”, “the stored Riot sign-in is not in a known shape”, “the stored Riot sign-in holds no session (<reason>)”, “<reason>; the file is left alone”, “no <path> block” |

Messages thrown out of the adapter reach the dialog as “Riot Games: <message>” (`adapter.ts:894`).

##### 14. Log parsing

- Discovery (`client-log.ts:18-48`): files in `%LOCALAPPDATA%\Riot Games\Riot Client\Logs\Riot Client Logs` matching `^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}(?:\.\d+)?_\d+_Riot Client\.log$` (case-insensitive), sorted by name descending (the name is the run's local start time). A file of at most 64 KiB (`STUB_LOG_MAX_BYTES`) that contains `ProcessSingleton: Did not acquire process singleton` is the stub a second `RiotClientServices.exe` (a game shortcut while the client runs) leaves behind and is skipped; a stat or read error reads as not a stub.
- Marks and tails (`client-log.ts:419-456`): identical to Battle.net's: `{file, size}`; `eventsSince` reads from the mark when the newest file is the same and has not shrunk, else the whole new file; at most the last 8 MiB with the first partial line dropped; `currentRunEvents` = the whole current log; `currentRunBuild` reads the first 512 bytes for the build (`client-log.ts:459-468`).
- Line grammar: `<seconds since start, 6.3 digits>| <LEVEL>| <body>`; the first two lines are `Logging started at <local ISO time>` and `Application Version: <build>`. Lines that do not match the prefix are only looked at for the command-line echo: an indented `--launch-product=<id>` / `--launch-patchline=<id>` line (ids `[a-z0-9_]+`, lower-cased) is collected and emitted as one `launch-request` at the next prefixed line (`client-log.ts:89-90,133-150`).
- Skipped on purpose: any line over 16 KiB and **any line containing `telemetry` (case-insensitive)**, since those embed cookie and token values; the parser returns only fields a strict pattern matched, never a raw line (`client-log.ts:9-12,139`).
- Deduplication: consecutive identical `launch-request`, `product-context`, `patch-state`, `psl-state`, `signed-out`, `settings-written` and `quit` events count once (`client-log.ts:115-132`).
- Events, first match wins in this order (`client-log.ts:154-237`):

| Event | Distinguishing text | Fields |
| --- | --- | --- |
| `started` | `Logging started at <ISO>`; `Application Version: <n.n.n.n>` fills the build of the preceding `started` or starts one | `at`, `build` |
| `psl-state` | `riot-client-lifecycle: PSLProvider: PSL state changed to <State>` (build 140) | state. Known states: `PendingInitialization`, `PendingClientId`, `PendingPersistenceInitialization`, `RestoreAuthorization`, `PendingEulaData`, `PendingEulaAcceptance`, `PendingPersistentFlagsData`, `PendingProductContext`, `PendingProductContextInitialization`, `PendingLoginStrategy`, `LoginTokenExchange`, `PendingCreateAuthorization` |
| `manual-sign-in` | `rso-auth: Using direct token exchange` | |
| `signed-out` | `riot-client-auth: Deleted current RSO authorization` or `rso-auth: Successfully deleted RSO authorization` | |
| `session-rejected` / `auth-error` | level `ERROR` and `rso-auth: error '<name>'`; with `Failed to create RSO authorization (<3 digits>) invalid_grant` → rejected, detail “the client refused the saved sign-in (<status> invalid_grant)”; else auth-error with the error name | |
| `settings-written` | `private-settings: Updated private settings for user <x> with key <key>` | key (`psl.authorization.riot-client`, `rso-authenticator.tdid`, …) |
| `product-context` | `riot-client-lifecycle: Setting product context to <product>.<patchline>, willAutoLaunch=<true\|false>` | product, patchline, autoLaunch |
| `lifecycle` | `riot-client-lifecycle: Context #<n>: Middleware transition (<From>) -> (<To>)` | from, to (`WaitForLaunch` = page up; `WaitForSessionExit` = game running) |
| `launch-starting` | `product-launcher: Launch starting: productId='<p>' patchlineId='<pl>'` | product, patchline |
| `launch-declined` | `riot-client-lifecycle: Client is already running. Explicitly request focus instead of launching (<reason>)` or `riot-client-lifecycle: Attempting to launch default product with reason (<reason>)` (reason 1 to 80 characters without parentheses; seen: “Product is not up to date”, “Auto-launch is disabled”, “No product context or launch arguments specified at boot”) | reason |
| `patch-state` | `patch-proxy: HandleProductPatchlineStatusUpdate: productId: <p>, patchlineId: <pl>, status: <Status>` (builds 116 and 140; `launchable` null) | product, patchline, status (`UpToDate`, `OutOfDate`, `DependencyUpdate`, …) |
| `patch-state` | `patch-proxy: ProductPatchline: <x> state for <p>.<pl> changed from <A> to <B>, launchable: <0\|1>` (build 116) | status = B, launchable |
| `game-session-ended` | `product-launcher: Session for <p> on patchline <pl> has ended` | product, patchline |
| `signed-in` (how `restored`) | `riot-login: Successfully logged in with persistent session state` (builds up to 116) | |
| `session-rejected` | `riot-login: Did not restore persistent session state: Failed` (up to 116), detail “the client could not authenticate with the saved sign-in” | |
| `no-session` | `riot-login: Did not restore persistent session state: No stored session exists` (up to 116) | |
| `signed-in` (how `unknown`) + `settings-written` (`riot-login`) | `riot-login: Persisting <n> RSO auth cookies: … and <m> RSO auth scopes` (up to 116): a sign-in only when m > 0 and the last verdict so far is not a sign-in | |
| `quit` | `riot-client-lifecycle: Attempting to quit` | |
| `exit` | `ProcessSingleton: Removal of LockFile complete` | |

- `lastSignIn(events)` (`client-log.ts:242-310`): scanning backwards, the first decisive event: `psl-state` `PendingProductContext` or `PendingProductContextInitialization` → `{ok:true, how}` where `how` is `manual` when the attempt (back to the previous `PendingInitialization`) holds `manual-sign-in` or `LoginTokenExchange`, `restored` when it holds `RestoreAuthorization`, else `unknown`; `psl-state` `PendingLoginStrategy` → the login page with a reason from the attempt: `rejected` (a `session-rejected`), else `error` (“the sign-in did not complete (<error>)” after an `auth-error`), else `signed-out` (“signed out” after a `signed-out` or `PendingCreateAuthorization`), else `error` (“the saved sign-in was not restored” after a `RestoreAuthorization` with no verdict), else `no-session` (“no saved sign-in”); the pre-116 `signed-in`, `session-rejected` (`rejected`), `no-session`, `signed-out` events as themselves. Null while the client is still working or never tried.
- `startupFinished`: a verdict exists or `PendingEulaAcceptance` was logged (`client-log.ts:313-315`). `exitedCleanly`: an `exit` event (`client-log.ts:318-320`).
- `launchAccepted(product, patchline)`: any `product-context` or `launch-starting` naming `<p>.<pl>`. `launchDeclined`: the reason of the last `launch-declined` logged while the product's lines are the latest to name a product. `launchPageShown`: a `lifecycle` to `WaitForLaunch` or a `launch-declined` under the same condition. `gameStarted`: a `lifecycle` to `WaitForSessionExit` under the same condition, or the product's `game-session-ended`. The transitions and the refusals name no product, so another product chosen in the client meanwhile takes what follows (`client-log.ts:358-417`).
- `patchStates` / `productNeedsUpdate` / `productsNeedingUpdate`: the latest state per `<p>.<pl>`; needs update when status is `outofdate` (case-insensitive) or `launchable` is false (`client-log.ts:338-356`).

## 6. Background work

Everything that runs without a user action: the startup sequence and its ordering guarantees, the quit sequence and its budgets, every timer and poll in main and in the renderer, and the network work (artwork lookups, update checks, one Riot loopback request). Adapter-internal waits (closing a launcher, watching a sign-in log, launch confirmation) have their own `timing` defaults per adapter and belong to the adapter sections; they are only pointed at here.

### 6.1 Startup timeline

Steps are in order; each waits for the one before unless noted.

**Before Electron's `ready`** (the main module body, `src/main/index.ts:66-119`):

1. The entry module switches on Node's compile cache at `%TEMP%\one-launch-compile-cache`, then imports the main bundle (`src/main/boot.ts:4-9`).
2. The data folder is chosen from `--data-dir=`, `ONELAUNCH_DATA_DIR` (unpackaged only) or the default; **[dev-only]** the fake-launcher folder and update test feed are resolved from the environment (unpackaged only). Any failure writes one stderr line and exits with code 2 (`src/main/index.ts:79-87`, `src/main/platform/paths.ts:45-54`).
3. The app user model id is set to `dev.onelaunch.app` (`src/main/index.ts:88`).
4. The single-instance lock is requested; a second instance exits with code 0 at once, before opening the log, the database or a window (`src/main/index.ts:92-94`). The first instance later receives `second-instance` and shows and focuses its window (`src/main/index.ts:728-730,156-167`).
5. The `onelaunch-art` scheme is registered as privileged (standard, secure, stream) (`src/main/index.ts:97`).
6. The application menu is set to none so Electron never builds its default menu (`src/main/index.ts:100`).
7. The data, artwork, backups and logs folders are created and the log is opened (level `debug` in dev, else `info`; console mirror in dev only) (`src/main/index.ts:102-105`).
8. Startup marks are taken: process creation time, Node start, module-evaluated time (`src/main/index.ts:107-119`).
9. Hardening handlers for every web contents are installed: navigation to a non-trusted URL is blocked and logged, webviews are refused, `window.open` is denied and logged (`src/main/index.ts:247-259`).
10. `bootstrap` is scheduled for `ready` (`src/main/index.ts:715-717`).

**`bootstrap`, from `ready`** (`src/main/index.ts:421-713`). Nothing in it yields between the window creation and the IPC registration, which is the ordering guarantee the renderer relies on (see below).

1. The `starting` INFO line: version, Electron version, dev flag, data folder, data folder source, platform source `real` or `fake` (`src/main/index.ts:423-430`).
2. Open and migrate the database (section 4.3); log `database migrated` when anything applied (`src/main/index.ts:432-437`).
3. Settings: seed `whatsNew.lastSeen` on a fresh database; apply the stored `logLevel` (non-dev); bind the close-to-tray reader (`src/main/index.ts:439-443`).
4. Build the platform (real registry, process controller, shell opener, global `fetch`, Riot loopback requester, login item), or **[dev-only]** the fake platform from the scenario folder, loaded as a separate chunk only then (`src/main/index.ts:445-458`, `src/main/platform/wiring.ts:81-98`).
5. Permission request and permission check handlers that deny everything (`src/main/index.ts:460-462`).
6. The `onelaunch-art` protocol handler (`src/main/index.ts:464-475`).
7. **The window**: created hidden at the saved placement; shown at once (maximized first if it was) unless the start is hidden (`--start-hidden` and close-to-tray on); the page starts loading; `rememberBounds` attaches. The window title is `One Launch`, or **[dev-only]** `One Launch (fake launchers)` (`src/main/index.ts:295-333,409-413,480-482`).
8. Vault: create it, store the probe when absent, read the status; log `vault` at INFO, WARN when unavailable, ERROR when stored secrets no longer decrypt; startup continues (`src/main/index.ts:484-492`).
9. Services in order: accounts, clipboard (with the private writer), library, backups, the four adapters (Steam, Epic, Battle.net, Riot), **[dev-only]** the fake seed of accounts/sessions/settings, the launcher service, the switch journal (`src/main/index.ts:493-576`).
10. `SwitchRecoveryService.load()` reads what the last run left behind, before anything in this run can start a switch (`src/main/index.ts:577-579`).
11. The launch service (with the hide-after-instant-play hook), the artwork service, the update service, the quit cleanup closure, the rescan closure, the native file pickers (`src/main/index.ts:580-660`).
12. `registerIpcHandlers` for every channel; the `ipc` mark is taken (`src/main/index.ts:662-690`).
13. The deferred block is armed on the window's first `ready-to-show` (`src/main/index.ts:694-712`).

**Ordering guarantee**: because steps 8 to 12 never await, no IPC message (the preload's startup requests included), `ready-to-show` or `did-finish-load` can be dispatched before the handlers exist. The window is created first so its renderer process starts and fetches the page while main builds the services (`src/main/index.ts:477-479`). `recovery.load()` runs before the launch service exists so a journal written in this run is never read as a recovery (`src/main/services/switch-journal.ts:147-150`).

**The deferred block**, 250 ms after `ready-to-show` (`src/main/index.ts:694-712`):

1. Skipped entirely when a quit has begun or the database is already closed (a scenario's fast restart can do this) (`src/main/index.ts:697`).
2. The tray icon is created when close-to-tray is on (`src/main/index.ts:698`, `:170-188`).
3. A hidden start logs `started in the tray` if the tray exists; otherwise the window is shown, since nothing else could reach it (`src/main/index.ts:699-702`).
4. The startup library scan: every implemented launcher in order, then artwork prune, then artwork enqueue when auto-fetch is on; a failure logs `startup library scan failed` at WARN and the library keeps the last scan (`src/main/index.ts:634-640,703`, `src/main/services/library-scan.ts:17-45`).
5. The database copy into `backups\db` (section 4.3.6) (`src/main/index.ts:705-709`).
6. `updates.start()`: resolve availability, then schedule the first check 15 s later when available and automatic updates are on; a failure to start logs `updater did not start` at WARN (`src/main/index.ts:710`, `src/main/updates/update-service.ts:95-99`).

Steps 4 to 6 run concurrently with each other (none is awaited).

**Timing marks**: one `startup` INFO line is written once both the first paint (`ready-to-show`) and the bundle load (`did-finish-load`) have been timed, in whichever order they arrive, with every mark (creation epoch, process-to-Node, module loaded, ready, db, vault, ipc, window created, window shown, ready-to-show, did-finish-load) (`src/main/index.ts:335-361`). `app:info` exposes `processToNodeMs`, `windowShownMs`, `readyToShowMs`, `didFinishLoadMs` (`src/shared/models.ts:319-328`).

**A throw anywhere in `bootstrap`** logs `fatal during startup` at ERROR, shows the native error box “One Launch could not start” with the message, the log path and the data folder, and exits with code 1 (`src/main/index.ts:718-726`).

**The renderer's side of startup**:

1. At document start the preload sends the startup requests `library:list`, `launch:recent` (limit 8), `settings:getAll`, `accounts:list`, `app:info`, `recovery:get`, `updates:status`, each settling on its own (a rejection becomes that channel's error envelope) (`src/preload/index.ts:18-28`, `src/shared/ipc-contract.ts:243,249-257`).
2. The page's first module waits for those answers or 50 ms, whichever comes first, then React renders (`src/renderer/src/boot.ts:4-7`, `src/renderer/src/main.tsx:24-33`). Until React's first commit a CSS-only boot surface shows the title strip and, from 250 ms, fades the mark in over 200 ms (`src/renderer/index.html:11-14`, `src/renderer/src/index.css:155-159`).
3. Components mounting in that first render take their data from the startup answers (`peekStartup`); the root's first effect closes the window so every later mount asks main afresh (`src/renderer/src/lib/startup.ts:62-78`, `src/renderer/src/App.tsx:31-35`).
4. Once each, at mount: the recovery prompt (from the startup answer or one `recovery:get`) (`src/renderer/src/components/RecoveryDialog.tsx:112-132`); the setup guide opens when `firstRun.dismissed` is not `true` and there are no accounts (`src/renderer/src/components/SetupGuide.tsx:358-376`); the What's New card opens 400 ms later when the running version has unseen notes, no recovery is pending and the setup guide is not up; with the feature off it only records the version as seen (`src/renderer/src/components/WhatsNewDialog.tsx:12,170-200`).
5. Renderer errors outside React (window `error`, `unhandledrejection`) are reported to main through `app:reportError` and logged at ERROR (`src/renderer/src/main.tsx:15-21`, `src/main/ipc/index.ts:142-149`).

### 6.2 Quit timeline

**What a close request does** (the caption button, Alt+F4, or a quit already under way) (`src/main/index.ts:381-395`, `src/main/window-guard.ts:26-30`):

- A quit already under way (or a confirmed quit): the window closes.
- Otherwise, with a tray icon present and `window.closeToTray` on: the window hides; `window hidden to the tray` is logged at INFO.
- Otherwise, with a switch running: the user is asked (below).
- Otherwise: the window closes; `window-all-closed` then quits the app (`src/main/index.ts:732-734`).

**Quit during a switch** (`src/main/index.ts:194-231`): if the job has not been created yet (a start is still in its preflight), the switch is stopped and the close goes through without a prompt. Otherwise a native warning dialog, owned by the window (which is shown first, since a quit from the tray reaches it hidden): title “A switch is still running”, message “One Launch is still switching to <account>.”, detail “Quitting stops the switch. If <launcher> has not been started again yet, its files are put back first; after that <launcher> carries on signing in on its own. Left alone, the switch finishes once the launcher reports the sign-in.”, buttons “Keep running” (default and cancel) and “Quit anyway”. Only one prompt is open at a time. “Quit anyway” stops the switch (below), marks the quit confirmed and closes the window. **[dev-only]** a fake scenario can pre-answer the prompt (`src/main/index.ts:136-137,207-210`).

**Stopping the running switch** (`src/main/index.ts:142,147-153`): abort the job and wait up to 10 s (`QUIT_WAIT_MS`) for it to wind down; a switch that outlives the wait logs `switch still running at quit` and leaves its journal for the next start. Every adapter wait ends within one poll of an abort; the rest of the budget covers a restore of files written before the launcher restart.

**`before-quit`** (every quit path except a hard `app.exit`) (`src/main/index.ts:736-754`):

1. `quitting` is set, so later closes are real closes.
2. With a running switch not yet confirmed and a window: the quit is cancelled and the prompt above runs; if the user keeps running, `quitting` is reset.
3. Otherwise, once only: the quit is cancelled, the running switch is stopped (10 s budget), then the cleanup runs raced against a 2 s cap, any failure logs `quit cleanup failed` at WARN, and `app.quit()` is called again (this time passing through).

**The cleanup** (`src/main/index.ts:625-632`): stop the updater (clears its timer, cancels a download); stop the artwork queue (drops pending jobs; an in-flight job finishes without touching the database); tell the launcher service a quit has begun (its later failures log at debug) and wait up to 1.5 s (`LAUNCHER_IDLE_AT_QUIT_MS`) for in-flight adapter calls, logging `launcher work still running at quit` otherwise; clear a copied secret from the clipboard if it is unchanged, logging `cleared a copied value from the clipboard at quit` when it did.

**`will-quit`** (`src/main/index.ts:756-765`): destroy the tray icon, close the database (an error logs at WARN), log `exit`.

**`quit`** (`src/main/index.ts:767-770`): the last event; `installAtQuit(exitCode)` may start a pending update installer (section 6.3, Updates). It runs after the database is closed and stays synchronous.

**Logoff or shutdown** (`session-end`) (`src/main/index.ts:397-403`): the quit counts as confirmed (no prompt), the updater is told so it will not install, the switch is aborted and the cleanup runs at once.

**Uncaught exception in main** (`src/main/index.ts:772-805`): logged at ERROR; the updater is told (no install at this quit). If a window exists and no crash dialog was shown in the last 60 s, the window is shown and a native error dialog asks: title “One Launch hit an unexpected error”, message “Something went wrong inside One Launch.”, detail “<reason>\n\nThe details are in <log path>. Relaunching is the safe choice; keeping it running may leave things half-done.”, buttons “Relaunch” (default) and “Keep running” (cancel). Relaunch marks the quit confirmed, schedules a relaunch with the same arguments minus `--start-hidden`, calls `app.quit()` (so the switch is stopped, the clipboard cleared and the database closed) and, as a fallback, `app.exit(0)` after 15 s.

**Unhandled promise rejection**: logged at ERROR, nothing else (`src/main/index.ts:806-808`).

**Renderer process gone** (`src/main/index.ts:363-371`): logged at ERROR with reason and exit code; unless it was a clean exit, the page reloads once; a second loss within 60 s stays down.

### 6.3 Timers, polls and watchers

| Owner | Trigger | Interval / budget | Stops when | On error |
| --- | --- | --- | --- | --- |
| Main: deferred startup block (`src/main/index.ts:694-712`) | First `ready-to-show` | Once, 250 ms later | Skipped if quitting or DB closed | Each part logs its own WARN (scan, copy, updater) |
| Main: `rememberBounds` (`src/main/index.ts:273-293`) | resize, move, maximize, unmaximize | Debounced 500 ms; also at once on `close` | Window destroyed | n/a |
| Main: renderer reload guard (`src/main/index.ts:363-371`) | `render-process-gone` | Reload once; a second loss within 60 s is not reloaded | n/a | Logged at ERROR |
| Main: crash dialog guard (`src/main/index.ts:777-805`) | `uncaughtException` | One dialog per 60 s; relaunch fallback `app.exit(0)` after 15 s | n/a | `could not show the crash dialog` at WARN |
| Main: quit budgets (`src/main/index.ts:142,144,751`) | `before-quit` | Switch wind-down 10 s; cleanup cap 2 s; adapter idle 1.5 s | n/a | `quit cleanup failed` at WARN; quit continues |
| Main: `UpdateService` schedule (`src/main/updates/update-service.ts:27-28,95-99,192-199`) | `start()` after availability says ok | First check 15 s after start, then every 6 h (both overridable by the constructor; the app uses the defaults) | `stop()`, automatic updates off, or an update is verified (`ready`) | A failed check sets `failed` with step `check` and logs at WARN; the next check is still scheduled 6 h later (`:197`) |
| Main: update download (`src/main/updates/update-service.ts:244-266`) | Verified manifest | Until done; progress reported 0..100 | `stop()`; turning automatic updates off cancels a **background** download only (`:107-119`); a cancelled download returns the state to `idle` | `failed` step `download`, WARN |
| Main: artwork queue (`src/main/services/artwork.ts:173-177,311-356`) | After every scan (auto-fetch on), `artwork.autoFetch` turned on, refresh (card menu, Settings), API key saved | Concurrency 2; each request `AbortSignal.timeout` 15 s; bodies capped at 10 MiB (images), 1 MiB (JSON); redirects at most 3; a miss is not retried automatically for 7 days | `stop()` at quit drops the queue; an in-flight job finishes without writing | Problems are recorded per game in `artwork_attempts`; a crashed job logs `artwork job crashed` at ERROR and counts as failed |
| Main: clipboard clear (`src/main/services/clipboard.ts:3,37-70`) | `accounts:copy` | 30 s (`DEFAULT_CLIPBOARD_CLEAR_MS`), timer unref'd so it never keeps the process alive | A newer copy replaces the pending one; quit cleanup clears early | A clipboard read failure means no clear |
| Main: private clipboard write (`src/main/platform/clipboard-private.ts:26-54`) | `accounts:copy` | PowerShell child killed after 8 s | n/a | Falls back to the plain clipboard write; `clipboard history exclusion unavailable; copied with the plain clipboard` at WARN (`src/main/index.ts:498`) |
| Main: `LaunchService` job (`src/main/services/launch.ts:289-427`) | `launch:start` | One job at a time; the ready grace from the adapter (`readyAt`) is slept before the launch step; each step's duration is logged (`launch step finished`) | `cancel`, `stop()` (abort signal); `whenIdle(ms)` resolves false after `ms` | A refusal logs at WARN, anything else `launch job failed` at ERROR; the job ends `failed` |
| Main: preflight reuse (`src/main/services/launch.ts:67-70,233-242`) | A preflight before a start | A preflight for the same account is reused for up to 10 s; a direct job (launcher already signed in as the account) reuses one only up to 2 s old and takes it again otherwise | Each preflight answers one start | n/a |
| Main: in-memory job history (`src/main/services/launch.ts:65,417-418`) | Every finished job | Newest 20 kept for `launch:status` | Process exit | `launch:status` for an older job: `NOT_FOUND` |
| Main: `LauncherService.status` (`src/main/services/launchers.ts:50,185-213`) | `launchers:status` | No timer; a pass over 2 s logs `launcher status was slow` at WARN | n/a | The launcher's row gets `attention` with the message; the list still answers |
| Main: child processes for registry reads/writes (`src/main/platform/registry.ts:175-184`) | Adapter reads, status, install detection | 10 s per `reg.exe`/`cmd.exe` call, 8 MiB output cap | n/a | `REGISTRY` error; exit code 1 reads as “not there” |
| Main: child processes for process listing/control (`src/main/platform/processes.ts:469-472`) | Adapter checks, preflight, switch | 15 s per PowerShell call, 8 MiB output cap | n/a | An empty process list is an error, never “nothing runs” (`:529-531`) |
| Main: Riot loopback launch request (`src/main/launchers/riot/local-launch.ts:23,59-84`) | Riot Play on behalf of the user | 10 s socket timeout | Abort signal | The hand-over is left as it was; the reason is kept without any response body |
| Main: adapter waits (`src/main/launchers/wait.ts:11-50`) | Inside a switch or launch | `sleep(ms)` resolves early on abort; `waitUntil(cond, {timeoutMs, pollMs})` polls until true or the deadline and throws `ABORTED` as soon as the signal aborts; `mergeTiming` applies overrides and never lets a finer `*PollMs` be slower than an overridden coarse `pollMs` | Abort signal | A timed-out wait returns false; the adapter decides |
| Renderer: `LaunchHost` job poll (`src/renderer/src/components/LaunchHost.tsx:88-112`) | A job in state `running` | `launch:status` every 150 ms | Job no longer running or host unmounted | The job is marked `failed` locally with the error text |
| Renderer: launch banner auto-dismiss (`src/renderer/src/components/LaunchHost.tsx:78-86`) | Job `done` and the dialog closed | 8 s | A new job replaces it | n/a (failures stay until dismissed) |
| Renderer: Library artwork/revision poll (`src/renderer/src/views/LibraryView.tsx:190-216`) | Library mounted | `artwork:status` every 500 ms while the queue has work, 1000 ms until games are on screen, 4000 ms otherwise; the library and recent strip reload when `revision` moves | Unmount | Retry after 3000 ms |
| Renderer: Library clock (`src/renderer/src/views/LibraryView.tsx:97-100`) | Library mounted | `now` refreshed every 60 s for relative times | Unmount | n/a |
| Renderer: Library launcher status (`src/renderer/src/views/LibraryView.tsx:127-143`) | Library empty and no status known | Once | n/a | Empty state keeps saying it is scanning |
| Renderer: Library reload after a job (`src/renderer/src/views/LibraryView.tsx:145-152`) | Active job leaves `running` | Once per transition | n/a | n/a |
| Renderer: Settings › Artwork queue (`src/renderer/src/components/ArtworkPanel.tsx:43-47`) | Panel mounted | `artwork:status` every 3 s | Unmount | Row shows `…` until the next poll |
| Renderer: setup guide game count (`src/renderer/src/components/SetupGuide.tsx:40-69`) | Guide open | `artwork:status` every 1 s; `library:list` again when `scanned` and the revision moved | Guide closed | Asked again next tick |
| Renderer: update status (`src/renderer/src/lib/updates.ts:6,13-36`) | Sidebar (always mounted) and Settings › Updates | Every 1 s while `checking` or `downloading`; otherwise every 60 s in the sidebar, every 5 s in the Updates panel (`src/renderer/src/components/Sidebar.tsx:21`, `src/renderer/src/components/UpdatesPanel.tsx:13`); the store ignores an unchanged answer | Unmount | The next poll tries again |
| Renderer: flash messages (`src/renderer/src/lib/useFlash.ts:11-28`) | A status text is set | 3 s (info, success), 6 s (error), or a caller-given time | A newer flash restarts the timer | n/a |
| Renderer: What's New open delay (`src/renderer/src/components/WhatsNewDialog.tsx:12,195-197`) | Startup with unseen notes | 400 ms | Setup guide up at that moment (card waits for the next start) | Swallowed |
| Renderer: startup prefetch cap (`src/renderer/src/boot.ts:7`, `src/renderer/src/lib/startup.ts:40-60`) | Page load | 50 ms | n/a | A failed answer makes the component fetch as usual |
| Renderer: motion helpers (`src/renderer/src/lib/motion.ts:18-58`) | Setup guide demos | `useCountUp` animates 600 ms on frames; `useLoop` ticks every `everyMs` | Reduced motion on (held still) | n/a |
| Renderer: sidebar width watcher (`src/renderer/src/App.tsx:71-77`, `src/renderer/src/store/ui.ts:8`) | Window width crosses 1100 px | Media-query event | Unmount | n/a |

Adapter timing defaults (poll intervals, shutdown, startup, sign-in, manual sign-in, capture and launch-confirmation budgets) are per adapter: Steam `src/main/launchers/steam/adapter.ts:83-99`, Epic `src/main/launchers/epic/adapter.ts:115-130`, Battle.net `src/main/launchers/battlenet/adapter.ts:136-160`, Riot `src/main/launchers/riot/adapter.ts:143-174`; the real platform passes no overrides (`src/main/platform/wiring.ts:96`), **[dev-only]** the fake platform shortens them. See the adapter sections.

### 6.4 Network work

Only three things on the machine talk to a network, and the renderer cannot (CSP `connect-src 'none'`, `electron.vite.config.ts:19`):

1. **Artwork lookups and downloads** (`src/main/services/artwork.ts`), through the platform's `fetch` (the global `fetch` for the real platform, `src/main/platform/wiring.ts:92`). Triggered by the queue above; details of endpoints and checks in section 7.
2. **Update checks and downloads** (`src/main/updates/engine.ts`, `src/main/updates/update-service.ts`), through electron-updater. Triggered by the schedule above and by “Check for updates”; a manual check is ignored while stopped, unavailable, already verified, or already running (`src/main/updates/update-service.ts:101-105`). A background check whose answer arrives after automatic updates were turned off is dropped without downloading (`:232-235`). Details in section 7.
3. **One Riot loopback request** to `127.0.0.1` on the Riot Client's own port, only when Riot's “press Play” setting is on (`src/main/launchers/riot/adapter.ts:985-986,1001`, `src/main/launchers/riot/local-launch.ts`). Details in section 7.

The artwork service's queue semantics (`src/main/services/artwork.ts:243-356`):

- `enqueueMissing` queues every installed game without a grid file on disk (a launcher icon standing in counts as missing), skipping games whose last miss is under 7 days old unless the user asked (`ignoreAttempts`).
- `refresh(gameId, force)`: one game (forced replaces even a manual image) or all (`force` false: missing ones, ignoring the retry window; `force` true: every game except those with a manual image).
- A request for a game already running is covered by that run, unless it is a forced request and the run is not forced: then the game runs again afterwards (`again`).
- A job holds its downloads in memory and writes nothing until after its last await; it then drops everything if the service was stopped or the user picked a manual image meanwhile (`src/main/services/artwork.ts:64,406-415`).
- `status()` reports `queued`, `running`, `completed`, `failed`, `hasApiKey`, the library `revision` and `scanned` (`src/main/services/artwork.ts:231-241`); it is what the renderer polls.
- `idle()` resolves when the queue has drained (used by tests and scenarios) (`src/main/services/artwork.ts:290-294`).

### 6.5 The library scan

`scanLibrary` (`src/main/services/library-scan.ts:17-45`) runs at startup (deferred block), on “Rescan”, and after a launcher folder is chosen or reset (not awaited there) (`src/main/ipc/index.ts:330-333,366-368,387,397`):

- Launchers are scanned one after another, in launcher-id order.
- A launcher whose adapter reports `NOT_INSTALLED` is synced as having no installed games (its rows become `installed = 0`) and reported with `installed: false`.
- Any other adapter failure is logged at WARN, leaves that launcher's rows untouched, and is reported on its entry as `error`.
- `markScanned` is set in a `finally`, so `ArtworkStatus.scanned` becomes true even after a failure; until then an empty library means “still scanning”.
- After the scan, main prunes the artwork cache and, with auto-fetch on, queues missing artwork, logging `artwork queued` with the count (`src/main/index.ts:634-640`).

## 7. External integrations

Every network API, loopback request, protocol handler and hand-off to the system shell. The renderer has no network access at all (CSP `connect-src 'none'`, `electron.vite.config.ts:19`); the only code that talks to a network is the artwork service, the updater, and one Riot loopback request from main.

### 7.1 What leaves the PC, and what never does

**Leaves the PC:**

| Data | To | When | Citation |
| --- | --- | --- | --- |
| A game's display name (trademark marks `®™©` stripped, URL-encoded) | Steam store search, Steam community search | Looking up the Steam appid of a non-Steam game without a known id | `src/main/services/artwork.ts:74-77,485-489` |
| A Steam appid | Steam CDN hosts; SteamGridDB (`steam/<appid>`) | Downloading covers; SteamGridDB lookups by appid | `src/main/services/artwork.ts:516,540` |
| A game's display name (URL-encoded) | SteamGridDB autocomplete | Only when no Steam appid is known | `src/main/services/artwork.ts:542` |
| The SteamGridDB API key | SteamGridDB API only, in the `Authorization: Bearer` header; never on an image download, never following a redirect | Each SteamGridDB API call | `src/main/services/artwork.ts:534,573,585-591` |
| The app's version and the fixed staging id `00000000-0000-0000-0000-000000000000` (header `x-user-staging-id`) | GitHub (electron-updater's release feed for `rjf2016/one-launch`) | Update checks and downloads | `src/main/updates/engine.ts:10-17,61-62` |
| The Riot product id and patchline, with the lockfile's per-run password as HTTP basic auth | `127.0.0.1` only (the Riot Client's own local server) | Riot “press Play” | `src/main/launchers/riot/local-launch.ts:112-120` |
| `steam://`, `com.epicgames.launcher://`, `battlenet://` URIs and one `https://www.steamgriddb.com/...` page | The Windows shell (whatever handler is registered) | Game launches; “Get a key” | 7.6 |

**Never leaves the PC:** account logins, display names, passwords, session blobs, launcher account ids, SteamID64s, Riot IDs, the data folder path, the Windows user name, any per-install identifier (the updater's `.updaterId` is never written, `src/main/updates/engine.ts:12-17`), any log line, any telemetry or crash report (no such code exists; grep `fetch(` in `src/main` finds only `src/main/platform/wiring.ts:92` and the local `net.fetch` of a `file://` URL in `src/main/index.ts:468`). A Riot session is never exported anywhere (`docs/adapters/riot.md` rules; the only network code in the Riot adapter is the loopback request above).

### 7.2 Artwork: Steam (no key)

**Steam appid without a lookup** (`src/main/services/artwork.ts:627-633`): a Steam game's own `launcherGameId`; a Battle.net product's `steamAppId` from the product table; an Epic app's id from `EPIC_STEAM_APP_IDS`; otherwise none.

**Steam search** (non-Steam games with no known appid) (`src/main/services/artwork.ts:482-511`):

| Order | Endpoint | Method | Query |
| --- | --- | --- | --- |
| 1 | `https://store.steampowered.com/api/storesearch/` | GET | `?term=<name>&l=english&cc=US` (`:75`) |
| 2 | `https://steamcommunity.com/actions/SearchApps/<name>` | GET | path segment (`:76`) |

- Sent with `redirect: 'error'` and a 15 s timeout; no headers (`:489`, `:174`).
- A 404 moves to the next endpoint; any other non-2xx is an error for that endpoint; the JSON body is read up to 1 MiB (`:490-492`, `:87`).
- The answer is either an array or an object with `items`; each item must have `type` absent or `app`, a `name`, and an `id` or `appid` that is a non-negative integer (number or digit string) (`:493-504`).
- Accepted only when the item's name normalised equals the game's name normalised: lowercase, trademark marks removed, every run of non-letter/non-digit characters collapsed to one space, trimmed. A looser match would put another game's cover on the card (`:479-480,502,635-642`).
- On no match, `appId` is null; the last error (if any) is recorded as `Steam search: <error>` in the game's problems (`:384,510`).

**Steam CDN** (with an appid, per missing kind) (`src/main/services/artwork.ts:71-72,513-526`):

| Host | File per kind |
| --- | --- |
| `https://cdn.cloudflare.steamstatic.com/steam/apps/<appid>/<file>` | grid `library_600x900.jpg`, hero `library_hero.jpg`, logo `logo.png` |
| `https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/<appid>/<file>` | same |

The first host is tried; a `HTTP 404` moves to the second; any other failure stops with that error (`:517-523`). The second host answers with a redirect to `shared.steamstatic.com`, which is on the allowlist (`:83-84`). The result source is `steamcdn`.

### 7.3 Artwork: SteamGridDB (`api/v2`, key required)

Used for whatever kinds are still missing, only when a key is stored (`src/main/services/artwork.ts:394-404`). Without a key the recorded problem is “No SteamGridDB API key in Settings” (an appid was known) or “Not found on Steam; add a SteamGridDB API key in Settings” (`:402`).

Base `https://www.steamgriddb.com/api/v2` (`:79`). Every API call: GET, header `Authorization: Bearer <key>`, `redirect: 'error'`, 15 s timeout (`:534,573`).

| Step | Endpoint | Query |
| --- | --- | --- |
| Identify the game, when no Steam appid | `/search/autocomplete/<encoded name>` | none; the first result's numeric `id` is taken; none means problem `SteamGridDB: no match for "<name>"` (`:542-545`) |
| Grids | `/grids/steam/<appid>` or `/grids/game/<id>` | `dimensions=600x900&types=static&nsfw=false&humor=false` (`:80-81,548`) |
| Heroes | `/heroes/steam/<appid>` or `/heroes/game/<id>` | `types=static&nsfw=false&humor=false` |
| Logos | `/logos/steam/<appid>` or `/logos/game/<id>` | `types=static&nsfw=false&humor=false` |

Answer handling (`apiJson`, `src/main/services/artwork.ts:571-583`):

| Status | Outcome |
| --- | --- |
| 401 or 403 | `ArtworkError` code `UNAUTHORIZED`, text “the API key was rejected”; the whole SteamGridDB step for that game fails with `SteamGridDB: the API key was rejected` |
| 404 | Treated as an empty result (`[]`), so “no match” / “no <kind>” |
| 429 | `ArtworkError` code `RATE_LIMITED`, “rate limited, try again later” |
| other non-2xx | `HTTP <status>` |
| 2xx | Body (JSON up to 1 MiB) must have `success: true`, else the API's `errors` joined with `, ` and cut to 200 characters, or “request failed”; the `data` field is returned |

For each kind, the first asset's `url` is taken; none gives problem `SteamGridDB: no <kind>`; a URL not on an artwork host gives `SteamGridDB <kind>: the image is not on an artwork host`; a download failure gives `SteamGridDB <kind>: <error>` (`:547-564`). The result source is `steamgriddb`.

The key is read from the vault at the moment of each job and never cached or logged (`src/main/services/artwork.ts:22-23,140-141,396`, `src/main/index.ts:608`).

### 7.4 Artwork: image downloads (common rules)

`download(url)` (`src/main/services/artwork.ts:585-606`), used for Steam CDN and SteamGridDB images alike:

- **Host allowlist**: the URL must be `https:` on the default port and its host one of `cdn.cloudflare.steamstatic.com`, `shared.cloudflare.steamstatic.com`, `shared.steamstatic.com`, `cdn2.steamgriddb.com`; otherwise “not an artwork host” (`:83-96,590`).
- **No credentials**: images are fetched with no headers at all (`:591`).
- **Redirects by hand**: `redirect: 'manual'`; statuses 301, 302, 303, 307, 308 are followed, at most 3 hops (“too many redirects”), each `Location` resolved against the current URL and checked against the same allowlist (“redirected away from the artwork hosts”); a redirect without a parsable `Location` is `HTTP <status>`; the redirect body is cancelled (`:85-86,589-598`).
- **Status**: must be 2xx, else `HTTP <status>` (`:599`).
- **Content type**: must start with `image/` (case-insensitive), else “not an image (<type or 'no content type'>)” (`:600-601`).
- **Size**: a declared `content-length` over 10 MiB is refused before reading (“too large (<n> bytes)”); the body is streamed and the connection dropped the moment 10 MiB is exceeded (“too large (over <n> bytes)”) (`:98-120,176,602`).
- **Magic bytes**: the bytes must sniff as JPEG (`FF D8 FF`), PNG (8-byte signature) or WebP (`RIFF....WEBP`), else “unrecognised image data”; the sniffed type decides the file extension, not the URL or the content type (`:603-604`, `src/main/services/artwork-url.ts:69-87`).
- **Timeout**: one 15 s `AbortSignal.timeout` covers the whole redirect chain (`:587,591`).
- The launcher-icon fallback is a local file read, no network, accepting JPEG/PNG/WebP or a Windows `.ico` (ICONDIR header, type 1, at least one image), read with the 4 MiB launcher-file cap (`:450-467`, `src/main/services/artwork-url.ts:61-66`, `src/main/launchers/read-file.ts:5,13-19`).

**[dev-only]** The fake platform replaces `fetch` with a fixture reader and records every URL in `fake-platform.json` (`src/main/platform/wiring.ts:81-85`, `src/main/launchers/sim/report.ts:44-47`).

### 7.5 Updates (electron-updater over GitHub releases)

**Feed**: provider `github`, owner `rjf2016`, repo `one-launch`, set in code (`src/main/updates/engine.ts:10,61-62`); the installer build also writes `resources\app-update.yml` naming the same repo (`electron-builder.yml:48-53`). electron-updater's GitHub provider resolves the latest release and fetches its `latest.yml` and the named installer from the release's assets; those URLs are the library's, not the app's (see Open questions). As implemented by electron-updater 6.8.10's GitHub provider (`node_modules/electron-updater/out/providers/GitHubProvider.js`, library code, not the app's): `GET https://github.com/rjf2016/one-launch/releases.atom` (the release list), `GET https://github.com/rjf2016/one-launch/releases/latest` with `Accept: application/json` (reads `tag_name`; `allowPrerelease` is off so GitHub's own latest decides), `GET https://github.com/rjf2016/one-launch/releases/download/<tag>/latest.yml`, then the installer `…/releases/download/<tag>/<files[0].url>` and, when differential download applies, its `.blockmap`. Drafts are invisible to `releases/latest`, which is why publishing is the rollout (`docs/development.md:151`).

**Updater flags** (`src/main/updates/engine.ts:64-70`): the staging id is replaced by the constant `SHARED_STAGING_ID` = `00000000-0000-0000-0000-000000000000` so every copy sends the same `x-user-staging-id` and no `.updaterId` file is written; `autoDownload` off; `autoInstallOnAppQuit` off (main starts the installer itself); `allowPrerelease` off (electron-updater would turn it on for a `-beta` version; GitHub's latest release decides); `allowDowngrade` off; `disableWebInstaller` on. electron-updater's own log goes to the app log with its `error` and `info` lowered to WARN and DEBUG, so an offline PC never writes an ERROR line (`:19-28`). Differential download is left at the library's default for the GitHub feed and disabled only for the **[dev-only]** test feed (`:59`).

**Availability** (`src/main/updates/availability.ts:25-39`, inputs from `src/main/index.ts:445-451,616`), resolved once per run: updates are `unavailable` with a reason unless the exe is packaged (`development`), the data folder source is `default` (`custom-data-folder`), `Uninstall OneLaunch.exe` sits beside the exe (`not-installed`), and `HKCU\Software\37275e3e-77ba-54aa-bc32-31551d28dc0d` value `InstallLocation` equals the exe's folder, compared case- and separator-insensitively (`all-users-install`, also when the read fails). The GUID is electron-builder's UUIDv5 of the `appId`. **[dev-only]** A test feed makes everything available (`:26`).

**`latest.yml` (the manifest) and what is accepted** (`verifyManifest`, `src/main/updates/manifest.ts:56-89`), checked in this order, each failure a refusal with a reason and the text Settings shows:

| Check | Reason | Detail |
| --- | --- | --- |
| The build trusts at least one key | `no-keys` | “this build trusts no release key” |
| Manifest is an object | `malformed` | “the release manifest is not an object” |
| `packages` absent or null | `unsupported` | “the release uses a web installer” |
| `files` is an array of exactly one entry | `unsupported` | “the release must list exactly one file” |
| That entry is an object | `malformed` | “the release file entry is not an object” |
| `files[0].isAdminRightsRequired` is not `true` | `unsupported` | “the release asks for administrator rights” |
| `version` matches `^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$` | `malformed` | “the release version is not a plain version number” |
| `files[0].url` matches `^[A-Za-z0-9][A-Za-z0-9._-]*\.exe$` (a plain file name) | `malformed` | “the release file is not a plain .exe name” |
| `files[0].sha512` is 86 base64 characters plus `==` | `malformed` | “the release file has no SHA-512” |
| `files[0].size` is a positive safe integer | `malformed` | “the release file has no size” |
| `signature` and `keyId` both present | `unsigned` (when both absent) | “the release is not signed” |
| `signature` is 86 base64 characters plus `==` | `malformed` | “the release signature is malformed” |
| `keyId` is 16 lowercase hex characters | `malformed` | “the release key id is malformed” |
| `keyId` names a trusted key | `unknown-key` | “the release is signed with a key this build does not trust (<keyId>)” |
| Ed25519 verify of the payload | `bad-signature` | “the release signature does not match” |

**Signature payload** (`src/main/updates/manifest.ts:6,29-31`): the UTF-8 string
`one-launch-update-v1\n<version>\n<url>\n<sha512>\n<size>\n`
(five lines, each newline-terminated), signed raw with Ed25519 (no prehash). **Key id derivation** (`:33-35`): the first 16 hex characters of SHA-256 over the public key's SPKI DER bytes. The verifier accepts only an `ed25519` public key; any crypto error is a refusal (`:91-98`). **Trusted keys** (`src/main/updates/release-keys.ts:4-7`): two, with ids `616ad3e53aac3ce4` (primary) and `35f13c45c6049dbe` (backup); the SPKI values live in that file and need not be reproduced. An empty list refuses every release.

**Check and download flow** (`src/main/updates/update-service.ts:215-282`):

1. Availability gate; log `update check` with origin `background` or `manual`; state `checking`.
2. `checkForUpdates()`; a throw becomes state `failed` step `check` with `describeUpdateError` text (below), WARN. No newer version: state `up-to-date` with `checkedAt`.
3. A background check with automatic updates now off: state back to `idle`, nothing downloaded.
4. `verifyManifest` and the `downloadUpdate()` call happen in one synchronous step so no later check can swap the manifest in between. A refusal is state `failed` step `verify` with the detail; logged with the reason and the version.
5. State `downloading` with `version` and `percent` 0..100 (floored, clamped); log `update available` with version, key id and size. A throw: `failed` step `download`; a cancellation: `idle`.
6. The downloaded file is hashed (SHA-512, base64) and compared to the manifest's `sha512`; a mismatch is `failed` step `verify`, “the downloaded file does not match the signed release”; an unreadable file “the downloaded file could not be read”.
7. State `ready` with `version`; the schedule timer is cleared; no further check runs this session (`:102,194`). Log `update ready`.

**Install at quit** (`installAtQuit`, `src/main/updates/update-service.ts:143-172`, called from the app `quit` event after the database closed, `src/main/index.ts:767-770`): nothing without a verified download; without an explicit “Restart to update” and with automatic updates off the update is left for a restart; blocked after `session-end` (“Windows is shutting down or signing out”) or an uncaught exception (“an unexpected error happened in this run”); skipped on a non-zero exit code; the file is hashed again right before starting (“the downloaded file cannot be read” / “the downloaded file changed after it was verified” refuse at WARN); then `install(isSilent: true, relaunch)` where `relaunch` is true only for “Restart to update” (`requestInstall`, `:121-126`, IPC `updates:install` which is `BUSY` during a switch and `NOT_READY` without a verified download, `src/main/ipc/index.ts:541-545`, `src/main/updates/update-service.ts:19-25`). **[dev-only]** The test-feed updater's install is a no-op that logs `update install (test feed, installer not run)` (`src/main/updates/engine.ts:39-42`).

**Error text** (`describeUpdateError`, `src/main/updates/update-service.ts:44-58`): Chromium network errors (`net::ERR_INTERNET_DISCONNECTED`, `NAME_NOT_RESOLVED`, `CONNECTION_*`, `TIMED_OUT`, `PROXY_CONNECTION_FAILED`, …) and Node's `ECONNREFUSED`/`ECONNRESET`/`ENOTFOUND`/`ETIMEDOUT` read “the update server could not be reached”; `ERR_UPDATER_NO_PUBLISHED_VERSIONS` or `ERR_UPDATER_LATEST_VERSION_NOT_FOUND` “no release has been published yet”; `ERR_UPDATER_CHANNEL_FILE_NOT_FOUND` “the latest release has no update manifest”; `ERR_CHECKSUM_MISMATCH` or a “checksum mismatch” message “the downloaded file does not match the release”; an HTTP status “the update server answered HTTP <code>”; otherwise the message's first line cut to 160 characters, or “unknown error”.

**[dev-only] Test feed** (`src/main/updates/test-feed.ts`, `src/main/updates/engine.ts:53-59`): unpackaged only; `ONELAUNCH_UPDATE_FEED` must be an `http://127.0.0.1` URL (a trailing `/` is added), `ONELAUNCH_UPDATE_KEYS` a comma list of Ed25519 SPKI base64 keys (ids derived as above); anything else exits 2. The engine writes `<data folder>\dev-app-update.yml` (`provider: generic`, the URL, `updaterCacheDirName: update-cache`), points the updater at it, forces the dev config, keeps the cache under the data folder, and disables differential download.

### 7.6 Hand-offs to the system shell (`shell.openExternal`, `shell.openPath`)

| URI / target | Built from | When | Citation |
| --- | --- | --- | --- |
| `steam://rungameid/<appid>` | The game's `launcherGameId`, digits only (else “Not a Steam app id: <id>”) | Launching a Steam game | `src/main/launchers/steam/adapter.ts:465-471` |
| `com.epicgames.launcher://apps/<ns>%3A<item>%3A<AppName>?action=launch&silent=true` (`catalog` form, the default) or `com.epicgames.launcher://apps/<AppName>?action=launch&silent=true` (`appname` form, or when the catalog ids are unknown) | The app name (`^[A-Za-z0-9_.-]{1,128}$`, else “Not an Epic app name: <id>”) and the manifest's catalog namespace and item id; the form is the `epic.launchUri` setting | Launching an Epic game | `src/main/launchers/epic/adapter.ts:627-635`, `src/main/launchers/epic/manifests.ts:118-129` |
| `battlenet://game/<tact code>` | The product table's TACT code; only with the `battlenet.launch` setting `uri`. The default `exec` form starts `Battle.net.exe --exec=launch <program code>` as a process instead (no shell URI) | Launching a Battle.net game | `src/main/launchers/battlenet/adapter.ts:755-766`, `src/main/launchers/battlenet/products.ts:178-183` |
| `https://www.steamgriddb.com/profile/preferences/api` | Fixed table keyed by `steamgriddb-api-key`, the only `app:openUrl` target; the renderer names the target, never a URL | “Get a key” in Settings › Artwork | `src/main/ipc/index.ts:26-28,135-140`, `src/shared/models.ts:197-199`, `src/renderer/src/components/ArtworkPanel.tsx:74-81` |
| Folder open (`shell.openPath`) | `logs`, `data` or `backups` folder of the data directory, chosen by name | Settings “Open log folder” / “Open data folder”, Backups “Open folder”, the error boundary's log button | `src/main/ipc/index.ts:126-133`; callers `src/renderer/src/views/SettingsView.tsx:126-131,350-353`, `src/renderer/src/components/BackupsPanel.tsx:170`, `src/renderer/src/components/ErrorBoundary.tsx:44` |

A failed `openPath` is `OPEN_FAILED` with the shell's text (`src/main/ipc/index.ts:131`). The adapters receive `openExternal` from the platform; **[dev-only]** the fake platform records the URL instead (`src/main/index.ts:456`, `src/main/platform/wiring.ts:91`). Riot never uses a shell URI: a Riot game is started through the client's command line and the loopback request below (`src/main/launchers/riot/adapter.ts:13`).

### 7.7 The Riot Client loopback request

The one thing the app ever sends to anything of Riot's: the launch request the client's own Play button sends to its local server, so the user does not have to press Play (`src/main/launchers/riot/local-launch.ts:3-9`). It runs only when the `riot.pressPlay` setting is on (default on) and a requester exists (`src/main/launchers/riot/adapter.ts:985-986`, `src/main/index.ts:570`).

- **Lockfile**: `%LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile`, read at the moment of the call (`src/main/launchers/riot/adapter.ts:353-355,361-364`). Format: one line `name:pid:port:password:protocol`. Accepted only when there are exactly five parts, `name` is `Riot Client`, `protocol` is `https`, `pid` is digits, `password` non-empty, and `port` is 1..65535 (`src/main/launchers/riot/local-launch.ts:24,32-40`). A missing file: “the Riot Client has no lockfile to answer on”; a wrong shape: “the lockfile is not in the shape One Launch knows” (`:102-109`).
- **Request**: `POST https://127.0.0.1:<port>/product-launcher/v1/products/<product>/patchlines/<patchline>`, headers `authorization: Basic base64("riot:<password>")`, `accept: application/json`, `content-length: 0`, empty body (`:22,112-120,68`). Product and patchline must match `^[a-z0-9_]+$` (`:25,101`).
- **Transport**: `node:https` directly (honours no proxy, follows no redirect), a fresh agent with `rejectUnauthorized: false` (the client's certificate is self-signed) and `keepAlive: false`, socket timeout 10 s, the job's abort signal (`:23,56-83`). The module is loaded on the first launch, not at startup.
- **Answer**: only the status is kept; the body is drained and dropped. 2xx is success; anything else “the Riot Client answered the launch request with HTTP <status>”. A transport failure is “the launch request did not reach the Riot Client (<label>)” where the label is `timed out`, an error code, or an error name, never a message (`:73-77,86-95,121-124`).
- The password is read for the call and never kept, logged or put in an error (`:8`). A refused or ignored request never fails the launch; the hand-over stays with the reason (`src/main/launchers/riot/adapter.ts:988-1007`).

### 7.8 The `onelaunch-art:` protocol handler

An app-owned scheme serving the artwork cache to the renderer (section 4.9 has the grammar): privileged (standard, secure, stream) registration before `ready`; the handler maps `onelaunch-art://art/<uuid>/<kind>.<ext>` to a file strictly inside the artwork folder, reads it through `net.fetch` of a `file://` URL, and answers 404 with an empty body for any URL outside the grammar or any missing file, else 200 with the content type and `cache-control: max-age=31536000, immutable` (`src/main/index.ts:97,464-475`, `src/main/services/artwork-url.ts:36-52`). No other scheme, deep link or protocol client is registered (grep `setAsDefaultProtocolClient|registerFileProtocol|protocols:` over `src/`, `electron-builder.yml`, `build/` gives nothing).

## 8. OS integration

How the app sits in Windows: the window and its chrome, the tray icon, Start with Windows, the installer and uninstaller, how an update is installed, the clipboard, how processes and the registry are touched, what the system shell is handed, and what the app deliberately does not do.

### 8.1 The main window

**Creation** (`src/main/index.ts:295-322`):

| Property | Value |
| --- | --- |
| Size | The saved placement's size, else 1280 × 800; position from the saved placement when it is on screen, else Windows' default |
| Minimum size | 960 × 600 (`src/main/index.ts:301-302`, `src/main/window-guard.ts:20`) |
| Shown at creation | No (`show: false`); shown by code right after, see below |
| Menu bar | Auto-hidden, and no application menu exists at all (`src/main/index.ts:100,304`) |
| Background colour | `#0b0d12`, so the window is on screen in the app's colour before the page paints |
| Title | `One Launch`; **[dev-only]** `One Launch (fake launchers)` on the fake platform (`src/main/index.ts:480`) |
| Icon | From the exe when packaged; in development the `build/icon.png` resource so the taskbar shows it (`src/main/index.ts:307-308`) |
| Title bar | `titleBarStyle: 'hidden'` with `titleBarOverlay` colour `#12151c`, symbol colour `#8f97ab`, height 35 px: the renderer draws the title bar, Windows keeps drawing the caption buttons (snap layouts, hover) in those colours (`src/main/index.ts:309-311`) |
| Web preferences | preload `preload/index.cjs`; `contextIsolation` on; `nodeIntegration` off; `sandbox` on; `webSecurity` on; insecure content off; spellcheck off; DevTools only in development (`src/main/index.ts:312-321`) |

**Show-before-load** (`src/main/index.ts:325-333`): unless the start is hidden, the window is maximized first when the saved placement says so, then shown, before the page has loaded; `windowShownMs` is taken. The renderer's first frame is therefore at the final size. `rememberBounds` attaches right after (section 8.2).

**Hidden start** (`src/main/window-guard.ts:50-53`, `src/main/index.ts:326-327,481`): with `--start-hidden` on the command line **and** `window.closeToTray` on, the window is created at the saved bounds but not shown; a saved maximize waits for the first show (`maximize()` would show it) (`src/main/index.ts:129-130,156-167`). The deferred startup block still runs; if the tray icon could not be created there, the window is shown since nothing else could reach it (`src/main/index.ts:699-702`). Without close-to-tray the flag is ignored and the window shows normally.

**Showing the window** (`showMainWindow`, `src/main/index.ts:155-167`): restore if minimized, apply a pending maximize, show if hidden, record the first-shown time, focus. Used by the tray, a second instance, the quit prompt and the crash dialog.

**Page load** (`src/main/index.ts:409-413`): the packaged `renderer/index.html` over `file://`; **[dev-only]** the Vite dev server URL from `ELECTRON_RENDERER_URL`.

**Development-only keys** **[dev-only]** (`src/main/index.ts:373-379`): F12 toggles DevTools, Ctrl+R reloads. Not present in a packaged build.

**Renderer recovery** (`src/main/index.ts:363-371`): a crashed or killed renderer reloads once; a second loss within 60 s stays down; a clean exit never reloads.

### 8.2 Window placement memory

- Saved as the `window.bounds` setting (JSON `{x, y, width, height, maximized}`), 500 ms after the last resize, move, maximize or unmaximize, and at once on close; never while minimized; the normal (unmaximized) bounds are stored when maximized (`src/main/index.ts:273-293`).
- Read at creation through `placementFrom` (`src/main/window-guard.ts:55-71`): the four numbers must be finite; width and height are raised to the minimum size; the placement is used only when enough of the title bar is on a connected display's work area: the window's left edge is at least 64 px left of the area's right edge, its right edge at least 64 px right of the area's left edge, its top edge at least 8 px above the area's top is tolerated and it must be more than 64 px above the area's bottom. Otherwise position and size fall back to the defaults. `maximized` is honoured independently of the position check. Unparsable JSON reads as nothing saved.

### 8.3 Single instance, identity, hardening

- **Single instance**: a second process exits with code 0 before opening anything; the first receives `second-instance` and shows and focuses its window (`src/main/index.ts:90-94,728-730`). The lock is Electron's and is per `userData` folder, so a `--data-dir=` run coexists with the real one.
- **App user model id**: `dev.onelaunch.app`, set before `ready` (`src/main/index.ts:88`). Matches `appId` in `electron-builder.yml:1`.
- **No application menu**: set to null before `ready` so Electron never builds its default (`src/main/index.ts:99-100`).
- **Permissions**: every permission request and permission check is denied (clipboard reads included) (`src/main/index.ts:460-462`).
- **Navigation**: `will-navigate` to anything but the app's own page is blocked and logged `blocked navigation` at WARN; `will-attach-webview` is refused; `window.open` is denied and logged `blocked window.open` (`src/main/index.ts:247-259`). A trusted URL is exactly the packaged `index.html` file URL, or it followed by `#` or `?`; **[dev-only]** in development, any URL with the dev server's origin (`src/main/window-guard.ts:8-18`).
- **IPC sender check**: every handler first checks the sender frame's URL with the same rule and answers `UNTRUSTED_SENDER` “Request rejected” otherwise (`src/main/ipc/handle.ts:45-49`, `src/main/index.ts:239-245`).
- **CSP**, injected into `index.html` at build time (`electron.vite.config.ts:13-24,43-59`): `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: blob: onelaunch-art:; font-src 'self' data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`. **[dev-only]** The dev CSP allows inline scripts and styles and `ws://localhost:*`/`http://localhost:*` connections for HMR (`:30-41`).
- **Electron fuses** in the packaged exe (`electron-builder.yml:26-31`): `runAsNode` off, `NODE_OPTIONS` ignored, Node CLI inspect arguments off, only `app.asar` is loaded, asar integrity validation on.
- **Preload**: exposes `window.oneLaunch` with `invoke` (channels on the allowlist only; others answer `UNKNOWN_CHANNEL`) and `startup` (the prefetched answers) and nothing else (`src/preload/index.ts:9-40`).

### 8.4 Tray icon

- Exists while `window.closeToTray` is on and the icon could be created; created in the deferred startup block and created or destroyed at once when the setting changes (`src/main/index.ts:169-188,686,698`).
- Image: `build/icon.png` resized to 16 px × the primary display's scale factor (at least 16); an unreadable icon throws “tray icon not found at <path>”, which is logged `tray icon unavailable; closing the window quits` at WARN, and closing the window then quits as if the setting were off (`src/main/tray.ts:13-17`, `src/main/index.ts:177-187`).
- Tooltip: the window title (`One Launch`) (`src/main/index.ts:698`, `src/main/tray.ts:18`).
- Context menu: “Open One Launch”, a separator, “Quit One Launch” (`src/main/tray.ts:19-21`).
- Left click and double click both open (show and focus) the window (`src/main/tray.ts:22-23`). Quit calls `app.quit()`, which runs the normal quit sequence (section 6.2) (`src/main/index.ts:183`).
- Destroyed at `will-quit` (`src/main/index.ts:756-758`).

### 8.5 Close, hide and quit rules (`src/main/window-guard.ts`)

- **Close request** (`closeRequestAction`, `:26-30`): a quit under way closes; otherwise close-to-tray (setting on **and** tray present) hides; otherwise a running switch asks (the prompt in section 6.2); otherwise closes, and the app quits when the last window closes (`src/main/index.ts:381-395,732-734`).
- **Hide after a one-click Play** (`hideAfterInstantPlay`, `:38-48`, called from `src/main/index.ts:587-603`): the window hides to the tray when the job was started as an instant Play from a card, went direct (nothing switched), ended `done`, left no hand-over (Riot's “press Play”), the tray exists, `window.closeToTray` is on and `play.hideToTray` is on (default on). Any switch, failure or hand-over keeps the window. Logged `window hidden to the tray after an instant launch`.
- **Start hidden** (`startHidden`, `:50-53`): see 8.1.

### 8.6 Native dialogs owned by main

| Dialog | Text | Citation |
| --- | --- | --- |
| Startup failure (error box) | Title “One Launch could not start”; body “<reason>\n\nDetails are in the log:\n<log path>\n\nData folder:\n<data folder>” | `src/main/index.ts:718-726` |
| Quit during a switch (warning) | Section 6.2 | `src/main/index.ts:213-222` |
| Unexpected error (error) | Section 6.2 | `src/main/index.ts:785-795` |
| Choose an image (open file) | Title “Choose an image”, filter “Images” `png, jpg, jpeg, webp`, single file; owned by the window when it exists | `src/main/index.ts:642-650` |
| Locate a launcher (open file) | Title “Locate <exe name>”, filter “<launcher display name>” `exe`, single file | `src/main/index.ts:652-660` |

Main never accepts a file path from the renderer; both pickers run in main and the renderer only names the game or the launcher (`src/main/ipc/index.ts:349-357,370-389`).

### 8.7 Start with Windows

- **Registry value**: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, name `OneLaunch`, written through Electron's `setLoginItemSettings` with the exe path, `args: ['--start-hidden']` and `name: 'OneLaunch'`; `enabled: true` is passed when turning it on. Turning off passes `openAtLogin: false` with the same path, args and name (`src/main/platform/login-item.ts:6-7,37-42`). No elevation; it is the entry Settings › Apps › Startup and Task Manager list.
- **Status** (`:30-35`): `available` and `openAtLogin`. `openAtLogin` is `executableWillLaunchAtLogin` for the exe path passed **quoted** (Electron parses that option as a command line, and an unquoted path with a space would end at the space); so an entry the user disabled in Task Manager reads as off.
- **Availability** (`loginItemAvailable`, `:14-17`): the same rule as update availability (`perUserInstall`, 8.9): packaged, default data folder, uninstaller beside the exe, per-user install location equal to the exe's folder. Worked out once per run and cached (`:44-46`). Reasons: a dev run would register Electron's exe; a scratch `--data-dir=` run must never become the copy Windows starts; a never-installed copy has no uninstaller to remove its entry; an all-users install's uninstaller reaches only one account's HKCU.
- Unavailable: `status` is `{available: false, openAtLogin: false}`; `autostart:set` answers `UNAVAILABLE` “Start with Windows is not available for this copy of One Launch” (`src/main/ipc/index.ts:551-559`).
- **[dev-only]** The fake platform uses an in-memory login item that is always available (`src/main/platform/login-item.ts:49-60`).
- The installer's handling of the value is in 8.8; the crash dialog's relaunch drops `--start-hidden` (`src/main/index.ts:799`).

### 8.8 Installer and uninstaller (electron-builder NSIS)

From `electron-builder.yml` and `build/installer.nsh`:

| Aspect | Behaviour | Citation |
| --- | --- | --- |
| Product and executable | `productName: One Launch` (display name: shortcut, Apps & features, window title); `executableName: OneLaunch` (the exe `OneLaunch.exe`, the uninstaller `Uninstall OneLaunch.exe`, the default folder `Programs\OneLaunch`, no spaces) | `electron-builder.yml:2-4`, `src/main/updates/availability.ts:9` |
| App id | `dev.onelaunch.app`; electron-builder's registry key for the install is `HKCU\Software\37275e3e-77ba-54aa-bc32-31551d28dc0d` (UUIDv5 of the app id) | `electron-builder.yml:1`, `src/main/updates/availability.ts:6-8` |
| Installer type | NSIS, assisted (`oneClick: false`), the user may change the folder, per-user (`perMachine: false`; the wizard still offers all users, which the app then treats as unsupported for updates and autostart) | `electron-builder.yml:33-40` |
| Licence page | `build/license.txt` (written by `npm run notices`: the MIT licence plus where the third-party notices are installed) | `electron-builder.yml:41-42` |
| Artifact name | `OneLaunch-<version>-setup.exe` | `electron-builder.yml:35-36` |
| Files beside the exe | `THIRD-PARTY-NOTICES.txt`, plus electron-builder's `LICENSE.electron.txt` and `LICENSES.chromium.html`; only the win32-x64 better-sqlite3 prebuild, unpacked from the asar | `electron-builder.yml:9-22` |
| Uninstall keeps data | `deleteAppDataOnUninstall: false`: accounts, backups and the whole `%APPDATA%\OneLaunch` survive an uninstall | `electron-builder.yml:43-45` |
| Folder move | An existing per-user install in a folder ending `\One Launch` (the old product-name folder) is redirected to `\OneLaunch` beside it: in `.onInit` for a silent update (`customInit`), and on leaving the install-mode page for the wizard (`customInstallMode` leave function, which re-reads the folder from the registry) | `build/installer.nsh:1-33` |
| Run value at install | After installing, if `HKCU\...\Run` value `OneLaunch` exists and either the install is for all users or the value does not start with the quoted new exe path, both the `Run` value and `HKCU\...\Explorer\StartupApproved\Run` value `OneLaunch` are deleted. The installer never writes the value (antivirus blocks an installer that writes a Run key), so a reinstall into another folder turns Start with Windows off and the user turns it on again | `build/installer.nsh:35-50` |
| Run value at uninstall | Both values are deleted, except when the uninstaller runs as part of an update (`${isUpdated}`), since an update runs the old uninstaller first | `build/installer.nsh:52-57` |
| Publish | GitHub `rjf2016/one-launch`, draft releases; every build passes `--publish never`; the publish block makes electron-builder write `latest.yml` and `resources/app-update.yml` | `electron-builder.yml:48-53` |
| Signing | Builds are unsigned (no `publisherName`); the updater relies on the Ed25519 manifest signature instead (section 7.5) | `electron-builder.yml` (no `sign`/`publisherName`), `docs/architecture.md` › Updates |

### 8.9 Install detection and update installation

- **Per-user install test** (`perUserInstall`, `src/main/updates/availability.ts:31-39`): packaged exe (else `development`); data folder source `default` (else `custom-data-folder`); `Uninstall OneLaunch.exe` in the exe's folder (else `not-installed`); `HKCU\Software\37275e3e-77ba-54aa-bc32-31551d28dc0d` value `InstallLocation`, read with the registry reader, equal to the exe's folder case- and separator-insensitively (else `all-users-install`, also on a read failure). Shared by updates and Start with Windows.
- **Installing an update** (`src/main/updates/update-service.ts:143-172`, `src/main/updates/engine.ts:91-93`): from the app `quit` event, after the database has closed, main itself calls electron-updater's `install(isSilent = true, relaunch)`; the NSIS `--updated` run then kills a still-running app after about 2 s, which is why nothing of the app's own cleanup may still be running. Conditions: a verified download exists; either “Restart to update” was pressed (then `relaunch` is true) or `updates.auto` is on; not after a logoff/shutdown, not after an uncaught exception, not on a non-zero exit code; and the file's SHA-512 still matches. The installer then replaces the install in place and, with `relaunch`, starts the new exe.
- `updates:install` from the renderer refuses during a switch (`BUSY`) and without a verified download (`NOT_READY`) and otherwise quits the app (`src/main/ipc/index.ts:541-545`, `src/main/updates/update-service.ts:121-126`). The renderer disables “Restart to update” while a job runs and asks the current view's unsaved-edits guard first (`src/renderer/src/lib/updates.ts:38-44`, `src/renderer/src/components/Sidebar.tsx:99-135`, `src/renderer/src/components/UpdatesPanel.tsx:103-106`).

### 8.10 Protocol scheme, folders and URLs handed to the shell

- The only custom scheme is `onelaunch-art`, registered as privileged before `ready` and served by main for the artwork cache; it is an in-process scheme, not a system protocol handler (section 4.9 and 7.8) (`src/main/index.ts:97,464-475`).
- `app:openPath` opens one of the data folder's `logs`, `data` or `backups` folders in Explorer through `shell.openPath`; a failure is `OPEN_FAILED` with the shell's text (`src/main/ipc/index.ts:126-133`).
- `app:openUrl` opens one fixed URL, the SteamGridDB API key page, through `shell.openExternal` (`src/main/ipc/index.ts:26-28,135-140`).
- Game launches hand `steam://`, `com.epicgames.launcher://` and (with the `uri` setting) `battlenet://` URIs to the shell (section 7.6).

### 8.11 Clipboard

- `accounts:copy` copies a login or the decrypted password (`NO_PASSWORD` “No password is stored for this account” when none) (`src/main/ipc/index.ts:310-324`).
- **Private write** (`src/main/platform/clipboard-private.ts`): Windows only. Runs `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -NonInteractive -STA -ExecutionPolicy Bypass -Command <script>` with the text on **stdin** (never argv), hidden window, 8 s timeout. The script builds a `System.Windows.Forms.DataObject` with `UnicodeText` plus three formats each set to a 4-byte zero: `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`, which keep the value out of Win+V history and cloud clipboard sync, then `Clipboard.SetDataObject($d, $true)`. A non-zero exit fails with the first 200 characters of stderr; `node:child_process` is loaded on the first copy.
- **Service** (`src/main/services/clipboard.ts:37-54`): after the private write the clipboard is read back and must equal the text; otherwise (or on any error) the plain Electron clipboard write is used, `historyExcluded` is false, and main logs `clipboard history exclusion unavailable; copied with the plain clipboard` with the reason at WARN (`src/main/index.ts:494-499`). The result tells the renderer `clearAfterMs` (30000) and `historyExcluded`.
- **Clearing** (`:56-70`): 30 s later, or at quit, the clipboard is cleared only if its text still hashes (SHA-256) to what was copied; only the hash is kept in memory meanwhile. A newer copy cancels the earlier timer.
- **[dev-only]** The fake platform substitutes a recording clipboard and private writer (`src/main/platform/wiring.ts:64-65`).

### 8.12 Process listing and control (`src/main/platform/processes.ts`)

Everything goes through Windows PowerShell (`%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -NonInteractive -Command <script>`), hidden window, 15 s timeout, 8 MiB output cap, loaded on first use (`:5-9,469-472,497-504`). Nothing goes through WMI: `tasklist`, `taskkill`, `Get-Process -Path`/`Path`, and WMI `ExecutablePath` are deliberately never used because they make the WMI host (or PowerShell) ask every process for memory-read access, which anti-cheat drivers watch (`:59,96,185,486-490`). Grep `tasklist|taskkill|Get-WmiObject|Get-CimInstance` over `src/main` returns only the comments saying so.

| Operation | Method | Notes |
| --- | --- | --- |
| Running image names (`runningNames`, `isRunning`, `anyRunning`) | `[System.Diagnostics.Process]::GetProcesses()` names only, lower-cased, `.exe` dropped; one listing answers a set of names | An empty list is an error (“Could not list the running processes (PowerShell printed no names)”), never “nothing runs”, since a switch would otherwise write under a running launcher (`:60-77,515-532`) |
| Processes with paths (`listProcesses`, preflight only) | `NtQuerySystemInformation(SystemProcessIdInformation)` for each pid's native image path (opens no process) plus `QueryDosDevice` to map `\Device\...` to drive letters and `\Device\Mup\` to UNC; output as JSON with UTF-8 console encoding so non-ASCII paths survive; the script aborts if it cannot name its own process | `:95-182,534-537` |
| Close (`close`) | Snapshot the process table (`NtQuerySystemInformation` class 5), find processes by image name (case-insensitive, `.exe` optional), `EnumWindows`, post `WM_CLOSE` to the topmost **visible** unowned window of each (the one `taskkill /IM` would pick), or to every hidden unowned window when none is visible; does not wait | `denied > 0` (an `ERROR_ACCESS_DENIED` on the post) throws `PROCESS_ACCESS_DENIED`; a process with no window gets nothing and a kill follows (`:184-188,347-375,543-549`) |
| Kill (`kill`, tree by default) | Same snapshot; collect targets by image name, with children (parent id only counted when the parent is older than the child, since Windows reuses ids), never One Launch itself or any process it descends from; `OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE)` on each, re-list to confirm pid and creation time unchanged, `TerminateProcess(1)`; a process already gone is not a failure | `denied` or `failed` count only processes with the image name (a child a tree kill could not end is not an error); `PROCESS_ACCESS_DENIED` when denied, “Could not end <image> (Windows error <code>)” when failed (`:377-430,551-557`) |
| Start (`start`) | `spawn` detached, stdio ignored, visible window, `unref`; resolves when the process exists; rejects “Could not start <exe>: <reason>” | `:474-484,539-541` |

- **Image-name validation**: every name must match `^[A-Za-z0-9_.-]+(?: [A-Za-z0-9_.-]+)*$` (single interior spaces allowed, for `Riot Client.exe`; quotes and `$` refused because the name lands inside a single-quoted PowerShell string); otherwise “Refusing to query/close/kill an unexpected image name” (`:40-41,520,545,552`).
- **Result parsing**: the control scripts print one JSON line `{found, signalled, denied, failed, error}`; anything else throws “Could not <action> (PowerShell printed no result)” so a close or kill never passes for done on misunderstood output (`:435-454,506-513`).
- **Error text**: never Node's `err.message` (it holds the whole command line); a timeout reads “PowerShell timed out”, otherwise the exit code and up to 200 characters of stderr (`:456-463`).
- **Access denied message**, verbatim: “Windows would not let One Launch close <image>. It is most likely running as administrator: close it yourself, or start One Launch as administrator too, then try again.” (`:43-52`).
- Off Windows every lister returns empty/false and every control is a no-op (`:521,527,535,546,553`).
- Riot: the adapters only ever pass image names; the Riot adapter never lists or touches a Riot game process or Vanguard by any other means (`docs/adapters/riot.md`; the Riot adapter uses only this module's `anyRunning`/`runningNames` for its process checks, see the Riot section).

### 8.13 Registry access (`src/main/platform/registry.ts`)

- Hives: `HKCU` and `HKLM` only (`:11-16`).
- **Reads** go through `cmd.exe /d /s /c "chcp 65001>nul & "<System32>\reg.exe" "query" "<key>" ..."` with verbatim arguments, because `reg.exe` prints in the console's OEM code page and would turn a non-ASCII path into look-alikes; under code page 65001 it prints UTF-8, and `chcp` in the same `cmd.exe` is the only way to give the child console a code page (`:87-96,200-203`). Every argument is refused if it contains a double quote, `%`, `^`, `&`, `|`, `<`, `>`, `!` or a control character: “Refusing to pass unexpected characters to cmd.exe” (`:83-85,94`).
- **Writes** (`reg add ... /v <name> /t REG_SZ|REG_DWORD|REG_BINARY /d <data> /f`, `reg delete ... /v <name> /f`) run `reg.exe` directly, so the data never meets `cmd.exe` (`:268-304`). Writes happen only inside a switch's `runSwitch`/`applyRestore` and backup restore (`:35`).
- Both use the absolute `%SystemRoot%\System32\reg.exe` / `cmd.exe` so a stray `reg.exe` on PATH is never picked up (`:69-81`); 10 s timeout, 8 MiB output cap; `node:child_process` is loaded on the first call (`:5-9,175-184`).
- **Refused characters and shapes** (`:159-170`): every key segment and value name must match `^[\w .(){}-]*\w[\w .(){}-]*$` (at least one word character, so `.` and `..` never pass) else “Refusing to touch a registry path with unexpected characters”; string data must have no control character or double quote and be at most 1024 characters, binary data must be 1..16384 bytes as hex pairs, else “Refusing to write unexpected characters to <name>”; a search text must match `^[\w .-]{3,64}$`; a subkey name that `reg.exe` itself reported may hold anything but `\`, `"` or a control character (1..255 characters, not `.`/`..`), else “Refusing to read a registry subkey with unexpected characters”. A DWORD must be an integer 0..0xFFFFFFFF (`:286`).
- **Read operations**: one value (`query`, `/v <name>`, or `/ve` for the default value); string (`REG_SZ`/`REG_EXPAND_SZ` only) and DWORD (`REG_DWORD`/`REG_QWORD`, hex `0x…` or decimal); all values under a key; the whole subtree (`/s`); the subtree filtered by data text (`/s /f <text> /d`, fast because `reg.exe` filters); the values of one reported subkey (`:205-266`). `reg.exe` exit code 1 means “not there” (or no match) and reads as null/empty; any other failure is `REGISTRY` “reg query failed for <hive>\<key>\<name>: <error>” (`:212-216,237-240`).
- **Write failures** report the exit code and up to 200 characters of stderr, never Node's message (which holds the command line and so the data), and drop stderr when it echoes data of 8 or more characters; a timeout reads “reg.exe timed out”; deleting an absent value is success (`:149-157,274-278,297-303`).
- Off Windows reads return null/empty and writes throw “Registry writes are Windows-only” (`:207,233,269,295`).
- **[dev-only]** `StaticRegistry` is the in-memory registry used by the fake platform and tests, recording every write (`:307-443`).

### 8.14 Deliberately absent

Each confirmed by grep over `src/`, `electron-builder.yml` and `build/`:

| Capability | Status | Evidence |
| --- | --- | --- |
| Global keyboard shortcuts | None. Shortcuts exist only inside the focused window (Ctrl+1/2/3 views, Ctrl+B sidebar, Ctrl+F search, Back; `src/renderer/src/App.tsx:37-69`, `src/renderer/src/views/LibraryView.tsx:178-188`) | `grep -rn "globalShortcut" src electron-builder.yml build` returns nothing |
| System notifications or toasts | None | `grep -rn "new Notification\|Notification(" src` returns nothing |
| Deep links / custom protocol client | None. `onelaunch-art` is an in-process scheme for the renderer only | `grep -rn "setAsDefaultProtocolClient\|protocols:" src electron-builder.yml build` returns nothing; `registerSchemesAsPrivileged`/`protocol.handle` only at `src/main/index.ts:97,465` |
| File associations | None | `grep -rn "fileAssociations" electron-builder.yml build src` returns nothing |
| Jump list, taskbar progress, badges | None | `grep -rn "setJumpList\|setProgressBar\|setBadge\|setUserTasks" src` returns nothing |
| Code signing of the exe | None yet | `electron-builder.yml` has no `sign`/`certificate`/`publisherName` |

## 9. Settings

Settings are plain strings in the `settings` table (section 3.3), read by main at the moment they matter and by the renderer through `settings:getAll`. There are 19 keys. Nothing here is a secret; secrets live in the vault (section 3.2).

### 9.1 Storage semantics (`SettingsService`)

| Operation | Behaviour | Source |
| --- | --- | --- |
| `get(key)` | the stored string, or `null` when the row is absent | `src/main/services/settings.ts:18-20` |
| `set(key, value)` | insert or overwrite (upsert) | `src/main/services/settings.ts:13,22-24` |
| `delete(key)` | removes the row; the key reads as unset again | `src/main/services/settings.ts:26-28` |
| `getAll()` | every row whose key is one of the 19 known keys, as a partial record; unknown keys in the table are skipped | `src/main/services/settings.ts:30-36` |
| `getBool(key, fallback)` | `fallback` when unset; otherwise true only for the strings `true` or `1`, false for anything else | `src/main/services/settings.ts:38-42` |
| `getInt(key, fallback)` | `fallback` when unset; otherwise `parseInt` base 10, `fallback` again when that is not a finite number | `src/main/services/settings.ts:44-49` |

- There are no defaults in the table: a fresh database has no rows except the one seeded for What's New (9.4). Every default below lives in the call that reads the key.
- Main reads settings as thunks at use time (`settings.getBool(...)` inside a closure), so a change applies to the next operation that reads it unless stated otherwise (`src/main/index.ts:505-620`).
- The renderer never caches settings beyond a query result; `settingsChanged()` bumps a store revision that re-runs every `settings:getAll` query bound to it (`src/renderer/src/store/ui.ts:83-84,203`, `src/renderer/src/views/SettingsView.tsx:62,107`).

### 9.2 Validation in `settings:set`

The channel accepts only the 19 keys and a string value of at most 4096 chars, refuses the five main-only keys (“<key> is not user-settable”), and applies per-key rules (every failure `BAD_REQUEST`; messages in section 5.3). After storing it logs `setting updated` with key and value, calls main's `onSettingChanged` hook, and queues artwork when `artwork.autoFetch` flips from a stored `false` to `true` (`src/main/ipc/index.ts:192-231`).

| Rule | Keys |
| --- | --- |
| exactly `true` or `false` | `dryRun`, `artwork.autoFetch`, `steam.autoLogin`, `riot.pressPlay`, `play.hideToTray`, `firstRun.dismissed`, `window.closeToTray`, `updates.auto`, `whatsNew.enabled` |
| `debug` / `info` / `warn` / `error` | `logLevel` |
| `catalog` / `appname` | `epic.launchUri` |
| `uri` / `exec` | `battlenet.launch` |
| version string (`^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$`, ≤64 chars) | `whatsNew.lastSeen` |
| 1 to 3 digits, 1..200 | `backups.keep` |
| refused | `window.bounds`, `steam.path`, `epic.path`, `battlenet.path`, `riot.path` |

Note that `getBool` also accepts a stored `1`, which `settings:set` never writes; only a hand-edited database or **[dev-only]** a fake seed could store it (`src/main/services/settings.ts:41`, `src/main/launchers/sim/seed.ts:40-42,95-98`).

### 9.3 The keys

All 19 are returned by `settings:getAll` when a row exists (`src/main/services/settings.ts:30-36`). "Who writes" names the process that calls `set`; "Applies" says when a change takes effect.

| Key | Settings UI | Default when unset (and where applied) | Accepted values | Who writes | What it affects | Applies | Source |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `dryRun` | Advanced › Launching › “Preview mode”, hint “Play shows what a switch would do instead of doing it.” | off: `getBool('dryRun', false)` in the launch service; renderer reads `=== 'true'` | `true` / `false` | renderer (Settings toggle; the launch dialog's “switch now” path writes `false`) | `launch:start` refuses with `DRY_RUN` while on; `launch:preflight` reports it so the dialog shows the plan instead of switching; the sidebar shows a “Preview mode is on” card | at once (read per start) | `src/main/services/launch.ts:141-143,172`, `src/renderer/src/views/SettingsView.tsx:93,287-288`, `src/renderer/src/components/LaunchDialog.tsx:457`, `src/renderer/src/components/Sidebar.tsx:44` |
| `logLevel` | Advanced › Diagnostics › “Log level”, hint “Logs stay on this PC, with passwords, tokens, emails and account ids removed.”; options Debug/Info/Warn/Error | `info`: the logger starts at `info` (`debug` in development) and at startup a stored valid level is applied only in a packaged or non-dev run; renderer shows `info` when unset | `debug`, `info`, `warn`, `error` | renderer | the threshold of the main log | at once (`settings:set` calls `setLogLevel` before storing) and at every later start outside development | `src/main/index.ts:74,104,441-442`, `src/main/ipc/index.ts:197-200`, `src/main/services/logger.ts:228,243-245`, `src/renderer/src/views/SettingsView.tsx:94,360-367` |
| `artwork.autoFetch` | Artwork › “Fetch artwork automatically”, hint “Looks up missing covers after each scan; only game names and ids are sent.” | on: `getBool('artwork.autoFetch', true)`; renderer reads `!== 'false'` | `true` / `false` | renderer | whether a library scan (startup and `library:rescan`, and the two Locate/Reset rescans) queues missing covers; manual refreshes run regardless | next scan; turning it on from a stored `false` queues at once | `src/main/index.ts:610,634-640`, `src/main/services/artwork.ts:262-265`, `src/main/ipc/index.ts:222,226-229`, `src/renderer/src/components/ArtworkPanel.tsx:190-191`, `src/renderer/src/views/SettingsView.tsx:274` |
| `backups.keep` | Backups › “Snapshots to keep”, hint “Per launcher. Older ones go as new ones are taken; Kept snapshots never count.”; dropdown 5, 10, 20 (default), 50, 100 plus the stored value if it is another number | 20: `getInt('backups.keep', 20)`; renderer shows 20 when unset or below 1 | whole number 1..200 as 1 to 3 digits | renderer | how many ordinary snapshots per launcher survive pruning (pinned ones have their own fixed quota of 10); also quoted in dry-run plan text (“The newest <N> snapshots are kept.”) | next snapshot (pruning runs after each snapshot is written) | `src/main/index.ts:505-506`, `src/main/services/backup.ts:9-11,203,238-251`, `src/renderer/src/components/BackupsPanel.tsx:12-13,151-162,219-227`, `src/main/launchers/steam/adapter.ts:572` |
| `epic.launchUri` | Advanced › Launching › “Epic launch link”, hint “Change only if Epic games stop launching after an Epic update.”; options “Catalog link (default)” / “App name link” | `catalog`: the adapter thunk maps unset or invalid to `catalog`; renderer shows `catalog` | `catalog` / `appname` | renderer | the form of the `com.epicgames.launcher://` URL `launchGame` opens | next Epic launch | `src/main/index.ts:533-536`, `src/main/launchers/epic/adapter.ts:631-634`, `src/renderer/src/views/SettingsView.tsx:95,290-298` |
| `battlenet.launch` | Advanced › Launching › “Battle.net launch”, hint “Change only if games stop launching after a Battle.net update.”; options “Launch command (default)” / “battlenet:// link” | `exec`: thunk maps unset or invalid to `exec`; renderer shows `exec` | `exec` / `uri` | renderer | whether a Battle.net game is started with `Battle.net.exe --exec="launch <code>"` or a `battlenet://game/<code>` link | next Battle.net launch | `src/main/index.ts:551-554`, `src/main/launchers/battlenet/adapter.ts:755-761`, `src/shared/models.ts:264-270`, `src/renderer/src/views/SettingsView.tsx:96,299-307` |
| `steam.autoLogin` | Launchers › Play › “Steam automatic sign-in”, hint “Passes a stored Steam password to Steam for you; other programs can see it while Steam runs.” | off: `getBool('steam.autoLogin', false)`; renderer reads `=== 'true'` | `true` / `false` | renderer | when on and the account has a stored password, a Steam switch puts the password on Steam's `-login` command line instead of handing the sign-in to the user; affects the plan too | next Steam plan or switch | `src/main/index.ts:514`, `src/main/launchers/steam/adapter.ts:126-127,531,787`, `src/renderer/src/views/SettingsView.tsx:97,228-239` |
| `riot.pressPlay` | Launchers › Play › “Start Riot games for you”, hint “Presses Play in the Riot Client for you. Off, it opens the game's page and you press Play.” | on: `getBool('riot.pressPlay', true)`; renderer reads `!== 'false'` | `true` / `false` | renderer | after the client shows the game's page, whether One Launch sends the client's own local launch request; off (or when no local launch is wired) the job ends with a `handOff` telling the user to press Play | next Riot launch | `src/main/index.ts:570`, `src/main/launchers/riot/adapter.ts:193-194,906-947,984-986`, `src/renderer/src/views/SettingsView.tsx:98,240-251` |
| `play.hideToTray` | Launchers › Play › “Hide One Launch after Play”, hint “Sends the window to the tray as the game starts, when no account switch is needed.” or, when close-to-tray is off, “Needs Minimize One Launch to System Tray, in General.”; the toggle shows off and is disabled while `window.closeToTray` is off | on: `getBool('play.hideToTray', true)` in the job-finished hook; renderer reads `!== 'false'` | `true` / `false` | renderer | after a one-click (instant) Play whose job was direct, ended `done` and left no hand-off, the window hides to the tray, provided a tray icon exists and `window.closeToTray` is on | next finished job | `src/main/index.ts:588-603`, `src/main/window-guard.ts:32-48`, `src/renderer/src/views/SettingsView.tsx:99,252-267` |
| `steam.path`, `epic.path`, `battlenet.path`, `riot.path` | Launchers › launcher status panel: Locate and reset actions (no free-text field) | unset: adapters fall back to the registry and default locations | an install folder; written only from a file the user picked in main's own dialog | main only (`launchers:locate` sets, `launchers:resetPath` deletes); `settings:set` refuses | the folder tried first when detecting the launcher, and the executable One Launch starts; reported as `chosenPath` in `launchers:status` | next detection (a background rescan starts at once) | `src/shared/models.ts:249-262`, `src/main/ipc/index.ts:370-399`, `src/main/index.ts:515,537,555,569`, `src/main/launchers/types.ts:131-135` |
| `window.bounds` | none | unset: 1280×800, OS-placed, not maximized | JSON `{ x, y, width, height, maximized }`; main-only | main only, 500 ms after a resize, move, maximize or unmaximize, and on close; never while minimized | the window's position and size at the next start, only if enough of the title bar lands on a connected display's work area (64 px sideways, top edge within 8 px above the work area); width and height floored at 960×600; a saved maximize is applied at the first show | next start | `src/main/index.ts:266-292`, `src/main/window-guard.ts:20,55-71` |
| `window.closeToTray` | General › “Minimize One Launch to System Tray”, hint “Closing One Launch minimizes the app.” | on: `getBool('window.closeToTray', true)`; renderer reads `!== 'false'` | `true` / `false` | renderer | with the setting on and a tray icon present, closing the window hides it; off, closing quits (asking first while a switch runs). The tray icon exists while the setting is on; with the `--start-hidden` flag and the setting on the app starts in the tray. Gates `play.hideToTray` | at once: `onSettingChanged` creates or destroys the tray icon; the close rule reads the setting at each close | `src/main/index.ts:443,481,685-686,381-394`, `src/main/window-guard.ts:26-30,51-53`, `src/renderer/src/views/SettingsView.tsx:100,198-205` |
| `firstRun.dismissed` | General › “Setup guide” › “Show again” reopens the guide but does not change the key | unset: the guide opens at startup when the key is not `true` and there are no accounts | `true` / `false` (only `true` is ever written) | renderer (leaving the setup guide by any route) | whether the setup guide opens by itself at startup | next start | `src/renderer/src/components/SetupGuide.tsx:96,197-206,360-375`, `src/renderer/src/views/SettingsView.tsx:206-208` |
| `updates.auto` | General › Updates › “Keep One Launch updated automatically”, hint “Downloads new versions in the background and installs them the next time One Launch restarts.” | on: `getBool('updates.auto', true)` when the update service is built; renderer reads `!== 'false'` | `true` / `false` | renderer | on: a check 15 s after start, then every 6 h; a found and signature-verified release is downloaded and installed at the next normal quit. Off: no background checks; a background check already under way stops at `idle` when it finds something; a background download is cancelled; a downloaded update is installed only after “Restart to update” | at once through `onSettingChanged` | `src/main/index.ts:619,687`, `src/main/updates/update-service.ts:27-28,107-119,143-158,192-199,232-235`, `src/renderer/src/components/UpdatesPanel.tsx:118-124`, `src/renderer/src/views/SettingsView.tsx:213,217` |
| `whatsNew.enabled` | General › Updates › “Show what's new after an update”, hint “The first time you open One Launch after an update, a short list of what changed.” | on: renderer reads `=== 'false'` as off | `true` / `false` | renderer | on: at startup, when no recovery prompt is pending and the setup guide is not open, the newest release note at or below the running version that is newer than `whatsNew.lastSeen` opens after a short delay. Off: nothing opens, and the running version is recorded as seen so old notes never resurface when turned back on. The Settings “What's new” button shows the latest notes regardless | next start | `src/renderer/src/components/WhatsNewDialog.tsx:165-205`, `src/shared/whats-new.ts:80-99`, `src/renderer/src/components/UpdatesPanel.tsx:125-131`, `src/renderer/src/views/SettingsView.tsx:101,218` |
| `whatsNew.lastSeen` | none | unset on a data folder from before the feature (reads as older than every release); the running version on a brand-new database | a version string | both: main seeds it once on a fresh database; the renderer writes the running version when the card closes, and at startup when the feature is off and the value differs | which release the card shows next; a save that fails still closes the card, so it returns next start | next start | `src/main/services/whats-new.ts:8-12`, `src/main/index.ts:440`, `src/renderer/src/components/WhatsNewDialog.tsx:57-67,181-190`, `src/shared/whats-new.ts:36-38,84-94` |

Notes on the table:

- The renderer's visible defaults and main's defaults agree for every key (`!== 'false'` for the on-by-default booleans, `=== 'true'` for `dryRun` and `steam.autoLogin`).
- `logLevel` in development: the stored value is ignored at startup (the log stays at `debug`) but a change made in Settings during the run still applies at once (`src/main/index.ts:442`, `src/main/ipc/index.ts:199`).
- `play.hideToTray` is stored independently of `window.closeToTray`; the toggle shows `hideToTray && closeToTray` and is disabled while close-to-tray is off, so the stored value survives toggling close-to-tray off and on (`src/renderer/src/views/SettingsView.tsx:263-264`).
- Error display: a refused `settings:set` shows the error under the row that changed and reloads the settings so the control springs back (`src/renderer/src/views/SettingsView.tsx:103-111,124`). For `backups.keep` the panel shows “Could not save the setting: <message>” (`src/renderer/src/components/BackupsPanel.tsx:156-162,218`).

### 9.4 What's New seeding on a brand-new database

- At startup, right after migrations, if the database was just created (its schema version was 0 before migrating) and `whatsNew.lastSeen` is unset, main stores the running version under `whatsNew.lastSeen`. An existing database is never touched, and a brand-new one that somehow already has the key keeps it (`src/main/services/whats-new.ts:1-12`, `src/main/index.ts:436-440`).
- Effect: a fresh install never shows the What's New card; an upgraded data folder from before the feature (key unset) is treated as having seen nothing, so the newest applicable release shows once (`src/shared/whats-new.ts:80-94`).

### 9.5 Main-only keys

`window.bounds`, `steam.path`, `epic.path`, `battlenet.path`, `riot.path`. The renderer may read them (`settings:get`, `settings:getAll`) but `settings:set` refuses them with `BAD_REQUEST` “<key> is not user-settable”. The launcher folders are written only by `launchers:locate` from a file the user picked in main's dialog and checked by the adapter, because One Launch starts the executable found there; `launchers:resetPath` deletes them (`src/shared/models.ts:249-262`, `src/main/ipc/index.ts:196,385,395`). **[dev-only]** The fake seed may set any non-main-only key (`src/main/launchers/sim/seed.ts:40-42,95-98`).

### 9.6 Renderer-side preferences (not settings)

Kept in the renderer's `localStorage`, inside try/catch, never sent to main and never part of `settings:getAll`. They are per Windows user and per data folder's Chromium profile (`src/main/platform/paths.ts:52`).

| Key | Values | Default | Written when | Effect | Source |
| --- | --- | --- | --- | --- | --- |
| `sidebar` | `expanded` / `collapsed` | absent: follows the window width (collapsed below 1100 px) | the user toggles the sidebar (Ctrl+B or the button); the key is removed when the choice matches what the width would pick anyway, and when the window crosses the breakpoint | icons-only sidebar or full sidebar at the next start | `src/renderer/src/store/ui.ts:7-29,159-160,178-189` |
| `library.sort` | `name` / anything else | `lastPlayed` (any value other than `name`) | the user changes the library sort | the library grid's sort order | `src/renderer/src/views/LibraryView.tsx:21-23,39-45,220-227` |

## 10. Dependencies

Every package in `package.json` (`package.json:36-67`), what it is used for, and a Rust crate that covers the same need where I am confident of the match. "unknown" means no equivalent I would name without research; the rewrite should decide. Versions are those pinned at the time of writing.

### Runtime dependencies (shipped in the installer)

| Package | Version | Used for | Rust equivalent |
| --- | --- | --- | --- |
| `better-sqlite3` | 13.0.3 | The one synchronous SQLite connection in main: schema migrations, every table, the online-backup copy (`src/main/db/index.ts`). Only the win32-x64 Node-API prebuild ships (`electron-builder.yml:12-15`). | `rusqlite` (bundled SQLite, `backup` feature for the online backup API) |
| `electron-updater` | 6.8.10 | The update engine: reads the GitHub releases feed, downloads the installer (differential where possible), runs the NSIS installer with `--updated` (`src/main/updates/engine.ts`). The app only uses it after its own Ed25519 manifest check and starts the installer itself. | unknown. The pieces are ordinary (HTTPS GET of `releases/latest` and `latest.yml`, SHA-512, run an installer at quit) and could be built from `reqwest`/`ureq` + `sha2` + `std::process`; no single crate matches electron-updater's role. |

### Dev dependencies bundled into the renderer at build time (ship as part of the page bundle)

| Package | Version | Used for | Rust equivalent |
| --- | --- | --- | --- |
| `react`, `react-dom` | 19.3.0 | The renderer UI (`src/renderer/src/**`). | GPUI itself (the rewrite's UI framework) |
| `zustand` | 5.0.15 | The renderer's UI-state store: view, account route, settings tab, dialogs, launch request, sidebar state (`src/renderer/src/store/ui.ts`). | GPUI `Model`/`Entity` state; no crate needed |
| `lucide-react` | 1.45.0 | Every icon in the interface (imported in 25 renderer files; `grep -rl "from 'lucide-react'" src/renderer/src`). | unknown as a crate; the Lucide SVG set itself is MIT and can be embedded as SVG assets |
| `tailwindcss`, `@tailwindcss/vite` | 4.3.3 | Styling, CSS-first design tokens in `src/renderer/src/index.css`. | none needed (GPUI styles in code); the token values in `index.css:3-56` are what to carry over |

### Build, packaging and tooling (never shipped)

| Package | Version | Used for | Rust equivalent |
| --- | --- | --- | --- |
| `electron` | 44.7.0 | The runtime: Chromium + Node, `safeStorage` (DPAPI), `BrowserWindow`, `Tray`, `dialog`, `shell.openExternal`, `protocol`, `app.setLoginItemSettings`, single-instance lock. | `windows` / `windows-sys` for DPAPI (`CryptProtectData`), registry, shell (`ShellExecuteW`), tray (`Shell_NotifyIconW`), Run key; GPUI for the window; `single-instance` or a named mutex for the lock |
| `electron-vite`, `vite`, `@vitejs/plugin-react` | 5.0.0 / 7.3.6 / 5.2.0 | Bundling main, preload and renderer; dev server with HMR; the CSP meta tag injection (`electron.vite.config.ts`). | cargo |
| `electron-builder` | 26.15.3 | NSIS per-user installer, Electron fuses, `latest.yml` and `app-update.yml`, asar packing (`electron-builder.yml`). | unknown; candidates are `cargo-wix` (MSI) or NSIS driven by a build script. Section 8 lists what the installer must do. |
| `typescript`, `typescript-eslint`, `eslint`, `@eslint/js`, `eslint-config-prettier`, `eslint-plugin-react-hooks`, `globals`, `prettier` | various | Type checking (two tsconfig projects), lint, formatting (`docs/development.md:157-160`). | rustc / clippy / rustfmt |
| `vitest`, `jsdom`, `@testing-library/react`, `@testing-library/dom`, `@testing-library/user-event` | 5.0.0 / 29.1.1 / 16.3.3 / 10.4.2 / 14.6.7 | Unit tests (node), renderer tests (jsdom) (`vitest.config.ts`, `docs/development.md:58-76`). | `cargo test`; GPUI's own test harness for views |
| `@types/better-sqlite3`, `@types/node`, `@types/react`, `@types/react-dom` | various | Type declarations. | none |

### Windows programs the app shells out to (not packages, but dependencies all the same)

| Program | Used for | Rust equivalent |
| --- | --- | --- |
| `cmd.exe /d /s /c "chcp 65001>nul & reg.exe query …"` | Every registry read, so non-ASCII values come back as UTF-8 (`src/main/platform/registry.ts`). | `windows` crate registry APIs (`RegOpenKeyExW`, `RegQueryValueExW`, `RegEnumValueW`) read UTF-16 natively; no child process needed |
| `reg.exe add` / `reg.exe delete` | Registry writes (`src/main/platform/registry.ts`). | `RegSetValueExW`, `RegDeleteValueW` |
| Windows PowerShell 5.1 (`powershell.exe -NoProfile -NonInteractive`) with `Add-Type` C# | Process listing without opening game processes (`NtQuerySystemInformation`), posting `WM_CLOSE`, terminating process trees, and the history-excluded clipboard write (`src/main/platform/processes.ts`, `src/main/platform/clipboard-private.ts`). | `windows` crate: `NtQuerySystemInformation` via `windows-sys`/`ntapi`, `EnumWindows` + `PostMessageW(WM_CLOSE)`, `OpenProcess(PROCESS_TERMINATE)`; clipboard formats `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard` via `SetClipboardData`. Section 8 lists the constraints (what must never be called). |
| The launchers' own executables and URI schemes | Starting launchers and games (`steam.exe -shutdown`, `steam://rungameid/`, `EpicGamesLauncher.exe`, `com.epicgames.launcher://`, `Battle.net.exe --exec=`, `battlenet://`, `RiotClientServices.exe --launch-product=`). | `std::process::Command` (detached), `ShellExecuteW` for URIs |

### Other Rust crates the spec implies

Named here only because the spec's behaviour needs them; each is a well-known crate for that purpose: `serde`/`serde_json` (JSON files: journal, manifests, `.item` manifests, `RiotClientInstalls.json`), `serde_yaml` or a hand-written reader (Riot's YAML; the app deliberately uses a tiny purpose-built reader, `src/main/launchers/riot/session.ts:216`), `sha2` (SHA-256 fingerprints and SHA-512 installer hashes), `ed25519-dalek` (update manifest signatures), `base64`, `uuid` (v4 ids), `time` or `chrono` (ISO-8601 timestamps), `regex` (log parsing; keep every pattern linear, see section 4), `tracing` + a rotating file appender (the 1 MiB × 5 log, with the redaction layer written by hand), `reqwest` or `ureq` (artwork and update HTTP, with manual redirect handling), `image` only if the rewrite wants to decode covers itself (the app only sniffs magic bytes and hands files to Chromium).

## 11. Known issues & bloat

What the rewrite should **not** carry over. Every item is cited; "grep-verified" means the identifier was searched for across `src/`, `test/` and `scripts/` and no caller was found. Items are grouped by what they cost. The project's own roadmap already lists some of these (`docs/roadmap.md:54-72`).

### 11.1 Memory and CPU

- **Every registry read and every process check is a child process.** A registry read is `cmd.exe` + `chcp` + `reg.exe`; a process check is a `powershell.exe` that compiles C# with `Add-Type` on each run (about 0.2 s each, `docs/architecture.md:39,163`; `src/main/platform/registry.ts:80-100`, `src/main/platform/processes.ts:460-470`). The adapters call them freely: `launchers:status` runs a registry read and a process listing per launcher in parallel on every call (`src/main/services/launchers.ts:181-213`); Steam's `runningGame` lists every process and resolves every game folder twice when `RunningAppID` is absent (`src/main/launchers/steam/adapter.ts:434-443`, `src/main/launchers/running-game.ts:25-31`); Battle.net's `runningGame` re-runs the `reg query /s /f` product search plus one query per product on every preflight while the client runs (`src/main/launchers/battlenet/adapter.ts:667-680,484-487`); Riot's `findRunningGame` re-runs detection, lists the metadata folder and reads one registry value per product in every preflight, switch, launch, restore and status pass (`src/main/launchers/riot/adapter.ts:745-765,504-513`), and `gameIcon` reads a registry value per game per call (`:534-547`). A native Rust implementation (registry API, `NtQuerySystemInformation`) removes all of this; the constraints that must survive are in section 8.
- **Polling never backs off.** Main never pushes, so the renderer polls: `launch:status` every 150 ms while a job runs (a job can last the 10-minute manual sign-in budget, so thousands of round trips per switch, each a deep copy of the job) (`src/renderer/src/components/LaunchHost.tsx:107`, `src/main/services/launch.ts:441-449`); `artwork:status` every 0.5 to 4 s from the Library plus every 3 s from Settings › Artwork and every 1 s from the setup guide, which also re-fetches the whole `library:list` on each revision change (`src/renderer/src/views/LibraryView.tsx:190-216`, `components/ArtworkPanel.tsx:43-47`, `components/SetupGuide.tsx:40-69`); `updates:status` from two independent pollers at once on Settings › General (60 s sidebar, 5 s panel, 1 s while busy) (`lib/updates.ts:6`, `components/Sidebar.tsx:21`, `components/UpdatesPanel.tsx:13`). None of it pauses while the window is hidden or in the tray (`docs/renderer.md:42`). A push channel (or GPUI's own state propagation) replaces every poll.
- **Repeated reads that should be cached.** `app:info` is requested by five mounts (`TitleBar`, `Sidebar`, `SettingsView`, `WhatsNewHost`, `WhatsNewDevPanel`) and `settings:getAll` by the sidebar, Settings, the setup guide and both startup hosts (`src/renderer/src/components/TitleBar.tsx:10`, `Sidebar.tsx:39`, `views/SettingsView.tsx:59`, `components/WhatsNewDialog.tsx:178`). Epic's `currentSession` (an 8 MiB log tail parse plus a `stat`) is recomputed by `sessionState`, `listRememberedAccounts`, `buildSwitchPlan`, `saveOutgoing`, `holdsUnownedSession` (twice per pinned switch) and the health check (`src/main/launchers/epic/adapter.ts:436,1002,1336`); Epic's `launchGame` re-parses every `.item` manifest to find two catalog ids (`:629`). Battle.net's `storedValueName` decrypts the whole session blob just to show a value name in a plan that is otherwise documented as decrypting nothing (`src/main/launchers/battlenet/adapter.ts:1128-1136,818`).
- **Unbounded or unpruned data.** `launch_history` grows one row per launch attempt with no index on `(game_id, result, launched_at)`, no cap and no foreign keys, so deleted accounts and games leave orphans (`src/main/services/launch.ts:451-458`, `docs/roadmap.md:56`). `artwork` and `artwork_attempts` rows are pruned only on a library scan (`src/main/services/artwork.ts:198-229`). `settings` rows for removed keys live on forever (`src/main/services/settings.ts:30-36`). The redaction name set stops growing silently at 2,000 names and never shrinks (`src/main/services/logger.ts:46-50`). The database is copied in full on every start (`src/main/index.ts:705-709`).
- **The poster grid is not virtualised** and re-renders on every status poll and keystroke; only `memo` on `GameCard` softens it (`src/renderer/src/components/GameCard.tsx:36`, `views/LibraryView.tsx:477-481`).
- **Install size.** 372 MiB, 98.8 % Electron; shipping one locale and dropping `dxcompiler.dll`/`dxil.dll` would save 74 MiB (`docs/roadmap.md:64`). A native binary removes the question.
- **Uncapped file reads** in the adapters, unlike every other launcher-file read (which goes through the 4 MiB / 1 MiB caps of `src/main/launchers/read-file.ts`): Steam's post-shutdown re-read of `loginusers.vdf` and `verifyIdentity`'s read (`src/main/launchers/steam/adapter.ts:1017,982`), Epic's `writeSession` reads (`epic/adapter.ts:1063`), Battle.net's `writeFile` re-read of `Battle.net.config` (`battlenet/adapter.ts:1297`), Riot's lockfile read (`riot/local-launch.ts:104`). The rewrite should cap all of them.

### 11.2 Duplicated state and code

- The store caches two domain answers (`launcherStatus`, `updateStatus`) against its own rule of "UI state only" (`src/renderer/src/store/ui.ts:105-108`, `docs/renderer.md:33`).
- `errorMessage`/`errorText` helpers are defined eight times in the renderer (`AccountDetailView.tsx:66`, `SessionPanel.tsx:10`, `LaunchDialog.tsx:66`, `LaunchHost.tsx:8`, `RecoveryDialog.tsx:11`, `BackupsPanel.tsx:21`, `SetupGuideSteps.tsx:34`, `ArtworkPanel.tsx:19`) and once in main (`src/main/services/launch.ts:72`).
- The clipboard note is built in two places that must stay identical (`src/renderer/src/views/AccountDetailView.tsx:245-249`, `components/LaunchDialog.tsx:169-173`).
- Two `RememberedRow` components, two `SkeletonRow` components, two readers of `launchers:rememberedAccounts` with their own selection state, and `keyOf`/`uidOf` doing the same thing (`components/ImportAccountsDialog.tsx:19-61,90-101`, `components/SetupGuideSteps.tsx:26,132,264-291`, `components/SetupGuide.tsx:125-160`, `components/LauncherStatusPanel.tsx:125`). The `ImportLauncher` type is defined twice (`store/ui.ts:35`, `components/SetupGuideSteps.tsx:13`).
- `SettingsView` keeps its own `EditableSetting` union of 11 keys that must be kept in step with `SETTING_KEYS` by hand (`src/renderer/src/views/SettingsView.tsx:31-42`).
- Two code paths serve the startup answers (the preload's `startup()` and the renderer's `invoke` fallback) (`src/renderer/src/lib/startup.ts:43-47`); `updates:status` and `accounts:list` are prefetched but nothing reads them as seeds (they only join an in-flight call).
- The quit path calls `app.quit()` twice by design (prevent, clean up, quit again) (`src/main/index.ts:737-754`); an explicit shutdown state machine is simpler.
- `AccountsService.checkUnique` loads every account of the launcher to compare `launcherUid` in JavaScript although a prepared statement for that lookup exists (`src/main/services/accounts.ts:164-166,258-261`).
- `backups:restore` lists and parses every manifest of the launcher to check the id exists, then the adapter loads the manifest again (`src/main/ipc/index.ts:470`, `src/main/services/backup.ts:220-231`).

### 11.3 Dead or unused surface

- IPC channels with no renderer caller (grep-verified): `app:ping`, `launchers:games` (`src/main/ipc/index.ts:107-111,401-405`; `docs/roadmap.md:70`).
- DTO fields that cross IPC on every call and are never read by the renderer (grep-verified): `AppInfo.chrome`, `.logDir`, `.dbPath`, `StartupMetrics.readyToShowMs`; `LaunchAttention.kind`, `.until`; `LaunchPreflight.alreadyActive`, `.launcherRunning`, `.runningGame`; `LaunchJob.backupId`; `SwitchRecovery.jobId`; `Game.installPath`, `.installed`, `.lastSeenAt`; `BackupSnapshot.path` (`src/shared/models.ts`). `settings:getAll` also returns the main-only `window.bounds` blob, which the renderer never uses (`src/main/services/settings.ts:30-36`, `docs/roadmap.md:68`).
- Shared types and helpers with no caller outside tests (grep-verified): `LaunchHistoryEntry` (`src/shared/models.ts:148-154`), `isSecretKey` and `SECRET_KEYS` (`:190,201-203`).
- Renderer props never passed (grep-verified): `Dropdown dense`, `Collapsible defaultOpen`, `TagInput placeholder`, `MenuItem.disabled` (so the disabled-item code paths in `MenuButton.tsx:33,140-147` never run), `LauncherDot className`, `MenuButton className`, `WhatsNewHost releases` (tests only). Exports used only in their own file: `flashClass`, `Flash`, `FlashTone`, `inputClass`, `phaseOf`, `SETUP_STEPS`, `SetupStepId`, `RememberedList`, `CardMenuItem`, `QueryState`. Unreachable branches: SessionPanel's `warn` tone (`describeState` never returns it, `SessionPanel.tsx:17-29,150`), `StepIcon`'s `skipped` branch identical to its fallback (`LaunchDialog.tsx:84-85`), `useFlash`'s initial 2,500 ms ttl that every caller overrides (`lib/useFlash.ts:14,22-26`). `LauncherStatus.implemented` still gates rows although all four adapters exist (`components/LauncherStatusPanel.tsx:29,56-60,68`, `src/shared/models.ts:488-489`).
- Main-process code with no production caller (grep-verified): `BackupService.readFile` (`src/main/services/backup.ts:272`), `ArtworkService.idle()` (`src/main/services/artwork.ts:291`), Steam `vdfHas` (`steam/vdf.ts:158-160`) and `SteamAppManifest.buildId` (`steam/manifests.ts:52,75`), Epic `DISPLAY_NAME` (`epic/adapter.ts:98`), `LAUNCHER_LOG_RELATIVE` (`epic/launcher-log.ts:26`), `EpicItemManifest.installationGuid`/`launchExecutable` (`epic/manifests.ts`), Battle.net `GameEntry.lastInstalledAt`/`serverUid` (`battlenet/config.ts:108-135`), the parsed-but-unconsumed Battle.net events `sign-in-attempt` and `close-requested` (`battlenet/client-log.ts:99-106,168-171`) and `lineTime`, Riot `launchRequest(events)` (`riot/client-log.ts:323-330`), `RiotProductSettings.dependency` (`riot/products.ts:73-74,97`), the Riot `quit` event (`riot/client-log.ts:123,232-235`), `started.at` (`:155,210-213`), and `spliceSessionBlock` with a non-null body (used only by the simulator, `riot/session.ts:183-192`).
- Legacy compatibility that may no longer be needed: the Battle.net `v: 2` session blob parser (`battlenet/session.ts:47-56`), the pre-2.52.12 Battle.net `Launch request` event (`docs/adapters/battlenet.md:81`), the Riot pre-116 `riot-login:` markers and build-116 `launchable:` lines (`riot/client-log.ts:101,214-231`), Steam's `MostRecent` synonym (still needed for old clients, `docs/adapters/steam.md:36`).
- Battle.net's `plan.executable` is always true, so the dialog's `blocked` stage is dead for Battle.net (`docs/roadmap.md:71`). Steam's plan marks `manual` always skipped and `verify` never skipped on the credentials path, where the opposite happens at run time (`steam/adapter.ts:694-699,1093`).
- `secrets.diagnostics.roundtrip` and `vault:getSecret` exist only for the Settings self-test (`src/main/ipc/index.ts:170-174`, `src/renderer/src/views/SettingsView.tsx:135-148`).
- `getBool` accepting `'1'` has no writer (`src/main/services/settings.ts:41`); `settings:get` has one renderer caller (`backups.keep`), everything else uses `settings:getAll` (`components/BackupsPanel.tsx:151`).
- `plural(n, 'entry')` yields "entrys" (`epic/adapter.ts:1412`).
- Signing code in the client: `signManifest` and `publicKeyOf` have no caller in `src/` (only the release scripts) (`src/main/updates/manifest.ts:4,37-46`); two SHA-512 implementations exist only because the quit handler must stay synchronous (`manifest.ts:100-124`).
- Artwork makes up to six Steam CDN requests per game (2 hosts × 3 kinds) before SteamGridDB, and tries two Steam search endpoints in sequence (`src/main/services/artwork.ts:74-77,386-392,513-526`).
- The private clipboard write spawns PowerShell with `System.Windows.Forms` for one `SetDataObject` call (`src/main/platform/clipboard-private.ts:9-19,31`); the embedded C# of the process scripts is recompiled by `Add-Type` on every call (`src/main/platform/processes.ts:99-182,189-433`).
- `WindowsRegistry.query` puts `err.message` (the `cmd.exe` command line) into read errors, inconsistent with the write path's rule (`src/main/platform/registry.ts:215,239`); reads carry no secret, so noise rather than a leak.

### 11.4 Development-only weight in the production bundle

By standing decision (`docs/roadmap.md:30`): the four launcher simulators (`src/main/launchers/sim/`, about 1,900 lines), `StaticRegistry`, `StaticProcessController`, `MemoryLoginItem` and the fake-platform wiring (`src/main/platform/registry.ts:317-443`, `processes.ts:570-642`, `login-item.ts:49-60`, `wiring.ts`). The packaged exe ignores the environment variable that would use them, so they are inert weight there. The simulators also couple to adapter internals (`STEAM_HKLM_KEY`, `SETTINGS_INI_RELATIVE`). Appendix A describes what they do so the rewrite can decide whether a fake mode is worth rebuilding.

### 11.5 Behaviour that looks like a bug or a trap (keep out, or fix knowingly)

- The Epic, Battle.net and Riot snapshot reaches the journal before any write, so a switch whose writes are all no-ops can leave a `backupId` on a `failed` result with `restored: null`, and after a crash a recovery prompt for a write that never happened (`epic/adapter.ts:1041`, `docs/switching.md:119`). Steam's credentials path takes a snapshot but never fires `onBackup`, so `backupId` is set with `restored: null` and the snapshot counts against `backups.keep` for nothing (`steam/adapter.ts:1085,845-848`).
- Epic's `settingsPaths()` picks "newest" by mtime with a 2 s tie rule favouring `WindowsEditor`; a restore writes both files within milliseconds, so folder order, not the launcher, decides afterwards (`epic/adapter.ts:283-284`). Epic writes the session into both files but the launcher reads only the newest (`docs/roadmap.md:62`).
- `Game.lastLaunch` relies on SQLite's bare-column rule to pick the `account_id` that accompanies `MAX(launched_at)` (`src/main/services/library.ts:87-90`); the rewrite should select it explicitly.
- `rememberBounds` saves on every `close`, including a close turned into a hide; `isMaximized()` can read false for a hidden window and `pendingMaximize` is only set for a hidden start (`src/main/index.ts:288-292,326-332,381-395`).
- The database copies are pruned by file-name sort, so a clock set backwards makes a copy look older than it is (`src/main/db/index.ts:103-107`).
- `recovery:dismiss` failing still closes the prompt, so the question can return next start without explanation (`src/renderer/src/components/RecoveryDialog.tsx:36-42`). A `launch:status` poll error marks the job failed in the renderer while main's job may still be running (`components/LaunchHost.tsx:99-105`).
- The Import accounts dialog closes on Escape and backdrop click even while an import is in flight; the Restore dialog has no Escape handler (`components/ImportAccountsDialog.tsx:103-109,137`, `components/BackupsPanel.tsx:26-87`).
- `Dropdown` and `MenuButton` close on any `scroll` event captured on `window`, including scrolls inside unrelated containers (`components/Dropdown.tsx:80`, `components/MenuButton.tsx:59`).
- `launchers:importAccounts` and `accounts:create` change accounts without `requireIdle`, unlike `accounts:update` (`src/main/ipc/index.ts:247-259,413-421`; `docs/switching.md:16` says they should refuse during a job).
- `settings:set` queues artwork on enabling auto-fetch only when the previous stored value was exactly `'false'` (`src/main/ipc/index.ts:222`).
- The Riot rollback closes a running client with a shutdown that cannot be interrupted (up to 3 s + 10 s + 10 s plus a restart watch, three rounds) before restoring the file (`riot/adapter.ts:1689`).
- Epic's close step took 54 s once because the launcher handed off to its updater and never came back (`docs/roadmap.md:51`, B24); Battle.net waits its full 5 s restart watch after every clean shutdown (B22). Both are timing budgets the rewrite can revisit with the observations in `docs/adapters/*.md`.
- A hard kill in the first ~10 s of a data folder's first session loses the DPAPI key because Chromium has not yet written `chromium\Local State` (`docs/architecture.md:63`). This is an Electron/Chromium artefact; a native app owning its own DPAPI blob has no such window.
- The CSP is only a `<meta>` tag, not a header (`docs/roadmap.md:61`). Irrelevant to a native UI.

## 12. Open questions

Things the code does not settle, places where the project's docs and the code disagree, and product decisions the rewrite needs from you. Each names the file that was looked at. Nothing here was filled in by guessing in the sections above; where a section had to pick a reading, it says so and points here.

### 12.1 Product decisions for the rewrite

1. **Fake-launcher mode.** The simulators and test doubles ship in the bundle today so the driver can exercise Play without real launchers (`docs/roadmap.md:30`). Does the Rust app need an equivalent (a trait-object platform with an in-process fake), or will end-to-end testing be done another way? Appendix A has what the fakes model.
2. **Push vs poll.** Every background change reaches the UI by polling (section 6). GPUI can update views directly; confirm the rewrite drops the polls and the `revision` counter, or keeps the counter as the change signal.
3. **Child processes vs native APIs.** The app shells out to `reg.exe`, `cmd.exe` and PowerShell for every registry and process operation, with hard rules about what must never be used (`tasklist`, `taskkill`, WMI, `Get-Process`; section 8). A native implementation must honour the same constraints: list processes without opening them (`NtQuerySystemInformation`), close by `WM_CLOSE` to the right window, kill without the tree where the adapter says so, never touch a Riot game or Vanguard. Confirm the rewrite goes native and that the ETW trace described in `docs/architecture.md:40-41` is the acceptance test.
4. **The updater.** electron-updater's role (GitHub releases feed, differential downloads, NSIS `--updated` relaunch) has no single Rust equivalent (section 10). Decide: rebuild the same flow (signed `latest.yml`, download, verify, run the installer at quit), or change the distribution model. Only installed per-user copies on the default data folder update today (`src/main/updates/availability.ts`).
5. **Installer.** NSIS assisted, per-user, licence page, folder migration from `Programs\One Launch`, Run-key cleanup, data kept on uninstall (section 8). Which installer technology for the rewrite, and does it keep the same `HKCU\Software\<GUID>` install detection the updater relies on?
6. **DPAPI key ownership.** Today the key is Chromium's `os_crypt` in `chromium\Local State`, so the data folder is bound to that profile folder (`docs/architecture.md:63`). A native app calling `CryptProtectData` directly has no profile folder; existing ciphertext from the Electron app cannot be decrypted by the rewrite without Chromium's key wrapping. Decide whether the rewrite migrates secrets (it would need to read `Local State`, unwrap the key with DPAPI, and decrypt the AES-GCM `v10` blobs) or asks users to re-enter passwords and re-save sign-ins once. Backups' `.enc` files have the same dependency.
7. **Riot Games.** The rules in `docs/adapters/riot.md:9-24` are standing decisions. The rewrite must keep them; confirm nothing in the Rust toolchain (e.g. a process-listing crate that opens handles) violates rule 1.
8. **Wording and copy.** Section 2 quotes the behaviour-carrying strings; the rest is paraphrased per your choice. Is a full copy inventory wanted later, before the UI is built?
9. **Settings that read as on unless stored off.** `artwork.autoFetch`, `window.closeToTray`, `updates.auto`, `whatsNew.enabled`, `riot.pressPlay`, `play.hideToTray` default on; `dryRun` and `steam.autoLogin` default off (section 9). Keep the string-valued settings table, or type them?

### 12.2 Where the docs and the code disagree

10. `docs/architecture.md:92` says artwork is removed with its game by cascade; games are never deleted in code, so the cascade never fires and the prune is the only path (`src/main/db/migrations/index.ts:89,182`, `src/main/services/artwork.ts:194-229`).
11. `docs/architecture.md:115` says `updates.auto` off means no update traffic unless asked; a background check already in flight completes its network check (`src/main/updates/update-service.ts:107-119,232-235`).
12. `docs/switching.md:16` says handlers that change accounts answer `BUSY` during a job; `launchers:importAccounts` and `accounts:create` do not call `requireIdle` (`src/main/ipc/index.ts:247-259,413-421`).
13. `docs/switching.md:23` says `preflight(account, forGame?)` names the game so the plan's start step is the real one; Steam's and Epic's `preflight`/`buildSwitchPlan` take no game (`src/main/launchers/steam/adapter.ts:482,503`, `epic/adapter.ts:646,667`). Only Riot uses it.
14. `docs/switching.md:109` says the manifest's `meta` holds "registry values; `null` = absent"; Battle.net's `meta.unifiedAuth` is a JSON string of vault ciphertexts (`src/main/launchers/battlenet/adapter.ts:1553-1555`). The spec follows the code.
15. `docs/adapters/riot.md` › Switch says the plan warning carries the same sentence as the `GAME_RUNNING` refusal; the two texts differ (“…Close it first: One Launch never closes a Riot game, and no switch runs while one does.” vs “…Close it, then switch: One Launch never closes a Riot game.”) (`riot/adapter.ts:1268,784`).
16. `docs/adapters/riot.md` › Launch says the login-page check applies to a closed client only; the code applies `lastSignIn` to whatever the watch reads (`riot/adapter.ts:929-933`).
17. `docs/adapters/epic.md:96` describes the shutdown as "wait 20 s, then a forced kill"; the code also waits `killGraceMs` 10 s after the kill (`epic/adapter.ts:607`).
18. `docs/renderer.md:80` lists the Diagnostics rows without the "Build" row the code shows (`src/renderer/src/views/SettingsView.tsx:381`).
19. `docs/architecture.md:109` says `backups.keep` is "applied at the next snapshot"; the dry-run plan text also reads it at once (`steam/adapter.ts:572`).
20. `docs/adapters/battlenet.md` › Library says key names may contain `:` and are passed through; the parser drops any hit whose relative name contains `\` (`battlenet/products.ts:76`).

### 12.3 Behaviour the code leaves unverified or unobserved

21. Steam: the two-factor (“Need Two Factor”) and invalid-password wordings on `WaitingForCredentials` lines are unobserved; `classifyReason` matches "two factor"/"twofactor"/"mobile" and "password" (`steam/login-log.ts:49-51`, `docs/adapters/steam.md:66`). Whether `-login <name>` ever signs in without a password when a session for that login is remembered (`docs/adapters/steam.md:56`). The `-login` argument is the One Launch login, not the `AccountName` from `loginusers.vdf`; they match only case-insensitively (`steam/adapter.ts:923,786`). `STALE_SESSION_DAYS` = 60 is a guess from one observation (`steam/adapter.ts:102-107`). `signedInAs` trusts `ActiveUser` while the log is silent, so a Steam heading for its form can look already active (`docs/adapters/steam.md:97`).
22. Epic: only `TOKEN_EXPIRED` is a confirmed session-rejection wording; `refresh token`, `invalid_grant`, `token … invalid|expired|revoked` are guesses (`epic/launcher-log.ts:108-109`). Whether `LogTimes=Local` ever appears (`docs/adapters/epic.md:121`). Whether the self-relaunch (`-AllowSoftwareRendering`) produces a second log file that resets the mark (`epic/launcher-log.ts:159`). An expected uid with a TRUE line whose id is empty keeps polling to the deadline (`epic/adapter.ts:1114-1115`). `captureFresh(0)` on an in-place sign-in cannot catch a blob rotated a moment later (`epic/adapter.ts:1264,1177-1188`). `rememberPassword` and `mostRecent` are the same boolean for Epic (`epic/adapter.ts:552-553`).
23. Battle.net: whether a running game is a child of `Battle.net.exe` (the forced kill assumes so, `battlenet/adapter.ts:704-705`). Twenty of twenty-two products carry `verified: false` (`battlenet/products.ts:21-48`). The self-update log sequence is observed but not parsed (`docs/adapters/battlenet.md:115-126`). An id-less `signed-in` line with an expected uid polls to the deadline; without one it is accepted and nothing is linked (`battlenet/adapter.ts:1352-1353`). The credentials path maps a `failed` verdict to `timeout`, so a `Deleting registry token` at start never marks a stale snapshot (`:1384`). Whether `GameLaunchWindowBehavior = 2` should feed the plan (`docs/adapters/battlenet.md:113`). Whether the forced kill by image name could end a forwarder `Battle.net.exe --exec=` started by something else (`:705`).
24. Riot: whether Riot's `direct_launch_opt_in` ever ships (if it does, the arguments start the game again and no press is sent) (`docs/adapters/riot.md:160`). How long a rotated-out refresh token stays valid (`:161`). 2XKO's image names are unverified (`riot/products.ts:24`). Whether `Sessions\`, `RiotClientSettings.yaml`, `client.config.yaml` matter (`docs/adapters/riot.md:171`). A logged sign-in with the file still holding no session after 15 s ends `signed-in` with a null account id and no link (`riot/adapter.ts:1503-1504`). `holdsUnownedSession` returns false on an unreadable file, so such a snapshot is not pinned (`:695-702`). Window processes left with no core are closed at every `shutdown`, including from a rollback (`:805-808`).
25. Updates: electron-updater's cache layout under `%LOCALAPPDATA%\one-launch-updater` is the library's default, not app code (`src/main/updates/engine.ts:61-62`); the rewrite needs its own. A manual check never re-arms the 6 h timer; `setAuto(true)` schedules a check 15 s later even right after one (`update-service.ts:101-119,195-199`). Uninstall, upgrade in place with the app running, and a real update are listed as unverified by hand (`docs/roadmap.md:39`).
26. Window: whether the saved `maximized` flag survives a hide-while-maximized (`src/main/index.ts:288-292,326-332`). The vault banner's claim that the key is lost "when its `chromium` folder was deleted" is correct for the current layout (`docs/architecture.md:63`) but will not be for a native app.
27. Riot `pressPlay` with no local-launch helper wired reads as off; only the fake platform can lack it today (`riot/adapter.ts:984-986`, `src/main/platform/wiring.ts`).
28. `LauncherStatus.implemented` can no longer be false now that all four adapters exist; confirm it is dropped (`src/shared/models.ts:488-489`).
29. Whether the Battle.net `v: 2` blob exists in any real data folder (`battlenet/session.ts:47-56`), whether `firstRun.dismissed` should accept `false` (`src/main/ipc/index.ts:204-218`), and whether the development-only `logLevel` exception (dev always starts at debug) should survive (`src/main/index.ts:441-442`).
30. Whether `launch_history` should keep rows for deleted accounts and games (C2 in `docs/roadmap.md:56`), and what the retention should be.
31. Updates, continued: electron-updater is left free to use differential (blockmap) downloads on the GitHub feed (`src/main/updates/engine.ts:59` disables them only for the test feed); `verifyManifest` accepts any plain `.exe` name, not only `OneLaunch-<version>-setup.exe` (`manifest.ts:60-66`, `electron-builder.yml:36`). Decide whether the rewrite downloads whole files and pins the name.
32. Artwork: SteamGridDB's autocomplete takes the first hit without a name check, unlike the Steam search's exact-match rule (`src/main/services/artwork.ts:542-545`); `IMAGE_HOSTS` lists `shared.steamstatic.com` because one CDN host was observed redirecting there, and nothing else (`:83-84`). Intended asymmetry; keep the host list configurable.
33. Window placement numbers (64 px sideways, 8 px above, 64 px from the bottom, `src/main/window-guard.ts:66`) and the 35 px overlay vs 36 px title strip (`src/main/index.ts:311`, `src/renderer/src/components/TitleBar.tsx:17`): keep exactly, or re-derive for GPUI's own title bar?
34. Installer: whether the rewrite's installer should hide the all-users choice entirely (today it is offered and the resulting install is refused updates and autostart, `src/main/updates/availability.ts`, `build/installer.nsh`), and whether an in-place update must preserve the Run value by the same `${isUpdated}` mechanism or any other (`build/installer.nsh:53`).
35. Process close: `WM_CLOSE` goes to the first visible unowned top-level window in z-order, chosen for parity with `taskkill /IM` (`src/main/platform/processes.ts:357-364`). Is `taskkill` parity the requirement, or any visible top-level window?

## Appendix A. Development-only surfaces [dev-only]

None of this is product behaviour. It ships in the Electron bundle by a standing decision (`docs/roadmap.md:30`) because the driver needs it, and the rewrite can carry it, replace it, or drop it. It is described so that decision is informed and so the fixtures under `fixtures/dev-only/` make sense.

### A.1 Fake launcher platform

- Activated only in an unpackaged run by `ONELAUNCH_FAKE_LAUNCHERS=<absolute folder>`; the packaged exe ignores the variable (`src/main/platform/wiring.ts:18-37`). The folder must contain `scenario.json`; otherwise the app exits with code 2 before opening anything, like a bad `--data-dir=`.
- `createPlatform` then swaps the real platform for: a recording in-memory registry (`StaticRegistry`), a simulated process controller (`SimProcessController`) running four client simulators (`src/main/launchers/sim/{steam,epic,battlenet,riot}.ts`), path roots inside the scenario folder (`steam`, `epic`, `battlenet`, `riot`, `localappdata`, `appdata`, `programdata`, `games`, `nowhere`), short adapter timings, a fixture-backed `fetch` for artwork, an in-memory login item, a clipboard recorder, and a seed of accounts/sessions/settings applied once when the data folder has no accounts (`src/main/launchers/sim/scenario.ts:173-191`, `seed.ts:49-52`).
- The `starting` log line records `platformSource: 'fake'`, `app:info` reports it, the window title becomes “One Launch (fake launchers)” and the title bar shows a “Fake launchers” badge, so no screenshot of a fake run can pass for a real one (`src/main/index.ts:429,480`, `docs/renderer.md:26`).
- Everything the fake platform was asked to do is written to `<dataDir>\fake-platform.json` (rewritten whole on each change, temp file then rename): `registryWrites[{op, path, value?}]`, `processEvents[{kind: start|close|kill, exe, args}]`, `opened[]` (URIs), `fetched[]` (URLs), `clipboardWrites` (count), `riotLaunchRequests[]` (`<product>.<patchline>`) (`src/main/launchers/sim/report.ts:6-14,59-65`). Example: `fixtures/dev-only/fake-platform.json`.

### A.2 `scenario.json` (read by the app) and `knobs.json`

Schema (`src/main/launchers/sim/scenario.ts:42-66`, `types.ts`):

| Field | Type | Meaning |
| --- | --- | --- |
| `version` | `1` | Required. |
| `installed` | `("steam"\|"epic"\|"battlenet"\|"riot")[]` | Which simulators exist. Required. Only `version` and `installed` are validated. |
| `files` | `Record<relPath, "@fixtures/<f>" \| string \| {inline}>` | Read by the driver (`scripts/app-driver.mjs:100-126`), never by the app: files to lay down in the copy; `{root}` and `{root:vdf}` are substituted in `inline`. |
| `registry` | `Record<"HIVE\\key\\name", string \| number \| "binary:<hex>">` | Applied over each simulator's default registry. |
| `processes` | `{running[], table[{name,pid,path}], shutdownPolls (2), startFails[], elevated[], reappearsAfterPolls{image:n}}` | Process-table knobs. A running Riot game is `table: [{name: "VALORANT-Win64-Shipping", pid: 1, path: null}]`. |
| `knobs.steam` | `{expiredSessions[], wrongPassword, loginVerdict, selfUpdateMs (null; negative = never finishes), reactMs (0), signInWhenAskedMs, signInAs, runningAs}` | Steam simulator (`sim/steam.ts:51-60`). |
| `knobs.epic` | `{expired[], rotateOnStart (false), signInError, reactMs, signInWhenAskedMs, signInAs, runningAs}` | Epic simulator (`sim/epic.ts:47-56`). |
| `knobs.battlenet` | `{rejected[], rotateOnSignIn (false), reactMs, signInWhenAskedMs, signInAs, runningAs, updatesPending[], gamesRunning[]}` | Battle.net simulator (`sim/battlenet.ts:77-88`). |
| `knobs.riot` | `{rejected[], rotateOnStart (true), authError, reactMs, signInWhenAskedMs, signInAs, staySignedIn (true), runningAs, updatesPending[], directLaunch (false), pressPlayFails (false)}` | Riot simulator (`sim/riot.ts:205-218`). |
| `timing.<launcher>` | `Record<string, number>` | Overrides of `SIM_TIMING` (`scenario.ts:22-40`): `pollMs`/`logPollMs`/`processPollMs` 100, `shutdownMs` 3000, `killGraceMs` 1000, `restartWatchMs` 500, `cleanExitWatchMs` 200, `restartSettleMs` 2000, `startupMs` 5000, `signInMs` 8000, `guardMs` 8000, `updateMs` 20000, `manualSignInMs` 30000, `captureMs` 3000, `readyGraceMs` 300, `launchConfirmMs`/`playConfirmMs` 2000. |
| `onDisk` | `{epic?: uid, battlenet?: uid}` | Which account owns the session blob already on disk. |
| `seed` | `{settings?, accounts?[{launcher, displayName, login, launcherUid?, password?}], sessions?[{launcher, login, launcherUid, blob \| refreshToken}]}` | Applied once per data folder (`sim/seed.ts:27-31`). A Riot session is seeded from a `refreshToken` alone; the simulator builds the whole file around it. |
| `quitDuringSwitch` | `"keep" \| "quit"` | Answers the native quit-during-switch prompt, which cannot be driven over CDP (`src/main/index.ts:137,207-209`). |

`knobs.json` in the same folder is merged over the scenario: `processes` shallow-merged, each `knobs.<launcher>` shallow-merged, each `timing.<launcher>` merged as `SIM_TIMING` then scenario then knobs, `quitDuringSwitch` from knobs wins; every other field in `knobs.json` is ignored (`scenario.ts:82-87,269-285`). The committed scenarios are `all-four`, `none` and `unowned` (`test/fake-launchers/`, copied to `fixtures/dev-only/fake-launchers/`).

### A.3 Test doubles that ship

`StaticRegistry` (`src/main/platform/registry.ts`) and `StaticProcessController` (`src/main/platform/processes.ts`) are in-memory implementations of the registry and process interfaces, used by the adapter tests and by the fake platform. `MemoryLoginItem` (`src/main/platform/login-item.ts`) stands in for the HKCU Run value.

### A.4 Other development-only environment variables and flags

| Name | Effect | Source |
| --- | --- | --- |
| `ONELAUNCH_DATA_DIR` | Data-folder redirect, unpackaged only; `--data-dir=` wins. | `src/main/platform/paths.ts:49` |
| `ONELAUNCH_UPDATE_FEED=http://127.0.0.1:<port>/`, `ONELAUNCH_UPDATE_KEYS=<spki,…>` | Loopback update feed trusted with throwaway Ed25519 keys; cache in `<dataDir>\update-cache`; the installer is never run (`update install (test feed, installer not run)` is logged). Unpackaged only; a bad value exits 2. | `src/main/updates/test-feed.ts`, `engine.ts:36-59` |
| `ONELAUNCH_LIVE=1` | Un-skips the read-only live tests against real launcher installs. | `docs/development.md:44` |
| `ELECTRON_RENDERER_URL` | Set by electron-vite in dev; makes `isDev` true (devtools, F12, Ctrl+R reload, console log mirror, log level debug, relaxed CSP). | `src/main/index.ts:73-74,373-379` |
| `REMOTE_DEBUGGING_PORT` | electron-vite passes `--remote-debugging-port`, which the driver and `agent-browser` attach to. | `scripts/dev-fake.mjs` |

### A.5 Developer panel

In development builds only (`import.meta.env.DEV`), Settings › Advanced ends with a Developer panel that lists every What's New entry and opens any of them regardless of the running version; saving the releases file hot-swaps the open card (`src/renderer/src/components/WhatsNewDevPanel.tsx`, `views/SettingsView.tsx:21,384-388`). A production bundle never contains it.

### A.6 The driver and scenarios

`scripts/app-driver.mjs` starts the built app on a fresh scratch data folder with `--data-dir=` and a random CDP port (9400-9799), drives it over the Chrome DevTools Protocol with plain Node, and checks that the app exits 0 with no ERROR line in its log. `scripts/drive.mjs` runs one of the ~30 committed scenarios in `scripts/scenarios/` (see `.claude/skills/run-one-launch/SKILL.md` for the table). `scripts/smoke-packaged.mjs` does the same against the packaged exe, read-only. None of this is needed by the product; it is the project's end-to-end test rig, and `SCREENSHOTS.md` names the scenarios that reach each screenshot state.

## Appendix B. How this document was checked

Mechanical cross-checks run against the repository after assembly, so a reader can rerun them:

| Check | Result |
| --- | --- |
| Every entry of `IPC_CHANNELS` (`src/shared/ipc-contract.ts`) appears in section 5 | 51 names checked (49 channels plus the two type-assertion lines the regex also matched), none missing |
| Every entry of `SETTING_KEYS` (`src/shared/models.ts`) appears in section 9 | 19 keys, none missing |
| Every file under `src/renderer/src/{views,components,lib}` is named in section 2 | all named |
| Every error code declared with `readonly code = '…'`, thrown as `new IpcRequestError('…')` or produced by the registrar appears in section 3 | 32 codes, none missing |
| Every host and URI in `src/main` network and `openExternal` call sites appears in section 7 | 11 call sites, all hosts present |
| Every OS integration point in `src/main/index.ts`, `tray.ts`, `window-guard.ts`, `platform/*`, `build/installer.nsh`, `electron-builder.yml` is described in section 8 | checked by behaviour wording (single instance, menu, permissions, logoff, renderer crash reload, navigation blocks, clipboard formats, uninstaller name, app id) |
| Every `*_MS` constant under `src/` appears with its value in the spec | 14 constants, all present |
| Every `path:line` citation names an existing file and a line within it | 1,296 citations with line numbers, 0 beyond end of file after one fix |
| No real login, email, account id, token or `C:\Users\<name>` path anywhere under `rewrite-prep/` (the repo's own `scripts/hooks/no-secrets.mjs` rules, run over the untracked files) | SPEC.md and SCREENSHOTS.md: 0 findings. Fixtures: 17 findings, all synthetic by construction (randomly generated job/account UUIDs in the journal files, SHA-256 hashes of placeholder files in the backup manifests, and the sample JWT the generator writes so the log fixture shows it redacted) |

Not checked mechanically: the verbatim strings (quoted by the drafters from the source and spot-checked), the step-by-step adapter sequences (written from a full read of each adapter), and anything in section 12.
