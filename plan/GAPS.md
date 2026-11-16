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

What is there: a click on a card or on an entry of the Recent strip opens the Play flow (the account list, the check, the confirm stage with the plan behind "Details", preview mode, the running job with its phases and steps, the hand-over panel, every end of a job), the banner stands for the job while the dialog is closed, and a start asks about a switch the last run did not finish. All of it on fake launchers only: see Steam, Epic and Battle.net below.

- Riot Games has no adapter. A game of Riot's opens the dialog, which says that the launcher is not available in this build yet. In the app without the feature `fake` every game does, because that build reaches no launcher at all.
- The Play flow of an Epic game was not run in the window. `ol-ui` was not changed for Epic: it asks the backend the same questions as for Steam, and the backend's answers are what the replays and the tests of `ol-api` hold. The owner's check is to play Rocket League® as Alt on fake launchers.
- The Play flow of a Battle.net game was run in the window on fake launchers, in captures: Rescan shows Overwatch, a click on it lists the three accounts, Play as Alt ends in "Launched as Alt", and the details name each step with no token in them. `ol-ui` was not changed for Battle.net. Not seen in the window: the confirm stage for a client that runs, the hand-over of an account without a saved token, and a game the client refuses.
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

What is there: on fake launchers the backend finds Steam, scans its games into the Library, lists and imports the accounts it remembers, plans and runs a switch (snapshot, journal, the writes, rollback, restore, recovery of an interrupted switch) and starts a game. Proven against the oracle: the 21 files of `oracle/l1/steam` and the two path files of `oracle/l1/core` (1,468 of 1,482 cases; the 14 others hold half a surrogate pair, a fraction or a number where the Rust type takes only text or a whole number); eight scripts of `oracle/l2` that turn on Steam alone, and the Steam jobs of `import` and `handover`; five of the Steam adapter's goldens in `oracle/tests`.

- Nothing reaches a real launcher. The registry, process, shell and clock ports have only their fakes (`ol-platform-fake`), and the file port only `ScratchFiles`, which refuses every path outside the folders it was given. `Backend::start`, which the app uses, has no launchers: every launcher command answers `NOT_IMPLEMENTED` and a scan finds nothing. The real ports in `ol-platform-win` wait for the owner's go-ahead, and the first real switch is the owner's.
- The app reaches the fakes only when built with the feature `fake` and started with `--fake-launchers=<folder>`. The fake machine's file port reaches that folder and `backups\` of the data folder, and nothing else. The folder is held to the plain-path rule of the data folder, and a scenario is laid down only in an empty or a new folder. Refused are a folder that holds anything but a scenario's own files and folders, and one whose `steam\steam.exe`, `EpicGamesLauncher.exe`, `Battle.net.exe` or `Battle.net Launcher.exe` is not an empty file. A folder that passes both is taken at its word.
- A fake machine in the app keeps real time (`FakeClock::following_the_system`), so the age of a remembered session is counted from today: from 2026-11-10 on, `user_1` of the fixtures is past 60 days and every plan for it carries the warning about an expired session, which the Electron app's shots of 2026-11-06 do not show.
- Of a scenario's seed the accounts, the settings and Epic's and Battle.net's saved sign-ins are applied. Riot's are not: Riot is not built.
- Still without an interface: the account import and the Backups tab.
- The replay compares the Steam, Epic and Battle.net parts of a recorded run (`ol_replay::check`): every platform event, the job events, and of the final state the accounts, the three launchers' games and saved sign-ins (each by the digest of what the vault sealed), the launch history, the settings, the journal, the backups (a Battle.net snapshot's tokens by the digest of what was sealed), `loginusers.vdf`, `GameUserSettings.ini`, `Battle.net.config` and every registry value but Riot's. Not compared, because nothing builds them yet: Riot's games, files, registry values and saved sign-ins, artwork, the vault probe, the log lines and the copies to the clipboard.
- Not replayed: `play-riot` and `riot-stay-signed-in`. `play-all`, `unowned` and `needs-update` are replayed without the calls that name a Riot account, game or snapshot: they are held to every event outside their Riot job, and to the final state without Riot's accounts and launches. `first-run` is replayed with its two reads of `launchers:status` and its read of `artwork:status` passed over: neither command exists, and a read leaves nothing behind.
- A `crash` step of the replay does not end a process: it leaves the switch's thread standing at the wait it is in, for good, and starts the app again beside it. What is on disk is what a crash would leave; the first database connection stays open until the test ends.
- `oracle/tests/goldens`: of the 74 Steam adapter goldens only the five dry-run plans are checked. The 69 others (real switches, preflights, restores, health checks) and the pipeline goldens need the set-up of each Electron test rebuilt.
- Not built of the Steam adapter: `installFromExe` (Locate), the health check and `launchers:status`.
- Not built around it: the rules of `settings:set` (`Backend::set_setting` stores what it is given), changing or deleting an account and its password, the copy to the clipboard (`accounts:copy`), the artwork queue after a scan, the `instant` flag of `launch:start` that lets the window hide, and the prompt when the app quits during a switch (`Backend::shut_down` stops the switch and waits for it).
- Nothing is logged: the Electron app writes a line for each start, refusal, failure and rollback of a switch, with account names masked.
- The launch job runs on a thread of its own and the adapter is plain blocking code over the ports, where `architecture.md` planned one thread and an executor. The owner has not decided whether to keep it. Since Epic, that thread asks the database's thread for what a switch needs of the accounts and their saved sign-ins (`StoreThread`), and waits for the answer.
- Launcher-owned writes are the eleven functions at the top of `crates/ol-services/src/switch.rs`: Steam's file and its two registry values, Epic's file, Battle.net's file and the set and the removal of a value under its `UnifiedAuth` key, the restore of each launcher, and the one write of a snapshot's files that the three restores share. Nothing but review keeps another module from asking a writer port for the same.

