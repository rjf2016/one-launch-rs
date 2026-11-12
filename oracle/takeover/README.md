# Takeover fixtures and contract

What a Rust installer, updater and vault must match so that an installed Electron copy updates to the Rust build, and an Electron build can replace it again. Produced by `scripts/oracle/takeover/` on the Electron repo's `oracle-tooling` branch; the design is `plan/design/takeover.md`.

| Path | What | Committed |
| --- | --- | --- |
| `contract.md` | The installer and updater contract as numbered assertions (U, I1 to I9, A, R, T, B, O), each with its source lines and how it can be checked | yes |
| `nsis/` | The electron-builder NSIS templates byte for byte (one, `include/StdUtils.nsh`, recorded by digest in `nsis/templates-held-back.json` instead), `build/installer.nsh`, the effective defines, the plug-in inventory and licence notes (`nsis/README.md`) | yes |
| `vault/portable-v10.json` | AES-256-GCM vectors in the `v10` layout under a throwaway key, and the blobs a codec must refuse, each with what Electron does with it | yes |
| `vault/LAYOUT.md` | Where the key sits and what a stored value looks like, measured on a scratch folder | yes |
| `vault/make-local-fixture.mjs` | Writes a machine-local data folder with placeholder secrets through the real app, and reads one back | yes (the script); its output never |
| `migrations/v1.json` to `v5.json` | The schema after each migration step | yes |
| `migrations/v1-oldest-migrated.json` | `fixtures/db/onelaunch-v1-oldest.db` after the app migrates a copy of it | yes |

## Using the vectors

`vault/portable-v10.json`: `key.base64` is the 32-byte key; each `valid` entry gives `nonce`, `ciphertext`, `tag` and the whole `blob` (`v10` + nonce + ciphertext + tag) in base64 and the `plaintext` as a string. A codec must decrypt every `blob` to its `plaintext`, and encrypting `plaintext` with that key and `nonce` must give `blob` exactly. Each `failures` entry is a blob a codec must refuse (`wrong-key` with `otherKey`); its `electron` field says what Electron 44.7.0 does with it: `safeStorage` (measured) and `app` (what `SafeStorageVault` makes of it). Two cases are not errors in Electron: zero bytes decrypt to the empty string, and `v10` with fewer than 12 more bytes ends the process. Which of those the Rust codec copies is a triage decision.

## The same-machine cross-read

DPAPI output is bound to the Windows user and the machine, so no DPAPI-wrapped key is ever committed. Parity is tested in one job on one machine:

1. Electron writes, Rust reads: `node oracle/takeover/vault/make-local-fixture.mjs <new or empty folder>` (needs the Electron repo with `node_modules` and the unpackaged build `out/`; `--electron=<folder>` if it is not the sibling `one-launch`). It starts the unpackaged app on `<folder>\data` with fake launchers (scenario `none`), creates four accounts (three with placeholder passwords: ASCII, multi-byte UTF-8, 680 bytes) and two named secrets (one empty, one multi-byte) through the app's IPC, quits it, and writes `<folder>\fixture.json` with every placeholder. The Rust side opens `<folder>\data` and must decrypt each value to what `fixture.json` says.
2. Rust writes, Electron reads: the Rust side writes a data folder with its own `fixture.json` in the same shape, then `node make-local-fixture.mjs --read <folder>` starts the unpackaged app on it and prints a PASS or FAIL line for: vault available, probe readable, `diagnostics.roundtrip` equal to the expected placeholder, and every account listed with the expected `hasSecret`.

The script refuses a folder inside either repo, inside any user profile (`\Users\`, the home, `%APPDATA%`, `%LOCALAPPDATA%`), a drive root, or (when writing) one that exists and is not empty. It never touches the real data folder: the app always gets `--data-dir=`.

## The migration dumps

Made by `scripts/oracle/takeover/migrations.gen.ts` with the app's own code, so another dump (ticket T03's core vector) can be compared field by field:

- Step `k` (1 to 5) is a new, empty file opened with `openDatabase` (`src/main/db/index.ts`, so WAL, `foreign_keys = ON` and the probe write apply), then `migrate(db, MIGRATIONS.slice(0, k))`.
- `user_version` is `PRAGMA user_version`.
- `master` is `SELECT type, name, tbl_name, sql FROM sqlite_master` (no `rootpage`, which depends on the data), sorted by `type`, then `name`, comparing UTF-16 code units; `sql` is SQLite's stored text, `null` for automatic indexes.
- `tables` has one entry per `master` row of type `table`, in the same order, each with `PRAGMA table_info("<name>")`, `PRAGMA index_list("<name>")` and `PRAGMA foreign_key_list("<name>")` exactly as better-sqlite3 returns them (rows as objects with SQLite's column names, in SQLite's order).
- `v1-oldest-migrated.json`: a copy of `fixtures/db/onelaunch-v1-oldest.db` through the same `openDatabase` and a full `migrate(db)`; `migration` is what `migrate` returned and `equalsFreshV5` whether its dump deep-equals `v5.json`'s (it does).

## Regenerating

From the Electron repo root: `node scripts/oracle/generate.mjs takeover`. The migration, NSIS and vector files repeat byte for byte. `vault.gen.ts` also runs Electron's `safeStorage` on a throwaway profile under the scratch folder (DPAPI on that profile only), and `nsis-plugins.gen.ts` needs the electron-builder cache an earlier packaged build left on the machine. `node scripts/oracle/takeover/check.mjs` runs the ticket's checks.
