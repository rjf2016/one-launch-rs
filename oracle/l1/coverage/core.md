# Coverage: core

Generators: `core/artwork.gen.ts`, `core/fs-path.gen.ts`, `core/install.gen.ts`, `core/ipc.gen.ts`, `core/logger.gen.ts`, `core/manifest.gen.ts`, `core/migrate.gen.ts`, `core/paths.gen.ts`, `core/renderer-phases.gen.ts`, `core/renderer-recovery.gen.ts`, `core/renderer-sort.gen.ts`, `core/renderer-time.gen.ts`, `core/whats-new.gen.ts`, `core/window-guard.gen.ts`

| Export | Source | Blocks | Never run | Not measured |
| --- | --- | --- | --- | --- |
| `canonicalPayload` | `src/main/updates/manifest.ts` | 1 | 0 | 0 |
| `verifyManifest` | `src/main/updates/manifest.ts` | 44 | 0 | 0 |
| `updateAvailability` | `src/main/updates/availability.ts` | 3 | 0 | 0 |
| `redactString` | `src/main/services/logger.ts` | 8 | 0 | 0 |
| `redactValue` | `src/main/services/logger.ts` | 33 | 1 | 2 |
| `formatLine` | `src/main/services/logger.ts` | 5 | 0 | 0 |
| `resolveArtworkPath` | `src/main/services/artwork-url.ts` | 13 | 0 | 0 |
| `sniffImage` | `src/main/services/artwork-url.ts` | 20 | 0 | 0 |
| `normalizeLauncherPath` | `src/main/platform/fs-path.ts` | 4 | 0 | 0 |
| `pathKey` | `src/main/platform/fs-path.ts` | 1 | 0 | 0 |
| `dataDirFromArgv` | `src/main/platform/paths.ts` | 11 | 0 | 0 |
| `isTrustedRendererUrl` | `src/main/window-guard.ts` | 8 | 0 | 0 |
| `closeRequestAction` | `src/main/window-guard.ts` | 7 | 0 | 0 |
| `hideAfterInstantPlay` | `src/main/window-guard.ts` | 7 | 0 | 0 |
| `startHidden` | `src/main/window-guard.ts` | 2 | 0 | 0 |
| `placementFrom` | `src/main/window-guard.ts` | 15 | 0 | 0 |
| `loginItemAvailable` | `src/main/platform/login-item.ts` | 1 | 0 | 0 |
| `compareVersions` | `src/shared/whats-new.ts` | 24 | 0 | 0 |
| `pickWhatsNew` | `src/shared/whats-new.ts` | 15 | 0 | 0 |
| `migrate` | `src/main/db/index.ts` | 9 | 0 | 1 |
| `registerIpcHandlers` | `src/main/ipc/index.ts` | 141 | 54 | 1 |
| `phaseOf` | `src/renderer/src/lib/phases.ts` | 2 | 0 | 0 |
| `phaseLabel` | `src/renderer/src/lib/phases.ts` | 8 | 0 | 0 |
| `groupSteps` | `src/renderer/src/lib/phases.ts` | 33 | 0 | 0 |
| `sentence` | `src/renderer/src/lib/phases.ts` | 1 | 0 | 0 |
| `stoppedNote` | `src/renderer/src/lib/phases.ts` | 15 | 0 | 0 |
| `jobAnnouncement` | `src/renderer/src/lib/phases.ts` | 15 | 0 | 0 |
| `planPhases` | `src/renderer/src/lib/phases.ts` | 7 | 0 | 0 |
| `recoveryCopy` | `src/renderer/src/lib/recovery.ts` | 9 | 0 | 0 |
| `formatRelative` | `src/renderer/src/lib/time.ts` | 8 | 0 | 1 |
| `formatDateTime` | `src/renderer/src/lib/time.ts` | 3 | 0 | 0 |
| `sortAccounts` | `src/renderer/src/views/AccountsView.tsx` | 9 | 0 | 0 |
| `lastPlayedOf` | `src/renderer/src/components/GameCard.tsx` | 8 | 0 | 0 |

## canonicalPayload

`src/main/updates/manifest.ts:29:1-31:2`, 1 blocks, 0 never run.

## verifyManifest

`src/main/updates/manifest.ts:56:1-89:2`, 44 blocks, 0 never run.

