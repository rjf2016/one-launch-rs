# Takeover: installer and updater contract

What the first Rust installer and the Rust updater must satisfy so that an installed Electron copy updates to the Rust build through its existing updater, and so that an Electron build can replace a Rust one. `E\` is the Electron repo at the pinned commit. Read from source; nothing here was run. Milestone 0 ticket T11 turns this into a written contract with assertions.

## What the Electron copy does at update time

- At quit it spawns, detached, `"%LOCALAPPDATA%\one-launch-updater\pending\OneLaunch-<v>-setup.exe" --updated /S`, plus `--force-run` only after "Restart to update" (`E\node_modules\electron-updater\out\NsisUpdater.js`, `E\src\main\updates\engine.ts`).
- No `/D=`, no `--package-file`, no elevation, no Authenticode check.
- Before that it requires a semver-greater version and a matching sha512 (`AppUpdater.js`), plus the Ed25519 check in `E\src\main\updates\manifest.ts`.
- It tries `<installer>.blockmap` and falls back to a full download on any error, so clients do not need a blockmap. `E\scripts\release-sign.mjs` does refuse a draft without one and imports Electron sources, so it must be ported; the blockmap requirement is a triage row.

## Installer contract

Templates: `E\node_modules\app-builder-lib\templates\nsis\`. The GUID is `37275e3e-77ba-54aa-bc32-31551d28dc0d`.

1. **Init.** Mutex named with the GUID (`include\allowOnlyOneInstallerInstance.nsh`). `$INSTDIR` comes from `HKCU\Software\<GUID>\InstallLocation` (`multiUser.nsh`), then the `\One Launch` to `\OneLaunch` move (`E\build\installer.nsh`). `/S` shows no pages.
2. **Wait for the app.** With `--updated`: 300 ms, then 1 s, then stop, then force. The stock check uses `Get-CimInstance Win32_Process`, which is WMI; the `customCheckAppRunning` hook allows an image-name check instead. Triage row.
3. **Run the old uninstaller, verbatim.** `uninstallOldVersion` (`include\installUtil.nsh`) reads `UninstallString` from `HKCU\...\Uninstall\<GUID>`, copies it to temp and runs `/S /KEEP_APP_DATA /currentuser [--keep-shortcuts] --updated _?=<InstallLocation>`. Five tries; a non-zero result aborts with exit 2. This is what removes the Electron files. The shipped uninstaller moves then deletes `$INSTDIR`, skips the Run value because of `--updated`, keeps shortcuts, never touches `%APPDATA%`, and deletes both registry keys.
4. **Extract and register.** Write `Uninstall OneLaunch.exe`, then:
   - `HKCU\Software\<GUID>`: `InstallLocation`, `KeepShortcuts="true"`, `ShortcutName="One Launch"`.
   - Uninstall key: `DisplayName "One Launch <version>"`, `UninstallString "<dir>\Uninstall OneLaunch.exe" /currentuser`, `QuietUninstallString` (same plus `/S`), `DisplayVersion`, `DisplayIcon "<exe>,0"`, `Publisher`, `NoModify`, `NoRepair`, `EstimatedSize`.
5. **Shortcuts.** Kept on the `--keep-shortcuts` path; otherwise `One Launch.lnk` on Start menu and desktop with AUMID `dev.onelaunch.app`.
6. **Run value.** Survives only if it starts with `"<INSTDIR>\OneLaunch.exe"`. So the Rust exe must be `OneLaunch.exe` in the same folder and accept `--start-hidden`. The installer never writes the value.
7. **Relaunch.** Only with `--force-run` and silent: `ExecShellAsUser <Start-menu link or exe> open --updated`. The Rust app must tolerate `--updated`.
8. **Rust uninstaller.** Honours the same flags and `_?=`, so a later installer or an Electron reinstall can drive it.
9. **Every Rust release** satisfies 1 to 8, because stragglers jump straight to whatever is latest.

## App obligations

- `SetCurrentProcessExplicitAppUserModelID("dev.onelaunch.app")`.
- Icon at index 0, `asInvoker` manifest, `--data-dir=`.
- First Rust version greater than the last Electron one.
- Schema stays at v5 and the vault stays `v10` until cutover is declared stable.

## Rust updater

- Availability: port of `E\src\main\updates\availability.ts` (packaged, default data folder, uninstaller beside the exe, `InstallLocation` equals the exe folder).
- Feed: `GET github.com/<owner>/<repo>/releases/latest` with `Accept: application/json` for `tag_name`, then `releases/download/<tag>/latest.yml`.
- A strict YAML subset parser, then a port of `verifyManifest` with identical refusal reasons and texts.
- Payload `one-launch-update-v1\n<version>\n<url>\n<sha512>\n<size>\n`; key id is SHA-256 of the SPKI, first 16 hex characters; the same two pinned keys.
- Semver greater-than, no downgrade; download to the same `pending\` folder; SHA-512 base64 check.
- Port of the `update-service.ts` state machine (15 s first check, 6 h interval).
- Synchronous re-hash at quit after the DB closes, then detached spawn with the same argv. Blocked by session end, crash, or non-zero exit.
- `describeUpdateError` maps ureq error kinds onto the same sentences.
- `xtask` writes `latest.yml` in the same shape: `version`, `files[{url, sha512, size}]`, `path`, `sha512`, `releaseDate`.

## Tooling

Vendor the electron-builder NSIS templates (MIT), frozen with the effective defines, plus `E\build\installer.nsh`, compiled by a pinned `makensis` with the same plugin set. The generated script is in `E\dist\builder-debug.yml` after a build. Not verified: whether stock StdUtils parses `--flag`; reuse electron-builder's plugin binaries (licence check) or parse with `${GetOptions}`.

## Rollback

There is no downgrade and no staged rollout. Rollback means publishing an Electron build with a higher version; the symmetric contract lets its installer replace the Rust install, and the frozen schema and vault let it open the same data folder.

## What only the owner can check

- The real Electron to Rust update on the owner's install: taskbar pin, shortcut icon, Run value, passwords decrypting.
- SmartScreen and antivirus reaction to a new unsigned exe.
- `--start-hidden` at a real logon, and shutdown with a pending update.
- A rollback by installing the Electron installer over Rust.
- An all-users install.
- Signing and publishing.

Everything else runs on GitHub-hosted runners only: install the public Electron release, run the Rust installer with the exact argv, assert files, registry, shortcuts and relaunch. Never on the owner's PC.
