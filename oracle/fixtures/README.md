# Fixtures

Sample copies of every file format One Launch persists (SPEC.md § 4), plus the launcher-owned files it reads and the external API responses it parses. Everything here is a placeholder: logins under `example.com`, made-up ids and blobs, a reversible XOR transform standing in for DPAPI. Nothing was taken from a real data folder or a real launcher install.

**How they were made.** `_generator/gen-fixtures.test.ts` builds the app-owned fixtures by calling the app's own modules (the real migrations, `AccountsService`, `LibraryService`, `LauncherSessionStore`, `SecretStore`, `SwitchJournal`, `BackupService`, the logger, `signManifest`/`verifyManifest`, `FakeReport`). It runs as a vitest file from the one-launch repo root with `npx vitest run --config rewrite-prep/fixtures/_generator/vitest.config.ts`; it will not run from a copy of this folder because it imports the repo's source. Launcher-owned files and API responses are byte-for-byte copies of the repo's sanitised test fixtures (`test/fixtures/`). Timestamps and UUIDs are whatever the run produced; nothing depends on their values.

**Ciphertext is not DPAPI.** Every `*_enc` column, every `.enc` backup file and the Battle.net token in a manifest's `meta` were produced with `test/helpers/fake-vault.ts` (each byte XOR `0x5a`, then for backups base64 first). Real DPAPI output is bound to one Windows user and would be useless to anyone else; the shape (a BLOB column, a file of raw bytes, a base64 string in JSON) is what matters.

**`.log` files.** The one-launch repo's `.gitignore` hides `*.log` outside `test/fixtures/`, so `git status` in that repo will not list the logs in this folder. They are present on disk.

## `data-folder/` — one realistic `%APPDATA%\OneLaunch`

The layout a user's data folder has after some use (SPEC.md § 4). `chromium/` is a stub: in a real folder it is Chromium's profile and holds the DPAPI-wrapped key in `Local State`.

| Path | What it is | Written by (repo) |
| --- | --- | --- |
| `onelaunch.db` | SQLite, `user_version` 5, single file (WAL checkpointed). 9 accounts across the four launchers (2 with passwords, tags, notes, `last_used_at`), 3 session rows (Epic verified, Battle.net stale with `stale_reason`, Riot whole-file blob), 9 games (one Steam game `installed = 0`, two `needs_update = 1`, several `last_played_at`), 5 artwork rows (sources `steamcdn`, `steamgriddb`, `manual`, `launcher`), 2 `artwork_attempts`, 8 `launch_history` rows (`ok`/`failed`/`cancelled`), 8 settings (incl. `window.bounds`, `steam.path`, `whatsNew.lastSeen`), 3 secrets (`vault.probe`, `steamgriddb.apiKey`, `diagnostics.roundtrip`). | `src/main/db/`, `src/main/services/*` |
| `switch.json` | A journal left by a crash while the launcher was signing in (`stepId: "wait"`, `backupId` naming the Steam snapshot in this folder, `rollbackFailed: false`). The next start would offer recovery in phase `signing-in`. | `src/main/services/switch-journal.ts` |
| `artwork/<game uuid>/{grid,hero,logo}.{jpg,png,webp,ico}` | Cached covers, one folder per game id. Files are minimal headers with the right magic bytes, not real images. The `.ico` is a launcher-icon stand-in (source `launcher`). | `src/main/services/artwork.ts` |
| `backups/steam/<id>/manifest.json` + `config/loginusers.vdf` | Two Steam snapshots: a switch snapshot with `meta {AutoLoginUser, RememberPassword: null}`, and a “Before restoring <id>” snapshot taken with `protect`. | `src/main/services/backup.ts`, `src/main/launchers/steam/adapter.ts` |
| `backups/epic/<id>/manifest.json` + `Config/Windows/GameUserSettings.ini.enc` | Two Epic snapshots: one with two files where the `WindowsEditor` copy did not exist (`existed: false`, `sha256: null`), and one **pinned** (“Holds a sign-in that no One Launch account owns”). Sensitive files are stored encrypted, `sha256` is of the ciphertext. | same |
| `backups/battlenet/<id>/manifest.json` + `Battle.net.config` | `meta.unifiedAuth` is a JSON string `[{name, enc}]` with the token vault-encrypted and base64. | `src/main/launchers/battlenet/adapter.ts` |
| `backups/riot/<id>/manifest.json` + `Data/RiotGamesPrivateSettings.yaml.enc` | The one Riot file, sensitive. | `src/main/launchers/riot/adapter.ts` |
| `backups/db/onelaunch-<iso>.db` | Two online-backup copies of the database (one per start; the app keeps the newest 5). Note the `:` → `-` in the name. | `src/main/db/index.ts` `backupDatabase` |
| `logs/onelaunch.log` | Twelve lines written through the real logger: the `starting`, `database migrated`, `vault`, `startup` and `exit` lines, child scopes (`main:scan`, `main:ipc`, …), and lines that exercise the redaction (`[USER]`, `[ACCOUNT]`, `[EMAIL]`, `[UUID…xxxx]`, `[REDACTED]` for a `-login` password and a JWT). | `src/main/services/logger.ts` |
| `logs-rotation-example/onelaunch.log`, `.1.log` … `.5.log` | The rotation naming, produced with a 160-byte cap instead of 1 MiB so the files appear. `.5.log` is the oldest kept; a sixth rotation deletes it. | `FileSink` in the logger |
| `chromium/README.txt` | Placeholder for the Chromium profile. | — |

Manifests record the absolute path each file was read from. Those were rewritten to the paths a real PC would show (`C:\Program Files (x86)\Steam\…`, `C:\Users\<user>\AppData\…`); the hashes are over the stored copies and still verify.

