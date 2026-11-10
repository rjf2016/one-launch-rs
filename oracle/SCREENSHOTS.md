# Screenshot checklist

Every screen and state from SPEC.md § 2, with a suggested filename and how to reach it. Capture at **1280×800** unless a row says otherwise; rows marked `+960` also want a **960×600** capture (the window minimum, where the sidebar collapses and forms hit their caps) with `-960` appended to the filename. Dark mode is the only mode.

## How to reach the states

Two ways to run the app for captures:

- **Fake launchers (safe, every state reachable).** From the repo: `npm run dev:fake` starts a dev build on a fresh scratch data folder against four simulated launchers seeded with placeholder accounts (`test/fake-launchers/all-four/scenario.json`: Steam "Persona One"/"Persona Three", Epic "Main"/"Alt"/"New", Battle.net "Main"/"Alt"/"New", Riot "Riot Main"/"Riot Alt"/"Riot New"; games Rocket League (Steam and Epic), Overwatch, Hearthstone, VALORANT, League of Legends). Play, Stop, rollback, restore and recovery are all safe here. The title bar shows a "Fake launchers" badge in every capture made this way; that is expected. Options: `npm run dev:fake -- --fake=none` (no launchers: setup guide and empty states), `-- --fake=unowned`, `-- --knobs='<json>'` (simulator knobs, see SPEC.md Appendix A.2), `-- --keep` (keep the scratch folder).
- **Real launchers (read-only states only).** The installed app on your normal data folder. Only for states that need the real badge-free chrome, real artwork, real launcher status rows, the updater, Start with Windows, the tray and the installer. Never press Play, Switch, Restore or a Settings toggle you do not mean, and never run a switch for a screenshot; use fake launchers for anything that writes.

Setting a knob or a setting by hand: Settings › Advanced › Preview mode etc. are ordinary toggles. Simulator knobs are JSON on the `--knobs` flag, for example a running Riot game: `--knobs='{"processes":{"table":[{"name":"VALORANT-Win64-Shipping","pid":1,"path":null}]}}'`; an expired Steam session: `--knobs='{"knobs":{"steam":{"expiredSessions":["76561198000000003"]}}}'`; an elevated Steam: `--knobs='{"processes":{"elevated":["steam.exe"]}}'`; Battle.net wanting to update Overwatch: `--knobs='{"knobs":{"battlenet":{"updatesPending":["prometheus"]}}}'`; the Riot Client refusing a saved sign-in: `--knobs='{"knobs":{"riot":{"rejected":["33333333-3333-4333-8333-333333333333"]}}}'`.

The committed driver scenarios (`node scripts/drive.mjs scripts/scenarios/<name>.mjs --fake --shots=<folder>`) already produce many of these captures automatically; the "Scenario" column names the one that reaches the state, as a shortcut.

## App shell and sidebar

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 1 | `shell-boot-surface.png` | The CSS-only boot surface (title strip, brand mark fading in) before React's first commit | Hard to catch by hand: record a screen video of a cold start of the installed app and grab the frame ~300 ms in, or throttle with `--trace-startup` | `startup-timing` |
| 2 | `shell-default.png` `+960` | Library open, sidebar expanded, no dialogs | Start the app with ≥ 1 game | `views` |
| 3 | `shell-sidebar-collapsed.png` | Sidebar collapsed to icons | Ctrl+B, or shrink the window below 1100 px | `views` (960 shot) |
| 4 | `shell-fake-badge.png` | "Fake launchers" badge in the title bar [dev-only] | Any `dev:fake` run | any `--fake` |
| 5 | `sidebar-preview-mode-card.png` | "Preview mode is on" card at the bottom of the sidebar | Settings › Advanced › Preview mode on, go back to Library | `preview` |
| 6 | `sidebar-update-ready-card.png` | "Update ready · Restart to install <version>" card | Installed copy with a newer published release, after the background download; or the `update` scenario's loopback feed | `update` |
| 7 | `sidebar-update-ready-disabled.png` | Update card disabled during a running switch | Same, while a fake switch runs (tooltip "...once the switch has finished") | — |
| 8 | `shell-error-boundary.png` | "Something went wrong in the interface" fallback with Reload / Open logs | Dev build: throw inside a component (temporary edit), or `agent-browser eval` a render error | — |
| 9 | `shell-window-min-960x600.png` | The whole window at its minimum size | Resize to 960×600 | `views` |

