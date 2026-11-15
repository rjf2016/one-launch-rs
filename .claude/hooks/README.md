# Safety hooks

`../settings.json` runs `guard.cjs` in five places. The rules themselves are in `CLAUDE.md` › "Standing rules" and `plan/design/workflow.md` › "Hooks"; this folder only enforces them. A hook is a seatbelt: CI repeats every check.

| Event | Tools | Mode | What it does |
| --- | --- | --- | --- |
| PreToolUse | Bash, PowerShell (and Monitor, which Claude Code hands over as Bash) | `shell` | Reads the command as text and refuses what the rules forbid. Before a `git commit` it scans what the commit would hold |
| PreToolUse | Edit, Write, NotebookEdit, MultiEdit | `write` | Refuses a path the ticket's `write_scope` does not name, or an owner-approved path when no ticket is set; and, wherever the file is, a path in the real data folder, in the key folder or to a `.pem` file |
| PreToolUse | Read, Grep, Glob | `read` | Refuses the same three places: Read's `file_path`, Grep's `path` and `glob`, Glob's `pattern` and `path`. Everything else may be read |
| PostToolUse | Edit, Write, MultiEdit | `format` | Runs `rustfmt` on a changed `.rs` file. Never blocks |
| SubagentStop | every agent type | `stop` | Runs `cargo run --locked --package xtask -- check --ticket <id>` when a ticket is set, and keeps the agent going while it fails |

Nothing in here runs any part of a command it reads.

## How everyday calls are written

The hook reads the text of a call, not what a variable will hold, so:

- **The wrappers by a path written out.** `pwsh -NoProfile -File "D:\work\repo\scripts\agent\git.ps1" -C "D:\work\repo" push -u origin m1/x`, or `scripts/agent/gh.ps1 pr view 5` from inside the checkout. `& "$repo\scripts\agent\git.ps1"` is refused, and so is a commit or a push with `-C "$repo"`: the hook cannot tell which script or which folder that is.
- **GitHub only through them.** `scripts/agent/git.ps1 fetch`, `pull`, `clone`, `push` and `ls-remote`, also for a local folder; `scripts/agent/gh.ps1` for everything `gh` does. Plain `gh` is refused whole, reads included.
- **Stage in one call, commit in the next.** `git add ...` and then, alone, `git commit -m "..."`. Only a `cd` may stand before the commit; a message may come from `cat <<'EOF'` or `Get-Content` inside `-m "$(...)"`.
- **`pr merge` with a plain `--auto`.** `scripts/agent/gh.ps1 pr merge 5 --auto --squash --match-head-commit <sha> --subject "..." --body-file squash.md`.
- **A message or a pull request body that must name the real data folder, the key folder or a `.pem` file goes in a file** (`-F`, `--body-file`), written with Write.
- **The app only with a data folder:** `cargo run -p ol-app -- --data-dir <scratch>` or `OneLaunch.exe --data-dir=<scratch>`.
- **No process tool has an allowed form,** and no launcher, installer or registry write either. A refusal says which rule it was.

A call takes about 40 ms more than without the hooks (node starts, reads, exits); a `git commit` about 75 ms, because git is asked what is staged. Measured on the PC the hooks were written on, median of 50, by `bench.cjs`: Read, Grep, Glob 38 ms; Bash and PowerShell 39 to 41 ms; Edit and Write 36 to 40 ms; a push through the wrapper 43 ms; a commit with nothing staged 71 ms; a commit of five staged files, one of them changed again since, 80 ms. On a slower PC the commit comes close to the 100 ms the ticket allows.

## Fail closed

Each hook is `node -e <loader> <guard.cjs> <mode>` with no shell in between. The loader refuses (exit code 2) when `guard.cjs` is missing, does not parse, throws, or ends without a verdict. `guard.cjs` refuses when a rule file is missing, the input is not the JSON it expects, or anything throws. `"onFailure": "block"` on the three PreToolUse hooks covers what a script cannot: `node` not starting, and a timeout. `"disableAllHooks": false` keeps a switch in the user's own settings from turning the hooks off; `.claude/settings.local.json` and `--settings` still can.

