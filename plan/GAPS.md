# Known gaps

What is known to be missing, unchecked or unlike the Electron app, and the slice of `LEAN.md` that settles it. Remove a line when it is settled.

## Library (slices 3 and 5)

- A click on a card, its Play mark, its More button, Rescan and Scan now do nothing. There is no launch dialog, no card menu and no scan yet.
- No Recent strip: nothing reads the last launches (`launch:recent`, the newest 8) from `launch_history`.
- The empty Library always says "No games found". Nothing looks for launchers, so "Scanning your launchers…" and "No launchers found" never show.
- The Library is read once, at start. Nothing reloads it, because nothing changes it yet; a scan, an artwork fetch or a launch will have to.
- A database that cannot be opened shows its message in place of the grid ("Could not load the library: …") and the app keeps running. The Electron app shows the box "One Launch could not start", with the log and the data folder, and exits with 1.
- The Filter and Sort menus take the mouse and Escape only: no arrow keys, Home, End or Enter, and they stay open while the window is resized.
- Names sort by their lower-cased characters. The Electron app sorts them by the rules of the system language; `triage.toml` X-02 asks for the order of the Windows regional settings, through a native call.
- A poster appears at once, without the 200 ms fade, and the Update badge and the More button have no blur behind them.
- A poster is decoded into a box of 240 by 360 px times the display scale, whatever the card's size.
- The title bar still carries the badge GPUI PROOF, and the window title says so too.
- The search field has no accessible name or role.
- The grid has no draggable scrollbar.

## Data (slices 2 and 6)

- Neither build has read a vault key the other wrapped. The Rust vault is checked against the recorded vectors in `oracle/takeover/vault/` and against a key of its own under real DPAPI. The owner's `--doctor` run on a copy of real data settles reading; `oracle/takeover/vault/make-local-fixture.mjs` would settle both directions.
- A vault key that does not unwrap is never replaced. The "clear stored secrets" flow has to decide what happens then.
- Two first starts at once on a fresh folder can each write a vault key. The single-instance lock closes this.
- Not done at start: opening the vault, and storing the vault probe when it is absent.
- Nothing is written to `logs\` yet. `database copied` and `database copy failed` go to the console; `starting` and `database migrated` are not written at all.
- `ol-api` hands over the accounts (`Backend::accounts`), and no screen asks for them yet.
- The check that refuses the real data folder does not look at links inside an allowed folder: `onelaunch.db` as a hard link to the real database, or `chromium` as a junction into the real folder. A mapped network drive and a `subst` drive were not tried.
- That check and its plain-path rule go away in the installed build (slice 6), which runs on the real folder.
- A release build has no console, so `--doctor` there needs its output redirected to a file.
- `Secret::expose()` returns the text, and the fields of `Account`, `Game` and `Settings` are public. Their `Debug` output is redacted, but a caller can still format a field. Look at every such call in the security review.
- `oracle/fixtures/README.md` reads as 2 accounts with passwords; the fixture database holds 3. The tests assert 3. The README is the owner's to correct.

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

## Tray (slice 5)

- The tray listens for raw mouse messages. Listen for `NIN_SELECT` and `NIN_KEYSELECT` instead, and change the probe to match.
- After a cancelled tray menu the keyboard focus does not go back to the tray (`NIM_SETFOCUS`).
- A close request that arrives while GPUI is in the middle of an update closes the window instead of hiding it.
- The window handle is found by GPUI's class name `Zed::Window`. Check it when the GPUI pin moves.

## All screens (slice 5)

- Accessible names and roles.

## Performance (slice 7)

- Idle CPU with the window shown is about 1 % of a core, against 0.11 % for the Electron app. GPUI redraws every window on each compositor tick (`gpui-pre-windows`, `platform.rs`). Lowering it means patching or vendoring GPUI.

## Only the owner can check

- The snap flyout, mouse drags and resizes, real tray clicks, the clipboard keys, another display scale.
- `--doctor` on a copy of the real data folder.