## Library

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 10 | `library-populated.png` `+960` | Poster grid with artwork, Recent strip, toolbar | Fake run after a Play; or the real app | `views` |
| 11 | `library-scanning.png` | Empty state "Scanning your launchers…" with disabled Scan now | First second of a fresh folder's first start (`dev:fake -- --fake=none`, before the scan finishes) | `first-run` |
| 12 | `library-no-launchers.png` | "No launchers found" empty state | `dev:fake -- --fake=none`, dismiss the setup guide | `first-run` |
| 13 | `library-no-games.png` | "No games found" empty state | A real PC with launchers installed and no games, or fake `all-four` with the games' install folders removed from the scenario copy | — |
| 14 | `library-load-error.png` | "Could not load the library: <message>" | Needs a failing `library:list` (corrupt DB mid-run); optional | — |
| 15 | `library-no-match-search.png` | `No games match “xyz”` with "Clear search and filters" | Type a nonsense search | `views` |
| 16 | `library-no-match-filter.png` | "No <launcher> games installed" | Filter on a launcher with no games (fake: filter Steam after removing its game, or filter Riot in `unowned`) | — |
| 17 | `library-search-active.png` | Search text entered, clear button visible, grid filtered | Type "rock" | `views` |
| 18 | `library-filter-dropdown-open.png` | Filter dropdown open with per-launcher counts | Click Filter | `views` |
| 19 | `library-sort-dropdown-open.png` | Sort dropdown open (Last played / Name) | Click Sort | `views` |
| 20 | `library-fetching-artwork.png` | "Fetching artwork (n)" in the header | Settings › Artwork › Refresh all, return to Library quickly (real app) | — |
| 21 | `library-add-key-link.png` | "Add a free key for missing covers" header link | Real app with a non-Steam game lacking a cover and no SteamGridDB key, after the scan | — |
| 22 | `library-card-hover.png` | Poster with Play roundel and More button on hover | Hover a card | — |
| 23 | `library-card-busy.png` | Poster roundel spinning (instant Play in flight) | Click a poster whose game was launched before (fake) and capture within ~300 ms | `already-active` |
| 24 | `library-card-update-badge.png` | "Update" badge on a poster | Fake with `updatesPending` knob (Battle.net `prometheus`) or Steam manifest with update bit | `needs-update` |
| 25 | `library-card-launcher-icon.png` | VALORANT tile showing the game's own icon as stand-in artwork | Fake `all-four` (VALORANT's `.ico` placeholder) | `views --fake` |
| 26 | `library-card-monogram.png` | Poster with no artwork: two-letter monogram behind the name | Fake run before artwork fixtures load, or a game Steam cannot find | `views --fake` |
| 27 | `library-card-removed-account.png` | Caption "removed account · <time>" | Launch a game as an account (fake), then delete that account | — |
| 28 | `library-card-menu.png` | CardMenu open: "Artwork: <source>", Play as…, Refresh artwork, Choose portrait image… | Right-click a poster | `views` |
| 29 | `library-recent-strip.png` | Recent strip with ≥ 2 entries | After two Plays as different accounts (fake) | `play-all` |

## Accounts

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 30 | `accounts-list.png` `+960` | Grouped list, ready/not-ready icons, "Used …"/"Sign-in saved"/"Not used yet", "Password stored" | Fake `all-four` after one Play | `views` |
| 31 | `accounts-empty.png` | "No accounts yet" with Import accounts / Add account menus | `dev:fake -- --fake=none` | `first-run` |
| 32 | `accounts-import-menu.png` | Import menu open (From Steam, From Battle.net, Epic/Riot add by hand) | Click Import | `import` |
| 33 | `accounts-add-menu.png` | Add account menu open (four launchers) | Click Add account | — |
| 34 | `accounts-flash-imported.png` | Header flash "Imported n accounts from Steam" | Complete an import (fake) | `import` |
| 35 | `accounts-flash-nothing-new.png` | "Nothing new to import" | Import with everything already linked | `import` |
| 36 | `accounts-load-error.png` | "Could not load accounts: <message>" | Optional (needs a failing IPC) | — |