## Epic (slice 4)

What is there: on fake launchers the backend finds the Epic Games Launcher, scans its games into the Library, lists the accounts that signed in on the PC, says what is saved of an account's sign-in and whose session the launcher holds now (`Backend::session`), plans and runs a switch (the window close, the outgoing session saved, the encrypted snapshot, the write of `GameUserSettings.ini`, the sign-in read from the launcher's log, the hand-over, the new session saved, a wrong account, rollback, restore, recovery) and starts a game. Proven against the oracle: the 13 files of `oracle/l1/epic` (1,010 of 1,044 cases; of the 34 others 10 hold half a surrogate pair, 18 are refusals of text that is not JSON, worded by the JavaScript engine, 4 are nested deeper than 128 levels, 1 holds a number past the largest float and 1 is a session with its keys in another order than the Rust type's); of `oracle/l2`, `play-epic`, `handover`, `wrong-account`, `quit-during-switch` and `recovery` in full, and the Epic jobs of `play-all` and `unowned`, the restore of a pinned snapshot at the end of `unowned` among them.

- A saved sign-in rests in two places: the column `blob_enc` of `launcher_sessions`, sealed by the vault, and a snapshot's copy of `GameUserSettings.ini`, sealed the same way as `<relative>.enc`. In memory it is wiped after use where it is a value of its own (`Secret`, `EpicSessionBlob`, the JSON around it). The text of the settings file, which holds it once it is written, is not wiped.
- `Backend::seed_session` stores a sign-in as it is given. Only the fake launchers' seed calls it, but it is in every build: the feature `fake` does not gate it.
- Not built: saving the launcher's current session from an account's page (`sessions:capture`), forgetting one (`sessions:forget`), `installFromExe` (Locate), the health check and `launchers:status`.
- The 48 Epic adapter goldens of `oracle/tests/goldens` are not checked.
- The folders `%LOCALAPPDATA%` and `%ProgramData%` come from whoever builds `Launchers` (`EpicMachine`). The fakes put the scenario's in. The default is where the Electron app falls back to when Windows names none, because only `ol-platform-win` reads the environment: the real ports have to put the real ones in.
- Not exercised by any test or recorded run: a launcher that starts itself again after it was closed (the three tries of the shutdown), two settings files at once (`Config\Windows` and `Config\WindowsEditor`), a session that changes at every start (`rotateOnStart`), and a sign-in that fails for a reason other than a refused token.
- Carried over from the Electron app, because `oracle/l1/epic` records it, and against "every other byte left alone" on a file no launcher writes: in a settings file that mixes CRLF and bare LF a rewrite makes every line end CRLF, and when such a file ends in a bare LF the character before it is lost; and a byte that is not UTF-8 in the settings file in use becomes U+FFFD when the file is rewritten. Keeping each line's own end and refusing such a file would differ from the recorded cases on purpose. The owner decides.

## Battle.net (slice 4)

What is there: on fake launchers the backend finds the Battle.net client (the folder chosen in Settings, the Uninstall key, the command of the `battlenet` link, the default folder), scans its games into the Library (a search of the Uninstall key and one small read for each product, the last play from `Battle.net.config`), lists the logins the client saved, says what is saved of an account's sign-in and whose token the registry holds (`Backend::session`), plans and runs a switch (the window close and then the end of the client alone, the outgoing token saved, the snapshot, the registry writes, the rewrite of `Battle.net.config`, the start through `Battle.net Launcher.exe`, the sign-in read from the client's log, the hand-over, the new token saved, a wrong account, rollback, restore, recovery) and starts a game with `--exec=launch`, watching the log for its start and refusing a game the log says needs an update, is not playable or is running. Proven against the oracle: the 24 files of `oracle/l1/battlenet` (1,130 of 1,377 cases; of the 247 others 211 are refusals of text that is not JSON, worded by the JavaScript engine, 18 hold half a surrogate pair, 11 are values the Rust types do not take, 6 ask the product table for a name every JavaScript object has and 1 is nested deeper than 128 levels); of `oracle/l2`, `play-battlenet`, `game-running`, `import` and `first-run` in full, and the Battle.net jobs of `play-all`, `unowned` and `needs-update`; of `oracle/tests/goldens`, the ten dry-run plans and the two preflights that open six of the Battle.net adapter's goldens.

