# Known gaps

What is known to be missing, unchecked or unlike the Electron app, and the slice of `LEAN.md` that settles it. Remove a line when it is settled.

## Library (slice 2)

- The window still shows placeholder cards. It does not read the data folder yet.
- `ol-ui` may depend on `ol-api` only, so `ol-api` has to open the store on the backend thread and hand over the games and accounts. `Store` is `Send`, not `Sync`.
- The Library's default sort is last played. `installed_games()` comes sorted by name; the sort and its stored preference are the UI's.
- Posters are generated. Real artwork has to be decoded off the UI thread. `GameArtwork.grid` is relative to `<data>\artwork`; check the `<game id>/<kind>.<ext>` shape before joining it.
- Ctrl+F does not focus the search field.
- The search field has no accessible name or role.
- The grid has no draggable scrollbar.

## Data (slices 2 and 6)

- Neither build has read a vault key the other wrapped. The Rust vault is checked against the recorded vectors in `oracle/takeover/vault/` and against a key of its own under real DPAPI. The owner's `--doctor` run on a copy of real data settles reading; `oracle/takeover/vault/make-local-fixture.mjs` would settle both directions.
- A vault key that does not unwrap is never replaced. The "clear stored secrets" flow has to decide what happens then.
- Two first starts at once on a fresh folder can each write a vault key. The single-instance lock closes this.
- Not done at start: creating the data folder with `artwork`, `backups` and `logs`; writing `whatsNew.lastSeen` on a fresh database (`MigrationReport.from == 0`); the backup copy after first paint (`Store::backup` exists); storing the vault probe when it is absent.
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
- Artwork comes back as the stored relative path, not as an `onelaunch-art://` URL.
- `PRAGMA temp_store = MEMORY` on every open, so rows cannot spill into `%TEMP%`.

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