## Account detail

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 37 | `account-create-steam.png` `+960` | New Steam account form (Details with FIRST_PLAY line, Password, collapsed Advanced) | Add account › Steam | `first-run` (end) |
| 38 | `account-create-riot.png` | New Riot account form (different hints/placeholders) | Add account › Riot Games | — |
| 39 | `account-create-validation.png` | "Display name is required." / "Login is required." under fields after pressing Create | Press Create account on an empty form | — |
| 40 | `account-create-advanced-open.png` | Advanced expanded showing the launcher id field | Expand Advanced | — |
| 41 | `account-create-leave-guard.png` | Confirm dialog "Leave without creating this account?" | Type something, press Ctrl+1 | — |
| 42 | `account-edit.png` `+960` | Edit page: header, Details, Password (Not stored), Saved sign-in (Epic), Advanced collapsed, Danger zone | Open the Epic "Alt" account (fake) | `views` |
| 43 | `account-edit-created-flash.png` | "Account created. Pick a game in the Library and press Play." | Immediately after creating an account | — |
| 44 | `account-edit-dirty.png` | Dirty state: Discard replaces Switch to this account, Save changes enabled | Edit the display name | — |
| 45 | `account-edit-saved-flash.png` | "Saved" flash | Save | — |
| 46 | `account-edit-leave-guard.png` | "Leave without saving?" confirm | Edit, then navigate | — |
| 47 | `account-edit-password-stored.png` | Password panel "Stored · encrypted", Update password / Remove | An account with a password (fake Steam "Persona Three") | `views` |
| 48 | `account-edit-password-editing.png` | Password field open with eye toggle, Store/Update + Cancel | Press Store password | — |
| 49 | `account-edit-password-remove-confirm.png` | Two-step "Confirm remove" / Cancel | Press Remove | — |
| 50 | `account-edit-copy-flash.png` | "Password copied · clipboard clears in 30s while One Launch is open" | Press Copy password (fake run; the fake clipboard records it) | `handover` |
| 51 | `account-edit-session-saved.png` | Saved sign-in: "Saved <time>", "Last used for a switch…", signed-in-as row | Fake Epic "Alt" after a Play | `play-epic` |
| 52 | `account-edit-session-not-saved.png` | "Not saved yet", Save sign-in disabled with tooltip "Nothing to save…" | Fake Epic "New" | — |
| 53 | `account-edit-session-stale.png` | "Saved, but needs a fresh sign-in" + Why row | Fake with `--knobs='{"knobs":{"epic":{"expired":["22222222222222222222222222222222"]}}}'` after a Play on Alt | `handover` |
| 54 | `account-edit-session-unlinked.png` | Signed-in-as "An account not yet linked in One Launch" | Fake `unowned` scenario, an Epic account page | `unowned` |
| 55 | `account-edit-session-forget-confirm.png` | "Yes, forget it" / Cancel | Press Forget saved sign-in | — |
| 56 | `account-edit-riot-stay-signed-in.png` | Last sign-in row naming “Stay signed in” | Fake Riot with `--knobs='{"knobs":{"riot":{"staySignedIn":false,"signInWhenAskedMs":500}}}'`, Play on "Riot New", then open it | — |
| 57 | `account-edit-switch-preview.png` | Advanced › Switch preview rendered (pills, steps with before→after, footer) | Expand Advanced, press Preview | `views` |
| 58 | `account-edit-switch-preview-credentials.png` | Preview with "Needs credentials" pill and warnings | Preview on an account without a saved sign-in (fake Epic "New") | — |
| 59 | `account-edit-delete-confirm.png` | Danger zone two-step `Yes, delete “<name>”` | Press Delete account | — |

## Import accounts dialog

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 60 | `import-steam-list.png` | Steam import with pills (Signed in, Already in One Launch, Session may have expired) and "n selected" | Import › From Steam (fake) | `import` |
| 61 | `import-battlenet-list.png` | Battle.net import list | Import › From Battle.net (fake) | `import` |
| 62 | `import-loading.png` | "Reading loginusers.vdf…" | Capture within the first frames | — |
| 63 | `import-empty.png` | "Steam has not remembered any account on this PC yet." | Fake with an empty `loginusers.vdf` in the scenario copy | — |
| 64 | `import-all-linked.png` | "Everything here is already in One Launch." | Import twice | `import` |
| 65 | `import-error.png` | "Could not read Steam's remembered accounts: <message>" | Real app without Steam installed: Import › From Steam | — |

