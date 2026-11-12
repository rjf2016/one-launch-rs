# Takeover contract: installer and updater

What a Rust installer, uninstaller, updater and app must satisfy so that an installed Electron copy updates to the Rust build through its own updater, and an Electron build can later replace a Rust one. Built from `plan/design/takeover.md`; every line of that design is mapped to the assertions below in `scripts/oracle/takeover/coverage.json` (Electron repo, branch `oracle-tooling`).

Every assertion was checked by reading the cited source at the pin (`oracle/PIN`). Nothing here was run, except where a line says "measured". Where the source says something other than the design, the assertion states what the source says and ends with **Differs from the design:** and the difference.

## How to read a citation

- A path is relative to the Electron repo (`E\`) unless it starts with `oracle/`, which is this bundle. Tracked files are cited as they are at the pin; `node_modules/` is the installed tree of the pin's lockfile: app-builder-lib 26.15.3 (electron-builder 26.15.3), electron-updater 6.8.10, Electron 44.7.0. The templates under `node_modules/app-builder-lib/templates/nsis/` are vendored byte for byte under `oracle/takeover/nsis/templates/`, so every template line cited here is also that line of the vendored copy, except `include/StdUtils.nsh`, which is recorded by digest in `oracle/takeover/nsis/templates-held-back.json` and read from the Electron repo.
- `src/main/index.ts` lines are the pin's (the `oracle-tooling` branch moved them).
- `${NAME}` is an NSIS define; the values the build passes are in `oracle/takeover/nsis/defines.json`. `<GUID>` is `37275e3e-77ba-54aa-bc32-31551d28dc0d`. `<v>` is a release version.

## Check methods

| Tag | Meaning |
| --- | --- |
| `static` | A read of the cited source suffices; a reviewer or a test over the vendored file can confirm it. |
| `argv` | The command line of a spawned process, caught on a hosted runner (or in a unit test of the Rust updater with the spawn stubbed). |
| `runner-reg` | A registry read on a GitHub-hosted runner after the step. |
| `runner-fs` | Files and shortcuts on a GitHub-hosted runner after the step. |
| `runner-proc` | Process behaviour on a GitHub-hosted runner (what is waited for, ended, started). |
| `runner-exit` | The exit code of an installer or uninstaller on a hosted runner. |
| `unit` | A Rust unit test against the ported function, with vectors from this bundle or hand-ported tests. |
| `local` | Measured on a scratch folder on a developer machine (the vault fixture). |
| `owner` | Only the owner can check it (section O). |

## U. What the Electron copy does at update time

**U1** The Electron copy starts the installer from its `quit` event, after `will-quit` has closed the database: `installAtQuit(exitCode)` runs in `app.on('quit')` (`src/main/index.ts:756-769`, `src/main/updates/update-service.ts:142-172`). It calls `engine.install(relaunch)`, which is electron-updater's `install(isSilent = true, isForceRunAfter = relaunch)` (`src/main/updates/engine.ts:91-93`, `node_modules/electron-updater/out/BaseUpdater.js:42-68`). Check: `static`.

**U2** The argv is exactly `--updated`, `/S`, and `--force-run` only when `relaunch` is true, in that order; the executable is the downloaded installer (`node_modules/electron-updater/out/NsisUpdater.js:101-113`). `relaunch` is true only after "Restart to update" (`requestInstall`, `src/main/updates/update-service.ts:121-126,146`). Check: `argv`.

**U3** The process is spawned with `detached: true`, `stdio: 'ignore'`, no `env` (so the app's environment is inherited), and `unref()`; the call returns once a pid exists (`node_modules/electron-updater/out/BaseUpdater.js:129-147`). Check: `argv`.

**U4** The installer path is `%LOCALAPPDATA%\one-launch-updater\pending\<file>`, where `<file>` is the base name of the manifest's `files[0].url` (for this project `OneLaunch-<v>-setup.exe`): the cache root is `LOCALAPPDATA` (`node_modules/electron-updater/out/AppAdapter.js:6-20`), the folder is `updaterCacheDirName` from `resources\app-update.yml` (`node_modules/electron-updater/out/AppUpdater.js:542-556`), which electron-builder sets to the package name plus `-updater` (`node_modules/app-builder-lib/out/appInfo.js:126-128`, `node_modules/app-builder-lib/out/publish/PublishManager.js:198-201`), and the file sits in its `pending` subfolder (`node_modules/electron-updater/out/DownloadedUpdateHelper.js:30-32`, `node_modules/electron-updater/out/AppUpdater.js:573-589`). Check: `static`, `runner-fs`.

**U5** No `/D=` is passed: `installDirectory` is never set by the app (`node_modules/electron-updater/out/NsisUpdater.js:114-117`, `src/main/updates/engine.ts:51-71`). No `--package-file=` is passed: there is a package file only for a web installer, which the app disables and the manifest check refuses (`node_modules/electron-updater/out/NsisUpdater.js:118-122,40-43`, `src/main/updates/engine.ts:70`, `src/main/updates/manifest.ts:60-61`). Check: `argv`.

**U6** No elevation in the normal path: `elevate.exe` is used when `files[0].isAdminRightsRequired` is true, which `verifyManifest` refuses before any download (`node_modules/electron-updater/out/NsisUpdater.js:123-130`, `src/main/updates/manifest.ts:66`), and also when the plain spawn fails with `EACCES` or `UNKNOWN`, through `resources\elevate.exe`, which electron-builder copies into every NSIS build unless `packElevateHelper` is false (`node_modules/electron-updater/out/NsisUpdater.js:131-138`, `node_modules/app-builder-lib/out/targets/nsis/nsisUtil.js:56-89`). On `ENOENT` it opens the installer through `shell.openPath` instead (`node_modules/electron-updater/out/NsisUpdater.js:139-143`). Check: `static`. **Differs from the design:** the design says "no elevation"; the source keeps an `elevate.exe` fallback for a spawn that fails with `EACCES` or `UNKNOWN`, and a `shell.openPath` fallback for `ENOENT`.

**U7** No Authenticode check: `verifySignature` returns at once when `app-update.yml` has no `publisherName` (`node_modules/electron-updater/out/NsisUpdater.js:84-100`), and electron-builder writes one only when a signing certificate or `signtoolOptions.publisherName` is configured (`node_modules/app-builder-lib/out/publish/PublishManager.js:202-207`, `node_modules/app-builder-lib/out/codeSign/windowsSignToolManager.js:18-29`); `electron-builder.yml` configures neither (`electron-builder.yml:32-47`). Check: `static`; `runner-fs` (read `resources\app-update.yml` of the public release).

**U8** Before a download counts, electron-updater requires a version that is semver-greater than the running one (equal is "no update"; lower is refused because `allowDowngrade` is false) (`node_modules/electron-updater/out/AppUpdater.js:339-363`, `src/main/updates/engine.ts:69`), and the downloaded bytes must match `files[0].sha512` (`node_modules/electron-updater/out/AppUpdater.js:559-566,644-655`). The app adds the Ed25519 manifest check before the download starts (`src/main/updates/update-service.ts:237-244`, `src/main/updates/manifest.ts:56-89`) and its own SHA-512 check after it (`src/main/updates/update-service.ts:267-277`). Check: `static`, `unit`.

**U9** A `stagingPercentage` in `latest.yml` is honoured: every copy sends the staging id `00000000-0000-0000-0000-000000000000`, so a percentage above 0 lets every copy update and 0 lets none (`node_modules/electron-updater/out/AppUpdater.js:314-332`, `src/main/updates/engine.ts:12-17`). Check: `static`.

**U10** The download first tries `<installer url>.blockmap` and the old version's blockmap (the new URL with the new version replaced by the old) and falls back to a full download on any error on Windows (`node_modules/electron-updater/out/NsisUpdater.js:47-51`, `node_modules/electron-updater/out/providers/Provider.js:22-26`, `node_modules/electron-updater/out/AppUpdater.js:660-735`). A release therefore needs no blockmap for clients. The differential path also reads `%LOCALAPPDATA%\one-launch-updater\installer.exe`, the copy the stock installer leaves (I4.10). Check: `static`.

**U11** `scripts/release-sign.mjs` refuses a draft that lacks `<installer>.blockmap` (`scripts/release-sign.mjs:34-35`) and imports `src/main/updates/manifest.ts` and `src/main/updates/release-keys.ts` (`scripts/release-sign.mjs:11-12`), so a Rust release flow must port it or keep these two files. Whether the blockmap stays required is a triage row. Check: `static`.

## I. Installer contract

The installer is electron-builder's assisted NSIS installer (`oneClick: false`, per-user, folder choice allowed) with `build/installer.nsh` included (`electron-builder.yml:37-47`). The registry key is `HKCU\Software\<GUID>` (`node_modules/app-builder-lib/templates/nsis/multiUser.nsh:8`), the uninstall key `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\<GUID>` (`node_modules/app-builder-lib/templates/nsis/multiUser.nsh:9`). `<GUID>` is UUIDv5 of `appId` in electron-builder's namespace (`node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:28,157-158`, `src/main/updates/availability.ts:6-8`).

### I1. Init (`.onInit`)

**I1.1** The installer requests user level, not admin: `INSTALL_MODE_PER_ALL_USERS` is undefined (`node_modules/app-builder-lib/templates/nsis/installer.nsi:20-28`). Check: `static`.

**I1.2** Unless it is the UAC inner instance, it calls `CreateMutex(NULL, TRUE, "<GUID>")`; on `ERROR_ALREADY_EXISTS` (183) it brings the other installer's window to the front and aborts (`node_modules/app-builder-lib/templates/nsis/installer.nsi:72-74`, `node_modules/app-builder-lib/templates/nsis/include/allowOnlyOneInstallerInstance.nsh:11-30`). The name has no `Global\` prefix, so the mutex is per session. The uninstaller creates no mutex (`node_modules/app-builder-lib/templates/nsis/uninstaller.nsh:5-36`). Check: `static`; `runner-proc` (a second installer started while one runs leaves at once).

**I1.3** The install mode comes from the registry: a value `InstallLocation` under `HKLM\Software\<GUID>` marks a per-machine install, under `HKCU\Software\<GUID>` a per-user one; `/allusers` or `/currentuser` on the command line overrides both; per-user only (or neither) selects per-user (`node_modules/app-builder-lib/templates/nsis/assistedInstaller.nsh:84-154`). Check: `static`.

**I1.4** Per-user: `$INSTDIR` is `HKCU\Software\<GUID>\InstallLocation` when set, else `<UserProgramFiles>\OneLaunch` (the `UserProgramFiles` known folder, falling back to `%LOCALAPPDATA%\Programs`), and a `/D=` anywhere on the command line overrides it (`node_modules/app-builder-lib/templates/nsis/multiUser.nsh:21-56,105-141`). `OneLaunch` is `${APP_FILENAME}` (`oracle/takeover/nsis/defines.json` and `node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:166`). Check: `static`, `runner-reg`.

**I1.5** Then `customInit` from `build/installer.nsh` runs: when the folder came from the registry and ends in `\One Launch` (compared case-insensitively, LogicLib `==`), it becomes the same parent plus `\OneLaunch` (`node_modules/app-builder-lib/templates/nsis/installer.nsi:79-81`, `build/installer.nsh:2-18`). The wizard does the same in the install-mode page's leave function (`build/installer.nsh:20-33`). Check: `static`, `runner-fs`.

**I1.6** `/S` (NSIS's own silent switch) skips every page; the licence and directory pages are also skipped with `--updated` (`node_modules/app-builder-lib/templates/nsis/common.nsh:110-121`, `node_modules/app-builder-lib/templates/nsis/assistedInstaller.nsh:13-26`). Check: `runner-proc`.

**I1.7** A silent run against a per-machine install (`hasPerMachineInstallation` = 1) that is not elevated re-launches itself elevated through UAC and quits (`node_modules/app-builder-lib/templates/nsis/installer.nsi:99-121`). Check: `static`. **Differs from the design:** the design says `$INSTDIR` comes from `HKCU\...\InstallLocation`; that holds only when no `HKLM` install location exists (or `/currentuser` is given). With an all-users install, a silent update reads `HKLM` and elevates.

**I1.8** Flags are tested with `${StdUtils.TestParameter}` for `updated`, `force-run`, `keep-shortcuts`, `no-desktop-shortcut`, `delete-app-data`, `allusers`, `currentuser` (`node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:579`, `node_modules/app-builder-lib/out/targets/nsis/nsisScriptGenerator.js:29-38`), and with `${GetOptions}` (FileFunc, case-insensitive) for `/allusers`, `/currentuser` in init and `/S`, `--delete-app-data` in the uninstaller (`node_modules/app-builder-lib/templates/nsis/assistedInstaller.nsh:122-133`, `node_modules/app-builder-lib/templates/nsis/uninstaller.nsh:12-16,219-221`). How StdUtils matches a `--name` token is in the plugin binary and cannot be read here. Check: `runner-proc` (run the stock installer with `--updated` and see that pages and the uninstall flags follow it).

### I2. Wait for the app (`CHECK_APP_RUNNING`)

**I2.1** Runs in the install section unless this is the UAC inner instance (`node_modules/app-builder-lib/templates/nsis/installSection.nsh:35-37`), and in the uninstaller's `un.onInit` when silent (`node_modules/app-builder-lib/templates/nsis/uninstaller.nsh:1-3,18-19`). Check: `static`.

**I2.2** `build/installer.nsh` defines no `customCheckAppRunning`, so the stock check is used (`node_modules/app-builder-lib/templates/nsis/include/allowOnlyOneInstallerInstance.nsh:5-8,32-43`, `build/installer.nsh:1-57`). Check: `static`.

**I2.3** It first checks that `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe` has `Get-CimInstance` and that the process execution policy is not `Restricted` (`node_modules/app-builder-lib/templates/nsis/include/allowOnlyOneInstallerInstance.nsh:35-36,45-62`). Check: `static`.

**I2.4** With PowerShell available, "running" means any process from `Get-CimInstance -ClassName Win32_Process` (WMI) whose `Path` starts with `$INSTDIR`, case-insensitively; it is stopped with `Stop-Process -Id <pid>`, and forced with `-Force` (`node_modules/app-builder-lib/templates/nsis/include/allowOnlyOneInstallerInstance.nsh:64-67,81-95`). Without it, `tasklist` filtered to this user and the image name `OneLaunch.exe`, then `taskkill /IM OneLaunch.exe` excluding its own pid, `/F` when forced (`node_modules/app-builder-lib/templates/nsis/include/allowOnlyOneInstallerInstance.nsh:68-78,96-101`). Check: `static`. Replacing the WMI query (the `customCheckAppRunning` hook) is a triage decision; it is recorded here as it ships.

**I2.5** Sequence (`node_modules/app-builder-lib/templates/nsis/include/allowOnlyOneInstallerInstance.nsh:105-164`): skipped when the installer's own image name is `OneLaunch.exe`; with `--updated`, sleep 300 ms; look; if running: with `--updated` sleep 1 s (without, a message box whose silent default is OK); stop (not forced); sleep 300 ms; then up to two rounds of: look, and if still running sleep 1 s, force, look, and if still running sleep 2 s; after the second round a retry/cancel box whose silent default is Cancel, which quits the installer. Check: `runner-proc`. **Differs from the design:** the design's "300 ms, then 1 s, then stop, then force" is the first round; the source also matches every process under `$INSTDIR` (not only `OneLaunch.exe`), has an image-name fallback without PowerShell, and on a silent run that cannot close the app quits without installing.

### I3. Run the old uninstaller

**I3.1** `uninstallOldVersion` reads `UninstallString` from the uninstall key in the install's root (`SHELL_CONTEXT`, `HKCU` for per-user); with none it does nothing (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:142-164`, `node_modules/app-builder-lib/templates/nsis/installSection.nsh:52-53`). The executable is the first quoted string of that value; the folder is `InstallLocation`, else the uninstaller's parent folder (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:166-182`). Check: `static`.

