# Findings from M0-T02, T03, T06, T08, T10, T11

What the subagents found in the Electron app at the pin, in the plan and in the design, while capturing the oracle. Everything is recorded as the app behaves; nothing was fixed in Electron. For ticket M0-T12's gate report ("anything found in Electron that contradicts the spec"). The vector case ids named here are in `oracle/l1/<area>/`.

Decided since: rows X-11 to X-20 of `plan/triage.toml` cover items 1 to 4, 14, 17, 18, 20, 23, 25, 28, 29 and 48. Every other item is ported as the app behaves. Items 40 and 41 are corrected in `PLAN.md` and `design/oracle.md`, item 22 is amended in the spec (7.5 step 6), and item 43's three categories are approved in `plan/waivers.toml`. `plan/design/takeover.md` is left as written: `oracle/takeover/contract.md` is its checked form and wins where the two differ (items 28 to 39).

## Could crash or hang the app

1. **VDF parser never returns** (`src/main/launchers/steam/vdf.ts`, `tokenize`). A `\s` character the whitespace skip does not handle (vertical tab confirmed by the coordinator; form feed, NBSP, U+2028/2029, U+3000 and the rest of Zs by the same reading), outside a quoted string or comment, makes the bare-token loop push empty tokens until the heap is exhausted. A corrupted `loginusers.vdf`, `libraryfolders.vdf` or app manifest takes down the main process. No vector can record it.
2. **`safeStorage.decryptString` kills the main process on a short blob.** A stored value of `v10` followed by fewer than 12 bytes ends the process with 0xC0000409; nothing is thrown, so `SafeStorageVault` cannot report VAULT_UNREADABLE. A truncated `*_enc` value would crash the app at decrypt (`takeover/vault/portable-v10.json`, failures `prefix-only`, `truncated-in-nonce`).
3. **An empty blob decrypts to the empty string** with no error, so an empty `secret_enc` reads as an empty password.

## Rewriters that are not byte-exact, or misread a file

4. Epic `splitIni` drops the last character of the last line when a file has a CRLF anywhere but ends in a bare LF (`mixed-crlf-then-lf-end*`); a rewrite then writes the damaged line back.
5. Epic INI round trip is not exact: empty text comes back as `"\n"`; mixed line endings become the dominant one.
6. Epic `planSessionWrite(text, null)` on a file with no `[RememberMe]` adds the section; `rewriteSection` removes duplicate keys with an empty `changes` and `noop: false`.
7. Steam loginusers rewrite: one CRLF anywhere makes every line CRLF; with nothing to change it says `noop: true` while its text differs from the input.
8. Steam loginusers rewrite misreads layouts: a file on one line or a brace on the key line gives `noop: true` though the target is not flagged; a trailing comment after `"AutoLogin"` inserts a second line; a block the parser skips is still rewritten.
9. Riot `spliceSessionBlock` rejoins every line with one line ending (the doc comment says they stay as they are); `riot-client: null` followed by deeper lines is replaced with them.
10. Battle.net `planSavedAccountNames` can return `noop: true` with a change listed (empty list, blank login).

## Paths

