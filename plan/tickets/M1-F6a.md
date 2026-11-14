---
id: M1-F6a
title: Safety hooks
kind: feature
risk: safety
depends: [M1-F1]
write_scope:
  - .claude/settings.json
  - .claude/hooks/**
read:
  design: [plan/design/workflow.md "Hooks" and "Acting as the app", plan/decisions/M1-F0.md "Found here, for later tickets", CLAUDE.md "Standing rules"]
  source: []
acceptance:
  checks:
    - "the hook's own test run passes: every command in the two tables below, plus each table row in every quoting and casing variant the test file lists, gets the stated verdict"
    - "a PreToolUse call answers in under 100 ms (median of 50 on this PC)"
    - "an Edit or Write to spec/, oracle/, .github/, .claude/, deny.toml, scripts/agent/ or the policy code of xtask is refused; with a ticket set, so is any path outside its write_scope"
    - "with the hook file missing or throwing, the tool call is refused, not allowed"
    - "a commit whose staged diff holds a planted fake token, email address or path inside a Windows user profile is refused"
owner_step: none
---

## Outcome

`.claude/settings.json` is committed and wires hooks that hold in every agent session and subagent in this repo and in the slots:

- **PreToolUse on Bash and PowerShell**: refuses the commands of `workflow.md` › "Hooks", and everything that would reach GitHub other than through `scripts/agent/git.ps1` and `scripts/agent/gh.ps1`.
- **PreToolUse on Edit and Write**: refuses owner-approved paths, and paths outside the current ticket's `write_scope`. The current ticket is named by an ignored file in the checkout that the Workflow script writes (F7c); with none set, only the owner-approved paths are refused.
- **PostToolUse on Edit and Write**: runs rustfmt on a changed `.rs` file.
- **Before a commit**: the secret scan over the staged diff.
- **SubagentStop**: `cargo xtask check --ticket <id>` must pass when a ticket is set.

A refusal says which rule fired and names the rule in `CLAUDE.md` or `workflow.md`. The hook reads a command as text; it never runs any part of it.

Must refuse:

| Command | Rule |
| --- | --- |
| `tasklist`, `taskkill /IM x.exe`, `wmic process`, `Get-Process`, `gps`, `ps` (PowerShell), `Stop-Process`, `kill`, `Get-CimInstance Win32_Process`, `Get-WmiObject` | process tools |
| any command naming `%APPDATA%\OneLaunch`, `$env:APPDATA\OneLaunch`, `~\AppData\Roaming\OneLaunch`, or the same with `/` | the real data folder |
| `steam.exe`, `EpicGamesLauncher.exe`, `Battle.net.exe`, `RiotClientServices.exe`, `Start-Process steam://...`, `start riotclient://...` | starting a launcher |
| `OneLaunch-1.0.0-setup.exe /S`, `Start-Process x-setup.exe`, `msiexec /i`, `winget install`, `rustup-init.exe` | starting an installer |
| `reg add`, `reg delete`, `reg import`, `Set-ItemProperty HKCU:\...`, `New-ItemProperty HKLM:\...`, `Remove-Item HKCU:\...`, `regedit` | registry writes |
| `git tag v1`, `git push origin v1`, `git push --tags`, `gh release create` | tags and releases |
| `gh pr merge 5`, `gh pr merge 5 --squash`, `scripts/agent/gh.ps1 pr merge 5 --squash` | merge without `--auto` |
| `git commit --no-verify`, `git push --no-verify`, `git push --force`, `git push -f`, `git push --force-with-lease`, `git push origin +main` | hook bypass and force-push |
| `cargo add serde`, `cargo update`, `cargo install cargo-nextest` | dependencies |
| `target\debug\OneLaunch.exe`, `cargo run -p ol-app`, `cargo run -p ol-app -- --start-hidden` | the app without `--data-dir` |
| `gh pr list`, `gh api ...`, `gh auth status`, `gh auth token`, `git push origin x`, `git push git@github.com:...`, `ssh git@github.com` | GitHub other than as the app |
| `scripts/agent/git-credential.ps1 get`, any command naming the key folder or a `.pem` file | the app's key and tokens |

Must allow:

| Command |
| --- |
| `cargo build --locked`, `cargo nextest run -p ol-model`, `cargo xtask check`, `cargo run -p ol-app -- --data-dir C:\scratch\x` |
| `git status`, `git diff`, `git commit -m "..."`, `git log`, `git switch -c x` |
| `scripts/agent/git.ps1 push origin x`, `scripts/agent/gh.ps1 pr create ...`, `scripts/agent/gh.ps1 pr merge 5 --auto --squash`, `scripts/agent/gh.ps1 pr view 5` |
| `reg query HKCU\Software\Valve\Steam`, `Get-ItemProperty HKCU:\Software\Valve\Steam` |
| a command that only mentions a blocked word inside a path or file name, such as `Get-Content docs\tasklist-notes.md` |

## Out of scope

The owners file and `CLAUDE.md` (F6b). The checks CI repeats (F6c, F6e). The file that names the current ticket (F7c writes it; this ticket only reads it).

## Traps

- Two shells. The same thing is spelled differently in Bash and PowerShell, with aliases, backticks, `&`, `cmd /c`, `pwsh -c` and `bash -c` in front. Match after stripping those; the test file lists the variants.
- A hook is a seatbelt. The owner's own `gh` login and SSH key are on this PC and have admin rights (`plan/decisions/M1-F0.md`), so the GitHub rules do not stop an agent that uses them. That is why every bare `gh` and every push outside the wrapper is refused, reads included.
- `ps` and `kill` mean different things in Bash and PowerShell. Refuse both in both.
- Riot: there is no allowed form of any process tool, with or without a filter.
- No dependency: the hook is a script for a runtime already on this PC. Fail closed.
- `.claude/` is owner-approved once F6b lands, and this ticket's pull request needs the owner's look either way: it is the rulebook.