Same-module helper `refuse` (`src/main/updates/manifest.ts:48:1-50:2`): 1 blocks, 0 never run.

Same-module helper `field` (`src/main/updates/manifest.ts:52:1-54:2`): 1 blocks, 0 never run.

Same-module helper `signatureMatches` (`src/main/updates/manifest.ts:91:1-98:2`): 3 blocks, 0 never run.

## updateAvailability

`src/main/updates/availability.ts:25:1-28:2`, 3 blocks, 0 never run.

Same-module helper `perUserInstall` (`src/main/updates/availability.ts:31:1-39:2`): 11 blocks, 0 never run.

## redactString

`src/main/services/logger.ts:107:1-118:2`, 8 blocks, 0 never run.

Same-module helper `maskAccountNames` (`src/main/services/logger.ts:97:1-105:2`): 7 blocks, 0 never run.

## redactValue

`src/main/services/logger.ts:123:1-150:2`, 33 blocks, 1 never run.

- `src/main/services/logger.ts:138:34-138:55` if arm: `return String(value);`
- not measured: `src/main/services/logger.ts:123:53-123:54` default parameter, V8 keeps no counter for a default parameter: `0`
- not measured: `src/main/services/logger.ts:123:63-123:84` default parameter, V8 keeps no counter for a default parameter: `new WeakSet<object>()`

Same-module helper `maskAccountNames` (`src/main/services/logger.ts:97:1-105:2`): 7 blocks, 0 never run.

## formatLine

`src/main/services/logger.ts:262:1-278:2`, 5 blocks, 0 never run.

Same-module helper `onOneLine` (`src/main/services/logger.ts:258:1-260:2`): 1 blocks, 0 never run.

Same-module helper `clip` (`src/main/services/logger.ts:254:1-256:2`): 3 blocks, 0 never run.

Same-module helper `maskAccountNames` (`src/main/services/logger.ts:97:1-105:2`): 7 blocks, 0 never run.

## resolveArtworkPath

`src/main/services/artwork-url.ts:36:1-52:2`, 13 blocks, 0 never run.

## sniffImage

`src/main/services/artwork-url.ts:69:1-87:2`, 20 blocks, 0 never run.

## normalizeLauncherPath

`src/main/platform/fs-path.ts:8:1-17:2`, 4 blocks, 0 never run.

## pathKey

`src/main/platform/fs-path.ts:20:1-22:2`, 1 blocks, 0 never run.

## dataDirFromArgv

`src/main/platform/paths.ts:29:1-38:2`, 11 blocks, 0 never run.

## isTrustedRendererUrl

`src/main/window-guard.ts:8:1-18:2`, 8 blocks, 0 never run.

## closeRequestAction

`src/main/window-guard.ts:26:1-30:2`, 7 blocks, 0 never run.

## hideAfterInstantPlay

`src/main/window-guard.ts:38:1-48:2`, 7 blocks, 0 never run.

## startHidden

`src/main/window-guard.ts:51:1-53:2`, 2 blocks, 0 never run.

## placementFrom

`src/main/window-guard.ts:56:1-71:2`, 15 blocks, 0 never run.

## loginItemAvailable

`src/main/platform/login-item.ts:15:1-17:2`, 1 blocks, 0 never run.

## compareVersions

`src/shared/whats-new.ts:57:1-78:2`, 24 blocks, 0 never run.

Same-module helper `parseVersion` (`src/shared/whats-new.ts:40:1-45:2`): 5 blocks, 0 never run.

Same-module helper `compareIdentifiers` (`src/shared/whats-new.ts:47:1-54:2`): 12 blocks, 0 never run.

## pickWhatsNew

`src/shared/whats-new.ts:84:1-94:2`, 15 blocks, 0 never run.

Same-module helper `isVersionString` (`src/shared/whats-new.ts:36:1-38:2`): 3 blocks, 0 never run.

Same-module helper `parseVersion` (`src/shared/whats-new.ts:40:1-45:2`): 5 blocks, 0 never run.

Same-module helper `compareIdentifiers` (`src/shared/whats-new.ts:47:1-54:2`): 12 blocks, 0 never run.

## migrate

`src/main/db/index.ts:130:1-143:2`, 9 blocks, 0 never run.
- not measured: `src/main/db/index.ts:130:68-130:78` default parameter, V8 keeps no counter for a default parameter: `MIGRATIONS`