11. `normalizeLauncherPath` turns a UNC `\\server\share` (and `//server/share`) into `\server\share`; `C:` becomes `C:.`; a trailing separator stays on paths of three units or fewer.
12. `resolveArtworkPath` with an artwork folder at a drive root always returns null.
13. `dataDirFromArgv` treats `D:\a` and `d:\A`, or `D:\a` and `D:\a\`, as different folders and throws.
14. Battle.net `parseProductDb` cuts a non-ASCII install path (`D:/Jeux/Été` becomes `D:\Jeux`), accepts a file cut inside a path, and can promote junk to a codename.
15. Battle.net `parseAggregate` throws RangeError on a timestamp over 8.64e15 instead of returning null; an empty `icon_path` becomes `.`.
16. Steam `parseLibraryFolders` returns folders in JS integer-key order, not file order.

## Masking and trust

17. `maskReason` (Battle.net) does not mask an email with a non-ASCII local part or domain; its 200-unit cut can split an astral character. `maskLogin` can leave half a character.
18. Logger: `redactValue` reports a shared, non-cyclic reference as `[circular]`; `password= next` redacts "next"; a name stored as `İstanbul` never masks itself (non-unicode `/i`).
19. `isTrustedRendererUrl` trusts any `file:` URL when the dev URL is a `file:` URL (both origins are "null").
20. `PRODUCTS[codename]` (Battle.net `productCodes`, `parseUninstallEntries`) and `phaseOf` (renderer) look names up on plain objects: `__proto__`, `constructor`, `toString` return inherited values.
21. IPC: ids are not shape-checked; empty strings and lone surrogates pass wherever a string is accepted; a Date passes the object check; `accounts:create` does not wait for a running switch while update, delete and the password changes do.

## Updates

22. Spec 7.5 step 6 says a hash mismatch is failed(verify); through electron-updater it is always failed(download) "the downloaded file does not match the release".
23. A cancelled download does not abort its HTTP request (library): the connection stays open until quit, a 0-byte `temp-<file>.exe` is left in `pending`, and when the feed dropped the socket Chromium retried and downloaded after the cancel.
24. "the release manifest is not an object" cannot be reached through electron-updater (its version check throws first).
25. `verifyManifest` accepts YAML sizes `0x32`, `050`, `0o62`, `+50`, `5e1`, `50.0` as 50 with a valid signature; manifest versions have no length cap.
26. `compareVersions` is not antisymmetric for strings that parse to NaN; versions past 2^53 compare equal.
27. Connection refused is not retried (Chromium's error carries no `code`); read from the library, not measured.

## Where `plan/design/takeover.md` differs from the source (all marked in `takeover/contract.md`)

28. "No elevation" is not exact: NsisUpdater falls back to `resources\elevate.exe` on EACCES or UNKNOWN and to `shell.openPath` on ENOENT.
29. `$INSTDIR` comes from HKCU only for a per-user install; with an HKLM `InstallLocation` a silent update goes per-machine and re-launches elevated; `/D=` overrides.
30. The stock running-app check ends every process whose path starts with `$INSTDIR`, uses `Stop-Process`, falls back to `tasklist`/`taskkill` by image name without PowerShell, and after two failed rounds a silent install quits without installing.
31. If the old uninstaller cannot be started at all, the installer carries on over the old files.
32. `/KEEP_APP_DATA` is read nowhere; data survives because `--delete-app-data` is absent. `--delete-app-data` makes the stock uninstaller delete `%APPDATA%\OneLaunch`.
33. Shortcuts are kept only with `--keep-shortcuts`, which needs `--updated`, the registry value and the exe present.
34. The installer also writes `Comments`, and copies itself to `%LOCALAPPDATA%\one-launch-updater\installer.exe`.
35. The GitHub provider first requests `releases.atom`.
36. The templates are not purely MIT: `include/StdUtils.nsh` is LGPL-2.1-or-later. Most NSIS plugin binaries state no licence anywhere readable.
37. The generated NSIS script is not in `dist\builder-debug.yml`; the effective defines in `takeover/nsis/defines.json` were derived by reading `NsisTarget.js`. Confirmed afterwards against a local installer build with electron-builder's debug output on (scratch output folder, installer not run): all 30 defines and the five commands match, none is extra, and the uninstaller pass differs only by `BUILD_UNINSTALLER`. That build's installer is 112,980,620 bytes, 1,710 more than the beta.3 installer in `perf/baseline.json`.
38. Whether StdUtils parses `--flag` cannot be read from source.
39. A `stagingPercentage` in `latest.yml` would gate every copy at once (all share one staging id).

## Plan and design numbers that are wrong

40. `PLAN.md` says 971 tests are catalogued; the suite at the pin has 1031 (1026 run, 5 live).
41. The design says 54 distinct Lucide imports; there are 56.
42. Test files the T03 ticket names do not exist (`battlenet-log`, `riot-files`, `riot-log`); the tests live in other files. The migrations are in `src/main/db/index.ts`.
43. `waivers.toml` defines no categories yet; T02 proposes `web-only` (7 tests), `tooling` (25) and `native-platform` (36).

## Behaviour a Rust port gets wrong by default (each has vector cases)

44. JS regex `.` does not match CR, U+2028 or U+2029: a log line, INI entry or session key line holding one is dropped silently.
45. JS `\s` and `trim` include U+FEFF and not U+0085; Rust is the other way round. `\d`, `\w`, `\b` are ASCII-only in JS.
46. `/i` without the `u` flag does not fold U+017F or U+212A.
47. Line and string caps count UTF-16 units; cuts can split an astral character.
48. Several user-visible messages embed V8's own `JSON.parse` text with positions (Epic manifests, Epic and Battle.net blobs, the Battle.net config).
49. `JSON.parse` has no depth limit; object key order puts integer-like keys first; `JSON.stringify` clamps indent to 10.
50. Riot's id token goes through Node's lenient base64url; `is_dpop_bound` and `refresh_token_write_count` are read without unquoting.
51. `localeCompare` with base sensitivity orders `sortAccounts`; time strings are compared as text in `lastPlayedOf` and `lastUsedAt`.
52. `formatRelative` rounds a half toward positive infinity, so past and future differ; `sentence` upper-cases one UTF-16 unit (`ß` becomes `SS`).

## Smaller things

53. A `riot-adapter` test appears to write into its temp folder after its own cleanup, which suggests a switch outlives its test.
54. One `no-secrets` test skips itself when the temp volume has no 8.3 short names.
55. `jobAnnouncement` of a failed job whose error is `''` returns `''`; `recoveryCopy` with an empty account name prints "files for , so…"; `phaseLabel` and `recoveryCopy` return undefined on unknown ids.
56. Riot `parseInstalls` returns one shared empty result object for every malformed input.
57. Epic `launchUri` puts catalog ids into the URI unescaped and throws URIError on a lone surrogate.
58. `writeSession` (Epic) reads the INI without `readIni`'s UTF-16/NUL refusal.
59. Steam VDF: line numbers drift after a backslash-newline in a string; an unquoted `#base` swallows the next token; `mostRecentKeys` order differs from its doc comment.
60. Four adapters each carry their own copy of the same helper strings, so a shown string often cannot name one source (45 copy runs stay "open" between equal candidates in `assets/copy-map.json`).
61. About 460 ms of each ~760 ms warm start passes before any app code runs (`processToNodeMs`); `startup:library-frame` is a rAF time about 20 ms before the frame is presented.
62. Inline sort callbacks in `LibraryView.tsx`, `LaunchDialog.tsx` and `BackupsPanel.tsx` are not exported, so no vector reaches them.