- A switch writes three things and nothing else: the stored token as a binary value under `HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth`, the removal of every other value under that key, and `%APPDATA%\Battle.net\Battle.net.config` with the login moved to the front of `Client.SavedAccountNames`. The file is rewritten only when reading it and writing it back gives its own bytes; otherwise it is left and the switch ends in the hand-over.
- A saved token rests in two places: the column `blob_enc` of `launcher_sessions`, sealed by the vault, and a snapshot's manifest, where `meta.unifiedAuth` holds each value sealed the same way. A snapshot's copy of `Battle.net.config` is in the clear, as the Electron app's is: it holds the saved logins and no token. In memory a token is wiped after use (`TokenValue`, `Secret`). The test `a_saved_token_is_under_the_clients_key_and_encrypted_in_the_data_folder_and_nowhere_else` of `ol-api` holds a switch, its plan, its job, its errors, the fake machine's log and every file of the data folder to that.
- The plan says the token is written with `reg add /t REG_BINARY`, in the Electron app's words, which the goldens hold. How the real registry port writes a binary value is open, and so is whether the sentence then stays.
- Not built: saving the client's current token from an account's page (`sessions:capture`), forgetting one (`sessions:forget`), `installFromExe` (Locate), the health check and `launchers:status`. `parse_aggregate` is held to its recorded cases and nothing calls it: only the health check reads `aggregate.json`.
- Of the 42 Battle.net adapter goldens of `oracle/tests/goldens`, 36 are not checked, and of the six that are, only the plans and preflights they open with: the real switches, restores and launches need the set-up of each Electron test rebuilt.
- The folders `%APPDATA%`, `%LOCALAPPDATA%` and `%ProgramData%` come from whoever builds `Launchers` (`BattlenetMachine`), as Epic's do. The real ports have to put the real ones in.
- Not exercised by any test or recorded run: a client that starts itself again after it was closed (the three tries of the shutdown), one that ignores the window close and is ended, a token the client rejects (the saved sign-in marked stale), a sign-in as the wrong account, a manual sign-in that never comes, a registry write that fails, the `uri` form of a launch, and a game without a launch code.

