# Coverage: epic

Generators: `epic/ini.gen.ts`, `epic/log.gen.ts`, `epic/manifests.gen.ts`, `epic/session.gen.ts`

| Export | Source | Blocks | Never run | Not measured |
| --- | --- | --- | --- | --- |
| `splitIni` | `src/main/launchers/epic/ini.ts` | 7 | 0 | 0 |
| `joinIni` | `src/main/launchers/epic/ini.ts` | 4 | 0 | 0 |
| `rewriteSection` | `src/main/launchers/epic/ini.ts` | 25 | 0 | 0 |
| `parseItemManifest` | `src/main/launchers/epic/manifests.ts` | 19 | 1 | 0 |
| `parseLauncherInstalled` | `src/main/launchers/epic/manifests.ts` | 16 | 1 | 1 |
| `launchUri` | `src/main/launchers/epic/manifests.ts` | 5 | 0 | 0 |
| `parseLogEvents` | `src/main/launchers/epic/launcher-log.ts` | 25 | 3 | 0 |
| `lastSignIn` | `src/main/launchers/epic/launcher-log.ts` | 6 | 0 | 0 |
| `isSessionRejection` | `src/main/launchers/epic/launcher-log.ts` | 1 | 0 | 0 |
| `planSessionWrite` | `src/main/launchers/epic/session.ts` | 4 | 0 | 0 |
| `serializeBlob` | `src/main/launchers/epic/session.ts` | 1 | 0 | 0 |
| `parseBlob` | `src/main/launchers/epic/session.ts` | 13 | 0 | 0 |

## splitIni

`src/main/launchers/epic/ini.ts:13:1-18:2`, 7 blocks, 0 never run.

## joinIni

`src/main/launchers/epic/ini.ts:20:1-23:2`, 4 blocks, 0 never run.

## rewriteSection

`src/main/launchers/epic/ini.ts:79:1-129:2`, 25 blocks, 0 never run.

Same-module helper `findSections` (`src/main/launchers/epic/ini.ts:37:1-46:2`): 6 blocks, 0 never run.

## parseItemManifest

`src/main/launchers/epic/manifests.ts:48:1-76:2`, 19 blocks, 1 never run.

- `src/main/launchers/epic/manifests.ts:53:89-53:100` ?: false arm: `String(err)`

Same-module helper `str` (`src/main/launchers/epic/manifests.ts:37:1-40:2`): 4 blocks, 0 never run.

Same-module helper `num` (`src/main/launchers/epic/manifests.ts:42:1-45:2`): 5 blocks, 0 never run.

## parseLauncherInstalled

`src/main/launchers/epic/manifests.ts:97:1-116:2`, 16 blocks, 1 never run.

- `src/main/launchers/epic/manifests.ts:102:114-102:125` ?: false arm: `String(err)`
- not measured: `src/main/launchers/epic/manifests.ts:104:49-104:105` ?: true arm, no mapping from the source to the executed code: `(raw as { InstallationList?: unknown }).InstallationList`

Same-module helper `str` (`src/main/launchers/epic/manifests.ts:37:1-40:2`): 4 blocks, 0 never run.

## launchUri

`src/main/launchers/epic/manifests.ts:123:1-129:2`, 5 blocks, 0 never run.

## parseLogEvents

`src/main/launchers/epic/launcher-log.ts:60:1-93:2`, 25 blocks, 3 never run.

- `src/main/launchers/epic/launcher-log.ts:75:59-75:61` ?? right side: `''`
- `src/main/launchers/epic/launcher-log.ts:75:29-75:31` ?? right side: `''`
- `src/main/launchers/epic/launcher-log.ts:78:71-78:73` ?? right side: `''`

Same-module helper `lineTime` (`src/main/launchers/epic/launcher-log.ts:47:1-51:2`): 3 blocks, 0 never run.

Same-module helper `maskReason` (`src/main/launchers/epic/launcher-log.ts:54:1-58:2`): 3 blocks, 0 never run.

## lastSignIn

`src/main/launchers/epic/launcher-log.ts:98:1-105:2`, 6 blocks, 0 never run.

## isSessionRejection

`src/main/launchers/epic/launcher-log.ts:111:1-113:2`, 1 blocks, 0 never run.

## planSessionWrite

`src/main/launchers/epic/session.ts:103:1-111:2`, 4 blocks, 0 never run.

Same-module helper `mask` (`src/main/launchers/epic/session.ts:86:1-92:2`): 9 blocks, 0 never run.

Same-module helper `describe` (`src/main/launchers/epic/session.ts:80:1-84:2`): 7 blocks, 0 never run.

Same-module helper `shortFingerprint` (`src/main/launchers/epic/session.ts:37:1-39:2`): 3 blocks, 1 never run.

- `src/main/launchers/epic/session.ts:38:50-38:58` ?: false arm: `'(none)'`

Same-module helper `fingerprintOf` (`src/main/launchers/epic/session.ts:32:1-34:2`): 1 blocks, 0 never run.

## serializeBlob

`src/main/launchers/epic/session.ts:62:1-64:2`, 1 blocks, 0 never run.

## parseBlob

`src/main/launchers/epic/session.ts:66:1-77:2`, 13 blocks, 0 never run.