Same-module helper `validateMigrations` (`src/main/db/index.ts:174:1-185:2`): 8 blocks, 2 never run.

- `src/main/db/index.ts:177:33-179:6` if arm, a throw: `throw new Error('Migrations must be contiguous from 1; found version ${m.version}, expected ${expect`
- `src/main/db/index.ts:180:66-182:6` if arm, a throw: `throw new Error('Malformed migration at version ${m.version}');`

Same-module helper `getSchemaVersion` (`src/main/db/index.ts:111:1-113:2`): 1 blocks, 0 never run.

Same-module helper `applyWithForeignKeysOff` (`src/main/db/index.ts:158:1-172:2`): 7 blocks, 2 never run.

- `src/main/db/index.ts:162:60-164:6` if arm, a throw: `throw new Error('Migration ${label} rebuilds tables, but foreign keys could not be switched off (is `
- `src/main/db/index.ts:167:31-167:128` if arm, a throw: `throw new Error('Migration ${label} left ${orphans.length} row(s) pointing at a missing parent');`

Same-module helper `apply` (`src/main/db/index.ts:145:1-151:2`): 3 blocks, 0 never run.
- not measured: `src/main/db/index.ts:145:65-145:80` default parameter, V8 keeps no counter for a default parameter: `() => undefined`

## registerIpcHandlers

`src/main/ipc/index.ts:97:1-560:2`, 141 blocks, 54 never run.

- `src/main/ipc/index.ts:113:22-124:5` nested function (and 3 inside it): `(): AppInfo => ({`
- `src/main/ipc/index.ts:129:37-129:55` ?: true arm: `deps.paths.logsDir`
- `src/main/ipc/index.ts:129:58-129:123` ?: false arm (and 2 inside it): `target === 'backups' ? deps.paths.backupsDir : deps.paths.dataDir`
- `src/main/ipc/index.ts:131:18-131:68` if arm, a throw: `throw new IpcRequestError('OPEN_FAILED', problem);`
- `src/main/ipc/index.ts:131:5-131:68` if never false: `if (problem) throw new IpcRequestError('OPEN_FAILED', problem);`
- `src/main/ipc/index.ts:146:83-146:87` ?? right side: `null`
- `src/main/ipc/index.ts:153:26-153:57` nested function: `() => deps.vaultHealth.status()`
- `src/main/ipc/index.ts:155:25-159:4` nested function: `() => {`
- `src/main/ipc/index.ts:198:7-198:120` if never false: `if (!isLogLevel(value)) throw new IpcRequestError('BAD_REQUEST', 'logLevel must be debug, info, warn`
- `src/main/ipc/index.ts:219:5-221:6` if never false: `if (key === 'backups.keep' && !(/^\d{1,3}$/.test(value) && Number(value) >= 1 && Number(value) <= MA`
- `src/main/ipc/index.ts:222:79-222:113` && right side: `deps.settings.get(key) === 'false'`
- `src/main/ipc/index.ts:222:59-222:75` && right side: `value === 'true'`
- `src/main/ipc/index.ts:226:25-229:6` if arm (and 2 inside it): `const queued = deps.artwork.enqueueAfterScan();`
- `src/main/ipc/index.ts:228:7-228:62` if never false: `if (queued > 0) log.info('artwork queued', { queued });`
- `src/main/ipc/index.ts:233:29-233:73` nested function: `() => ({ settings: deps.settings.getAll() })`
- `src/main/ipc/index.ts:239:27-239:69` nested function: `() => ({ accounts: deps.accounts.list() })`
- `src/main/ipc/index.ts:255:89-255:91` ?? right side: `''`
- `src/main/ipc/index.ts:276:30-276:50` if arm: `patch.notes = notes;`
- `src/main/ipc/index.ts:315:28-317:6` if arm: `text = deps.accounts.require(id).login;`
- `src/main/ipc/index.ts:317:12-320:6` else arm (and 2 inside it): `text = deps.accounts.decryptPassword(id);`
- `src/main/ipc/index.ts:319:7-319:109` if never false: `if (text === null) throw new IpcRequestError('NO_PASSWORD', 'No password is stored for this account'`
- `src/main/ipc/index.ts:328:26-328:97` nested function: `() => ({ games: deps.library.list(), revision: deps.library.revision })`
- `src/main/ipc/index.ts:330:28-333:4` nested function: `async () => {`
- `src/main/ipc/index.ts:340:85-340:89` ?? right side: `null`
- `src/main/ipc/index.ts:347:28-347:69` nested function: `() => ({ status: deps.artwork.status() })`
- `src/main/ipc/index.ts:355:16-355:38` if arm: `return { game: null };`
- `src/main/ipc/index.ts:355:5-355:38` if never false: `if (!file) return { game: null };`
- `src/main/ipc/index.ts:362:30-362:88` nested function: `async () => ({ launchers: await deps.launchers.status() })`
- `src/main/ipc/index.ts:366:33-368:4` nested function (and 1 inside it): `(launcher: LauncherId): void => {`
- `src/main/ipc/index.ts:375:16-375:41` if arm: `return { install: null };`
- `src/main/ipc/index.ts:375:5-375:41` if never false: `if (!file) return { install: null };`
- `src/main/ipc/index.ts:377:19-382:6` if arm, a throw: `throw new IpcRequestError(`
- `src/main/ipc/index.ts:377:5-382:6` if never false: `if (!install) {`
- `src/main/ipc/index.ts:470:42-470:115` if arm, a throw: `throw new IpcRequestError('NOT_FOUND', 'That snapshot no longer exists');`
- `src/main/ipc/index.ts:485:85-485:89` ?? right side: `null`
- `src/main/ipc/index.ts:492:85-492:89` ?? right side: `null`
- `src/main/ipc/index.ts:515:81-515:106` ?: true arm: `Math.min(raw, MAX_RECENT)`
- `src/main/ipc/index.ts:515:71-515:78` && right side: `raw > 0`
- `src/main/ipc/index.ts:515:46-515:67` && right side: `Number.isInteger(raw)`
- `src/main/ipc/index.ts:522:26-522:67` nested function: `() => ({ recovery: deps.recovery.get() })`
- `src/main/ipc/index.ts:530:30-533:4` nested function: `() => {`
- `src/main/ipc/index.ts:537:28-537:55` nested function: `() => deps.updates.status()`
- `src/main/ipc/index.ts:539:27-539:53` nested function: `() => deps.updates.check()`
- `src/main/ipc/index.ts:549:30-549:59` nested function: `() => deps.loginItem.status()`
- `src/main/ipc/index.ts:555:53-555:161` if arm, a throw: `throw new IpcRequestError('UNAVAILABLE', 'Start with Windows is not available for this copy of One L`
- `src/main/ipc/index.ts:555:5-555:161` if never false: `if (!(await deps.loginItem.status()).available) throw new IpcRequestError('UNAVAILABLE', 'Start with`
- not measured: `src/main/ipc/index.ts:515:109-515:111` ?: false arm, no mapping from the source to the executed code: `10`

