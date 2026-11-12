# Coverage: steam

Generators: `steam/bootstrap-log.gen.ts`, `steam/login-log.gen.ts`, `steam/loginusers.gen.ts`, `steam/manifests.gen.ts`, `steam/vdf.gen.ts`

| Export | Source | Blocks | Never run | Not measured |
| --- | --- | --- | --- | --- |
| `parseVdf` | `src/main/launchers/steam/vdf.ts` | 27 | 0 | 0 |
| `vdfGet` | `src/main/launchers/steam/vdf.ts` | 6 | 0 | 0 |
| `vdfHas` | `src/main/launchers/steam/vdf.ts` | 1 | 0 | 0 |
| `vdfString` | `src/main/launchers/steam/vdf.ts` | 3 | 0 | 0 |
| `vdfObject` | `src/main/launchers/steam/vdf.ts` | 4 | 0 | 0 |
| `vdfInt` | `src/main/launchers/steam/vdf.ts` | 6 | 0 | 0 |
| `vdfFlag` | `src/main/launchers/steam/vdf.ts` | 1 | 0 | 0 |
| `parseLoginUsers` | `src/main/launchers/steam/loginusers.ts` | 11 | 0 | 0 |
| `findLoginUser` | `src/main/launchers/steam/loginusers.ts` | 8 | 0 | 0 |
| `planAutoLoginRewrite` | `src/main/launchers/steam/loginusers.ts` | 33 | 0 | 0 |
| `parseLibraryFolders` | `src/main/launchers/steam/manifests.ts` | 13 | 0 | 0 |
| `parseAppManifest` | `src/main/launchers/steam/manifests.ts` | 14 | 0 | 0 |
| `isFullyInstalled` | `src/main/launchers/steam/manifests.ts` | 1 | 0 | 0 |
| `needsUpdate` | `src/main/launchers/steam/manifests.ts` | 1 | 0 | 0 |
| `parseLoginStates` | `src/main/launchers/steam/login-log.ts` | 7 | 1 | 0 |
| `classifyReason` | `src/main/launchers/steam/login-log.ts` | 19 | 0 | 0 |
| `outcomeSince` | `src/main/launchers/steam/login-log.ts` | 1 | 0 | 0 |
| `latestOutcome` | `src/main/launchers/steam/login-log.ts` | 6 | 0 | 0 |
| `readLoginLogSince` | `src/main/launchers/steam/login-log.ts` | 6 | 1 | 1 |
| `updateInProgress` | `src/main/launchers/steam/bootstrap-log.ts` | 11 | 0 | 0 |
| `updatingSince` | `src/main/launchers/steam/bootstrap-log.ts` | 1 | 0 | 0 |

## parseVdf

`src/main/launchers/steam/vdf.ts:102:1-146:2`, 27 blocks, 0 never run.

Same-module helper `tokenize` (`src/main/launchers/steam/vdf.ts:27:1-95:2`): 42 blocks, 0 never run.

Same-module helper `newObject` (`src/main/launchers/steam/vdf.ts:98:1-100:2`): 1 blocks, 0 never run.

## vdfGet

`src/main/launchers/steam/vdf.ts:149:1-156:2`, 6 blocks, 0 never run.

## vdfHas

`src/main/launchers/steam/vdf.ts:158:1-160:2`, 1 blocks, 0 never run.

## vdfString

`src/main/launchers/steam/vdf.ts:162:1-165:2`, 3 blocks, 0 never run.

## vdfObject

`src/main/launchers/steam/vdf.ts:167:1-170:2`, 4 blocks, 0 never run.

## vdfInt

`src/main/launchers/steam/vdf.ts:173:1-178:2`, 6 blocks, 0 never run.

## vdfFlag

`src/main/launchers/steam/vdf.ts:180:1-182:2`, 1 blocks, 0 never run.

## parseLoginUsers

`src/main/launchers/steam/loginusers.ts:47:1-67:2`, 11 blocks, 0 never run.

Same-module helper `usersBlock` (`src/main/launchers/steam/loginusers.ts:40:1-44:2`): 3 blocks, 0 never run.

## findLoginUser

`src/main/launchers/steam/loginusers.ts:69:1-76:2`, 8 blocks, 0 never run.

## planAutoLoginRewrite

`src/main/launchers/steam/loginusers.ts:106:1-177:2`, 33 blocks, 0 never run.

Same-module helper `usersBlock` (`src/main/launchers/steam/loginusers.ts:40:1-44:2`): 3 blocks, 0 never run.

## parseLibraryFolders

`src/main/launchers/steam/manifests.ts:21:1-37:2`, 13 blocks, 0 never run.

## parseAppManifest

`src/main/launchers/steam/manifests.ts:59:1-77:2`, 14 blocks, 0 never run.

## isFullyInstalled

`src/main/launchers/steam/manifests.ts:79:1-81:2`, 1 blocks, 0 never run.

## needsUpdate

`src/main/launchers/steam/manifests.ts:83:1-85:2`, 1 blocks, 0 never run.

## parseLoginStates

`src/main/launchers/steam/login-log.ts:31:1-39:2`, 7 blocks, 1 never run.

- `src/main/launchers/steam/login-log.ts:36:53-36:55` ?? right side: `''`

## classifyReason

`src/main/launchers/steam/login-log.ts:46:1-55:2`, 19 blocks, 0 never run.

## outcomeSince

`src/main/launchers/steam/login-log.ts:90:1-92:2`, 1 blocks, 0 never run.

## latestOutcome

`src/main/launchers/steam/login-log.ts:58:1-65:2`, 6 blocks, 0 never run.

## readLoginLogSince

`src/main/launchers/steam/login-log.ts:80:1-88:2`, 6 blocks, 1 never run.

- `src/main/launchers/steam/login-log.ts:85:11-87:4` catch: `return '';`
- not measured: `src/main/launchers/steam/login-log.ts:80:74-80:88` default parameter, V8 keeps no counter for a default parameter: `MAX_TAIL_BYTES`

## updateInProgress

`src/main/launchers/steam/bootstrap-log.ts:49:1-59:2`, 11 blocks, 0 never run.

## updatingSince

`src/main/launchers/steam/bootstrap-log.ts:63:1-65:2`, 1 blocks, 0 never run.