## Launch dialog and banner

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 66 | `launch-pick.png` `+960` | Picker with accounts, Play disabled until one is chosen | Card menu › Play as… (fake) | `play-steam` |
| 67 | `launch-pick-switch-only.png` | "Switch account" title, "Switch <launcher> to <account>" subtitle | Account page › Switch to this account | `restore` |
| 68 | `launch-pick-update-note.png` | Update warning box above the picker (Battle.net / Riot wording) | `needs-update` knobs, Play as… on Overwatch | `needs-update` |
| 69 | `launch-empty-steam.png` | "No Steam accounts yet. Import the ones Steam remembers…" with "Import Steam accounts" | Delete the Steam accounts, Play a Steam game | — |
| 70 | `launch-empty-riot.png` | "No Riot Games accounts yet." with "Add Riot Games account" | Same for Riot | — |
| 71 | `launch-checking.png` | "Checking Steam…", primary "Checking…" | Capture right after Play (fake with `reactMs` raised) | `play-*` |
| 72 | `launch-confirm-close.png` | Confirm stage with the CloseBox ("… is running. Switching closes it." / "is then started again as") and "Close Steam and continue" | Play as the non-current account while the fake launcher runs | `play-steam` |
| 73 | `launch-confirm-warnings.png` | Confirm stage with plan warnings and notes, "Continue" | Fake `unowned` (plan warns about the unowned sign-in) | `unowned` |
| 74 | `launch-confirm-details-open.png` | The "Details" expander open showing the full plan | Expand Details | — |
| 75 | `launch-preview.png` | Preview-mode stage: "Preview mode is on, so nothing happened…" with phase list and "Turn preview off and play now" | Preview mode on, Play | `preview` |
| 76 | `launch-blocked.png` | Blocked stage (danger box, only Back) | Fake Epic with `GameUserSettings.ini` removed from the scenario copy, Play an Epic game | — |
| 77 | `launch-running.png` | Running stage: five phases, current step title, "Working…", Stop / Hide | During a fake switch | `play-all` |
| 78 | `launch-running-details.png` | "Show details" expanded: raw steps with status icons and monospace detail | Same, expand | `play-all` |
| 79 | `launch-handover-steam.png` | Hand-over panel: Steam Guard title, message, numbered steps with “Remember me” bold, Copy login / Copy password / Stop waiting | `handover` scenario knobs (Steam `loginVerdict` guard) | `handover` |
| 80 | `launch-handover-battlenet.png` | Hand-over for Battle.net (“Switch account”, “Keep me logged in”) | Fake Battle.net with `rejected` knob | `handover` |
| 81 | `launch-handover-riot.png` | Hand-over for Riot (“Stay signed in”) | Fake Riot with `rejected` knob | `play-riot` variant |
| 82 | `launch-handover-copied.png` | Clipboard note beside the hand-over buttons | Press Copy login | `handover` |
| 83 | `launch-stopping.png` | "Stopping…" after Stop | Press Stop mid-switch | `stop` |
| 84 | `launch-done-launched.png` | "Launched as <account>" success pill, Done | Complete a fake Play | `play-steam` |
| 85 | `launch-done-switched.png` | "Switched to <account>" (plain switch) | Switch to this account (fake) | `restore` |
| 86 | `launch-done-handoff-riot.png` | "Switched to <account>" plus the Riot hand-off box ("Press Play in the Riot Client window…") | Settings › Launchers › "Start Riot games for you" off, Play VALORANT (fake) | `play-riot` |
| 87 | `launch-needs-sign-in.png` | Warning box after the manual wait timed out, stoppedNote, Try again / Done | `handover` with a short `manualSignInMs` timing knob | `handover` |
| 88 | `launch-cancelled-before-writes.png` | Cancelled: "Nothing had been changed." note | Stop during Close | `stop` |
| 89 | `launch-cancelled-restored.png` | Cancelled with "<Client> was put back the way it was and left closed." | Stop between writes and start (pipeline-only; may be unreachable by hand) | — |
| 90 | `launch-cancelled-after-start.png` | Cancelled with "Nothing was rolled back. Once <client> is signed in…" | Stop after the launcher started | `stop` |
| 91 | `launch-failed-restored.png` | Failed, rollback succeeded ("…put back the way it was and left closed. Start it to check…") | `rollback` knobs (Steam never comes back) | `rollback` |
| 92 | `launch-failed-restore-failed.png` | Failed, "could not be put back automatically. Open Settings › Backups…" | Needs a failing restore; may be unreachable by hand (document if skipped) | — |
| 93 | `launch-failed-launch-only.png` | Failed: "<Client> is signed in as <account>; only the game launch failed." | `needs-update` knobs (Battle.net refuses the launch) | `needs-update` |
| 94 | `launch-failed-nothing-changed.png` | Failed: "Nothing had been changed yet." (refusal before writes) | Fake elevated launcher knob | `elevated` |
| 95 | `launch-error.png` | Error stage (role=alert box, Back) | Start two launches at once (BUSY), or a preflight error | — |
| 96 | `launch-game-running-riot.png` | GAME_RUNNING refusal for Riot ("…One Launch never closes a Riot game") | Riot game process knob, Play VALORANT | `game-running` (Battle.net) / knob |
| 97 | `banner-running.png` | Banner "Switching to <account> and launching <game>…" with Show | Press Hide during a switch | `play-all` |
| 98 | `banner-attention.png` | Banner "Waiting for you to sign in as <account>" | Hide during a hand-over | `handover` |
| 99 | `banner-done.png` | Banner "Launched <game> as <account>" (clears after 8 s) | Direct instant Play from a poster | `already-active` |
| 100 | `banner-failed.png` | Banner "The switch to <account> failed" with Dismiss | Hide, let a rollback finish | `rollback` |

