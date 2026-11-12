# NSIS sources of the Electron installer

What electron-builder 26.15.3 (app-builder-lib 26.15.3) compiles into `OneLaunch-<version>-setup.exe` at the pin, frozen so a Rust build can reproduce or replace it.

| Path | What | How it was made |
| --- | --- | --- |
| `templates/**` | 23 of the 24 files of `node_modules/app-builder-lib/templates/nsis/`, byte for byte | copied |
| `templates-held-back.json` | The one template not copied, `include/StdUtils.nsh`: its path in the package, package and version, size, SRI digest, licence and why it is held back | recorded |
| `build/installer.nsh` | One Launch's include (`customInit`, `customHeader`, `customInstallMode`, `customInstall`, `customUnInstall`) | copied |
| `dist/builder-debug.yml`, `dist/builder-effective-config.yaml` | What the earlier packaged build left in `E\dist` | copied |
| `defines.json` | The `-D` defines, `-X` commands and the full `makensis` argument list the build passes, with the source line of each | derived (below) |
| `plugins.json` | Every plug-in and tool binary of the two NSIS bundles the build uses: name, bundle, file version, SRI digest, licence, where the templates mention it | read from the electron-builder cache |

## The defines

`dist/builder-debug.yml` holds only file patterns: electron-builder stores the generated NSIS script there only when its debug logger is on during a run that builds the nsis target. So `defines.json` is derived by reading `NsisTarget.js` with `electron-builder.yml` and `package.json` at the pin; nothing was built. Values that only a build produces are placeholders:

| Placeholder | Stands for | Under the user profile |
| --- | --- | --- |
| `{project}` | the Electron repo folder the build ran in | no |
| `{dist}` | `{project}\dist` | no |
| `{templates}` | `{project}\node_modules\app-builder-lib\templates\nsis` (the `makensis` working folder) | no |
| `{icon}` | the `.ico` electron-builder converts from `build/icon.*` | yes (electron-builder's cache) |
| `{electron-builder-cache}` | `%LOCALAPPDATA%\electron-builder\Cache`, where `makensis` and the plug-ins live | yes |
| `{build-temp}` | electron-builder's temp folder, where it writes the generated language includes | yes (`%TEMP%`) |
| `{archive-sha512-hex-upper}`, `{ceil(unpacked-bytes/1024)}`, `{round(unpacked-bytes/1024)}` | the app archive's SHA-512 and the unpacked size | no |

The header electron-builder puts before `installer.nsi` (`NsisTarget.js:571-609`) is: `!include` of `include/StdUtils.nsh`, `!addincludedir` of `include`, the seven flag macros listed in `defines.json`, the `addLangs` macro (`MUI_LANGUAGE` for every bundled language, since `installerLanguages` is unset), `!addplugindir /x86-unicode` of the `nsis-resources-3.4.1` plug-ins, two generated `LangString` includes made from `templates/messages.yml` and `templates/assistedMessages.yml`, and `!addincludedir` plus `!include` of `build/installer.nsh`. Before `installer.nsi` itself comes the `licensePage` macro: `!insertmacro MUI_PAGE_LICENSE "{project}\build\license.txt"` (`nsisLicense.js:32-55`). Those async parts are added in the order their promises settle.

## Licences

- app-builder-lib declares `"license": "MIT"` in its `package.json`; the installed package ships no licence file of its own, so none is copied here.
- `include/StdUtils.nsh` carries its own header: LGPL-2.1-or-later, by LoRd_MuldeR. It is held back from `templates/` (below).
- No other template file states a licence; `include/getProcessInfo.nsh`, `include/FileAssociation.nsh` and `include/StrContains.nsh` name their authors in a header comment.
- NSIS itself (`makensis` and the stock plug-ins System, nsExec, nsDialogs and the rest) is zlib/libpng, per the `COPYING` file of the `nsis-3.0.4.1` bundle; its LZMA module is CPL-1.0, its bzip2 module the bzip2 licence.
- The `nsis-resources-3.4.1` bundle (StdUtils, UAC, nsis7z, WinShell, nsProcess, SpiderBanner, nsisunz, INetC, EmbedHTML) contains no licence file at all. StdUtils is LGPL-2.1-or-later by its header above; the others' licences are not stated in any file at hand.

No binary is vendored, because only NSIS's own files and StdUtils state a licence and the rest cannot be checked from what is here. `plugins.json` records each one so a Rust build can fetch the same bundles by the checksums electron-builder pins and confirm every file.

## The held-back template

`include/StdUtils.nsh` (app-builder-lib 26.15.3, 16329 bytes, LGPL-2.1-or-later) is not copied: its licence header carries the plug-in author's real contact address, and whether an LGPL file is vendored here at all is the owner's decision. `templates-held-back.json` records its digest; read the file itself from `node_modules/app-builder-lib/templates/nsis/include/StdUtils.nsh` in the Electron repo at the pin. The generator holds it back through one constant (`HELD_BACK` in `scripts/oracle/takeover/nsis.gen.ts`), so vendoring it later is a one-line change.

`templates/multiUser.nsh:4` holds `FOLDERID_UserProgramFiles`, the documented Windows known-folder id, which the bundle allowlist admits.
