# Safety hooks

`../settings.json` runs `guard.cjs` in four places. The rules themselves are in `CLAUDE.md` › "Standing rules" and `plan/design/workflow.md` › "Hooks"; this folder only enforces them. A hook is a seatbelt: CI repeats every check.

| Event | Tools | Mode | What it does |
| --- | --- | --- | --- |
| PreToolUse | Bash, PowerShell (and Monitor, which Claude Code hands over as Bash) | `shell` | Reads the command as text and refuses what the rules forbid. Before a `git commit` it scans what the commit would hold |
| PreToolUse | Edit, Write, NotebookEdit, MultiEdit | `write` | Refuses a path the ticket's `write_scope` does not name, or an owner-approved path when no ticket is set |
| PostToolUse | Edit, Write, MultiEdit | `format` | Runs `rustfmt` on a changed `.rs` file. Never blocks |
| SubagentStop | every agent type | `stop` | Runs `cargo run --locked --package xtask -- check --ticket <id>` when a ticket is set, and keeps the agent going while it fails |

Nothing in here runs any part of a command it reads.

## Fail closed

Each hook is `node -e <loader> <guard.cjs> <mode>` with no shell in between. The loader refuses (exit code 2) when `guard.cjs` is missing, does not parse, throws, or ends without a verdict. `guard.cjs` refuses when a rule file is missing, the input is not the JSON it expects, or anything throws. `"onFailure": "block"` on the two PreToolUse hooks covers what a script cannot: `node` not starting, and a timeout. `"disableAllHooks": false` keeps a switch in the user's own settings from turning the hooks off; `.claude/settings.local.json` and `--settings` still can.

## The ticket file

`.claude/ticket.local.json` in a checkout names the ticket that checkout works on. It is ignored by git (`*.local.*`) and written by the session that starts the ticket, later by the Workflow script. UTF-8, with or without a byte-order mark:

```json
{ "ticket": "M1-F6a" }
```

`ticket` is the id of a file `plan/tickets/<id>.md` in the same checkout; letters, digits and single dashes only. Other fields are ignored. No file means no ticket. A file that cannot be read this way refuses every Edit and Write in that checkout and keeps a subagent from stopping.

No Edit or Write may change that file, the `.git` folder or the `.git` file of a worktree, whatever the ticket says.

## Which checkout

The session's project folder is where the hooks are loaded from. It is not always where the work happens, so the checkout is found from the call:

- **Edit and Write**: the nearest folder above the file that holds `.git`. A path outside every checkout is not judged: the user's own `.claude` folder is not the repo's. A repository made inside another checkout does not hide the outer one, unless it is a worktree of it.
- **A commit**: the folder named by `-C`, or by a `cd` or `Set-Location` on its own before the commit, or else the folder the call runs in. This holds for `scripts/agent/git.ps1 -C <folder> commit` too.
- **The wrappers**: `scripts/agent/git.ps1` and `scripts/agent/gh.ps1` are the wrappers only in the checkout the hooks were loaded from and in its worktrees. A script of the same name anywhere else is refused.
- **SubagentStop**: the checkout that holds the folder the event names as `cwd`. An agent that worked in another checkout by absolute paths is not checked there.

Owner-approved paths are `spec/`, `oracle/`, `.github/`, `.claude/`, `deny.toml`, `scripts/agent/` and `xtask/src/policy/`, each from the root of a checkout. The list in `lib/checkout.cjs` and `.github/CODEOWNERS` say the same thing and change together.

## The secret scan

`lib/secrets.cjs` holds the rules of `xtask/src/secrets.rs`: the same patterns in the same order, the same allow list (`xtask/secrets.allow` of the checkout), the same skip of `Cargo.lock` and the same shorter rule set under `oracle/`. `test/secrets.test.cjs` reads the Rust file and fails when a pattern or a fixed list there differs from the one here.

It differs in four ways. It reads the files a commit would hold instead of the whole tree: every staged file, whole, as it is in the index, plus the changed tracked files when the commit stages more itself (`-a`, a path, `--include`, `--only`, `--patch`, or an option it does not know). JavaScript reads `\b` and `\w` as ASCII where Rust reads them as Unicode, which can only add findings. A commit must be the only command in its call, apart from a `cd` before it, because the scan runs before anything else in the call does. And JavaScript's regular expressions backtrack where Rust's do not: addresses are searched for in a way that stays linear, but a single line built from tens of thousands of `eyJ` with no dot can take seconds, and past the hook's timeout the commit is refused.