## Differences from the Electron app, on purpose

None of those about stored values and launcher files is visible on data the app wrote itself. The last four are about the window and the fake launchers.

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

Since Epic. The first is the one a user could meet, and is the owner's to confirm.

- When the vault cannot encrypt, a snapshot of `GameUserSettings.ini` fails and the switch with it, before anything is written. The Electron app stores the copy in the clear and writes a warning to its log.
- A saved sign-in that is not JSON is refused with "Stored Epic session has an unexpected shape", and a manifest that is not JSON with the Rust JSON reader's reason. The Electron app gives the JavaScript engine's reason in both.
- A saved sign-in, an `.item` file or `LauncherInstalled.dat` nested deeper than 128 levels, or holding a number past the largest float, does not read. The Electron app reads them.
- Epic's games are ordered by their lower-cased names, and its `.item` files are read in the order of their lower-cased names, so of two manifests for one app the first by name counts. The Electron app takes the order the folder lists them in.
- An `InstallSize` that is not a whole number counts as no size. Nothing stores or shows a size yet.
- Of two settings files, an older one that is not UTF-8 text (UTF-16, say) is left as it is. The Electron app rewrites it as if it were UTF-8, which breaks it; the launcher does not read that file.
- A relative program path in the command of the `com.epicgames.launcher` link is not resolved against the folder the app runs in: it finds no launcher.
- `epic.path` and `epic.launchUri` are read when the job starts, like the settings of a Steam switch.
- The scripted Epic launcher stamps its log with the fake machine's clock. The Electron app's stamps it with the system's.

Since Battle.net. The first two are the ones a user could meet, each on a file or a snapshot that is already broken, and are the owner's to confirm.

- A `Battle.net.config` that is not UTF-8 text is left as it is: the file step says so and the switch ends in the hand-over, as it does for a layout that cannot be written back. The Electron app reads such a file as UTF-8 and, when the rest of its layout is the usual one, rewrites it with U+FFFD in place of the bytes it could not read. Epic's settings file still carries the Electron app's way (see Epic above): the owner decides whether the two should agree.
- A snapshot whose record of the tokens is not text, or holds a token that is not hexadecimal, is refused before anything is touched ("Snapshot meta has an unexpected shape"). The Electron app takes a record that is not text for no tokens, puts the file back and removes every value under `UnifiedAuth`; and it writes whatever the vault gives back.
- A `Battle.net.config` that is not JSON is named with the Rust JSON reader's reason in the plan's file step and its warning. The Electron app gives the JavaScript engine's reason.
- A `Battle.net.config` or an `aggregate.json` nested deeper than 128 levels does not read. The Electron app reads them.
- A product whose codename is a name every JavaScript object has (`constructor`, `toString`) is not in the product table. In the Electron app the table answers with that member.
- Battle.net's games are ordered by their lower-cased names. The Electron app takes the order the registry search gives them in.
- A saved sign-in whose value name holds half a surrogate pair is refused, and a login or a refusal's reason is never masked through the middle of a pair.
- `battlenet.path` and `battlenet.launch` are read when the job starts, like the settings of a Steam switch.
- The scripted Battle.net client writes `config_key:placeholder` into its log where the Electron app's writes a made-up key of 32 hexadecimal digits, which the secret scan would stop.

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
- One real Steam switch, one real Epic switch and one real Battle.net switch, once the real ports exist. For Battle.net that is also the first time a token written by the registry port is read by the client.
- The Play flow of an Epic game in the window, on fake launchers; and of a Battle.net game with the client running, and as an account without a saved token.
