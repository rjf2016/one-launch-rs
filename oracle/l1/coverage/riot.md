# Coverage: riot

Generators: `riot/client-log.gen.ts`, `riot/local-launch.gen.ts`, `riot/products.gen.ts`, `riot/session.gen.ts`

| Export | Source | Blocks | Never run | Not measured |
| --- | --- | --- | --- | --- |
| `parseLogEvents` | `src/main/launchers/riot/client-log.ts` | 71 | 0 | 0 |
| `readSession` | `src/main/launchers/riot/session.ts` | 26 | 0 | 0 |
| `spliceSessionBlock` | `src/main/launchers/riot/session.ts` | 6 | 0 | 0 |
| `planSignOut` | `src/main/launchers/riot/session.ts` | 5 | 0 | 0 |
| `parseProductSettings` | `src/main/launchers/riot/products.ts` | 13 | 0 | 0 |
| `parseInstalls` | `src/main/launchers/riot/products.ts` | 11 | 0 | 0 |
| `launchArgs` | `src/main/launchers/riot/products.ts` | 1 | 0 | 0 |
| `parseLockfile` | `src/main/launchers/riot/local-launch.ts` | 12 | 0 | 0 |

## parseLogEvents

`src/main/launchers/riot/client-log.ts:126:1-240:2`, 71 blocks, 0 never run.

Same-module helper `lastVerdict` (`src/main/launchers/riot/client-log.ts:110:1-113:2`): 4 blocks, 0 never run.

## readSession

`src/main/launchers/riot/session.ts:216:1-244:2`, 26 blocks, 0 never run.

Same-module helper `splitLines` (`src/main/launchers/riot/session.ts:148:1-151:2`): 4 blocks, 0 never run.
- not measured: `src/main/launchers/riot/session.ts:149:47-149:50` ?: true arm, no mapping from the source to the executed code: `' '`

Same-module helper `findSessionBlock` (`src/main/launchers/riot/session.ts:154:1-164:2`): 5 blocks, 0 never run.

Same-module helper `unexpected` (`src/main/launchers/riot/session.ts:146:20-146:89`): 1 blocks, 0 never run.

Same-module helper `isNull` (`src/main/launchers/riot/session.ts:111:1-113:2`): 4 blocks, 0 never run.

Same-module helper `entriesOf` (`src/main/launchers/riot/session.ts:88:1-102:2`): 14 blocks, 0 never run.

Same-module helper `unquote` (`src/main/launchers/riot/session.ts:104:1-109:2`): 8 blocks, 0 never run.

Same-module helper `idTokenClaims` (`src/main/launchers/riot/session.ts:116:1-126:2`): 11 blocks, 0 never run.

Same-module helper `riotIdOf` (`src/main/launchers/riot/session.ts:135:1-144:2`): 7 blocks, 0 never run.

Same-module helper `fingerprintOf` (`src/main/launchers/riot/session.ts:46:1-48:2`): 1 blocks, 0 never run.

Same-module helper `indentOf` (`src/main/launchers/riot/session.ts:77:1-81:2`): 3 blocks, 0 never run.

Same-module helper `nameOf` (`src/main/launchers/riot/session.ts:128:1-132:2`): 6 blocks, 0 never run.

Same-module helper `riotIdText` (`src/main/launchers/riot/session.ts:55:1-57:2`): 1 blocks, 0 never run.

## spliceSessionBlock

`src/main/launchers/riot/session.ts:183:1-192:2`, 6 blocks, 0 never run.

Same-module helper `splitLines` (`src/main/launchers/riot/session.ts:148:1-151:2`): 4 blocks, 0 never run.
- not measured: `src/main/launchers/riot/session.ts:149:47-149:50` ?: true arm, no mapping from the source to the executed code: `' '`

Same-module helper `findSessionBlock` (`src/main/launchers/riot/session.ts:154:1-164:2`): 5 blocks, 0 never run.

Same-module helper `blockEnd` (`src/main/launchers/riot/session.ts:167:1-176:2`): 6 blocks, 0 never run.

Same-module helper `entriesOf` (`src/main/launchers/riot/session.ts:88:1-102:2`): 14 blocks, 0 never run.

Same-module helper `indentOf` (`src/main/launchers/riot/session.ts:77:1-81:2`): 3 blocks, 0 never run.

## planSignOut

`src/main/launchers/riot/session.ts:195:1-200:2`, 5 blocks, 0 never run.

Same-module helper `splitLines` (`src/main/launchers/riot/session.ts:148:1-151:2`): 4 blocks, 0 never run.
- not measured: `src/main/launchers/riot/session.ts:149:47-149:50` ?: true arm, no mapping from the source to the executed code: `' '`

Same-module helper `findSessionBlock` (`src/main/launchers/riot/session.ts:154:1-164:2`): 5 blocks, 0 never run.

Same-module helper `unexpected` (`src/main/launchers/riot/session.ts:146:20-146:89`): 1 blocks, 0 never run.

Same-module helper `isNull` (`src/main/launchers/riot/session.ts:111:1-113:2`): 4 blocks, 0 never run.

Same-module helper `entriesOf` (`src/main/launchers/riot/session.ts:88:1-102:2`): 14 blocks, 0 never run.

Same-module helper `unquote` (`src/main/launchers/riot/session.ts:104:1-109:2`): 8 blocks, 0 never run.

Same-module helper `idTokenClaims` (`src/main/launchers/riot/session.ts:116:1-126:2`): 11 blocks, 0 never run.

Same-module helper `riotIdOf` (`src/main/launchers/riot/session.ts:135:1-144:2`): 7 blocks, 0 never run.

Same-module helper `fingerprintOf` (`src/main/launchers/riot/session.ts:46:1-48:2`): 1 blocks, 0 never run.

Same-module helper `blockEnd` (`src/main/launchers/riot/session.ts:167:1-176:2`): 6 blocks, 0 never run.

Same-module helper `indentOf` (`src/main/launchers/riot/session.ts:77:1-81:2`): 3 blocks, 0 never run.

Same-module helper `nameOf` (`src/main/launchers/riot/session.ts:128:1-132:2`): 6 blocks, 0 never run.

Same-module helper `riotIdText` (`src/main/launchers/riot/session.ts:55:1-57:2`): 1 blocks, 0 never run.

## parseProductSettings

`src/main/launchers/riot/products.ts:87:1-99:2`, 13 blocks, 0 never run.

Same-module helper `unquote` (`src/main/launchers/riot/products.ts:79:1-84:2`): 8 blocks, 0 never run.

## parseInstalls

`src/main/launchers/riot/products.ts:116:1-134:2`, 11 blocks, 0 never run.

Same-module helper `stringOrNull` (`src/main/launchers/riot/products.ts:111:1-113:2`): 4 blocks, 0 never run.

## launchArgs

`src/main/launchers/riot/products.ts:66:1-68:2`, 1 blocks, 0 never run.

## parseLockfile

`src/main/launchers/riot/local-launch.ts:33:1-40:2`, 12 blocks, 0 never run.