**I3.2** The uninstaller is copied to `$PLUGINSDIR\old-uninstaller.exe` (the installer's temp plug-ins folder) and run with `ExecWait '"<copy>" /S /KEEP_APP_DATA <args> _?=<InstallLocation>'`, where `<args>` is `/currentuser` (per-user), then ` --keep-shortcuts` when I5.1 holds, then ` --updated`, or ` --delete-app-data` instead of ` --updated` when the installer itself was given `--delete-app-data` (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:184-224`). If the copy cannot be started it runs the original in place with the same arguments (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:227-231`). Check: `argv`.

**I3.3** A non-zero exit is retried after 1 s, at most five runs; then a retry/cancel box whose silent default is Cancel (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:212-239`). A non-zero last result makes the installer exit with code 2 (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:128-133`). If neither copy could be started, the installer logs it and carries on installing over the old files (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:124-126,241-242`). Check: `runner-exit`. **Differs from the design:** "a non-zero result aborts with exit 2" holds; an uninstaller that cannot be started at all does not abort.

**I3.4** For an all-users install the per-user key is also uninstalled in a second pass (`node_modules/app-builder-lib/templates/nsis/installSection.nsh:55-58`). Check: `static`.

**I3.5** `/KEEP_APP_DATA` is read nowhere in the templates. App data survives because the uninstaller deletes `%APPDATA%\OneLaunch` and `%APPDATA%\one-launch` only with `--delete-app-data`, or with `DELETE_APP_DATA_ON_UNINSTALL` defined and no `--updated`, and this build does not define it (`node_modules/app-builder-lib/templates/nsis/uninstaller.nsh:216-248`, `electron-builder.yml:43-45`, `oracle/takeover/nsis/defines.json`). Check: `static`. **Differs from the design:** the design reads `/KEEP_APP_DATA` as the reason data is kept; it is inert.

**I3.6** What the old (stock) uninstaller does with these flags, in order (`node_modules/app-builder-lib/templates/nsis/uninstaller.nsh:5-36,145-259`): silent from `/S`; waits for the app (I2); picks the mode from `/currentuser` and reads `$INSTDIR` from `InstallLocation`; runs `customUnInstall`, which with `--updated` leaves the Run values alone (`build/installer.nsh:52-57`); with `--updated` renames every file of `$INSTDIR` into `$PLUGINSDIR\old-install` and, if one is busy, puts them back and aborts; then removes `$INSTDIR` recursively; without `--keep-shortcuts` removes the AUMID registration, the old desktop link and the old Start-menu link; deletes nothing under `%APPDATA%` (I3.5); deletes the uninstall key and `HKCU\Software\<GUID>`. Check: `runner-fs`, `runner-reg`.

### I4. Extract and register

**I4.1** Files are extracted with `Nsis7z` into `$PLUGINSDIR\7z-out` and copied into `$INSTDIR`, retried five times, then a retry/cancel box whose silent default is Retry, then an in-place extract as a last resort (`node_modules/app-builder-lib/templates/nsis/include/extractAppPackage.nsh:1-17,92-138`). Check: `runner-fs`.

**I4.2** `Uninstall OneLaunch.exe` is written into `$INSTDIR` (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:100`, `node_modules/app-builder-lib/templates/nsis/common.nsh:16-17`). Check: `runner-fs`.

**I4.3** `HKCU\Software\<GUID>`: `InstallLocation` = `$INSTDIR`, `KeepShortcuts` = `true`, `ShortcutName` = `One Launch` (all REG_SZ); no `MenuDirectory` (no `MENU_FILENAME`) (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:103-109`). Check: `runner-reg`.

**I4.4** Uninstall key `DisplayName` = `One Launch <v>` (`${UNINSTALL_DISPLAY_NAME}`, suffix empty for both modes) (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:111-119`, `node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:473`). Check: `runner-reg`.

**I4.5** `UninstallString` = `"<INSTDIR>\Uninstall OneLaunch.exe" /currentuser`; `QuietUninstallString` = the same plus ` /S` (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:121-123`). Check: `runner-reg`.

**I4.6** `DisplayVersion` = `<v>`; `DisplayIcon` = `<INSTDIR>\OneLaunch.exe,0` (no `UNINSTALLER_ICON`) (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:125-130`). Check: `runner-reg`.

**I4.7** `Publisher` = `rjf2016` (`${COMPANY_NAME}`, the package.json `author`) and `Comments` = `Launch any game, from any launcher, on any account.` (`${APP_DESCRIPTION}`) (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:132-138`, `node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:167,490-493`). No `HelpLink`, `URLInfoAbout`, `URLUpdateInfo` or `Readme`: package.json has no homepage (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:140-154`). Check: `runner-reg`. **Differs from the design:** the design's list lacks `Comments`.

**I4.8** `NoModify` = 1 and `NoRepair` = 1 (REG_DWORD); `EstimatedSize` (REG_DWORD, KiB) = `${ESTIMATED_SIZE}`, the unpacked size the build computed (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:156-167`, `node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:256-259`). Check: `runner-reg`.

**I4.9** The values are written after the old uninstaller has deleted both keys (I3.6, `node_modules/app-builder-lib/templates/nsis/uninstaller.nsh:250-254`, `node_modules/app-builder-lib/templates/nsis/installSection.nsh:52-67`), so after a takeover only what the new installer writes exists. Check: `runner-reg`.

**I4.10** The installer copies itself to `%LOCALAPPDATA%\one-launch-updater\installer.exe` (`${APP_INSTALLER_STORE_FILE}`), the base file of a later differential download (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:88-96`, `node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:502`). Check: `runner-fs`. Not in the design; a Rust installer that skips it only costs the next Electron updater its differential download (U10).

### I5. Shortcuts

**I5.1** Shortcuts are kept when the installer runs with `--updated` (folder choice is allowed, so without `--updated` the keep path is off), `HKCU\Software\<GUID>\KeepShortcuts` read `true` before the old uninstaller ran, and `$INSTDIR\OneLaunch.exe` exists (`node_modules/app-builder-lib/templates/nsis/installSection.nsh:40-50`, `node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:84-93`). On that path an existing link under an old name is renamed to the new one and given the AUMID; nothing is created (`node_modules/app-builder-lib/templates/nsis/include/installer.nsh:189-247`). Check: `runner-fs`.

**I5.2** Otherwise `$SMPROGRAMS\One Launch.lnk` and, unless `--no-desktop-shortcut`, `$DESKTOP\One Launch.lnk` are created, target `$INSTDIR\OneLaunch.exe`, icon index 0, description `${APP_DESCRIPTION}`, AUMID `dev.onelaunch.app` set with `WinShell::SetLnkAUMI` (`node_modules/app-builder-lib/templates/nsis/common.nsh:84-108`, `node_modules/app-builder-lib/templates/nsis/include/installer.nsh:189-247`). Check: `runner-fs`.

### I6. Run value

**I6.1** After files, registry and shortcuts, `customInstall` reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `OneLaunch`; when it exists and either the install is for all users or its first characters are not exactly `"<INSTDIR>\OneLaunch.exe"` (with the quotes, compared case-insensitively), it deletes that value and `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run` value `OneLaunch` (`node_modules/app-builder-lib/templates/nsis/installSection.nsh:81-83`, `build/installer.nsh:35-50`). Check: `runner-reg`.

**I6.2** Neither the installer nor the uninstaller ever writes the Run value; the app writes it through `setLoginItemSettings` with the exe path, `args: ['--start-hidden']` and `name: 'OneLaunch'` (`src/main/platform/login-item.ts:6-7,37-42`). So the Rust exe must be `OneLaunch.exe` in the same folder and accept `--start-hidden`, which shows the window hidden only with close-to-tray on (`src/main/window-guard.ts:50-53`). Check: `static`, `runner-reg`.

**I6.3** The uninstaller deletes both values unless it runs with `--updated` (`build/installer.nsh:52-57`). Check: `runner-reg`.

### I7. Relaunch

**I7.1** The new app is started only when the run is silent and has `--force-run`: `HideWindow`, then `${StdUtils.ExecShellAsUser}` of the launch link with verb `open` and the argument `--updated` (`node_modules/app-builder-lib/templates/nsis/installSection.nsh:85-89,104-110`, `node_modules/app-builder-lib/templates/nsis/common.nsh:123-132`). Check: `runner-proc`.

**I7.2** The launch link is `$SMPROGRAMS\One Launch.lnk` when it exists after I5, else `$INSTDIR\OneLaunch.exe` (`node_modules/app-builder-lib/templates/nsis/installSection.nsh:71-75`). Check: `runner-proc`.

**I7.3** The Electron app ignores `--updated` (no code reads it: `src/main/index.ts:76-94` parses only `--data-dir=` and the dev variables, `src/main/platform/paths.ts:29-38`; `--start-hidden` is the one other flag, `src/main/window-guard.ts:50-53`). The Rust app must start normally with `--updated` on its command line. Check: `runner-proc`.

### I8. Rust uninstaller

**I8.1** It must accept the stock command lines: `/S`, `/currentuser`, `/allusers`, `--updated`, `--keep-shortcuts`, `--delete-app-data`, `/KEEP_APP_DATA` (accepted and ignored) and `_?=<dir>` (NSIS: run in place for that folder, wait-able, last argument) (I3.2, `node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:224`), with the same meaning as I3.6, and exit 0 only when the folder is gone, so that a later Electron installer (whose `uninstallOldVersion` runs whatever `UninstallString` names) or the next Rust installer can drive it. Check: `argv`, `runner-exit`, `runner-fs`.

**I8.2** `UninstallString` and `QuietUninstallString` written by the Rust installer keep the I4.5 shape: the first quoted string is the uninstaller path (`node_modules/app-builder-lib/templates/nsis/include/installUtil.nsh:166-167`). Check: `runner-reg`.

**I8.3** With `--delete-app-data` the stock uninstaller deletes `%APPDATA%\OneLaunch` (the data folder) and `%APPDATA%\one-launch` (`node_modules/app-builder-lib/templates/nsis/uninstaller.nsh:216-248`). Whether the Rust uninstaller honours that flag, which the updater never passes, is a triage decision. Check: `static`.

### I9. Every Rust release

**I9.1** Every Rust release satisfies I1 to I8, because an Electron copy on any old version jumps straight to the latest release (U8 compares with the newest only: `node_modules/electron-updater/out/providers/GitHubProvider.js:92-93,158-175`). Check: `runner-reg`, `runner-fs`, `runner-proc` on each release's CI.

## A. App obligations

**A1** `SetCurrentProcessExplicitAppUserModelID("dev.onelaunch.app")` before any window: the Electron app calls `app.setAppUserModelId('dev.onelaunch.app')` before `ready` (`src/main/index.ts:88`), the shortcuts carry the same id (I5.2) and it is `appId` (`electron-builder.yml:1`). Check: `runner-proc`, `owner` (taskbar pin).

**A2** The exe carries its icon at resource index 0 (registry `DisplayIcon` and both shortcuts use index 0, I4.6 and I5.2: `node_modules/app-builder-lib/templates/nsis/include/installer.nsh:129,197,223`) and an `asInvoker` manifest (`requestedExecutionLevel level="asInvoker"` in the manifest embedded in `node_modules/electron/dist/electron.exe`, which electron-builder renames to `OneLaunch.exe`). Check: `static` (read the PE resources of the Rust exe).

**A3** The exe accepts `--data-dir=<absolute folder>` as Electron does (`src/main/platform/paths.ts:8-9,29-38`); a run with it is not the installed copy for updates or autostart (`src/main/updates/availability.ts:33`). Check: `unit`.

**A4** The first Rust version is semver-greater than the last Electron one (U8, `node_modules/electron-updater/out/AppUpdater.js:357-362`). Check: `static` (release CI compares with the latest published tag).

**A5** The schema stays at `user_version` 5 until cutover is declared stable, because an Electron build refuses a newer schema with `DB_TOO_NEW` (`src/main/db/index.ts:115-134`); `oracle/takeover/migrations/v5.json` is the schema to keep. Check: `unit`.

**A6** The vault stays `v10` until cutover is declared stable: Electron writes `v10` blobs through `safeStorage` (`src/main/services/vault.ts:54-66`) and reads no other version, with the key at `<data>\chromium\Local State` (`oracle/takeover/vault/LAYOUT.md`, measured; `oracle/takeover/vault/portable-v10.json`). Check: `local` (the cross-read with `oracle/takeover/vault/make-local-fixture.mjs`), `unit` (the portable vectors).

## R. Rust updater

**R1** Availability is a port of `perUserInstall`: packaged, data folder source `default`, `Uninstall OneLaunch.exe` beside the exe, and `HKCU\Software\<GUID>\InstallLocation` equal to the exe's folder (`src/main/updates/availability.ts:25-39`); each failure has its reason (`development`, `custom-data-folder`, `not-installed`, `all-users-install`, also on a read failure). Check: `unit`.

**R2** Feed. electron-updater's GitHub provider sends, in order: `GET https://github.com/rjf2016/one-launch/releases.atom` (the feed must have an `entry`), `GET https://github.com/rjf2016/one-launch/releases/latest` with `Accept: application/json` for `tag_name` (allowPrerelease is off), then `GET https://github.com/rjf2016/one-launch/releases/download/<tag>/latest.yml`; every request carries `x-user-staging-id` (`node_modules/electron-updater/out/providers/GitHubProvider.js:40-48,92-93,113-129,158-185`, `node_modules/electron-updater/out/util.js:30-32`, `node_modules/electron-updater/out/AppUpdater.js:380-392`, `src/main/updates/engine.ts:10,61-62`). Check: `static`. **Differs from the design:** the design lists only `releases/latest` and `latest.yml`; the source fetches the Atom feed first, and an empty feed fails as "No published versions on GitHub". Whether the Rust updater keeps the Atom request is a triage note; `describeUpdateError` maps that failure (R8).

**R3** `latest.yml` is parsed with a strict YAML subset, then `verifyManifest` is ported with the same order, reasons and detail texts (`src/main/updates/manifest.ts:19-27,56-89`). Electron parses with `js-yaml` `load` (`node_modules/electron-updater/out/providers/Provider.js:91-103`). Check: `unit` (the core vectors of ticket T03).

**R4** Payload `one-launch-update-v1\n<version>\n<url>\n<sha512>\n<size>\n`, Ed25519 over its UTF-8 bytes; key id is the first 16 hex characters of SHA-256 over the SPKI DER; the same two pinned keys (`src/main/updates/manifest.ts:6,29-35,91-98`, `src/main/updates/release-keys.ts:4-7`). Check: `unit`.

**R5** Semver greater-than only, no downgrade, no prerelease channel switch (U8, `src/main/updates/engine.ts:68-69`); download to the same `pending\` folder (U4); SHA-512 base64 check of the whole file (`src/main/updates/manifest.ts:100-108`, `src/main/updates/update-service.ts:267-277`). Check: `unit`, `runner-fs`.

**R6** A port of the `UpdateService` state machine: first check 15 s after start, then every 6 h, manual check, auto off cancels a background download, `ready` stops checking (`src/main/updates/update-service.ts:27-28,95-140,187-283`). Check: `unit` (hand-ported tests).

**R7** At quit, after the database has closed: nothing without a verified download; left for a restart when auto is off and no "Restart to update"; blocked after session end (`Windows is shutting down or signing out`) or an uncaught exception (`an unexpected error happened in this run`); skipped on a non-zero exit code; a synchronous SHA-512 re-hash must still match; then the detached spawn with the U2 argv (`src/main/updates/update-service.ts:128-134,142-172`, `src/main/index.ts:398-400,756-769,780`). Check: `unit`, `argv`.

**R8** `describeUpdateError` maps the Rust HTTP client's error kinds onto the same sentences: unreachable, no release, no manifest, checksum mismatch, `HTTP <code>`, else the first line cut to 160 characters, else `unknown error` (`src/main/updates/update-service.ts:44-58`). Check: `unit`.

**R9** The release tooling writes `latest.yml` in the same shape: `version`, `files` with one `{url, sha512, size}`, `path`, `sha512`, `releaseDate`, no `stagingPercentage`, no `packages`, no `isAdminRightsRequired`; the owner's signing step appends `signature` and `keyId` (`node_modules/app-builder-lib/out/publish/updateInfoBuilder.js:140-147,180-192`, `scripts/release-sign.mjs:67-68`, `oracle/fixtures/updates/latest.yml:1-10`). Check: `static`, `unit`.

## T. Tooling

**T1** The electron-builder NSIS templates are vendored byte for byte with `build/installer.nsh` under `oracle/takeover/nsis/`; app-builder-lib declares the MIT licence (`node_modules/app-builder-lib/package.json:2,46`), but one vendored header is LGPL-2.1-or-later (`node_modules/app-builder-lib/templates/nsis/include/StdUtils.nsh:1-19`). Check: `static`. **Differs from the design:** the design calls the templates MIT; `include/StdUtils.nsh` carries its own LGPL-2.1-or-later header, and the installed package has no licence file of its own.

**T2** The effective defines are frozen in `oracle/takeover/nsis/defines.json`, derived from `node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:130-302,402-511`. `dist/builder-debug.yml` from the earlier build holds only file patterns: the NSIS script is stored in it only by a build that runs the nsis target with electron-builder's debug logger on (`node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:547-549`, `node_modules/app-builder-lib/out/packager.js:122,329`). Check: `static`. **Differs from the design:** the design says the generated script is in `dist\builder-debug.yml` after a build; it is not in the copy this build left.

**T3** The build uses the legacy toolset, `makensis` from `nsis-3.0.4.1` and plug-ins from `nsis-resources-3.4.1` (`x86-unicode`), each archive pinned by checksum (`node_modules/app-builder-lib/out/toolsets/windows.js:154-161`, `node_modules/app-builder-lib/out/targets/nsis/NsisTarget.js:555,582-586`). A Rust build that wants the same installer compiles with the same pair; `oracle/takeover/nsis/plugins.json` lists each binary's digest and licence. Check: `static`.

**T4** The plug-ins the One Launch build calls: StdUtils, UAC, nsis7z, WinShell (from `nsis-resources`) and System, nsExec, nsDialogs (from NSIS); SpiderBanner, nsisunz, INetC and EmbedHTML are only reached with one-click, zip, web or HTML-licence options this build does not set (`node_modules/app-builder-lib/templates/nsis/installSection.nsh:14-33`, `node_modules/app-builder-lib/templates/nsis/include/extractAppPackage.nsh:80-90`, `node_modules/app-builder-lib/templates/nsis/include/installer.nsh:5-7`, `node_modules/app-builder-lib/out/targets/nsis/nsisLicense.js:35-53`). Check: `static`.

**T5** Whether StdUtils' `TestParameter` matches `--flag` cannot be read from source (binary plug-in; the generated flag macros are `node_modules/app-builder-lib/out/targets/nsis/nsisScriptGenerator.js:29-38`); the alternative is `${GetOptions}`, which the templates already use for `/S`, `/allusers`, `/currentuser` and `--delete-app-data` (I1.8). Check: `runner-proc`.

**T6** This contract is the written form of `plan/design/takeover.md`, which says of itself that it was read from source and not run. Every assertion here cites lines at the pin (tracked files as `git show <pin>:<path>`, `node_modules/` as the pin's lockfile installs it: `package-lock.json:1`), and `scripts/oracle/takeover/check.mjs` confirms that each cited file exists, each cited line is within it, and each line of the design maps to an assertion. Check: `static`.

## B. Rollback

**B1** There is no downgrade (`allowDowngrade` false, U8) and no staged rollout (no `stagingPercentage`, R9). Rollback is a new Electron release with a higher version (`src/main/updates/engine.ts:69`, `node_modules/electron-updater/out/AppUpdater.js:357-362`), whose stock installer drives the Rust uninstaller through `UninstallString` (I3, I8), and which opens the same data folder because the schema (A5) and the vault (A6) did not move. Check: `owner` (O4), `runner-exit`.

## O. What only the owner can check

**O1** The real Electron to Rust update on the owner's install: taskbar pin, shortcut icon, Run value, passwords decrypting. Check: `owner`.

**O2** SmartScreen and antivirus reaction to a new unsigned exe. Check: `owner`.

**O3** `--start-hidden` at a real logon, and shutdown with a pending update (R7). Check: `owner`.

**O4** A rollback by installing the Electron installer over Rust (B1). Check: `owner`.

**O5** An all-users install (I1.7, I3.4). Check: `owner`.

**O6** Signing and publishing (U11, R9). Check: `owner`.

**O7** Everything else runs on GitHub-hosted runners only: install the public Electron release, run the Rust installer with the exact U2 argv, assert files, registry, shortcuts and relaunch (I1 to I8). Never on the owner's PC. Check: `runner-reg`, `runner-fs`, `runner-proc`, `runner-exit`.