## `db/` — database edge cases

| File | What it is |
| --- | --- |
| `onelaunch-empty-v5.db` | A brand-new database: all five migrations applied, no rows. What a first start creates before it seeds `whatsNew.lastSeen`. |
| `onelaunch-v1-oldest.db` | The oldest shape a user could still have: only migration 1 (`launcher` columns carry `CHECK (launcher IN ('steam','epic','battlenet'))`, no `needs_update`/`last_played_at`, no `artwork_attempts`, no `launcher_sessions`, artwork `source` CHECK without `launcher`). Holds 3 accounts (one with a password), 1 game, 1 artwork row, a setting and a secret. Opening it with the app runs migrations 2–5. |
| `onelaunch-large-v5.db` | 200 accounts, 1,000 games (5% not installed, 2% needing an update), 500 artwork rows, 10,000 `launch_history` rows. For checking list, sort and `lastLaunch`-join performance. ~3 MB. |

## `journal/` — `switch.json` states

Produced by driving `SwitchJournal` through one job: `switch-before-writes.json` (`backupId: null`, `stepId: "close"`; the next start drops this silently, nothing was written), `switch-writing.json` (`backupId` set, `stepId: "file"`; phase `writing`), `switch-signing-in.json` (`stepId: "wait"`; phase `signing-in`), `switch-rollback-failed.json` (`rollbackFailed: true`; phase `rollback-failed`), and `switch.json.unreadable`, the name an unparsable journal is renamed to (its content here is truncated JSON).

## `settings/`

| File | What it is |
| --- | --- |
| `settings-defaults.json` | All 19 setting keys with the effective value when unset and the code that applies the default. |
| `settings-non-defaults.json` | All 19 keys at non-default values, exactly as stored (strings). Also the shape `settings:getAll` returns. |
| `window-bounds-examples.json` | `window.bounds` values: normal, maximized, off-screen (dropped on read), too small (clamped to 960×600), malformed (ignored). |
| `localStorage.json` | The two renderer `localStorage` keys (`sidebar`, `library.sort`) and what their absence means. Not settings; per Chromium profile. |

## `updates/`

| File | What it is |
| --- | --- |
| `latest.yml` | The release manifest electron-updater reads, in the shape the release workflow publishes, with the app's Ed25519 `signature` and `keyId` appended. Signed with a **throwaway** key made during generation (public half in `README-keys.json`); `verifyManifest` accepts it only when that key is in the trusted list. The installer it describes is 50 placeholder bytes. |
| `latest-unsigned.yml` | The same without `signature`/`keyId`: refused as `unsigned`. |
| `README-keys.json` | The throwaway public key, its id, the signed payload format, and the ids of the two real pinned release keys. |
| `app-update.yml` | What electron-builder writes into `resources/` for an NSIS build (GitHub provider, owner, repo, draft releases, cache dir name). |
| `dev-app-update.yml` | [dev-only] What the test-feed mode writes into the data folder. |

## `dev-only/` [dev-only]

`fake-launchers/<name>/scenario.json` (the three committed fake-launcher scenarios and their README), `fake-platform.json` (one report with every event kind, written by the real `FakeReport`), and `knobs.json` (an example override with every mergeable field). See SPEC.md Appendix A.

## `launchers/` — files the adapters read and write

Byte-for-byte copies of the repo's sanitised fixtures (CRLF, tabs and BOMs preserved; every login, id, token and blob replaced with a placeholder of the same shape and length).

| Launcher | Files | Read for |
| --- | --- | --- |
| `steam/` | `loginusers.vdf` (remembered accounts; the one file a Steam switch rewrites), `libraryfolders.vdf` (library folders), `appmanifest_252950.acf` (an installed game) | SPEC.md § 4 and the Steam adapter |
| `epic/` | `GameUserSettings.ini` (the `[RememberMe]`/`[Offline]` session the switch rewrites), `A0000000000000000000000000000001.item` (an installed-game manifest), `LauncherInstalled.dat`, `EpicGamesLauncher.log` and `EpicGamesLauncher-expired-then-manual.log` (sign-in confirmation) | Epic adapter |
| `battlenet/` | `Battle.net.config` (the `Client.SavedAccountNames` list the switch reorders; Qt JSON layout), `0a1b2c3d.config` (a per-account preferences file, read for `HideOnClose`), three `battle.net-<stamp>.log` client logs (sign-in, product state, game launch events) | Battle.net adapter |
| `riot/` | `RiotGamesPrivateSettings.yaml` and `RiotGamesPrivateSettings-signed-out.yaml` (the session file the switch swaps; signed-in and `riot-client: null`), `RiotClientSettings.yaml` (last-session timestamps), `RiotClientInstalls.json` (install detection), two `*.product_settings.yaml` (install paths), seven `<time>_<pid>_Riot Client.log` run logs (build 140 PSL states, a pre-116 log, a singleton stub) | Riot adapter |

Not present (never read into fixtures because the app only probes them): the Battle.net `product.db` protobuf, `aggregate.json`, Steam's `steamui_login.txt` and `bootstrap_log.txt`, Epic's updater log. Their formats are described in SPEC.md § 4 from the parsers.

## `external/steamgriddb/`

Recorded SteamGridDB API responses the artwork service parses: `grids-steam-252950.json` (`/grids/steam/<appid>`), `heroes-game-5267.json` (`/heroes/game/<id>`), `autocomplete-rocket-league.json` (`/search/autocomplete/<name>`), `empty.json` (a success with no data), `unauthorized.json` (a 401 body). Steam store/community search responses are not recorded in the repo; their shape is in SPEC.md § 7.
