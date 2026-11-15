# Known gaps

What is known to be missing, unchecked or unlike the Electron app, and the slice of `LEAN.md` that settles it. Remove a line when it is settled.

## Library (slices 3 and 5)

- The More button of a card and a right-click on it do nothing: there is no card menu ("Play as…", "Refresh artwork", "Choose portrait image…").
- The Recent strip has no edge fades and no scrollbar; past four cards it scrolls sideways with a touchpad or a tilted wheel only.
- "No launchers found" comes from the last scan alone. Nothing reports a launcher's status (`launchers:status`), so a start whose scan fails says "No games found".
- The scan a start runs is asked for by the window, after the Library was first read. The Electron app's main process runs it; the result on screen is the same.
- Nothing reloads the Library after an artwork fetch, because nothing fetches artwork yet. A scan and a launch do.
- Not seen in a capture, because each lasts a few milliseconds on fake launchers: "Scanning your launchers…" with its disabled button, "Scanning…" on Rescan, and the turning Play mark of a one-click Play.
- A database that cannot be opened shows its message in place of the grid ("Could not load the library: …") and the app keeps running. The Electron app shows the box "One Launch could not start", with the log and the data folder, and exits with 1.
- The Filter and Sort menus take the mouse and Escape only: no arrow keys, Home, End or Enter, and they stay open while the window is resized.
- Names sort by their lower-cased characters. The Electron app sorts them by the rules of the system language; `triage.toml` X-02 asks for the order of the Windows regional settings, through a native call.
- A poster appears at once, without the 200 ms fade, and the Update badge and the More button have no blur behind them.
- A poster is decoded into a box of 240 by 360 px times the display scale, whatever the card's size.
- The search field has no accessible name or role.
- The grid has no draggable scrollbar.

## Launch dialog, banner and recovery dialog (slices 3 to 5)

What is there: a click on a card or on an entry of the Recent strip opens the Play flow (the account list, the check, the confirm stage with the plan behind "Details", preview mode, the running job with its phases and steps, the hand-over panel, every end of a job), the banner stands for the job while the dialog is closed, and a start asks about a switch the last run did not finish. All of it on fake launchers only: see Steam below.