## Confirm, recovery, setup guide, What's New

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 101 | `confirm-dialog.png` | ConfirmDialog (Leave / Stay) | Any leave guard (see 41, 46) | — |
| 102 | `recovery-writing.png` | "One Launch was closed during a switch" / writing phase, Restore recommended | Kill the app mid-switch (fake) and restart; `killHard` in the scenario | `recovery` |
| 103 | `recovery-signing-in.png` | Signing-in phase wording, Keep as is recommended | Kill during the sign-in wait, restart | `recovery` |
| 104 | `recovery-rollback-failed.png` | "The last switch could not be undone" | Needs a failed rollback; may be unreachable by hand | — |
| 105 | `recovery-no-snapshot.png` | "…The snapshot taken before that switch is gone…" with OK | Delete the snapshot folder from the scratch data folder before restarting | — |
| 106 | `recovery-restored.png` | "<launcher> was put back the way it was" / Done | Press Restore snapshot | `recovery` |
| 107 | `recovery-restore-error.png` | Restore failed, "Try the restore again" | Restore while the fake launcher runs | `recovery` |
| 108 | `setup-welcome.png` `+960` | Step 1 Welcome | `dev:fake -- --fake=none` (fresh folder) | `first-run` |
| 109 | `setup-launchers-none.png` | Step 2 with four "Not found" rows and Locate… | Same | `first-run` |
| 110 | `setup-launchers-found.png` | Step 2 with Found rows and the games count | Real app, Settings › General › Show again | — |
| 111 | `setup-launchers-loading.png` | Skeleton rows / "Looking for installed games" | First frames of step 2 | — |
| 112 | `setup-play.png` | Step 3 How Play works with the phase demo | Continue | `first-run` |
| 113 | `setup-play-preview-on.png` | Step 3 with the Preview mode sentence | Preview mode on first | — |
| 114 | `setup-accounts-import.png` | Step 4 with remembered-account rows and "Import n and continue" | Real app (Steam remembered accounts), Show again | — |
| 115 | `setup-accounts-none.png` | Step 4 "No launcher was found… nothing to import yet" and Add by hand chips | `--fake=none` | `first-run` |
| 116 | `setup-accounts-by-hand-selected.png` | A by-hand chip pressed | Click a chip | `first-run` |
| 117 | `setup-ready-all-set.png` | Step 5 "You're all set" with stat tiles | Real app with accounts | — |
| 118 | `setup-ready-almost.png` | Step 5 "Almost there" with "Add a <launcher> account" | `--fake=none`, chip chosen | `first-run` |
| 119 | `whats-new.png` `+960` | What's New card for the current entry | Settings › General › What's new › Show | `whats-new` |
| 120 | `whats-new-scrolled.png` | Card scrolled, scrollbar visible | Scroll | — |