Claude Code shows an agent the hook's own command line and then what the hook wrote, so the loader is kept short and holds no sentence: the words of a refusal come from `lib/rules.cjs`, and when even that file is gone the loader writes `fail closed` and the error code.

SubagentStop blocks again when the check still fails, also when `stop_hook_active` is set. Claude Code ends the turn after eight blocks in a row, and each block runs the whole check first. Until `check --ticket` exists in `xtask` (M1-F6c), every stop in a checkout with a ticket file fails that way.

## The ticket file

`.claude/ticket.local.json` in a checkout names the ticket that checkout works on. It is ignored by git (`*.local.*`) and written by the session that starts the ticket, later by the Workflow script. UTF-8, with or without a byte-order mark:

```json
{ "ticket": "M1-F6a" }
```

`ticket` is the id of a file `plan/tickets/<id>.md` in the same checkout; letters, digits and single dashes only. Other fields are ignored. No file means no ticket. A file that cannot be read this way refuses every Edit and Write in that checkout and keeps a subagent from stopping.

No Edit or Write may change that file, or a `.git` folder or `.git` file at any depth, whatever the ticket says.

## Which checkout

The session's project folder is where the hooks are loaded from. It is not always where the work happens, so the checkout is found from the call:

- **Edit and Write**: the nearest folder above the file that holds `.git`. The path must start with a drive letter or a share. A path outside every checkout is not judged: the user's own `.claude` folder is not the repo's. A repository made inside another checkout does not hide the outer one, unless it is a worktree of it. A folder is a worktree only when git registered it: its `.git` file names a folder under `.git/worktrees` that names the `.git` file back.
- **Read, Grep and Glob**: no checkout is looked for. The path is made canonical the same way (`..`, case, `\\?\`, a trailing dot, a junction) and then held against the three places by name, and against `%APPDATA%\OneLaunch` by where it is. A search is also refused when it starts in a folder that holds the real data folder, such as the user's profile or the root of that drive. Grep's `pattern` is the text searched for, not a place, and is not judged.
- **A commit**: the folder named by `-C`, or by a `cd` or `Set-Location` on its own before the commit, or else the folder the call runs in. This holds for `scripts/agent/git.ps1 -C <folder> commit` too. Behind `env`, `timeout`, `nohup`, `nice`, `time`, `command`, `builtin` or `exec` the commit is still read, as long as only `NAME=value` words or a number stand between; behind anything else that starts a command (`env -C`, `xargs`, `Start-Process`, `find -execdir`) it is refused, since it may run in another folder.
- **The wrappers**: `scripts/agent/git.ps1` and `scripts/agent/gh.ps1` are the wrappers only in the checkout the hooks were loaded from and in its registered worktrees. A script of the same name anywhere else is refused.
- **SubagentStop**: the checkout that holds the folder the event names as `cwd`. An agent that worked in another checkout by absolute paths is not checked there.

Owner-approved paths are `spec/`, `oracle/`, `.github/`, `.claude/`, `deny.toml`, `scripts/agent/` and `xtask/src/policy/`, each from the root of a checkout. The list in `lib/checkout.cjs` and `.github/CODEOWNERS` say the same thing and change together.

## The secret scan

`lib/secrets.cjs` holds the rules of `xtask/src/secrets.rs`: the same patterns in the same order, the same allow list (`xtask/secrets.allow` of the checkout), the same skip of `Cargo.lock` and the same shorter rule set under `oracle/`. `test/secrets.test.cjs` reads the Rust file and fails when a pattern or a fixed list there differs from the one here.

It differs in four ways. It reads the files a commit would hold instead of the whole tree: every staged file, whole, as it is in the index, plus the changed tracked files when the commit stages more itself (`-a`, a path, `--include`, `--only`, `--patch`, or an option it does not know). JavaScript reads `\b` and `\w` as ASCII where Rust reads them as Unicode, which can only add findings. Nothing but a `cd` may run before the commit in its call, because the scan runs before anything else in the call does; inside the commit's own words (`-m "$(...)"`) only `cat`, `echo`, `printf`, `date`, `Get-Content`, `Get-Date` and `Write-Output` may run, and none of them may write a file. And JavaScript's regular expressions backtrack where Rust's do not: addresses are searched for in a way that stays linear, but a single line built from tens of thousands of `eyJ` with no dot can take seconds, and past the hook's timeout the commit is refused.

## Tests

```
node --test ".claude/hooks/test/*.test.cjs"
node .claude/hooks/bench.cjs
```

The first runs every command of the two tables in `plan/tickets/M1-F6a.md` in each spelling `test/commands.test.cjs` lists, the path rules, the scan, and each hook through the exact command line of `settings.json`. `test/shells.test.cjs` holds some five hundred awkward texts: it asks the real PowerShell parser which commands each one holds, runs the bash ones (they only `echo`), and fails when a command the shell would run is one the hook does not see. The tests need `git`; `pwsh` and Git Bash for that comparison, which is skipped without them; `cargo` and `rustfmt` for the last two wiring tests. The second command times 50 runs of each kind of PreToolUse call and fails when a median reaches 100 ms. No CI job runs either yet.

## Refused because it cannot be read

A command the hook cannot follow is refused, not let through:

- a command name that is computed: `& $x`, `$(...)` or a glob in command position;
- command text for another shell that the calling shell puts together, since a value can carry a second command: `bash -c "echo $x"`, `pwsh -Command "Get-Date $x"`, `cmd /c echo $x`, and `cmd /c` with a `%NAME%` in it. In single quotes the inner shell reads the variable itself, and that is allowed;
- a computed word behind a command that starts another one: `Start-Process $x`, `env $x`, `xargs $x`;
- `Invoke-Expression` or `eval` on anything but one plain string, an encoded PowerShell command, `--%`, and a shell that takes its commands from standard input, also by path (`bash /dev/stdin`, `. /dev/stdin`);
- a control character in a command name, such as the one a PowerShell escape like `` `t `` or `` `0 `` puts there;
- a GraphQL call through `scripts/agent/gh.ps1 api` whose text comes from a file or from standard input;
- a `${...}` with a quote or another expansion inside, and `<<` inside `(( ))`: each can be read two ways;
- an option before a `git` or `cargo` command that the hook does not know;
- a setting on the command line that can name a program: `git -c <key>=...` and `cargo --config <key>=...` pass only for the keys listed in `lib/commands.cjs` (`GIT_SETTINGS`, `CARGO_SETTINGS`), so `-c alias.x=...`, `-c core.pager=...` and `--config target.x.runner=...` are refused;
- a git command that is told to run a program of its own: `--upload-pack`, `--receive-pack`, `--exec`, `--extcmd`, `rebase -x`, `difftool -x`, `bisect run`, `submodule foreach`, `filter-branch`; and `gh alias`, `gh extension` and `gh config` through the wrapper;
- shells nested more than eight deep, brackets nested more than two hundred deep, and more than four thousand commands in one call.

Some rules refuse more than they must, on purpose. A call that names a registry key and also runs a command that could write one (`Remove-Item`, `New-Item`, `cp` and the like) is refused whole. So is any call that names the real data folder, the key folder, a `.pem` file, `GIT_TRACE_REDACT` or a `GIT_CONFIG` variable, even inside a message. So is PowerShell text that names `Diagnostics.Process`, `[wmi]`, `[wmiclass]`, `[wmisearcher]`, a `System.Management` object class, `Microsoft.Win32.Registry`, `[scriptblock]::Create`, `.InvokeScript(` or `.NewScriptBlock(`: the registry is read with `reg query` or `Get-ItemProperty`. A `gh` call through the wrapper is judged by every bare word in it, because gh finds its command behind options the hook cannot tell from values: `release`, `auth`, `alias`, `extension`, `config`, and `pr` with `merge`, are refused or read as that command wherever they stand, and a word that is only a value goes in as `--option=value`. `pr merge` is read with gh's own table of options; one the table does not know is refused, and so is `--auto=true`. A pattern of a bash `case` is read as a command, so `case "$1" in kill) ...` is refused. An API call through the wrapper that sends anything to `git/refs` is refused, branch or tag. Behind a command that starts another one (`env`, `xargs`, `timeout`, `Start-Process`, `cmd /c`, the service tools `sc`, `net` and `Stop-Service`, and the like) every later word is read as if it could be the command, so `xargs grep kill` is refused. And in PowerShell a new statement may follow a `}`, a `param(...)` or the `while (...)` of a `do` loop with nothing in between (`if ($x) { } tasklist` runs `tasklist`), so the words after each of those are also read as a command of their own: `Write-Output { } tasklist` is refused although `tasklist` is only a word there, and a `Set-Location` in that place makes the folder unknown.

## Not covered

The hook reads text. It does not see:

- what a script file, a build script or a test does, or an inline program for another interpreter (`node -e`, `python -c`);
- a file written by a shell command (`Set-Content`, `>`, `sed -i`): only Edit and Write are checked against `write_scope`, so a shell can also rewrite a wrapper, a ticket file, these hooks, or a file a launcher owns (`loginusers.vdf`, `RiotGamesPrivateSettings.yaml`);
- a file read by a shell command or by another tool than Read, Grep and Glob, unless the call names one of the three places;
- .NET, COM and WMI reached from a PowerShell expression in another way than by the names above: through `using namespace`, a type held in a variable, reflection, `Add-Type` with its own code, or a COM object;
- an argument that is computed at run time, such as a branch or a URL held in a variable;
- text that reaches a command through a pipe (`echo ... | xargs -I{} sh -c {}`), and in cmd a command taken from a `for` variable;
- aliases and settings kept in files: a git alias, a cargo alias (`cargo xtask` is one), `push.followTags`, an ssh host alias, a PowerShell profile;
- a program named by an environment variable that another program reads: `GIT_EXTERNAL_DIFF`, `GIT_PAGER`, `GIT_EDITOR`, `GIT_SSH_COMMAND`, `PAGER`, `RUSTC_WRAPPER`, `CARGO_TARGET_*_RUNNER`, `CARGO_ALIAS_*`. Of the `GIT_*` variables only `GIT_TRACE_REDACT` and `GIT_CONFIG*` are refused by name, because they can show or store the app's token, and a commit is refused when any `GIT_*` variable is set in front of it;
- processes read without a process tool: `/proc` in Git Bash, a debugger, a performance counter;
- the real data folder, the key folder or a key file reached without naming it: a relative path after a `cd`, a wildcard or a brace list, an 8.3 short name, a .NET call for the folder;
- a renamed installer or launcher, a shortcut, or a program started by a path with no extension behind `Start-Process` and the like;
- a `--data-dir` value that is not a scratch folder, apart from the real data folder by name;
- commits that `git merge`, `cherry-pick`, `revert`, `rebase` or `am` make, and the text of a commit message;
- GitHub reached by a program the hook does not know as a network tool, or by `cargo` fetching a dependency;
- other API routes through `scripts/agent/gh.ps1 api` than merging, tags, releases and anything sent to `git/refs`, and a `pr merge --auto` on a pull request that can merge at once: the app's permissions and the rulesets are the limit there;
- which binary `OneLaunch.exe` is. Any path to it needs `--data-dir=<folder>`, the one form both builds read;
- a session where the hooks do not run at all: a workspace that is not trusted yet, `disableAllHooks` in local settings or on the command line, or a plugin that approves what a hook refused.