## Tests

```
node --test ".claude/hooks/test/*.test.cjs"
node .claude/hooks/bench.cjs
```

The first runs every command of the two tables in `plan/tickets/M1-F6a.md` in each spelling `test/commands.test.cjs` lists, the path rules, the scan, and each hook through the exact command line of `settings.json`. `test/shells.test.cjs` holds some four hundred awkward texts: it asks the real PowerShell parser which commands each one holds, runs the bash ones (they only `echo`), and fails when a command the shell would run is one the hook does not see. The tests need `git`; `pwsh` and Git Bash for that comparison, which is skipped without them; `cargo` and `rustfmt` for the last two wiring tests. The second command times 50 runs of each kind of PreToolUse call and fails when a median reaches 100 ms. No CI job runs either yet.

## Refused because it cannot be read

A command the hook cannot follow is refused, not let through:

- a command name that is computed: `& $x`, `$(...)` or a glob in command position;
- command text for another shell that the calling shell puts together, since a value can carry a second command: `bash -c "echo $x"`, `pwsh -Command "Get-Date $x"`, `cmd /c echo $x`, and `cmd /c` with a `%NAME%` in it. In single quotes the inner shell reads the variable itself, and that is allowed;
- a computed word behind a command that starts another one: `Start-Process $x`, `env $x`, `xargs $x`;
- `Invoke-Expression` or `eval` on anything but one plain string, an encoded PowerShell command, `--%`, and a shell that takes its commands from standard input;
- a `${...}` with a quote or another expansion inside, and `<<` inside `(( ))`: each can be read two ways;
- an option before a `git` or `cargo` command that the hook does not know;
- shells nested more than eight deep, brackets nested more than two hundred deep, and more than four thousand commands in one call.

Three rules refuse more than they must, on purpose. A call that names a registry key and also runs a command that could write one (`Remove-Item`, `New-Item`, `cp` and the like) is refused whole. So is any call that names the real data folder, the key folder or a `.pem` file, even inside a message. And behind a command that starts another one (`env`, `xargs`, `timeout`, `Start-Process`, `cmd /c` and the like) every later word is read as if it could be the command, so `xargs grep kill` is refused.

## Not covered

The hook reads text. It does not see:

- what a script file, a build script or a test does, or an inline program for another interpreter (`node -e`, `python -c`);
- a file written by a shell command (`Set-Content`, `>`, `sed -i`): only Edit and Write are checked against `write_scope`, so a shell can also rewrite a wrapper, a ticket file or these hooks;
- .NET, COM and WMI reached from a PowerShell expression (`[Diagnostics.Process]::Start`, `[Microsoft.Win32.Registry]::SetValue`, `[wmiclass]`), and text run through a method (`[scriptblock]::Create(...).Invoke()`);
- an argument that is computed at run time, such as a branch or a URL held in a variable;
- text that reaches a command through a pipe (`echo ... | xargs -I{} sh -c {}`), and in cmd a command taken from a `for` variable;
- aliases and settings kept in files: a git alias, a cargo alias (`cargo xtask` is one), `push.followTags`, an ssh host alias, a PowerShell profile;
- the real data folder reached without naming it: a relative path after a `cd`, a wildcard, an 8.3 short name;
- a renamed installer or launcher, a shortcut, or a program started by a path with no extension behind `Start-Process` and the like;
- a `--data-dir` value that is not a scratch folder, apart from the real data folder by name;
- commits that `git merge`, `cherry-pick`, `revert`, `rebase` or `am` make, and the text of a commit message;
- GitHub reached by a program the hook does not know as a network tool, or by `cargo` fetching a dependency;
- other API routes through `scripts/agent/gh.ps1 api` than merging, tags and releases, and a `pr merge --auto` on a pull request that can merge at once: the app's permissions and the rulesets are the limit there;
- which binary `OneLaunch.exe` is. Any path to it needs `--data-dir=<folder>`, the one form both builds read;
- a session where the hooks do not run at all: a workspace that is not trusted yet, `disableAllHooks` in local settings or on the command line, or a plugin that approves what a hook refused.