## Settings

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 121 | `settings-general.png` `+960` | General tab: General panel + Updates panel | Settings | `views` |
| 122 | `settings-general-autostart-disabled.png` | Start with Windows disabled (dev / custom data folder) | Any dev or fake run | `views --fake` |
| 123 | `settings-general-autostart-on.png` | Start with Windows on | Installed app, toggle on | `start-with-windows` (fake login item) |
| 124 | `settings-updates-idle.png` | Version row "Not checked yet", Check for updates | Installed app before the first check | `update` |
| 125 | `settings-updates-checking.png` | "Checking…" | Press Check for updates | `update` |
| 126 | `settings-updates-up-to-date.png` | "Up to date", "Last checked at <time>." | Installed app, current version | — |
| 127 | `settings-updates-downloading.png` | "Downloading <version> · n%" | Loopback feed scenario | `update` |
| 128 | `settings-updates-ready.png` | "<version> is ready", Restart to update | Loopback feed scenario | `update` |
| 129 | `settings-updates-failed.png` | "Check failed" + alert line | Offline, Check for updates (installed app) | `update` (503 case) |
| 130 | `settings-updates-unavailable-dev.png` | "Off for this copy" + "Updates work only in the installed app." | Dev run | `views` |
| 131 | `settings-updates-unavailable-custom-folder.png` | "Updates are off while One Launch runs from a custom data folder." | Installed exe with `--data-dir=` | `packaged-smoke` |
| 132 | `settings-launchers.png` `+960` | Launchers panel rows (health pills, Running/Not running) + Play panel | Settings › Launchers | `views` |
| 133 | `settings-launchers-skeleton.png` | Four skeleton rows before the first answer | First frames | — |
| 134 | `settings-launchers-row-expanded.png` | Row expanded: location line, Locate…/Reset, health checks, "Checked <time>" | Expand a row | `views` |
| 135 | `settings-launchers-not-installed.png` | "Not installed" rows with Locate… | `--fake=none` or real PC without a launcher | `first-run` |
| 136 | `settings-launchers-needs-attention.png` | "Needs attention" pill and message | Needs an adapter throw; fake Battle.net with a UTF-16 config in the scenario copy | — |
| 137 | `settings-launchers-chosen-folder.png` | "You chose this folder." with Reset | After Locate… (real app, pick the real exe) | — |
| 138 | `settings-launchers-locate-error.png` | NOT_A_LAUNCHER error under the row | Locate… and pick a wrong exe (real app) | — |
| 139 | `settings-play-hide-disabled.png` | "Hide One Launch after Play" off and disabled ("Needs Minimize One Launch to System Tray…") | General › tray toggle off, then Launchers | `close-to-tray` |
| 140 | `settings-artwork-no-key.png` `+960` | Artwork tab without a key: Get a key, paste field | Settings › Artwork | `views` |
| 141 | `settings-artwork-key-stored.png` | Masked key field + Remove key, "Key saved." | Paste a 20+ character key (fake run) | — |
| 142 | `settings-artwork-key-invalid.png` | "That does not look like a SteamGridDB API key…" | Type "abc", Enter | — |
| 143 | `settings-artwork-queue-busy.png` | Queue row "n in progress · …" | Refresh all (real app) | — |
| 144 | `settings-artwork-rescan-message.png` | "Library rescanned: n added, n removed." | Rescan | — |
| 145 | `settings-backups.png` `+960` | Backups tab with per-launcher sections, Snapshots to keep dropdown | Settings › Backups after a fake Play | `restore` |
| 146 | `settings-backups-empty.png` | "No snapshots yet. The first one is taken automatically before the first real switch." | Fresh folder | — |
| 147 | `settings-backups-kept-pinned.png` | A "Kept" badge with the pin reason | Fake `unowned` after a Play | `unowned` |
| 148 | `settings-backups-restore-dialog.png` | Restore dialog ("Restore Steam files from <date>?", file list, "Steam must be closed first…") | Press Restore | `restore` |
| 149 | `settings-backups-restore-refused.png` | Restore dialog error while the launcher runs | Restore while fake Steam runs | `restore` |
| 150 | `settings-backups-restored.png` | Success box "Restored snapshot <id>: n files…" | Restore with the launcher closed | `restore` |
| 151 | `settings-backups-keep-dropdown.png` | Snapshots to keep dropdown open (5/10/20 (default)/50/100) | Click the dropdown | — |
| 152 | `settings-advanced.png` `+960` | Advanced tab: Launching, Encryption, Diagnostics | Settings › Advanced | `views` |
| 153 | `settings-advanced-preview-on.png` | Preview mode toggle on | Toggle | `preview` |
| 154 | `settings-advanced-epic-dropdown.png` | Epic launch link dropdown open | Click | — |
| 155 | `settings-advanced-battlenet-dropdown.png` | Battle.net launch dropdown open | Click | — |
| 156 | `settings-encryption-working.png` | Status "Working", Last test "Passed in n ms" | Run test (real or dev run; the vault works in dev) | `views` |
| 157 | `settings-encryption-key-lost.png` | Status "Key lost" + the danger banner above the tabs ("Your stored passwords and saved sign-ins can no longer be read…") | Copy a scratch data folder's `onelaunch.db` into a fresh folder without its `chromium/` folder, start the app on it | — |
| 158 | `settings-vault-reset-confirm.png` | Banner two-step "Yes, clear them" | Press the banner button | — |
| 159 | `settings-vault-reset-done.png` | "Removed n passwords, n saved sign-ins, n other secrets." | Confirm | — |
| 160 | `settings-encryption-unavailable.png` | "Unavailable · passwords and sign-ins cannot be stored" | Not reproducible on a normal Windows install; skip and note | — |
| 161 | `settings-diagnostics-log-level-dropdown.png` | Log level dropdown open | Click | — |
| 162 | `settings-setting-save-error.png` | "Could not save the setting: <message>" under a row | Needs a failing `settings:set` (e.g. read-only DB); optional | — |
| 163 | `settings-developer-panel.png` | [dev-only] Developer panel with Preview What's New | Dev build, Advanced tab bottom | — |