- Epic Games, Battle.net and Riot Games have no adapter. A game of theirs opens the dialog, which says that the launcher is not available in this build yet. In the app without the feature `fake` every game does, because that build reaches no launcher at all.
- Without a way to reach the launcher of an unfinished switch, nothing is asked at start and the journal stays where it is. The app without the feature `fake` therefore never asks.
- The hand-over panel has no "Copy login" and no "Copy password": the copy to the clipboard is not built (`accounts:copy`).
- An empty account list says "No Steam accounts yet." and offers Close. "Import Steam accounts" and "Add … account" wait for the import dialog and the Accounts screen, and so does the sentence that points to them.
- A switch without a game ("Switch account", from an account's page) has no way in.
- The dialogs take the mouse, Enter and Escape. Enter presses the button drawn with the ring. There is no Tab order, no focus trap, and the keyboard does not go back to what opened the dialog.
- No tooltips: the launcher's game id on the badge, Stop, Hide, an entry of the Recent strip.
- The text of a plan and of a step's detail cannot be selected or copied.
- The plan behind "Details" ends with "This is a preview: nothing was changed." and leaves out "Generated <date and time>": the app has no way to the local time zone yet.
- Preview mode can be turned off from the dialog. The sidebar's "Preview mode is on" card and the setting itself wait for the Settings screen.
- No line for a screen reader says what the job is doing.
- Not seen in a capture, because each lasts a few milliseconds on fake launchers: "Checking Steam…" and "Stopping…". The blocked stage has no shot either: no plan of Steam's is ever one that cannot run.

## Data (slices 2 and 6)

- Neither build has read a vault key the other wrapped. The Rust vault is checked against the recorded vectors in `oracle/takeover/vault/` and against a key of its own under real DPAPI. The owner's `--doctor` run on a copy of real data settles reading; `oracle/takeover/vault/make-local-fixture.mjs` would settle both directions.
- A vault key that does not unwrap is never replaced. The "clear stored secrets" flow has to decide what happens then.
- Two first starts at once on a fresh folder can each write a vault key. The single-instance lock closes this.
- Not done at start: opening the vault, and storing the vault probe when it is absent.
- Nothing is written to `logs\` yet. `database copied` and `database copy failed` go to the console; `starting` and `database migrated` are not written at all.
- The check that refuses the real data folder does not look at links inside an allowed folder: `onelaunch.db` as a hard link to the real database, or `chromium` as a junction into the real folder. A mapped network drive and a `subst` drive were not tried.
- That check and its plain-path rule go away in the installed build (slice 6), which runs on the real folder.
- A release build has no console, so `--doctor` there needs its output redirected to a file.
- `Secret::expose()` returns the text, and the fields of `Account`, `Game` and `Settings` are public. Their `Debug` output is redacted, but a caller can still format a field. Look at every such call in the security review.
- `oracle/fixtures/README.md` reads as 2 accounts with passwords; the fixture database holds 3. The tests assert 3. The README is the owner's to correct.

## Steam (slice 3)

What is there: on fake launchers the backend finds Steam, scans its games into the Library, lists and imports the accounts it remembers, plans and runs a switch (snapshot, journal, the writes, rollback, restore, recovery of an interrupted switch) and starts a game. Proven against the oracle: the 21 files of `oracle/l1/steam` and the two path files of `oracle/l1/core` (1,468 of 1,482 cases; the 14 others hold half a surrogate pair, a fraction or a number where the Rust type takes only text or a whole number); nine scripts of `oracle/l2` and the Steam half of a tenth; five of the Steam adapter's goldens in `oracle/tests`.

- Nothing reaches a real launcher. The registry, process, shell and clock ports have only their fakes (`ol-platform-fake`), and the file port only `ScratchFiles`, which refuses every path outside the folders it was given. `Backend::start`, which the app uses, has no launchers: every launcher command answers `NOT_IMPLEMENTED` and a scan finds nothing. The real ports in `ol-platform-win` wait for the owner's go-ahead, and the first real switch is the owner's.
- The app reaches the fakes only when built with the feature `fake` and started with `--fake-launchers=<folder>`. The fake machine's file port reaches that folder and `backups\` of the data folder, and nothing else. The folder is held to the plain-path rule of the data folder, and a scenario is laid down only in an empty or a new folder. Refused are a folder that holds anything but a scenario's own files and folders, and one whose `steam\steam.exe` is not an empty file. A folder that passes both is taken at its word.
- A fake machine in the app keeps real time (`FakeClock::following_the_system`), so the age of a remembered session is counted from today: from 2026-11-10 on, `user_1` of the fixtures is past 60 days and every plan for it carries the warning about an expired session, which the Electron app's shots of 2026-11-06 do not show.
- Of a scenario's seed the accounts and the settings are applied. Its saved sign-ins (`sessions`) are not: no launcher that has them is built.
- Still without an interface: the account import and the Backups tab.
- The replay compares the Steam part of a recorded run: every platform event, the job events, and of the final state the accounts, Steam's games, the launch history, the settings, the journal, the backups, `loginusers.vdf` and Steam's registry values. Not compared, because nothing builds them yet: the other launchers' games, files, registry values and saved sign-ins, artwork, the vault probe and the log lines.
- Not replayed: `play-all`, `play-epic`, `play-battlenet`, `play-riot`, `game-running`, `needs-update`, `riot-stay-signed-in`, `unowned`, `wrong-account`, `recovery` and `quit-during-switch` (another launcher's), `first-run` (launcher status), and the Epic half of `handover`. The replay has no `crash` step: a thread cannot be ended the way a process can.
- `oracle/tests/goldens`: of the 74 Steam adapter goldens only the five dry-run plans are checked. The 69 others (real switches, preflights, restores, health checks) and the pipeline goldens need the set-up of each Electron test rebuilt.
- Not built of the Steam adapter: `installFromExe` (Locate), the health check and `launchers:status`.
- Not built around it: the rules of `settings:set` (`Backend::set_setting` stores what it is given), changing or deleting an account and its password, the copy to the clipboard (`accounts:copy`), the artwork queue after a scan, the `instant` flag of `launch:start` that lets the window hide, and the prompt when the app quits during a switch (`Backend::shut_down` stops the switch and waits for it).
- Nothing is logged: the Electron app writes a line for each start, refusal, failure and rollback of a switch, with account names masked.
- A snapshot file is always stored as it is. The encrypted form (`<relative>.enc`, for Epic's and Riot's files) and pinned snapshots are neither written nor read.
- The launch job runs on a thread of its own and the adapter is plain blocking code over the ports, where `architecture.md` planned one thread and an executor. The owner has not decided whether to keep it.
- Launcher-owned writes are the four functions at the top of `crates/ol-services/src/switch.rs`. Nothing but review keeps another module from asking a writer port for the same.
## Differences from the Electron app, on purpose

None is visible on data the app wrote itself.

- A stored value shorter than 31 bytes, the empty one included, is `VAULT_UNREADABLE`. So is a decrypted value that is not UTF-8.
- Integer settings parse strictly: `35px` is not 35.
- A non-text value in a text column fails the read and names the column.
- Artwork comes back as the stored relative path, not as an `onelaunch-art://` URL. The Library gets the file's path, joined only when the stored one has the shape `<game id>/<kind>.<ext>`.
- `PRAGMA temp_store = MEMORY` on every open, so rows cannot spill into `%TEMP%`.
- The backup copy just made is never pruned. The Electron app prunes by name alone, so after the clock is set back it deletes the copy it has just taken.
- The Library's sort is the row `library.sort` of the settings table (`name` or `lastPlayed`), as `triage.toml` X-01 decides. The Electron app keeps the choice in its page's storage and never reads that row; the choice made there is not taken over.
- A last-played time that is not a full timestamp (`2026-01-02T03:04:05.678Z`) leaves the card's line without a time.
- A blank character outside a string that is none of space, tab, carriage return, line feed and byte order mark (a form feed, a no-break space) is skipped in a VDF file. The Electron app never comes back from such a file.
- A number written `-0` in a Steam file is 0.
- A `Timestamp` in `loginusers.vdf` or a `LastPlayed` in an app manifest outside the years 0 to 9999 reads as no time. The Electron app fails the whole account list or the whole scan on one.
- A game folder a manifest names relative to a library folder that is itself not a full path is taken as `steamapps\common`. The Electron app resolves it against the folder it runs in.
- The games a scan finds are ordered by their lower-cased names, as the Library is.
- A journal whose `startedAt` is not a full timestamp is unreadable and moved aside. The Electron app takes any text its date parser takes.
- A snapshot manifest whose `bytes` is not a whole number from 0 up is unreadable.
- A job is reported as finished once its launch is in `launch_history` and the account's last use is stored. The Electron app reports it first.
- The settings a switch depends on (`steam.path`, `steam.autoLogin`, `backups.keep`) are read when the job starts. The Electron app reads each at the moment it is used.
- The fake process table records a password argument as `<password>`. The Electron app's fake report, a development file, holds it in the clear.
- A launch dialog opened while a job runs shows that job under its own game's name. The Electron app shows the running job under the name of the game that was clicked.
- A double-click on a card is one click: its second click neither plays a second time nor closes the dialog the first one opened.
- A fake run writes no `fake-platform.json` into the data folder. The Electron app's report of what its fake platform was asked to do is a development file.
- The fake launchers are named by the arguments `--fake-launchers`, `--fake-scenario` and `--fake-fixtures`, and the app itself lays the scenario down. The Electron app reads `ONELAUNCH_FAKE_LAUNCHERS` and leaves the laying down to its driver script.

## Tray (slice 5)

- The tray listens for raw mouse messages. Listen for `NIN_SELECT` and `NIN_KEYSELECT` instead, and change the probe to match.
- After a cancelled tray menu the keyboard focus does not go back to the tray (`NIM_SETFOCUS`).
- A close request that arrives while GPUI is in the middle of an update closes the window instead of hiding it.
- The window handle is found by GPUI's class name `Zed::Window`. Check it when the GPUI pin moves.

## All screens (slice 5)

- Accessible names and roles.
- The app's own yes or no question (the confirm dialog, "Leave" and "Stay") is not built. Only the account forms and the restart for an update ask it; nothing in the Play flow does.

## Performance (slice 7)

- Idle CPU with the window shown is about 1 % of a core, against 0.11 % for the Electron app. GPUI redraws every window on each compositor tick (`gpui-pre-windows`, `platform.rs`). Lowering it means patching or vendoring GPUI.

## Only the owner can check

- The snap flyout, mouse drags and resizes, real tray clicks, the clipboard keys, another display scale.
- With the mouse, which the capture steps cannot stand in for: the wheel over a long plan in the launch dialog and over the Recent strip, and a double-click on a card.
- `--doctor` on a copy of the real data folder.
- One real Steam switch, once the real ports exist.