## phaseOf

`src/renderer/src/lib/phases.ts:25:1-27:2`, 2 blocks, 0 never run.

## phaseLabel

`src/renderer/src/lib/phases.ts:35:1-48:2`, 8 blocks, 0 never run.

## groupSteps

`src/renderer/src/lib/phases.ts:62:1-82:2`, 33 blocks, 0 never run.

## sentence

`src/renderer/src/lib/phases.ts:85:1-87:2`, 1 blocks, 0 never run.

## stoppedNote

`src/renderer/src/lib/phases.ts:94:1-105:2`, 15 blocks, 0 never run.

## jobAnnouncement

`src/renderer/src/lib/phases.ts:111:1-125:2`, 15 blocks, 0 never run.

## planPhases

`src/renderer/src/lib/phases.ts:128:1-133:2`, 7 blocks, 0 never run.

## recoveryCopy

`src/renderer/src/lib/recovery.ts:11:1-41:2`, 9 blocks, 0 never run.

## formatRelative

`src/renderer/src/lib/time.ts:13:1-22:2`, 8 blocks, 0 never run.
- not measured: `src/renderer/src/lib/time.ts:13:57-13:67` default parameter, V8 keeps no counter for a default parameter: `new Date()`

## formatDateTime

`src/renderer/src/lib/time.ts:24:1-27:2`, 3 blocks, 0 never run.

## sortAccounts

`src/renderer/src/views/AccountsView.tsx:16:1-23:2`, 9 blocks, 0 never run.

## lastPlayedOf

`src/renderer/src/components/GameCard.tsx:19:1-25:2`, 8 blocks, 0 never run.