## Native dialogs, tray, installer (real app)

| # | Filename | State | How to reach | Scenario |
| --- | --- | --- | --- | --- |
| 164 | `native-quit-during-switch.png` | "A switch is still running" message box (Keep running / Quit anyway) | Fake run with Settings › General tray toggle off, start a switch, close the window | `quit-during-switch` (auto-answered; take by hand) |
| 165 | `native-crash-dialog.png` | "One Launch hit an unexpected error" (Relaunch / Keep running) | Dev build: throw from a main-process timer (temporary edit) | — |
| 166 | `native-startup-error.png` | "One Launch could not start" error box | Start the exe on a `--data-dir=` whose `onelaunch.db` is a non-SQLite file | — |
| 167 | `native-file-picker-locate.png` | "Locate steam.exe" open-file dialog with the launcher filter | Settings › Launchers › Locate… | — |
| 168 | `native-file-picker-image.png` | "Choose an image" dialog | Card menu › Choose portrait image… | — |
| 169 | `tray-menu.png` | Tray icon context menu (Open One Launch / Quit One Launch) and tooltip | Right-click the tray icon (installed app) | — |
| 170 | `tray-hidden-window.png` | Window gone after Close with the tray setting on (taskbar + tray visible) | Close the window | `close-to-tray` |
| 171 | `installer-welcome.png`, `installer-license.png`, `installer-install-mode.png`, `installer-folder.png`, `installer-finish.png` | NSIS assisted installer pages, including the licence page and the "Only for me / All users" page | Run `dist/OneLaunch-<version>-setup.exe` (your step; back up the data folder first) | — |
| 172 | `installer-smartscreen.png` | SmartScreen "unknown publisher" prompt | First run of a freshly downloaded installer | — |
| 173 | `windows-startup-apps-entry.png` | Settings › Apps › Startup showing "OneLaunch" | After Start with Windows on | — |
| 174 | `uninstaller.png` | Uninstall confirmation | Apps & features › One Launch › Uninstall (data folder is kept) | — |

## Notes

- Rows marked "may be unreachable by hand" exist in the code but need a failure the simulators do not script; capture them only if a knob or a temporary edit gets you there, otherwise note them as skipped.
- Reduced-motion variants are not listed; if the rewrite needs them, repeat 108 and 119 with Windows' "Animation effects" off (the guide's loops freeze, entrances disappear).
- The `views` scenario's own shots (`library`, `accounts`, `account`, `settings-*` at both sizes) are a good baseline set and already follow the 1280/960 pairing.
