# Coverage: battlenet

Generators: `battlenet/client-log.gen.ts`, `battlenet/config.gen.ts`, `battlenet/product-state.gen.ts`, `battlenet/products.gen.ts`, `battlenet/session.gen.ts`

| Export | Source | Blocks | Never run | Not measured |
| --- | --- | --- | --- | --- |
| `parseConfig` | `src/main/launchers/battlenet/config.ts` | 8 | 1 | 0 |
| `detectLayout` | `src/main/launchers/battlenet/config.ts` | 5 | 0 | 0 |
| `serializeConfig` | `src/main/launchers/battlenet/config.ts` | 9 | 0 | 0 |
| `roundTrips` | `src/main/launchers/battlenet/config.ts` | 2 | 0 | 0 |
| `planSavedAccountNames` | `src/main/launchers/battlenet/config.ts` | 17 | 0 | 0 |
| `maskLogin` | `src/main/launchers/battlenet/config.ts` | 5 | 0 | 0 |
| `splitSavedAccountNames` | `src/main/launchers/battlenet/config.ts` | 5 | 0 | 0 |
| `parseLogEvents` | `src/main/launchers/battlenet/client-log.ts` | 60 | 0 | 0 |
| `lineTime` | `src/main/launchers/battlenet/client-log.ts` | 3 | 0 | 0 |
| `maskReason` | `src/main/launchers/battlenet/client-log.ts` | 1 | 0 | 0 |
| `isForwarderLog` | `src/main/launchers/battlenet/client-log.ts` | 2 | 0 | 0 |
| `lastSignIn` | `src/main/launchers/battlenet/client-log.ts` | 8 | 0 | 0 |
| `endedSignedOut` | `src/main/launchers/battlenet/client-log.ts` | 6 | 0 | 0 |
| `productStates` | `src/main/launchers/battlenet/product-state.ts` | 4 | 0 | 0 |
| `productNeedsUpdate` | `src/main/launchers/battlenet/product-state.ts` | 1 | 0 | 0 |
| `gameStarted` | `src/main/launchers/battlenet/product-state.ts` | 6 | 0 | 0 |
| `gameRunning` | `src/main/launchers/battlenet/product-state.ts` | 8 | 0 | 0 |
| `PRODUCTS` | `src/main/launchers/battlenet/products.ts` | - | - | - |
| `productCodes` | `src/main/launchers/battlenet/products.ts` | 8 | 0 | 0 |
| `parseProductDb` | `src/main/launchers/battlenet/products.ts` | 20 | 0 | 0 |
| `parseAggregate` | `src/main/launchers/battlenet/products.ts` | 23 | 0 | 1 |
| `launchCommand` | `src/main/launchers/battlenet/products.ts` | 3 | 0 | 0 |
| `serializeBlob` | `src/main/launchers/battlenet/session.ts` | 1 | 0 | 0 |
| `parseBlob` | `src/main/launchers/battlenet/session.ts` | 10 | 0 | 0 |

## parseConfig

`src/main/launchers/battlenet/config.ts:39:1-48:2`, 8 blocks, 1 never run.

- `src/main/launchers/battlenet/config.ts:44:92-44:103` ?: false arm: `String(err)`

## detectLayout

`src/main/launchers/battlenet/config.ts:28:1-37:2`, 5 blocks, 0 never run.

## serializeConfig

`src/main/launchers/battlenet/config.ts:50:1-57:2`, 9 blocks, 0 never run.

## roundTrips

`src/main/launchers/battlenet/config.ts:60:1-67:2`, 2 blocks, 0 never run.

## planSavedAccountNames

`src/main/launchers/battlenet/config.ts:168:1-186:2`, 17 blocks, 0 never run.

Same-module helper `section` (`src/main/launchers/battlenet/config.ts:69:1-76:2`): 10 blocks, 1 never run.

- `src/main/launchers/battlenet/config.ts:72:64-72:76` if arm: `return null;`

Same-module helper `str` (`src/main/launchers/battlenet/config.ts:78:1-81:2`): 3 blocks, 0 never run.

## maskLogin

`src/main/launchers/battlenet/config.ts:155:1-159:2`, 5 blocks, 0 never run.

## splitSavedAccountNames

`src/main/launchers/battlenet/config.ts:84:1-90:2`, 5 blocks, 0 never run.

## parseLogEvents

`src/main/launchers/battlenet/client-log.ts:87:1-175:2`, 60 blocks, 0 never run.

## lineTime

`src/main/launchers/battlenet/client-log.ts:72:1-76:2`, 3 blocks, 0 never run.

## maskReason

`src/main/launchers/battlenet/client-log.ts:79:1-85:2`, 1 blocks, 0 never run.

## isForwarderLog

`src/main/launchers/battlenet/client-log.ts:26:1-28:2`, 2 blocks, 0 never run.

## lastSignIn

`src/main/launchers/battlenet/client-log.ts:180:1-188:2`, 8 blocks, 0 never run.

## endedSignedOut

`src/main/launchers/battlenet/client-log.ts:191:1-198:2`, 6 blocks, 0 never run.

## productStates

`src/main/launchers/battlenet/product-state.ts:18:1-22:2`, 4 blocks, 0 never run.

## productNeedsUpdate

`src/main/launchers/battlenet/product-state.ts:25:1-27:2`, 1 blocks, 0 never run.

## gameStarted

`src/main/launchers/battlenet/product-state.ts:30:1-36:2`, 6 blocks, 0 never run.

## gameRunning

`src/main/launchers/battlenet/product-state.ts:39:1-47:2`, 8 blocks, 0 never run.

## PRODUCTS

A value (`src/main/launchers/battlenet/products.ts:25:14-48:2`), no code of its own to cover.

## productCodes

`src/main/launchers/battlenet/products.ts:51:1-56:2`, 8 blocks, 0 never run.

## parseProductDb

`src/main/launchers/battlenet/products.ts:112:1-137:2`, 20 blocks, 0 never run.

## parseAggregate

`src/main/launchers/battlenet/products.ts:148:1-173:2`, 23 blocks, 0 never run.
- not measured: `src/main/launchers/battlenet/products.ts:155:49-155:91` ?: true arm, no mapping from the source to the executed code: `(raw as { installed?: unknown }).installed`

## launchCommand

`src/main/launchers/battlenet/products.ts:178:1-183:2`, 3 blocks, 0 never run.

## serializeBlob

`src/main/launchers/battlenet/session.ts:43:1-45:2`, 1 blocks, 0 never run.

## parseBlob

`src/main/launchers/battlenet/session.ts:48:1-56:2`, 10 blocks, 0 never run.
