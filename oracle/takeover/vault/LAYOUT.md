# Vault layout

How One Launch's secrets sit on disk, as Electron 44.7.0 `safeStorage` writes them on Windows. Every fact in the block below was measured on a scratch data folder written by the real app (`make-local-fixture.mjs`), not assumed; `scripts/oracle/takeover/check.mjs` (Electron repo) measures it again on a fresh folder and compares with this block. Values are described by length and prefix only. The wrapped key, and anything it decrypts, appears nowhere in this bundle.

```json layout
{
  "localState": "data/chromium/Local State",
  "notAt": "data/Local State",
  "encoding": "UTF-8 JSON",
  "keyPath": ["os_crypt", "encrypted_key"],
  "osCryptKeys": ["audit_enabled", "encrypted_key"],
  "keyEncoding": "base64",
  "wrappedPrefix": "DPAPI",
  "wrappedPrefixBytes": 5,
  "dpapiBlobStartsWith": "01 00 00 00 d0 8c 9d df 01 15 d1 11 8c 7a 00 c0 4f c2 97 eb",
  "unwrap": "CryptUnprotectData of the bytes after the prefix, current user, no entropy",
  "unwrappedKeyBytes": 32,
  "blobPrefix": "v10",
  "nonceBytes": 12,
  "tagBytes": 16,
  "overheadBytes": 31,
  "columns": ["accounts.secret_enc", "secrets.value_enc", "launcher_sessions.blob_enc"],
  "probe": { "key": "vault.probe", "value": "one-launch-vault-probe-v1" }
}
```

## Where the key is

- The data folder's `chromium` subfolder is Electron's `sessionData` (`src/main/platform/paths.ts:51-52`); Chromium writes `Local State` there, not at the top of the data folder.
- `Local State` is a JSON object. Its `os_crypt` member held exactly two keys on the scratch folder: `audit_enabled` (a boolean) and `encrypted_key` (a string).
- `os_crypt.encrypted_key` is standard base64. Decoded, it is the 5 ASCII bytes `DPAPI` followed by a DPAPI blob; on the scratch folder the decoded value was 283 bytes (5 + 278) and the base64 text 380 characters. The DPAPI blob's length depends on the machine's DPAPI parameters; only its start is fixed: the version `01 00 00 00` and the DPAPI provider GUID bytes listed in the block.
- The blob unwraps with `CryptUnprotectData` for the current user, without entropy or prompt, to 32 bytes: the AES-256 key.
- Chromium writes the key some seconds into a data folder's first session; a hard kill before that loses it (`scripts/scenarios/lib.mjs:14-25`). The app itself never reads or writes `Local State` (`src/main/services/vault.ts:12-16`).

## A stored value

| Bytes | Length | What |
| --- | --- | --- |
| 0 to 2 | 3 | ASCII `v10` |
| 3 to 14 | 12 | AES-GCM nonce, random per value |
| 15 to n-17 | UTF-8 length of the plaintext | ciphertext |
| n-16 to n-1 | 16 | GCM tag |

AES-256-GCM with no associated data. Every ciphertext column on the scratch folder (`accounts.secret_enc`, `secrets.value_enc`) was exactly 31 bytes longer than the UTF-8 plaintext, the empty string included (31 bytes). `launcher_sessions.blob_enc`, the `.enc` files of a backup snapshot and the Battle.net token bytes in a snapshot manifest (base64 of the blob) go through the same `Vault.encrypt`, which is `safeStorage.encryptString` (`src/main/services/sessions.ts:98`, `src/main/services/backup.ts:18-22,178-180`, `src/main/launchers/battlenet/adapter.ts:1552-1554`, `src/main/services/vault.ts:54-57`); the fixture holds no session or snapshot, so those three were not measured. `portable-v10.json` has vectors in this layout under a throwaway key.

## What Electron does with a value that is not a good v10 blob

Measured with real `safeStorage.decryptString` on a throwaway profile, each case in a process of its own (`scripts/oracle/takeover/vault/safestorage-probe.mjs`); the per-case results are the `electron` field of each failure in `portable-v10.json`.

- A wrong tag, ciphertext, nonce or key, or a blob cut short at 30 bytes or one byte short of whole: it throws "Error while decrypting the ciphertext provided to safeStorage.decryptString.", which the app turns into `VaultUnreadableError` (`VAULT_UNREADABLE`) (`src/main/services/vault.ts:59-66`).
- The prefix `v11`: Chromium tries the bytes as a bare DPAPI blob, logs "Failed to decrypt: The data is invalid. (0xD)" (Windows' own message text) and throws the same error.
- Any other prefix (`v20`), or none: it throws "Error while decrypting the ciphertext provided to safeStorage.decryptString. Ciphertext does not appear to be encrypted."
- Zero bytes: it returns the empty string; no error.
- `v10` followed by fewer than 12 bytes (the prefix alone, or a cut inside the nonce): the process ends with `0xC0000409` after "out_of_range was thrown in -fno-exceptions mode"; nothing is thrown to the app.

## What `vault-health` checks

`src/main/services/vault-health.ts`:

- At start, when the vault is available and the `secrets` row `vault.probe` is absent, it stores `one-launch-vault-probe-v1` there; it never overwrites one (`:8,40-45`, called from `src/main/index.ts:497`).
- `status()` is `readable: null` when the vault is unavailable or there is no probe, `true` when the probe decrypts to that constant, `false` when it decrypts to anything else or throws `VaultUnreadableError` (`:47-57`).
- `reset()` sets every `accounts.secret_enc` to NULL, deletes every `launcher_sessions` and `secrets` row, logs a WARN with the counts and stores a fresh probe (`:59-67`).
