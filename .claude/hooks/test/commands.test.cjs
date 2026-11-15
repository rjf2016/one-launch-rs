'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { judge } = require('../lib/commands.cjs');
const { isWrapper } = require('../lib/checkout.cjs');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const io = { wrapper: (file, name) => isWrapper(file, name, ROOT), isTag: (_, name) => name === 'v1', commit: () => [] };
const verdict = (tool, command, cwd = ROOT, using = io) => judge({ tool, command, cwd, io: using })?.rule ?? 'allow';

const W = { word: true };
const X = { word: true, exe: true };

const REFUSED = [
  ['process', 'tasklist', X],
  ['process', 'taskkill /IM x.exe', X],
  ['process', 'wmic process', X],
  ['process', 'Get-Process', W],
  ['process', 'gps', W],
  ['process', 'ps', W],
  ['process', 'Stop-Process', W],
  ['process', 'kill', W],
  ['process', 'Get-CimInstance Win32_Process', W],
  ['process', 'Get-WmiObject', W],
  ['data', 'Get-ChildItem %APPDATA%\\OneLaunch'],
  ['data', 'Get-ChildItem $env:APPDATA\\OneLaunch'],
  ['data', 'Get-ChildItem ~\\AppData\\Roaming\\OneLaunch'],
  ['data', 'Get-ChildItem %APPDATA%/OneLaunch'],
  ['data', 'Get-ChildItem $env:APPDATA/OneLaunch'],
  ['data', 'Get-ChildItem ~/AppData/Roaming/OneLaunch'],
  ['launcher', 'steam.exe', W],
  ['launcher', 'EpicGamesLauncher.exe', W],
  ['launcher', 'Battle.net.exe', W],
  ['launcher', 'RiotClientServices.exe', W],
  ['launcher', 'Start-Process steam://...', W],
  ['launcher', 'start riotclient://...', W],
  ['installer', 'OneLaunch-1.0.0-setup.exe /S', W],
  ['installer', 'Start-Process x-setup.exe', W],
  ['installer', 'msiexec /i', X],
  ['installer', 'winget install', X],
  ['installer', 'rustup-init.exe', W],
  ['registry', 'reg add', X],
  ['registry', 'reg delete', X],
  ['registry', 'reg import', X],
  ['registry', 'Set-ItemProperty HKCU:\\...', W],
  ['registry', 'New-ItemProperty HKLM:\\...', W],
  ['registry', 'Remove-Item HKCU:\\...', W],
  ['registry', 'regedit', X],
  ['tags', 'git tag v1', X],
  ['github', 'git push origin v1', X],
  ['tags', 'git push --tags', X],
  ['tags', 'gh release create', X],
  ['merge', 'gh pr merge 5', X],
  ['merge', 'gh pr merge 5 --squash', X],
  ['merge', 'scripts/agent/gh.ps1 pr merge 5 --squash', W],
  ['bypass', 'git commit --no-verify', X],
  ['bypass', 'git push --no-verify', X],
  ['bypass', 'git push --force', X],
  ['bypass', 'git push -f', X],
  ['bypass', 'git push --force-with-lease', X],
  ['bypass', 'git push origin +main', X],
  ['deps', 'cargo add serde', X],
  ['deps', 'cargo update', X],
  ['deps', 'cargo install cargo-nextest', X],
  ['app', 'target\\debug\\OneLaunch.exe', W],
  ['app', 'target/debug/OneLaunch.exe', W],
  ['app', 'cargo run -p ol-app', X],
  ['app', 'cargo run -p ol-app -- --start-hidden', X],
  ['github', 'gh pr list', X],
  ['github', 'gh api ...', X],
  ['github', 'gh auth status', X],
  ['github', 'gh auth token', X],
  ['github', 'git push origin x', X],
  ['github', 'git push git@github.com:...', X],
  ['github', 'ssh git@github.com', X],
  ['key', 'scripts/agent/git-credential.ps1 get', W],
  ['key', 'Get-ChildItem D:\\Coding\\slop\\one-launch-rs-bot'],
  ['key', 'Get-Content app.private-key.pem'],
];

const ALLOWED = [
  ['cargo build --locked', X],
  ['cargo nextest run -p ol-model', X],
  ['cargo xtask check', X],
  ['cargo run -p ol-app -- --data-dir C:\\scratch\\x', X],
  ['cargo run --locked --package xtask -- check', X],
  ['git status', X],
  ['git diff', X],
  ['git commit -m "..."', { ...X, alone: true }],
  ['git log', X],
  ['git switch -c x', X],
  ['scripts/agent/git.ps1 push origin x', W],
  ['scripts/agent/gh.ps1 pr create ...', W],
  ['scripts/agent/gh.ps1 pr merge 5 --auto --squash', W],
  ['scripts/agent/gh.ps1 pr view 5', W],
  ['reg query HKCU\\Software\\Valve\\Steam', X],
  ['Get-ItemProperty HKCU:\\Software\\Valve\\Steam', W],
  ['Get-Content docs\\tasklist-notes.md', W],
];

const sh = {
  single: (c) => `'${c.replace(/'/g, "'\\''")}'`,
  double: (c) => `"${c.replace(/[\\"$`]/g, '\\$&')}"`,
};
const ps = {
  single: (c) => `'${c.replace(/'/g, "''")}'`,
  double: (c) => `"${c.replace(/[`"$]/g, '`$&')}"`,
};
const LEADS = { leads: true };

const SHAPES = {
  Bash: [
    ['as written', (c) => c],
    ['after &&', (c) => `true && ${c}`, LEADS],
    ['after ||', (c) => `false || ${c}`, LEADS],
    ['after ;', (c) => `echo start; ${c}`, LEADS],
    ['on a second line', (c) => `echo start\n${c}`, LEADS],
    ['after a comment with an apostrophe', (c) => `echo start # don't stop\n${c}`, LEADS],
    ['after a heredoc with an apostrophe', (c) => `cat <<'EOF'\nit's text\nEOF\n${c}`, LEADS],
    ['at the end of a pipe', (c) => `echo x | ${c}`, LEADS],
    ['at the start of a pipe', (c) => `${c} | cat`],
    ['in the background', (c) => `${c} &`],
    ['with output redirected', (c) => `${c} > /dev/null 2>&1`],
    ['in a subshell', (c) => `(${c})`],
    ['in a group', (c) => `{ ${c}; }`],
    ['in $( )', (c) => `echo "$(${c})"`, LEADS],
    ['in backticks', (c) => `echo \`${c.replace(/[\\`]/g, '\\$&')}\``, LEADS],
    ['in an if', (c) => `if true; then ${c}; fi`, LEADS],
    ['after an assignment', (c) => `FOO=1 ${c}`],
    ['behind command', (c) => `command ${c}`],
    ['behind exec', (c) => `exec ${c}`],
    ['behind env', (c) => `env FOO=1 ${c}`],
    ['behind nohup', (c) => `nohup ${c} &`],
    ['behind time', (c) => `time ${c}`],
    ['behind timeout', (c) => `timeout 30 ${c}`],
    ['behind xargs', (c) => `echo x | xargs ${c}`, LEADS],
    ['bash -c', (c) => `bash -c ${sh.single(c)}`],
    ['bash -lc', (c) => `bash -lc ${sh.double(c)}`],
    ['sh -c', (c) => `sh -c ${sh.single(c)}`],
    ['eval', (c) => `eval ${sh.single(c)}`],
    ['cmd //c', (c) => `cmd //c ${sh.double(c)}`],
    ['cmd.exe /c', (c) => `cmd.exe /c ${c}`, { unquoted: true }],
    ['pwsh -Command', (c) => `pwsh -NoProfile -Command ${sh.single(c)}`],
    ['powershell -c', (c) => `powershell.exe -c ${sh.double(c)}`],
  ],
  PowerShell: [
    ['as written', (c) => c],
    ['after ;', (c) => `Write-Output start; ${c}`, LEADS],
    ['after &&', (c) => `Write-Output start && ${c}`, LEADS],
    ['on a second line', (c) => `Write-Output start\n${c}`, LEADS],
    ['after a comment with an apostrophe', (c) => `Write-Output start # don't stop\n${c}`, LEADS],
    ['after a here-string with an apostrophe', (c) => `Write-Output @'\nit's text\n'@\n${c}`, LEADS],
    ['at the end of a pipe', (c) => `Write-Output x | ${c}`, LEADS],
    ['at the start of a pipe', (c) => `${c} | Out-Null`],
    ['with output redirected', (c) => `${c} *> $null`],
    ['behind the call operator', (c) => `& ${c}`],
    ['in a called script block', (c) => `& { ${c} }`, LEADS],
    ['assigned', (c) => `$out = ${c}`],
    ['in $( )', (c) => `Write-Output "$(${c})"`, LEADS],
    ['in parentheses', (c) => `(${c})`],
    ['in an if', (c) => `if ($true) { ${c} }`, LEADS],
    ['in ForEach-Object', (c) => `1..2 | ForEach-Object { ${c} }`, LEADS],
    ['in a function', (c) => `function go { ${c} }; go`, LEADS],
    ['Invoke-Expression', (c) => `Invoke-Expression ${ps.single(c)}`],
    ['iex', (c) => `iex ${ps.double(c)}`],
    ['Start-Job', (c) => `Start-Job { ${c} }`, LEADS],
    ['Invoke-Command', (c) => `Invoke-Command -ScriptBlock { ${c} }`, LEADS],
    ['cmd /c', (c) => `cmd /c ${c}`],
    ['cmd.exe quoted', (c) => `cmd.exe /d /c ${ps.double(c)}`],
    ['pwsh -Command', (c) => `pwsh -NoProfile -Command ${ps.double(c)}`],
    ['pwsh script block', (c) => `pwsh -NoProfile -Command { ${c} }`, LEADS],
    ['powershell -c', (c) => `powershell.exe -c ${ps.single(c)}`],
    ['bash -c', (c) => `bash -c ${ps.single(c)}`],
  ],
};

const mixed = (w) => [...w].map((c, k) => (k % 2 ? c.toUpperCase() : c.toLowerCase())).join('');
const backticked = (w) => {
  const at = [...w].findIndex((c, k) => k > 0 && /[A-Za-z]/.test(c) && !'abefnrtuv'.includes(c));
  return at < 0 ? w : `${w.slice(0, at)}\`${w.slice(at)}`;
};

const SPELLINGS = {
  Bash: [
    ['upper case', (w) => w.toUpperCase()],
    ['mixed case', mixed],
    ['double-quoted', (w) => `"${w}"`],
    ['single-quoted', (w) => `'${w}'`],
    ['split by empty quotes', (w) => `${w.slice(0, 2)}""${w.slice(2)}`],
    ['with a backslash escape', (w) => `\\${w}`],
  ],
  PowerShell: [
    ['upper case', (w) => w.toUpperCase()],
    ['mixed case', mixed],
    ['called in double quotes', (w) => `& "${w}"`],
    ['called in single quotes', (w) => `& '${w}'`],
    ['dot-sourced', (w) => `. ${w}`],
    ['with a backtick escape', backticked],
  ],
};

const PROGRAMS = {
  Bash: [
    ['.exe', (w) => `${w}.exe`],
    ['.EXE with a trailing dot', (w) => `${w}.EXE.`],
    ['full path', (w) => `/c/Windows/System32/${w}.exe`],
    ['full path with a drive', (w) => `"C:\\Program Files\\Tools\\${w}.exe"`],
  ],
  PowerShell: [
    ['.exe', (w) => `${w}.exe`],
    ['.EXE with a trailing dot', (w) => `${w}.EXE.`],
    ['full path', (w) => `C:\\Windows\\System32\\${w}.exe`],
    ['called by a quoted full path', (w) => `& "C:\\Program Files\\Tools\\${w}.exe"`],
  ],
};

function variants(tool, command, flags = {}) {
  const eaten = tool === 'Bash' && command.includes('\\');
  const out = SHAPES[tool]
    .filter(([, , shape]) => !(flags.alone && shape?.leads) && !(eaten && shape?.unquoted))
    .map(([name, shape]) => [name, shape(command)]);
  if (!flags.word) return out;
  const cut = command.indexOf(' ');
  const [word, rest] = cut < 0 ? [command, ''] : [command.slice(0, cut), command.slice(cut)];
  for (const [name, spell] of SPELLINGS[tool]) out.push([name, spell(word) + rest]);
  if (flags.exe) for (const [name, spell] of PROGRAMS[tool]) out.push([name, spell(word) + rest]);
  return out;
}

function mismatches(command, flags, expected) {
  const wrong = [];
  for (const tool of ['Bash', 'PowerShell']) {
    for (const [name, text] of variants(tool, command, flags)) {
      const got = verdict(tool, text);
      if (got !== expected) wrong.push(`${tool}, ${name}: ${JSON.stringify(text)} gave ${got}`);
    }
  }
  return wrong;
}

for (const [rule, command, flags] of REFUSED) {
  test(`refused in every variant: ${command}`, () => {
    assert.deepEqual(mismatches(command, flags, rule), []);
  });
}

for (const [command, flags] of ALLOWED) {
  test(`allowed in every variant: ${command}`, () => {
    assert.deepEqual(mismatches(command, flags, 'allow'), []);
  });
}

test('the variant lists are as long as they look', () => {
  assert.equal(SHAPES.Bash.length, 32);
  assert.equal(SHAPES.PowerShell.length, 27);
  assert.equal(variants('Bash', 'tasklist', X).length, 42);
  assert.equal(variants('PowerShell', 'tasklist', X).length, 37);
  assert.equal(variants('Bash', 'git commit -m "..."', { ...X, alone: true }).length, 31);
  assert.equal(variants('Bash', 'target\\debug\\OneLaunch.exe', W).length, 37);
});

test('Bash eats an unquoted backslash before it hands words to cmd, and the hook reads what cmd gets', () => {
  assert.equal(verdict('Bash', 'cmd.exe /c target\\debug\\OneLaunch.exe'), 'allow');
  assert.equal(verdict('Bash', 'cmd.exe /c "target\\debug\\OneLaunch.exe"'), 'app');
  assert.equal(verdict('Bash', 'cmd.exe /c target\\\\debug\\\\OneLaunch.exe'), 'app');
  assert.equal(verdict('Bash', 'cmd.exe /c target/debug/OneLaunch.exe'), 'app');
  assert.equal(verdict('PowerShell', 'cmd.exe /c target\\debug\\OneLaunch.exe'), 'app');
});

const HOME_DATA = `C:\\Users\\someone\\AppData\\Roaming\\OneLaunch`;
const NBSP = '\u00A0';

const MORE_REFUSED = [
  ['unreadable', 'Bash', '$(echo tasklist) /v'],
  ['unreadable', 'Bash', 'x=tasklist; $x'],
  ['unreadable', 'Bash', '`echo tasklist`'],
  ['unreadable', 'Bash', "$'\\x74asklist'"],
  ['unreadable', 'Bash', '/c/Windows/System32/task*'],
  ['unreadable', 'Bash', 't{ask,}list'],
  ['unreadable', 'Bash', 'echo tasklist | bash'],
  ['unreadable', 'Bash', "bash <<'EOF'\ntasklist\nEOF"],
  ['unreadable', 'Bash', 'bash -s <<< tasklist'],
  ['unreadable', 'Bash', 'eval "$x"'],
  ['unreadable', 'Bash', 'pwsh -EncodedCommand dABhAHMAawBsAGkAcwB0AA=='],
  ['unreadable', 'Bash', 'pwsh -enc dABhAHMAawBsAGkAcwB0AA=='],
  ['unreadable', 'Bash', 'powershell -e dABhAHMAawBsAGkAcwB0AA=='],
  ['unreadable', 'Bash', 'pwsh \u2013ec dABhAHMAawBsAGkAcwB0AA=='],
  ['unreadable', 'Bash', "echo ${x:-'}'} ; tasklist ; echo ' '"],
  ['unreadable', 'Bash', "git -c alias.x='!tasklist' x"],
  ['unreadable', 'Bash', 'cmd'],
  ['unreadable', 'Bash', 'echo tasklist | bash "-s"'],
  ['unreadable', 'Bash', 'pwsh "-EncodedCommand" dABhAHMAawBsAGkAcwB0AA=='],
  ['unreadable', 'Bash', 'echo $((1 << 2))'],
  ['unreadable', 'Bash', 'echo $((1 << 2))\ntasklist'],
  ['unreadable', 'Bash', "echo $((cat <<'EOF'\nit's\nEOF\n)); tasklist"],
  ['unreadable', 'Bash', 'git --some-new-option x tag v1'],
  ['unreadable', 'Bash', 'cargo --some-new-option x add serde'],
  ['unreadable', 'Bash', 'git -c core.pager=tasklist --paginate log'],
  ['unreadable', 'Bash', 'git -c diff.external=tasklist diff'],
  ['unreadable', 'Bash', 'git --config-env=core.editor=X commit'],
  ['unreadable', 'Bash', 'git log --upload-pack=tasklist'],
  ['unreadable', 'Bash', "git rebase --exec 'tasklist' HEAD~1"],
  ['unreadable', 'Bash', 'git rebase -x tasklist HEAD~1'],
  ['unreadable', 'Bash', 'git bisect run tasklist'],
  ['unreadable', 'Bash', "git submodule foreach 'tasklist'"],
  ['unreadable', 'Bash', "git filter-branch --tree-filter 'tasklist' HEAD"],
  ['unreadable', 'Bash', 'git difftool --extcmd=tasklist'],
  ['unreadable', 'Bash', 'cargo --config \'alias.r2="run -p ol-app"\' r2'],
  ['unreadable', 'Bash', 'cargo test --config target.x86_64-pc-windows-msvc.runner=tasklist'],
  ['unreadable', 'Bash', 'cargo build --config=build.rustc-wrapper=tasklist'],
  ['unreadable', 'Bash', 'cargo --config extra.toml build'],
  ['unreadable', 'PowerShell', 'scripts/agent/git.ps1 push --receive-pack=tasklist ../x m'],
  ['unreadable', 'PowerShell', "scripts/agent/gh.ps1 alias set --shell x 'tasklist'"],
  ['unreadable', 'PowerShell', 'scripts/agent/gh.ps1 extension install o/r'],
  ['unreadable', 'PowerShell', 'scripts/agent/gh.ps1 config set pager tasklist'],
  ['unreadable', 'Bash', 'eval "echo $x"'],
  ['unreadable', 'Bash', 'x=\'; tasklist\'; bash -c "echo $x"'],
  ['unreadable', 'Bash', 'x=tasklist; env $x'],
  ['unreadable', 'Bash', 'x=tasklist; timeout 5 $x'],
  ['unreadable', 'Bash', 'x=tasklist; echo a | xargs $x'],
  ['unreadable', 'Bash', 'find . -exec $x {} \\;'],
  ['unreadable', 'Bash', 'x="a & tasklist"; cmd //c "echo $x"'],
  ['unreadable', 'PowerShell', "$x = 'a & tasklist'; cmd /c echo $x"],
  ['unreadable', 'PowerShell', "$env:X = 'a & tasklist'; cmd /c \"echo %X%\""],
  ['unreadable', 'PowerShell', "$x = '; tasklist'; pwsh -NoProfile -Command \"Get-Date $x\""],
  ['unreadable', 'PowerShell', "$x = '; tasklist'; bash -c \"echo $x\""],
  ['unreadable', 'PowerShell', "$x = 'tasklist'; Start-Process $x"],
  ['unreadable', 'PowerShell', 'Start-Process (Get-Command tasklist).Source'],
  ['unreadable', 'PowerShell', "$x = 'tasklist'; Set-Alias go $x; go"],
  ['unreadable', 'Bash', 'echo tasklist | cmd //k'],
  ['unreadable', 'PowerShell', '& $x'],
  ['unreadable', 'PowerShell', '& (Get-Command tasklist)'],
  ['unreadable', 'PowerShell', "$c = 'tasklist'; & $c"],
  ['unreadable', 'PowerShell', "'tasklist' | iex"],
  ['unreadable', 'PowerShell', 'iex $x'],
  ['unreadable', 'PowerShell', 'Invoke-Expression ("task" + "list")'],
  ['unreadable', 'PowerShell', "cmd /c echo --% ' & tasklist & '"],
  ['unreadable', 'PowerShell', "'tasklist' | pwsh -Command -"],
  ['unreadable', 'PowerShell', "'tasklist' | pwsh -NoProfile"],
  ['process', 'Bash', 'cmd //c "t^asklist"'],
  ['process', 'Bash', 'cmd /ctasklist'],
  ['process', 'Bash', 'cmd //c "echo a & tasklist"'],
  ['process', 'Bash', 'cmd //c \'"a & tasklist & b"\''],
  ['process', 'Bash', 'alias k=kill; k 1'],
  ['process', 'Bash', 'function f { tasklist; }; f'],
  ['process', 'Bash', 'f() { tasklist; }; f'],
  ['process', 'Bash', 'ps aux'],
  ['process', 'Bash', 'pkill -f x'],
  ['process', 'Bash', 'pgrep steam'],
  ['process', 'Bash', 'kill -9 1'],
  ['process', 'Bash', 'sudo taskkill /F /IM x.exe'],
  ['process', 'Bash', 'echo 1 | xargs -n1 taskkill /PID'],
  ['process', 'Bash', 'find . -name x -exec taskkill {} \\;'],
  ['process', 'Bash', 'git commit -m "run `tasklist` now"'],
  ['process', 'Bash', 'bash "-c" tasklist'],
  ['process', 'Bash', "pwsh '-Command' tasklist"],
  ['process', 'Bash', 'echo "$(case x in x) tasklist;; esac)"'],
  ['process', 'Bash', 'echo "$(if true; then case x in x) tasklist;; esac; fi)"'],
  ['process', 'Bash', 'echo "$(case x in x) echo a;; esac)"; tasklist'],
  ['process', 'Bash', 'echo "$(case x in x) echo "a";; esac)"; tasklist; echo "b"'],
  ['process', 'Bash', 'echo a\rtasklist'],
  ['process', 'Bash', 'cat <<EOF\n$(tasklist)\nEOF'],
  ['process', 'Bash', 'query process'],
  ['process', 'Bash', 'C:\\\\Windows\\\\System32\\\\tasklist.exe'],
  ['process', 'Bash', 'while true; do tasklist; done'],
  ['process', 'Bash', 'case x in x) tasklist;; esac'],
  ['process', 'Bash', 'trap "taskkill /IM x.exe" EXIT'],
  ['process', 'PowerShell', "Start-Process cmd -ArgumentList '/c tasklist'"],
  ['process', 'PowerShell', 'Set-Alias x tasklist; x'],
  ['process', 'PowerShell', 'function f { tasklist }; f'],
  ['process', 'PowerShell', 'Microsoft.PowerShell.Management\\Get-Process'],
  ['process', 'PowerShell', 'Get-CimInstance Win32_OperatingSystem'],
  ['process', 'PowerShell', 'gwmi Win32_Process'],
  ['process', 'PowerShell', 'Invoke-CimMethod -ClassName Win32_Process -MethodName Create'],
  ['process', 'PowerShell', 'tasklist /FI "IMAGENAME eq RiotClientServices.exe"'],
  ['process', 'PowerShell', 'Write-Output @"\n$(tasklist)\n"@'],
  ['process', 'PowerShell', "Write-Output 'abc\u2019 ; tasklist ; \u2018def'"],
  ['process', 'PowerShell', `tasklist${NBSP}/v`],
  ['process', 'PowerShell', 'qprocess'],
  ['process', 'PowerShell', '$p = Get-Process'],
  ['process', 'PowerShell', 'foreach ($p in Get-Process) { $p }'],
  ['process', 'PowerShell', '$h = @{ a = Get-Process }'],
  ['process', 'PowerShell', 'Get-Process -Name steam -ErrorAction SilentlyContinue'],
  ['process', 'PowerShell', 'Stop-Process -Name RiotClientServices -Force'],
  ['process', 'PowerShell', 'Write-Output a <# note #>; tasklist'],
  ['process', 'PowerShell', 'Write-Output a\rtasklist'],
  ['launcher', 'PowerShell', '& "C:\\Program Files (x86)\\Steam\\steam.exe" -silent'],
  ['launcher', 'PowerShell', 'Start-Process "C:\\Riot Games\\Riot Client\\RiotClientServices.exe"'],
  ['launcher', 'PowerShell', "& 'C:\\Riot Games\\Riot Client\\Riot Client.exe'"],
  ['launcher', 'PowerShell', 'explorer steam://open/main'],
  ['launcher', 'PowerShell', 'cmd /c start "" steam://run/730'],
  ['launcher', 'PowerShell', 'Invoke-Item battlenet://'],
  ['launcher', 'PowerShell', 'saps com.epicgames.launcher://apps'],
  ['launcher', 'PowerShell', 'rundll32 url.dll,FileProtocolHandler steam://open/main'],
  ['launcher', 'PowerShell', "Start-Process -FilePath steam.exe -ArgumentList '-silent'"],
  ['launcher', 'PowerShell', 'VALORANT.exe'],
  ['launcher', 'PowerShell', 'Start-Process steam:open/main'],
  ['process', 'PowerShell', '$p=Get-Process'],
  ['process', 'PowerShell', '$p =Get-Process'],
  ['process', 'PowerShell', '$a, $b = $c = Get-Process'],
  ['process', 'PowerShell', '$h["k"]+=Get-Process'],
  ['process', 'PowerShell', '@{a=Get-Process}'],
  ['process', 'PowerShell', '[ordered]@{ a = Get-Process }'],
  ['process', 'PowerShell', 'tasklist = 1'],
  ['process', 'PowerShell', 'Write-Output a<# ; tasklist ; #>'],
  ['process', 'PowerShell', 'Write-Output a>#b ; tasklist'],
  ['process', 'PowerShell', 'return tasklist'],
  ['process', 'Bash', 'echo "$(case x in (x) tasklist;; esac)"'],
  ['process', 'Bash', 'case x in (x) tasklist;; esac'],
  ['launcher', 'Bash', 'start steam://run/730'],
  ['launcher', 'Bash', 'cmd //c start "" "C:\\\\Program Files (x86)\\\\Steam\\\\steam.exe"'],
  ['installer', 'PowerShell', 'choco install git'],
  ['installer', 'PowerShell', 'scoop install x'],
  ['installer', 'PowerShell', 'Add-AppxPackage x.msix'],
  ['installer', 'PowerShell', '.\\thing.msi'],
  ['installer', 'PowerShell', 'Install-Module Pester'],
  ['installer', 'PowerShell', '& .\\OneLaunch-Setup.exe'],
  ['installer', 'PowerShell', 'Start-Process .\\OneLaunch-1.0.0-setup.exe -ArgumentList /S'],
  ['installer', 'PowerShell', 'cmd /c "x-setup.exe /S"'],
  ['installer', 'PowerShell', '& "C:\\Program Files\\OneLaunch\\Uninstall OneLaunch.exe"'],
  ['installer', 'Bash', 'winget upgrade --all'],
  ['installer', 'Bash', './rustup-init.exe -y'],
  ['installer', 'Bash', 'msiexec.exe //x thing.msi'],
  ['registry', 'PowerShell', 'Set-Location HKCU:\\Software'],
  ['registry', 'PowerShell', 'cd HKLM:\\'],
  ['registry', 'PowerShell', "'HKCU:\\Software\\x' | Remove-Item"],
  ['registry', 'PowerShell', "$k = 'HKCU:\\Software\\x'; Remove-Item $k"],
  ['registry', 'PowerShell', 'New-Item -Path HKCU:\\Software\\x'],
  ['registry', 'PowerShell', 'sp HKCU:\\x a b'],
  ['registry', 'PowerShell', 'reg.exe ADD HKCU\\x /v a /d b'],
  ['registry', 'PowerShell', 'reg copy HKCU\\a HKCU\\b'],
  ['registry', 'PowerShell', 'regedit /s x.reg'],
  ['registry', 'PowerShell', 'setx FOO bar'],
  ['registry', 'PowerShell', '.\\x.reg'],
  ['registry', 'PowerShell', 'Start-Process x.reg'],
  ['registry', 'PowerShell', 'New-PSDrive -Name R -PSProvider Registry -Root HKEY_CURRENT_USER'],
  ['registry', 'PowerShell', 'Set-ItemProperty -Path Registry::HKEY_CURRENT_USER\\Software\\x -Name a -Value b'],
  ['registry', 'PowerShell', 'New-Item tmp -ItemType Directory; Get-ItemProperty HKCU:\\Software\\Valve\\Steam'],
  ['registry', 'Bash', "reg add 'HKCU\\Software\\x' //v a"],
  ['registry', 'Bash', 'powershell -c "Set-ItemProperty HKCU:\\\\Software\\\\x a b"'],
  ['tags', 'Bash', 'git tag'],
  ['tags', 'Bash', 'git tag -l'],
  ['tags', 'Bash', 'git tag -d v1'],
  ['tags', 'Bash', 'git -C . tag v2'],
  ['tags', 'Bash', 'git update-ref refs/tags/v2 HEAD'],
  ['tags', 'Bash', 'git --no-pager --attr-source HEAD -P tag v2'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 tag v1'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 push origin v1'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 push origin HEAD:refs/tags/v2'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 push origin tag v2'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 push --tags'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 push --follow-tags origin x'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 release list'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 release create v1'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/releases -f tag_name=v1'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/git/refs -f ref=refs/tags/v1 -f sha=abc'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --admin'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge --auto=false 5'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 -R o/r pr merge 5 --squash'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 api -X PUT repos/o/r/pulls/5/merge'],
  ['merge', 'PowerShell', "scripts/agent/gh.ps1 api graphql -f query='mutation { mergePullRequest(input: {}) { clientMutationId } }'"],
  ['merge', 'PowerShell', "Start-Process pwsh -ArgumentList '-File scripts/agent/gh.ps1 pr merge 5'"],
  ['merge', 'PowerShell', "Start-Process pwsh -ArgumentList '-NoProfile','-File','scripts/agent/gh.ps1','pr','merge','5'"],
  ['process', 'PowerShell', "Start-Process cmd -ArgumentList '/c','tasklist'"],
  ['unreadable', 'PowerShell', 'Start-Process -FilePath pwsh -ArgumentList @("-Command", "Get-Process")'],
  ['merge', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --squash'],
  ['merge', 'Bash', 'pwsh -NoProfile scripts/agent/gh.ps1 pr merge 5 --squash'],
  ['merge', 'Bash', 'env FOO=1 pwsh -File scripts/agent/gh.ps1 pr merge 5'],
  ['merge', 'Bash', 'pwsh "-File" scripts/agent/gh.ps1 pr merge 5'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 --some-new-option x pr merge 5'],
  ['bypass', 'Bash', 'git commit -n -m x'],
  ['bypass', 'Bash', 'git commit -nm x'],
  ['bypass', 'Bash', 'git commit -anm x'],
  ['bypass', 'Bash', 'git commit --no-verif -m x'],
  ['bypass', 'Bash', 'git -c core.hooksPath=/dev/null commit -m x'],
  ['bypass', 'Bash', 'git config core.hooksPath .nohooks'],
  ['bypass', 'Bash', 'git merge --no-verify x'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --force'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push -fu origin x'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push -uf origin x'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --force-with-lease=x origin x'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --force-if-includes origin x'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push origin +x'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push origin +HEAD:x'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --mirror'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 commit --no-verify -m x'],
  ['deps', 'Bash', 'cargo +stable add serde'],
  ['deps', 'Bash', 'cargo --locked update'],
  ['deps', 'Bash', 'cargo remove serde'],
  ['deps', 'Bash', 'cargo rm serde'],
  ['deps', 'Bash', 'cargo generate-lockfile'],
  ['deps', 'Bash', 'cargo install --path xtask'],
  ['deps', 'Bash', 'cargo.exe ADD serde'],
  ['deps', 'Bash', 'rustup run stable cargo add serde'],
  ['deps', 'PowerShell', '& cargo add serde'],
  ['deps', 'Bash', 'cargo -q --config net.offline=true -Zunstable-options --color=never add serde'],
  ['app', 'Bash', 'cargo run'],
  ['app', 'Bash', 'cargo r -p ol-app'],
  ['app', 'Bash', 'cargo run --bin OneLaunch'],
  ['app', 'Bash', 'cargo run --package=ol-app'],
  ['app', 'Bash', 'cargo run -pol-app'],
  ['app', 'Bash', 'cargo run -p ol-app -- --data-dir'],
  ['app', 'Bash', 'cargo run -p ol-app --data-dir /c/scratch/x'],
  ['app', 'Bash', 'cargo run --manifest-path crates/ol-app/Cargo.toml'],
  ['app', 'Bash', 'cargo run -p $pkg'],
  ['app', 'Bash', './target/release/OneLaunch.exe --start-hidden'],
  ['app', 'Bash', 'target/debug/OneLaunch'],
  ['app', 'PowerShell', '.\\target\\debug\\OneLaunch.exe --data-dir C:\\scratch\\x'],
  ['app', 'PowerShell', '& "$env:LOCALAPPDATA\\Programs\\OneLaunch\\OneLaunch.exe"'],
  ['github', 'Bash', 'git fetch'],
  ['github', 'Bash', 'git pull'],
  ['github', 'Bash', 'git clone https://github.com/o/r'],
  ['github', 'Bash', 'git ls-remote origin'],
  ['github', 'Bash', 'git remote update'],
  ['github', 'Bash', 'git remote show origin'],
  ['github', 'Bash', 'git push'],
  ['github', 'Bash', 'git push https://github.com/o/r.git x'],
  ['github', 'Bash', 'git -C . push origin x'],
  ['github', 'Bash', 'git submodule update --init'],
  ['github', 'Bash', 'GIT_SSH_COMMAND=x git push'],
  ['github', 'Bash', 'gh'],
  ['github', 'Bash', 'gh.exe pr view 5'],
  ['github', 'Bash', 'gh repo view'],
  ['github', 'Bash', 'ssh -T git@github.com'],
  ['github', 'Bash', 'scp x git@github.com:y'],
  ['github', 'Bash', 'curl https://api.github.com/repos/o/r'],
  ['github', 'Bash', 'curl -H "Accept: x" https://raw.githubusercontent.com/o/r/main/x'],
  ['github', 'Bash', 'wget https://github.com/o/r/archive/main.zip'],
  ['github', 'PowerShell', 'Invoke-RestMethod https://api.github.com/user'],
  ['github', 'PowerShell', 'irm -Uri https://api.github.com/user'],
  ['github', 'PowerShell', 'iwr https://github.com/o/r'],
  ['github', 'PowerShell', 'Invoke-WebRequest -Uri "https://api.github.com/repos/o/r/pulls"'],
  ['github', 'PowerShell', 'C:\\scratch\\scripts\\agent\\gh.ps1 pr view 5'],
  ['github', 'PowerShell', 'cd ..; scripts/agent/gh.ps1 pr view 5'],
  ['github', 'PowerShell', '& "$PWD\\scripts\\agent\\gh.ps1" pr view 5'],
  ['github', 'PowerShell', 'if ($true) { cd .. }; scripts/agent/gh.ps1 pr view 5'],
  ['github', 'Bash', '(cd /c/scratch); scripts/agent/gh.ps1 pr view 5'],
  ['key', 'PowerShell', 'pwsh -NoProfile -File scripts/agent/git-credential.ps1 get'],
  ['key', 'Bash', 'echo "protocol=https" | pwsh -File scripts/agent/git-credential.ps1 get'],
  ['key', 'Bash', 'git credential fill'],
  ['key', 'PowerShell', 'scripts/agent/git.ps1 credential fill'],
  ['key', 'PowerShell', 'scripts/agent/gh.ps1 auth token'],
  ['key', 'PowerShell', 'scripts/agent/gh.ps1 auth status'],
  ['key', 'PowerShell', 'Import-Module scripts/agent/AgentApp.psm1; (New-AgentToken).token'],
  ['key', 'PowerShell', '$t=New-AgentJwt'],
  ['key', 'PowerShell', 'Invoke-AgentApi -Path /user -Bearer $t'],
  ['key', 'PowerShell', 'Get-Content D:\\Coding\\slop\\one-launch-rs-bot\\app.private-key.pem'],
  ['key', 'Bash', 'ls D:/Coding/slop/one-launch-rs-bot'],
  ['key', 'Bash', 'cat key.pem'],
  ['key', 'PowerShell', 'Get-ChildItem *.pem'],
  ['key', 'PowerShell', "$env:ONE_LAUNCH_AGENT_KEY_DIR = 'C:\\x'"],
  ['key', 'Bash', 'openssl rsa -in x.PEM'],
  ['data', 'PowerShell', 'Get-ChildItem "$env:APPDATA\\OneLaunch"'],
  ['data', 'PowerShell', 'ls $env:APPDATA/OneLaunch/logs'],
  ['data', 'PowerShell', 'cmd /c dir %appdata%\\onelaunch'],
  ['data', 'PowerShell', 'Get-Content $HOME\\AppData\\Roaming\\OneLaunch\\onelaunch.db'],
  ['data', 'PowerShell', 'Join-Path $env:APPDATA OneLaunch'],
  ['data', 'PowerShell', "[IO.Path]::Combine($env:APPDATA, 'OneLaunch')"],
  ['data', 'PowerShell', 'ls %APPDATA%\\x\\..\\OneLaunch'],
  ['data', 'PowerShell', 'ls %LOCALAPPDATA%\\..\\Roaming\\OneLaunch'],
  ['data', 'PowerShell', 'ls %APPDATA%\\\\OneLaunch'],
  ['data', 'PowerShell', 'ls %APPDATA%\\.\\OneLaunch'],
  ['data', 'PowerShell', 'ls %AppData%\\ONELAUNCH.'],
  ['data', 'PowerShell', 'ls %APPDATA%\\x\\\\..\\OneLaunch'],
  ['data', 'PowerShell', 'ls %APPDATA%\\x\\.\\..\\OneLaunch'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 -R o/r release create v1'],
  ['key', 'PowerShell', 'scripts/agent/gh.ps1 --repo o/r auth token'],
  ['app', 'Bash', 'cargo run -p xtask --bin OneLaunch'],
  ['process', 'PowerShell', 'cmd /c "for %i in (1 2) do tasklist"'],
  ['process', 'PowerShell', 'cmd /c if exist x tasklist'],
  ['process', 'PowerShell', 'cmd /c "if 1==1 (echo a) else taskkill /IM x.exe"'],
  ['process', 'PowerShell', "Write-Output a@'\nit' ; tasklist ; 'x'"],
  ['process', 'PowerShell', `tasklist${String.fromCharCode(0x2028)}/v`],
  ['process', 'Bash', 'find . -maxdepth 0 -exec tasklist \\;'],
  ['process', 'Bash', "find . -name x -execdir sh -c 'tasklist' \\;"],
  ['secrets', 'Bash', 'cd /tmp && git commit -m x'],
  ['data', 'PowerShell', 'ls %APPDATA%\\"One"Launch'],
  ['data', 'PowerShell', 'git commit -m "never touch %APPDATA%\\OneLaunch"'],
  ['data', 'PowerShell', `ls ${HOME_DATA}`],
  ['data', 'PowerShell', 'cargo run -p ol-app -- --data-dir %APPDATA%\\OneLaunch'],
  ['data', 'Bash', 'ls ~/AppData/Roaming/OneLaunch'],
  ['data', 'Bash', 'ls "$APPDATA/OneLaunch"'],
  ['data', 'Bash', 'ls ${APPDATA}/OneLaunch'],
  ['data', 'Bash', 'ls ~/AppData/Roaming/One\\Launch'],
  ['data', 'Bash', 'ls /c/Users/someone/AppData/Roaming/OneLaunch'],
  ['secrets', 'Bash', 'git add -A && git commit -m x'],
  ['secrets', 'Bash', 'git add .; git commit -m x'],
  ['secrets', 'Bash', 'echo x > f; git commit -am x'],
  ['secrets', 'Bash', 'GIT_INDEX_FILE=x git commit -m y'],
  ['secrets', 'Bash', 'env GIT_INDEX_FILE=x git commit -m y'],
  ['secrets', 'Bash', 'git --git-dir=x commit -m y'],
  ['secrets', 'Bash', 'git --work-tree x commit -m y'],
  ['secrets', 'Bash', 'cd "$somewhere" && git commit -m x'],
  ['secrets', 'Bash', 'git -C "$somewhere" commit -m x'],
  ['secrets', 'PowerShell', "$env:GIT_INDEX_FILE = 'x'; git commit -m y"],
  ['secrets', 'PowerShell', 'git add -A; scripts/agent/git.ps1 commit -m x'],
  ['process', 'PowerShell', 'tasklist{}'],
  ['process', 'PowerShell', 'tasklist(1)'],
  ['process', 'PowerShell', 'tasklist(1){2}'],
  ['process', 'PowerShell', '& tasklist(1)'],
  ['process', 'PowerShell', 'Get-Date | tasklist{ 1 }'],
  ['process', 'PowerShell', 'if (1) { } tasklist'],
  ['process', 'PowerShell', 'if(1){}tasklist'],
  ['process', 'PowerShell', 'if (1) { } else { } tasklist'],
  ['process', 'PowerShell', 'foreach ($i in 1) { } tasklist'],
  ['process', 'PowerShell', 'do { } while (0) tasklist'],
  ['process', 'PowerShell', 'do{}until(1)tasklist'],
  ['process', 'PowerShell', 'try { } catch { } tasklist'],
  ['process', 'PowerShell', 'function f { } tasklist'],
  ['process', 'PowerShell', 'switch (1) { 1 { } } tasklist'],
  ['process', 'PowerShell', 'param($x) tasklist'],
  ['process', 'PowerShell', 'param ($x) tasklist'],
  ['process', 'PowerShell', 'param()tasklist'],
  ['process', 'PowerShell', '[CmdletBinding()]param($x) tasklist'],
  ['process', 'PowerShell', '& { param($x) tasklist }'],
  ['process', 'PowerShell', "pwsh -Command 'param($x) tasklist'"],
  ['process', 'PowerShell', 'exit tasklist'],
  ['process', 'PowerShell', 'exit $x = tasklist'],
  ['process', 'PowerShell', 'return $x = tasklist'],
  ['process', 'PowerShell', 'throw $x = tasklist'],
  ['process', 'PowerShell', '@{ exit = tasklist }'],
  ['process', 'PowerShell', 'do { } until (1) tasklist'],
  ['process', 'PowerShell', 'PARAM($x) tasklist'],
  ['process', 'PowerShell', 'EXIT $x = tasklist'],
  ['process', 'PowerShell', '$x = if (1) { 2 } tasklist'],
  ['process', 'PowerShell', 'if (1) { } $y = tasklist'],
  ['process', 'PowerShell', 'param($x) [int]$y = tasklist'],
  ['process', 'PowerShell', 'if (1) { } . tasklist'],
  ['github', 'PowerShell', 'if (1) { } git push origin main'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 push origin param (1) --tags'],
  ['tags', 'PowerShell', 'scripts/agent/git.ps1 push origin { } --tags'],
  ['launcher', 'PowerShell', 'Stop-Service vgc'],
  ['launcher', 'PowerShell', 'Restart-Service -Name vgc -Force'],
  ['launcher', 'PowerShell', 'sc.exe stop vgk'],
  ['launcher', 'PowerShell', 'net stop vgc'],
  ['launcher', 'PowerShell', 'sc query "Steam Client Service"'],
  ['process', 'Bash', 'script -c tasklist'],
  ['process', 'PowerShell', 'taskmgr'],
  ['process', 'PowerShell', 'procexp64.exe'],
  ['process', 'PowerShell', 'handle.exe -p 1'],
  ['process', 'PowerShell', 'Process'],
  ['process', 'PowerShell', 'process -Name steam'],
  ['process', 'PowerShell', 'CimInstance Win32_Process'],
  ['process', 'PowerShell', 'WmiObject Win32_Process'],
  ['launcher', 'PowerShell', 'Service vgc'],
  ['process', 'PowerShell', '`u{74}asklist'],
  ['process', 'PowerShell', 'tasklis`u{74}'],
  ['process', 'PowerShell', '`u{000074}asklist /v'],
  ['process', 'PowerShell', '& "`u{74}asklist"'],
  ['process', 'PowerShell', 'pwsh -Command "Get-Date`ntasklist"'],
  ['process', 'PowerShell', 'pwsh -Command "Get-Date`u{3B}tasklist"'],
  ['process', 'PowerShell', 'pwsh -Command "Get-Date`\ntasklist"'],
  ['process', 'PowerShell', 'bash -c "echo a`ntasklist"'],
  ['unreadable', 'PowerShell', 'tasklist`0'],
  ['unreadable', 'PowerShell', '& "tasklist`t"'],
  ['unreadable', 'PowerShell', 'tasklist`n'],
  ['process', 'PowerShell', 'task""list'],
  ['process', 'PowerShell', "tasklist''"],
  ['process', 'PowerShell', 't"askl"ist /v'],
  ['process', 'PowerShell', "t'ask'list"],
  ['process', 'PowerShell', '& "tasklist"/v'],
  ['process', 'PowerShell', '& "tasklist"-v'],
  ['process', 'PowerShell', "& 'tasklist'x"],
  ['process', 'PowerShell', '& "tasklist"$x'],
  ['process', 'PowerShell', '."tasklist"'],
  ['process', 'PowerShell', '. "tasklist"-v'],
  ['process', 'PowerShell', '$x = ."tasklist"'],
  ['github', 'PowerShell', '& "git"-C . push origin main'],
  ['process', 'PowerShell', '"a".b = tasklist'],
  ['process', 'PowerShell', "'a'.b=tasklist"],
  ['process', 'PowerShell', '@($x)[0]=tasklist'],
  ['process', 'PowerShell', '@{}.b=tasklist'],
  ['process', 'PowerShell', '{}.b = tasklist'],
  ['process', 'PowerShell', "& @'\ntasklist\n'@"],
  ['process', 'Bash', 'cat <<<x\ntasklist'],
  ['process', 'Bash', 'cat <<< "x"\ntasklist'],
  ['process', 'Bash', 'cat <<<x true\ntasklist\nx'],
  ['unreadable', 'Bash', 'git -c user.namex=y status'],
  ['process', 'Bash', 'echo "$(f() case x in x) tasklist;; esac; f)"'],
  ['process', 'Bash', 'echo "$(function f case x in x) tasklist;; esac; f)"'],
  ['process', 'Bash', 'echo "$(\\case)"; tasklist; echo ""'],
  ['process', 'Bash', 'echo "$(case x in a) :;; case) :;; esac)"; tasklist; echo ""'],
  ['process', 'Bash', 'echo "$(case x in a) :;& case) :;; esac)"; tasklist; echo ""'],
  ['process', 'Bash', 'echo "$(case x in a) :;;& case) :;; esac)"; tasklist; echo ""'],
  ['process', 'Bash', 'echo "$(case x in\ncase) :;; esac)"; tasklist; echo ""'],
  ['process', 'Bash', 'echo "$(case x in (case) :;; esac)"; tasklist; echo ""'],
  ['process', 'Bash', 'echo "$(case x in a) :;; esac; case y in y) tasklist;; esac)"'],
  ['process', 'Bash', 'echo "$(case x in a) case y in y) :;; esac;; b) echo \'x"y\';; esac)"; tasklist; echo ""'],
  ['process', 'Bash', 'echo "$(case x in (a) case y in y) :;; esac;; b) echo \'x"y\';; esac)"; tasklist; echo ""'],
  ['process', 'Bash', 'time -p tasklist'],
  ['process', 'Bash', "bash -c -o pipefail 'tasklist'"],
  ['process', 'Bash', "bash -co pipefail 'tasklist'"],
  ['process', 'Bash', "bash -c -- 'tasklist'"],
  ['unreadable', 'Bash', ". /dev/stdin <<'EOF'\ntasklist\nEOF"],
  ['unreadable', 'Bash', "bash /dev/stdin <<'EOF'\ntasklist\nEOF"],
  ['unreadable', 'Bash', "source /dev/fd/0 <<'EOF'\ntasklist\nEOF"],
  ['unreadable', 'Bash', "bash /proc/self/fd/0 <<'EOF'\ntasklist\nEOF"],
  ['process', 'Bash', 'echo a \\>& tasklist'],
  ['process', 'PowerShell', 'Get-X a`>& tasklist'],
  ['process', 'PowerShell', 'Get-X # note\rtasklist'],
  ['process', 'PowerShell', "Get-X @'\nfoo\r'@; tasklist; Get-X @'\nbar\n'@"],
  ['process', 'PowerShell', "Get-X @'\rfoo\r'@; tasklist"],
  ['process', 'PowerShell', 'Get-X; `\rtasklist'],
  ['process', 'PowerShell', 'pwsh -NoProfile --command "Get-Date;tasklist"'],
  ['unreadable', 'PowerShell', 'pwsh --EncodedCommand dABhAHMAawBsAGkAcwB0AA=='],
  ['process', 'Bash', 'powershell "Get-Date;tasklist"'],
  ['process', 'Bash', 'powershell -NoProfile "Get-Date|tasklist"'],
  ['process', 'PowerShell', 'cmd /c "for /f %i in (\'tasklist\') do @echo %i"'],
  ['process', 'Bash', "cmd //c 'for /f usebackq %i in (`tasklist /v`) do @echo %i'"],
  ['process', 'PowerShell', 'cmd /c tasklist/v'],
  ['process', 'PowerShell', 'cmd /c "taskkill/IM x.exe"'],
  ['unreadable', 'Bash', 'tasklis{t..t}'],
  ['secrets', 'Bash', 'git commit -m "$(git add -A; echo msg)"'],
  ['secrets', 'PowerShell', 'git commit -m "$(git add -A)"'],
  ['secrets', 'Bash', 'git commit -m "$(cat msg.txt > tracked.txt)"'],
  ['secrets', 'Bash', 'git commit -m "`git add -A`"'],
  ['secrets', 'Bash', 'git -C "$(git add -A; pwd)" commit -m x'],
  ['secrets', 'Bash', 'env -C ../other git commit -m x'],
  ['secrets', 'Bash', 'env -C ../other env git commit -m x'],
  ['secrets', 'Bash', 'xargs git commit -m x'],
  ['secrets', 'Bash', 'find . -name x -execdir git commit -m y \\;'],
  ['data', 'PowerShell', 'Get-ChildItem (Join-Path $env:APPDATA -ChildPath OneLaunch)'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --body --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash -t --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/merges -f base=main -f head=m1/x'],
  ['tags', 'PowerShell', "scripts/agent/git.ps1 push origin 'refs/*:refs/*'"],
  ['tags', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 api repos/o/r/git/refs --input -'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/git/refs -f ref=refs/heads/x -f sha=abc'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 api -X DELETE repos/o/r/git/refs/heads/x'],
  ['unreadable', 'PowerShell', 'scripts/agent/gh.ps1 api graphql --input q.json'],
  ['unreadable', 'PowerShell', 'scripts/agent/gh.ps1 api graphql -F query=@q.graphql'],
  ['key', 'PowerShell', "$env:GIT_TRACE_REDACT='0'; $env:GIT_TRACE_CURL='1'; scripts/agent/git.ps1 ls-remote origin"],
  ['key', 'Bash', 'GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0=store pwsh -NoProfile -File scripts/agent/git.ps1 fetch'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr, merge, 5'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push, origin, +main'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --body "x, --auto"'],
  ['unreadable', 'PowerShell', 'pwsh $script'],
  ['unreadable', 'Bash', 'pwsh -NoProfile "$script" -x'],
  ['data', 'PowerShell', `ls %APPDATA%\\${'a'.repeat(50)}\\..\\OneLaunch`],
  ['data', 'PowerShell', `ls %APPDATA%${'/.'.repeat(30)}/OneLaunch`],
  ['data', 'PowerShell', `ls %APPDATA%${'/'.repeat(50)}OneLaunch`],
  ['secrets', 'Bash', 'find . -name x -execdir env git commit -m y \\;'],
  ['secrets', 'Bash', 'find . -execdir find . -exec git commit -m y \\; \\;'],
  ['process', 'Bash', "bash --rcfile x -c 'tasklist'"],
  ['process', 'Bash', 'case x in "$(case y in y) echo \'a"b\';; esac)") :;; esac; tasklist'],
  ['process', 'PowerShell', "Get-X @'\rit's\r'@; tasklist"],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject "--auto"'],
  ['merge', 'PowerShell', "scripts/agent/gh.ps1 pr merge 5 --squash --subject '--auto'"],
  ['merge', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --squash --subject "--auto"'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --body "--auto"'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --body-file --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --match-head-commit --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --author-email --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --repo --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash -b --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash -F --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash -A --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash -R --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -st --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -sdt --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -sb --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash -- --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge --squash -- --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject x --body --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto=false'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto=0'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto=true'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto --auto=0'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto --disable-auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --disable-auto=false --squash'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 -st pr merge --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr -sb merge --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge --body=--auto -s=false --auto -A --rebase --auto=0 --squash'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge -d -s --body --AUTO x --merge'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge -dt --auto -t--auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge --subject --auto -A --squash'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto --frobnicate'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto -x'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --admin'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --admin=true'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr -b x merge 5 --squash'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr --body x merge 5 --squash'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 -t x pr merge 5 --squash'],
  ['tags', 'PowerShell', 'scripts/agent/gh.ps1 -n notes release create v1'],
  ['key', 'PowerShell', 'scripts/agent/gh.ps1 -h github.com auth token'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 -f base=main api repos/o/r/merges -f head=x'],
  ['unreadable', 'PowerShell', 'scripts/agent/gh.ps1 -x y alias set m "pr merge"'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --mirro'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --mirr origin'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --mi'],
  ['bypass', 'PowerShell', 'scripts/agent/git.ps1 push --m'],
  ['process', 'PowerShell', '[System.Diagnostics.Process]::GetProcesses()'],
  ['process', 'PowerShell', "[Diagnostics.Process]::GetProcessesByName('x')"],
  ['process', 'PowerShell', "[Diagnostics.Process]::Start('steam.exe')"],
  ['process', 'PowerShell', "[System.Diagnostics.Process]::Start('steam://open/main')"],
  ['process', 'PowerShell', 'New-Object System.Diagnostics.ProcessStartInfo'],
  ['process', 'PowerShell', '([wmiclass]"Win32_Process").Create("x")'],
  ['process', 'PowerShell', '([wmisearcher]"select * from Win32_Process").Get()'],
  ['process', 'PowerShell', '(New-Object System.Management.ManagementObjectSearcher "select * from Win32_Process").Get()'],
  ['process', 'Bash', 'pwsh -NoProfile -Command "[Diagnostics.Process]::GetProcesses()"'],
  ['registry', 'PowerShell', "[Microsoft.Win32.Registry]::SetValue('HKEY_CURRENT_USER\\Software\\X', 'a', 'b')"],
  ['registry', 'PowerShell', "[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\\X', $true).SetValue('a', 'b')"],
  ['registry', 'PowerShell', '[Microsoft.Win32.RegistryKey]::OpenBaseKey(1, 0)'],
  ['unreadable', 'PowerShell', "[scriptblock]::Create('Get-Date').Invoke()"],
  ['unreadable', 'PowerShell', "[ScriptBlock] :: Create('Get-Date')"],
  ['unreadable', 'PowerShell', "$ExecutionContext.InvokeCommand.InvokeScript('Get-Date')"],
  ['unreadable', 'PowerShell', "$ExecutionContext.InvokeCommand.NewScriptBlock('Get-Date')"],
  ['bypass', 'Bash', 'git config core.hooksPath .githooks'],
  ['bypass', 'Bash', 'git config --local core.hooksPath .githooks'],
  ['bypass', 'Bash', 'git config --unset core.hooksPath'],
  ['bypass', 'Bash', 'git config set core.hooksPath .githooks'],
  ['bypass', 'Bash', 'git config unset core.hooksPath'],
  ['bypass', 'Bash', 'git config --get core.hooksPath --unset'],
  ['bypass', 'Bash', 'git config --file .git/config core.hooksPath x'],
  ['process', 'Bash', 'command tasklist'],
  ['process', 'Bash', 'command -p tasklist'],
  ['secrets', 'Bash', 'cd crates 2>&1 && git commit -m x'],
  ['unreadable', 'PowerShell', 'target\\debug\\OneLaunch.exe \u2013\u2013data-dir=C:\\scratch\\x'],
  ['unreadable', 'PowerShell', './target/debug/OneLaunch.exe \u2013-data-dir=C:/scratch/x'],
  ['app', 'PowerShell', './target/debug/OneLaunch.exe -\u2013data-dir=C:/scratch/x'],
  ['app', 'PowerShell', './target/debug/OneLaunch.exe --data\u2013dir=C:/scratch/x'],
  ['unreadable', 'PowerShell', 'cargo run -p ol-app -- \u2013\u2013data-dir=C:/scratch/x'],
  ['unreadable', 'PowerShell', 'cargo run -p ol-app -- \u2014data-dir=C:/scratch/x'],
  ['unreadable', 'PowerShell', 'cargo run -p ol-app -- \u2015\u2015data-dir C:/scratch/x'],
  ['unreadable', 'PowerShell', 'cargo run -p ol-app \u2013\u2013 --data-dir=C:/scratch/x'],
  ['app', 'PowerShell', 'cargo run -p ol-app -\u2013 --data-dir=C:/scratch/x'],
  ['app', 'PowerShell', 'cargo run -p ol-app -- --data\u2014dir=C:/scratch/x'],
  ['app', 'PowerShell', 'cargo run -p ol-app -- -\u2013data-dir C:/scratch/x'],
  ['unreadable', 'PowerShell', 'scripts/agent/gh.ps1 pr merge --squash \u2013\u2013auto'],
  ['unreadable', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -s \u2013\u2013auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -s -\u2013auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -s --au\u2013to'],
  ['unreadable', 'PowerShell', 'git config \u2013\u2013get core.hooksPath x'],
  ['bypass', 'PowerShell', 'git config -\u2013get core.hooksPath x'],
  ['unreadable', 'PowerShell', 'Get-ChildItem \u2013Recurse'],
  ['unreadable', 'PowerShell', 'pwsh -NoProfile -Command "cargo run -p ol-app -- \u2013\u2013data-dir=C:/scratch/x"'],
  ['unreadable', 'PowerShell', 'scripts/agent/git.ps1 push origin main \u2013\u2013dry-run'],
  ['app', 'Bash', './target/debug/OneLaunch.exe \u2013\u2013data-dir=/c/scratch/x'],
  ['app', 'Bash', 'cargo run -p ol-app -- \u2013\u2013data-dir=/c/scratch/x'],
  ['merge', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 -s \u2013\u2013auto'],
  ['app', 'PowerShell', 'target\\debug\\OneLaunch.exe --DATA-DIR=C:\\scratch\\x'],
  ['app', 'PowerShell', 'target\\debug\\OneLaunch.exe --data-dir='],
  ['app', 'PowerShell', 'target\\debug\\OneLaunch.exe --data-dir C:\\scratch\\x'],
  ['app', 'Bash', 'cargo run -p ol-app -- --data-dir'],
  ['app', 'Bash', 'cargo run -p ol-app -- --data-dir --start-hidden'],
  ['app', 'Bash', './target/release/OneLaunch.exe --start-hidden --Data-Dir=C:/scratch/x'],
  ['app', 'Bash', 'cargo run -p ol-app -- --DATA-DIR=C:/scratch/x'],
  ['app', 'Bash', 'cargo run -p ol-app -- --Data-Dir C:/scratch/x'],
  ['app', 'PowerShell', 'cargo run -p ol-app -- --DATA-DIR C:/scratch/x'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject $subject --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject ${subject} --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject $(Get-Content s.txt) --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject @() --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject @(Get-Content s.txt) --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject $pr.Title --auto'],
  ['merge', 'PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --auto --body-file $body'],
  ['merge', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --squash --subject $subject --auto'],
  ['merge', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --squash --subject ${subject} --auto'],
  ['merge', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --squash --subject $(cat s.txt) --auto'],
  ['merge', 'Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --squash --subject `cat s.txt` --auto'],
];

const MORE_ALLOWED = [
  ['Bash', 'git commit -m n'],
  ['Bash', 'git commit -m "-n"'],
  ['Bash', 'git commit --amend --no-edit'],
  ['Bash', 'git commit -m "fix \\`tasklist\\` call"'],
  ['Bash', 'git commit -m "$(cat <<\'EOF\'\nkill the process; it\'s done\nEOF\n)"'],
  ['Bash', `cd "${ROOT}" && git commit -m x`],
  ['PowerShell', 'git commit -m "kill the tasklist call; don\'t stop"'],
  ['PowerShell', "git commit -m @'\nkill it; tasklist\n'@"],
  ['PowerShell', `Set-Location "${ROOT}"; git commit -m x`],
  ['Bash', "cat <<'EOF' > notes.txt\ntasklist\nkill 1\nEOF"],
  ['Bash', 'grep -rn "tasklist" docs'],
  ['Bash', 'rg taskkill'],
  ['Bash', 'echo kill'],
  ['Bash', 'ls target/debug'],
  ['PowerShell', "Select-String -Pattern 'steam://' -Path spec\\x.md"],
  ['PowerShell', 'Get-Content docs\\kill.md'],
  ['PowerShell', 'Get-Command tasklist'],
  ['Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr view 5'],
  ['Bash', `pwsh -NoProfile -File "${ROOT}\\scripts\\agent\\git.ps1" -C "${ROOT}" status`],
  ['Bash', `pwsh -NoProfile -File "${ROOT}\\scripts\\agent\\git.ps1" -C "${ROOT}" commit -m "one step"`],
  ['Bash', `cd "${ROOT}" && pwsh -NoProfile -File scripts/agent/git.ps1 status`],
  ['PowerShell', `& scripts/agent/git.ps1 -C "${ROOT}" push origin m1/x`],
  ['PowerShell', `& "${ROOT}\\scripts\\agent\\gh.ps1" pr view 5`],
  ['PowerShell', './scripts/agent/gh.ps1 pr view 5'],
  ['PowerShell', '.\\scripts\\agent\\gh.ps1 pr checks 5'],
  ['PowerShell', 'scripts\\agent/..\\agent\\gh.ps1 pr view 5'],
  ['PowerShell', 'SCRIPTS/AGENT/GH.PS1 pr view 5'],
  ['PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/codeowners/errors'],
  ['PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/pulls/5/comments/1/replies -f body=done'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --squash --match-head-commit abc --subject s --body b'],
  ['PowerShell', 'scripts/agent/git.ps1 fetch origin'],
  ['PowerShell', 'scripts/agent/git.ps1 pull --ff-only'],
  ['PowerShell', 'scripts/agent/git.ps1 push -u origin m1/x'],
  ['PowerShell', 'scripts/agent/git.ps1 push origin --delete m1/x'],
  ['PowerShell', 'scripts/agent/git.ps1 push origin HEAD:m1/x'],
  ['PowerShell', 'scripts/agent/git.ps1 commit -m x'],
  ['Bash', 'cargo run -p xtask -- check --tier 0'],
  ['Bash', 'cargo run -p s05'],
  ['Bash', 'cargo run -p ol-app --features fake -- --data-dir=/c/scratch/x'],
  ['Bash', 'cargo test -p ol-model add'],
  ['Bash', 'cargo build -p ol-app'],
  ['Bash', 'cargo fmt --all'],
  ['Bash', 'cargo clippy --locked'],
  ['Bash', 'cargo deny check'],
  ['Bash', 'cargo metadata --locked'],
  ['Bash', 'cargo -q --locked +stable --config net.offline=true build'],
  ['Bash', 'git --no-pager -P --exec-path=/x --attr-source HEAD -c color.ui=never log -1'],
  ['Bash', 'echo "$(case "$1" in a) echo 1;; *) echo 2;; esac)" done'],
  ['Bash', 'case "$1" in a) case x in y) echo 1;; esac;; esac; echo done'],
  ['Bash', './target/debug/OneLaunch.exe --data-dir=/c/scratch/x --start-hidden'],
  ['PowerShell', '.\\target\\debug\\OneLaunch.exe --data-dir=C:\\scratch\\x'],
  ['PowerShell', 'reg query "HKCU\\Software\\Blizzard Entertainment\\Battle.net"'],
  ['PowerShell', 'cmd /c reg query "HKCU\\Software\\Blizzard Entertainment\\Battle.net"'],
  ['PowerShell', 'reg export HKCU\\Software\\Valve steam.reg'],
  ['PowerShell', 'Get-ItemProperty -Path HKLM:\\SOFTWARE\\x'],
  ['PowerShell', 'Get-ChildItem HKCU:\\Software'],
  ['PowerShell', 'Test-Path HKCU:\\Software\\x'],
  ['PowerShell', '(Get-ItemProperty HKCU:\\Software\\Valve\\Steam).SteamPath'],
  ['PowerShell', 'Remove-Item -Recurse target\\tmp'],
  ['PowerShell', 'New-Item -ItemType Directory x'],
  ['Bash', 'rm -rf target/tmp'],
  ['Bash', 'winget list'],
  ['Bash', 'install -m 755 a b'],
  ['Bash', 'python setup.py build'],
  ['Bash', 'npm install'],
  ['PowerShell', 'ls %LOCALAPPDATA%\\OneLaunch'],
  ['PowerShell', 'Get-ChildItem $env:APPDATA\\Code'],
  ['PowerShell', 'cargo run -p ol-app -- --data-dir C:\\scratch\\OneLaunch'],
  ['Bash', 'curl https://crates.io/api/v1/crates/serde'],
  ['PowerShell', 'Invoke-RestMethod https://example.com/x'],
  ['Bash', "bash -c 'git status'"],
  ['Bash', 'bash scripts/x.sh'],
  ['Bash', 'pwsh -NoProfile -File scripts/agent/check.ps1'],
  ['Bash', 'pwsh -NoProfile -Command "Get-ChildItem"'],
  ['Bash', 'pwsh -Version'],
  ['Bash', 'powershell -NoProfile -ExecutionPolicy Bypass -File x.ps1'],
  ['PowerShell', 'cmd /c dir'],
  ['Bash', 'for f in a b; do echo "$f"; done'],
  ['Bash', '[ -f x ] && echo y'],
  ['Bash', 'x=$(git rev-parse HEAD); echo "$x"'],
  ['Bash', 'case "$1" in a) echo 1;; *) echo 2;; esac'],
  ['Bash', 'echo $((1 + 2)) $((x > 1))'],
  ['Bash', 'echo "${HOME}/x" ${#arr[@]}'],
  ['Bash', 'arr=(a b c)'],
  ['Bash', '"$HOME/.cargo/bin/cargo" build --locked'],
  ['Bash', 'find . -name "*.rs" | xargs grep -l foo'],
  ['Bash', "awk '{print $1}' f"],
  ['Bash', "git log --format='%h %s' -5 | head -3"],
  ['Bash', ": 'a comment'"],
  ['PowerShell', "Get-ChildItem | Where-Object { $_.Length -gt 1mb } | ? Name -like '*.rs' | % { $_.Name }"],
  ['PowerShell', '$files = Get-ChildItem; foreach ($f in $files) { Write-Host $f }'],
  ['PowerShell', "$env:FOO = 'bar'; cargo build --locked"],
  ['PowerShell', 'if (-not (Test-Path x)) { New-Item -ItemType Directory x }'],
  ['PowerShell', "[System.IO.File]::ReadAllText('x')"],
  ['PowerShell', "(Get-Date).ToString('yyyy')"],
  ['PowerShell', "$h = @{ a = 1; b = 'kill' }"],
  ['PowerShell', 'try { cargo build --locked } catch { Write-Error $_ }'],
  ['PowerShell', "Write-Output \"a; b\" 'c | d'"],
  ['PowerShell', '& "C:\\Program Files\\Git\\cmd\\git.exe" status'],
  ['PowerShell', 'git log -1 --format=%s | Select-String kill'],
  ['PowerShell', '$out = .\\target\\debug\\OneLaunch.exe --data-dir=C:\\scratch\\x'],
  ['PowerShell', '$out=cargo run -p ol-app -- --data-dir=C:\\scratch\\x'],
  ['PowerShell', '$x = Get-Content a.txt; $y=1; Write-Output $x'],
  ['PowerShell', "$h = @{ n = 'kill'; e = { $_.Name } }"],
  ['PowerShell', 'Get-ChildItem | Select-Object @{n="a";e={$_.Length}}'],
  ['PowerShell', 'Write-Output a <# tasklist #> b'],
  ['PowerShell', "Write-Output @'\ntasklist\n'@"],
  ['PowerShell', 'Import-Module scripts/agent/AgentApp.psm1'],
  ['PowerShell', 'Get-Content scripts/agent/git-credential.ps1'],
  ['Bash', 'case "$x" in (a) echo 1;; (b) echo 2;; esac'],
  ['Bash', './setup.sh --prefix x'],
  ['Bash', 'git -c user.name=fixture -c user.email=fixture@example.com -c commit.gpgsign=false commit -m x'],
  ['Bash', 'git -c core.quotepath=off -c color.ui=never status'],
  ['Bash', 'git rebase main'],
  ['Bash', 'git bisect start'],
  ['Bash', 'git submodule status'],
  ['Bash', 'cargo --config net.offline=true build --locked'],
  ['Bash', 'cargo test --config profile.dev.debug=0 -p ol-model'],
  ['Bash', 'cargo run -p xtask -- --config x'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr comment 5 --body ok'],
  ['PowerShell', '.\\Setup-Env.ps1 -Fast'],
  ['PowerShell', "& 'tas''klist'"],
  ['Bash', 'find . -name kill -type f'],
  ['Bash', 'find "$dir" -name "*.rs" -exec grep -l foo {} +'],
  ['Bash', "bash -c 'echo $HOME'"],
  ['Bash', 'timeout 60 cargo test > "$log" 2>&1'],
  ['Bash', 'cd crates && git commit -m x 2>&1'],
  ['PowerShell', "pwsh -NoProfile -Command 'Get-Content $path'"],
  ['PowerShell', 'Start-Process notepad notes.txt *> $null'],
  ['PowerShell', 'cmd /c "echo 100%"'],
  ['PowerShell', 'param($p = (Get-Date))'],
  ['PowerShell', 'param($p) Get-Date'],
  ['PowerShell', 'Get-Date(1)'],
  ['PowerShell', 'trap [System.Exception] { } Get-Date'],
  ['PowerShell', 'Get-ChildItem (Get-Location) *.toml'],
  ['PowerShell', "Write-Host (Get-Date) 'tasklist'"],
  ['PowerShell', 'if ($x) { Get-Date } elseif ($y) { Get-Date } else { Get-Date }'],
  ['PowerShell', 'do { Get-Date } while ($false)'],
  ['PowerShell', 'try { Get-Date } catch [System.Exception] { throw } finally { Get-Date }'],
  ['PowerShell', 'Get-ChildItem | ForEach-Object { $_.Name } -ErrorAction Stop'],
  ['PowerShell', '(Get-Item x).Length -gt 5'],
  ['PowerShell', 'exit 1'],
  ['PowerShell', 'if ($x) { Get-Date }; Write-Output a b tasklist'],
  ['PowerShell', 'Get-Service wuauserv'],
  ['PowerShell', 'sc.exe query wuauserv'],
  ['PowerShell', 'function f { process { $_ } }'],
  ['PowerShell', '& { begin { } process { Get-Date } end { } }'],
  ['PowerShell', '"tasklist"'],
  ["PowerShell", "'tasklist' | Out-Null"],
  ['PowerShell', "@'\ntasklist\n'@ | Set-Content notes.txt"],
  ['PowerShell', '& "task"list'],
  ['PowerShell', 'Write-Output "a`tb" "c`nd" e`tf'],
  ['PowerShell', 'git commit -m "one`ntwo"'],
  ['PowerShell', 'pwsh -Command "Get-Date`nGet-Location"'],
  ['PowerShell', 'Write-Output `u{110000}'],
  ['PowerShell', 'Write-Output `U{74}asklist'],
  ['PowerShell', '"{0}" -f 1'],
  ['Bash', 'cat <<<x; echo ok'],
  ['Bash', 'cat <<<x\necho ok'],
  ['Bash', 'git commit -m "$(printf \'one\\ntwo\')"'],
  ['Bash', 'git commit -m "$(date +%F) notes"'],
  ['Bash', 'git commit > /dev/null -m "$(cat msg.txt)"'],
  ['PowerShell', 'git commit -m "$(Get-Content msg.txt -Raw)"'],
  ['PowerShell', 'scripts/agent/git.ps1 commit -m "one, two"'],
  ['Bash', 'git commit -m x; git log -1'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -sd --auto'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --match-head-commit abc --auto'],
  ['PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/git/refs/heads/main'],
  ['PowerShell', "scripts/agent/gh.ps1 api graphql -f query='query { viewer { login } }'"],
  ['PowerShell', 'scripts/agent/gh.ps1 api repos/o/r/issues/5/comments -f body=merges'],
  ['Bash', 'time cargo build --locked'],
  ['Bash', 'time -p cargo build --locked'],
  ['Bash', 'f() { echo a; }; arr=(); arr=(a b); echo done'],
  ['Bash', 'bash -o pipefail scripts/x.sh'],
  ['Bash', 'pwsh scripts/x.ps1 -Name a'],
  ['Bash', 'echo a >&2; echo b 2>&1 | cat'],
  ['Bash', 'case "$1" in a) echo 1;; esac; case "$2" in b) echo 2;; esac'],
  ['Bash', 'x=case; echo in; echo esac'],
  ['Bash', 'echo {1..3}'],
  ['Bash', '. scripts/env.sh'],
  ['Bash', 'source ./scripts/env.sh'],
  ['PowerShell', "Get-X @'\nfoo\n'@; Get-Date"],
  ['PowerShell', 'Get-Date # note\r\nGet-Location'],
  ['PowerShell', 'cmd /c "for /f %i in (\'ver\') do @echo %i"'],
  ['PowerShell', 'cmd /c dir/b'],
  ['PowerShell', 'Get-ChildItem $env:APPDATA\\Code; .\\target\\debug\\OneLaunch.exe --data-dir=C:\\scratch\\x'],
  ['PowerShell', 'Get-ChildItem $env:LOCALAPPDATA -Filter OneLaunch*'],
  ['PowerShell', "Start-Process 'tas''klist'"],
  ['Bash', 'echo hello &> out.txt tasklist'],
  ['Bash', 'git commit -m "$(cat < msg.txt)"'],
  ['Bash', 'cmd //c "git commit -m \\"x y\\" & xargs echo done"'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --squash --match-head-commit abc123 --subject "M1-F6a: x (#5)" --body-file squash.md'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --squash --subject "--auto" --auto'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 -s --auto -t x -F squash.md'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto -sd'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --squash --repo=o/r'],
  ['PowerShell', 'scripts/agent/gh.ps1 -R o/r pr merge 5 --auto -tsubject'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --disable-auto'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr list --label=release --search=merge'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr view 5 --json mergeable'],
  ['Bash', 'command -v gh'],
  ['Bash', 'command -v pwsh'],
  ['Bash', 'command -V tasklist'],
  ['Bash', 'git config --get core.hooksPath'],
  ['Bash', 'git config core.hooksPath'],
  ['Bash', 'git config get core.hooksPath'],
  ['Bash', 'git config --get-regexp hookspath'],
  ['Bash', 'git config --file .git/config --get core.hooksPath'],
  ['Bash', 'git config --file .git/config core.hooksPath'],
  ['Bash', 'git config --get-all core.hooksPath githooks'],
  ['Bash', 'git config -l core.hooksPath x'],
  ['PowerShell', 'Get-ItemProperty HKCU:\\Software\\Valve\\Steam -Name SteamPath'],
  ['Bash', 'rg -n "Diagnostics.Process" docs'],
  ['PowerShell', `cargo run -p ol-app -- --data-dir "${path.join(os.tmpdir(), 'OneLaunch-scratch')}"`],
  ['PowerShell', `.\\target\\debug\\OneLaunch.exe --data-dir="${path.join(os.tmpdir(), 'ol', 'OneLaunch')}"`],
  ['PowerShell', 'Get-ChildItem $env:LOCALAPPDATA\\Temp\\OneLaunch-scratch'],
  ['PowerShell', 'git commit -m "M1-F6a \u2013 the hooks"'],
  ['PowerShell', "Write-Output 'a \u2014 b' \"c \u2015 d\""],
  ['PowerShell', 'Write-Output "x" # a \u2013 b'],
  ['PowerShell', "git commit -m @'\na \u2013 b\n\u2013 c\n'@"],
  ['PowerShell', 'Write-Output a\u2013b'],
  ['PowerShell', 'Write-Output "\u2013 a" \'\u2014b\''],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --squash --subject "$subject"'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge 5 --auto --squash --subject=$subject'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr merge "$number" --auto --squash --body-file "$($body)"'],
  ['Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --auto --squash --subject "$subject" --body-file "$body"'],
  ['Bash', 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr merge 5 --auto --squash --subject="$(cat s.txt)"'],
  ['PowerShell', 'scripts/agent/gh.ps1 pr view $number'],
];

test('a refusal that names a .NET type reads as one sentence', () => {
  const said = (command) => judge({ tool: 'PowerShell', command, cwd: ROOT, io }).message;
  assert.equal(said('[Diagnostics.Process]::GetProcesses()'), 'this PowerShell text names a .NET or WMI type that lists, starts or ends processes, and no form of a process tool is allowed');
  assert.equal(said("[Microsoft.Win32.Registry]::SetValue('a', 'b', 'c')"), 'this PowerShell text names the .NET registry classes, which can write a key; read a key with reg query or Get-ItemProperty');
  assert.equal(said("[scriptblock]::Create('Get-Date')"), 'this PowerShell text makes a script out of a string at run time, and the hook cannot read that string');
  assert.match(said('scripts/agent/gh.ps1 pr merge 5 --squash --subject $subject --auto'), /quote it \("\$subject"\) or join it to its option \(--subject=\$subject\)/);
  assert.match(said('cargo run -p ol-app -- \u2013\u2013data-dir=C:/scratch/x'), /typographic dash/);
});

test('gh pr merge: no list of options that gh would read as a merge at once gets past', () => {
  const VALUE_LONG = ['author-email', 'body', 'body-file', 'match-head-commit', 'subject', 'repo'];
  const BOOL_LONG = ['admin', 'auto', 'delete-branch', 'disable-auto', 'merge', 'rebase', 'squash', 'help'];
  const SHORT = { A: 'author-email', b: 'body', F: 'body-file', t: 'subject', R: 'repo', d: 'delete-branch', m: 'merge', r: 'rebase', s: 'squash', h: 'help' };
  const TRUTH = { 1: true, t: true, T: true, TRUE: true, true: true, True: true, 0: false, f: false, F: false, FALSE: false, false: false, False: false };
  const pflag = (words) => {
    const flags = {};
    const bool = (name, text) => {
      if (text !== undefined && !(text in TRUTH)) throw new Error('not a truth value');
      flags[name] = text === undefined ? true : TRUTH[text];
    };
    for (let k = 0; k < words.length; k++) {
      const word = words[k];
      if (word === '--') break;
      if (word.startsWith('--')) {
        const cut = word.indexOf('=');
        const name = cut < 0 ? word.slice(2) : word.slice(2, cut);
        if (VALUE_LONG.includes(name)) {
          if (cut < 0 && ++k >= words.length) throw new Error('no value');
        } else if (BOOL_LONG.includes(name)) bool(name, cut < 0 ? undefined : word.slice(cut + 1));
        else throw new Error('unknown flag');
      } else if (word.length > 1 && word.startsWith('-')) {
        let rest = word.slice(1);
        while (rest) {
          const name = SHORT[rest[0]];
          if (!name) throw new Error('unknown shorthand');
          if (VALUE_LONG.includes(name)) {
            if (rest.length === 1 && ++k >= words.length) throw new Error('no value');
            rest = '';
          } else if (rest.length > 2 && rest[1] === '=') {
            bool(name, rest.slice(2));
            rest = '';
          } else {
            bool(name);
            rest = rest.slice(1);
          }
        }
      }
    }
    return flags;
  };
  const valueless = (word, known) => known.includes(word) || word.includes('=') || !(word.startsWith('--') || word.length === 2);
  const commandWords = (words, known) => {
    const found = [];
    for (let k = 0; k < words.length && words[k] !== '--'; k++) {
      if (!words[k].startsWith('-')) found.push(words[k]);
      else if (!valueless(words[k], known)) {
        if (words.length - k - 1 <= 1) break;
        k++;
      }
    }
    return found;
  };
  const without = (words, name, known) => {
    for (let k = 0; k < words.length && words[k] !== '--'; k++) {
      if (words[k] === name) return [...words.slice(0, k), ...words.slice(k + 1)];
      if (words[k].startsWith('-') && !valueless(words[k], known)) k++;
    }
    return words;
  };
  const cobra = (words) => {
    let rest = words;
    for (const [name, known] of [['pr', ['--help', '--version', '-h']], ['merge', ['--help', '-h']]]) {
      if (commandWords(rest, known)[0] !== name) return null;
      rest = without(rest, name, known);
    }
    return rest;
  };
  const mergesAtOnce = (words) => {
    const read = cobra(words);
    if (!read) return false;
    let flags;
    try {
      flags = pflag(read);
    } catch {
      return false;
    }
    const chosen = [flags.auto, flags['disable-auto'], flags.admin].filter(Boolean).length;
    return !flags.help && chosen <= 1 && !flags.auto && !flags['disable-auto'];
  };
  const PIECES = [
    ...['--auto', '--auto', '--auto', '--auto=true', '--auto=false', '--auto=0', '--auto=1', '--auto=F', '--auto=x', '--AUTO', '--admin', '--admin=false', '--admin=1'],
    ...['--disable-auto', '--disable-auto=false', '--squash', '--merge', '--rebase', '--delete-branch', '--squash=false', '-s', '-d', '-m', '-r', '-sd', '-s=false', '-d=true', '-ds=0'],
    ...['--body', '--body=--auto', '--subject', '--subject=x', '--body-file', '--match-head-commit', '--author-email', '--repo', '--repo=o/r', '-t', '-b', '-F', '-A', '-R'],
    ...['-t--auto', '-tx', '-t=x', '-st', '-dt', '-sb', '-sdt', '--', 'x', '5', 'true', 'false', '--frob', '--auto-merge', '-x', '-sx', '--help', '-h', '-'],
  ];
  let seed = 20261115;
  const random = (below) => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return Math.floor((seed / 2 ** 32) * below);
  };
  const missed = [];
  const counts = { atOnce: 0, allowed: 0 };
  for (let run = 0; run < 6000; run++) {
    const words = Array.from({ length: random(9) }, () => PIECES[random(PIECES.length)]);
    const cut = random(4) === 0 ? random(words.length + 1) : 0;
    const between = random(6) === 0 ? [PIECES[random(PIECES.length)]] : [];
    for (const number of [['5'], []]) {
      const list = [...words.slice(0, cut), 'pr', ...between, 'merge', ...number, ...words.slice(cut)];
      const atOnce = mergesAtOnce(list);
      const got = verdict('PowerShell', `scripts/agent/gh.ps1 ${list.join(' ')}`);
      counts.atOnce += atOnce ? 1 : 0;
      counts.allowed += got === 'allow' ? 1 : 0;
      if (atOnce && got !== 'merge') missed.push(list.join(' '));
      if (got !== 'allow' && got !== 'merge') missed.push(`${got}: ${list.join(' ')}`);
    }
  }
  assert.deepEqual(missed.slice(0, 10), []);
  assert.ok(counts.atOnce > 1000 && counts.allowed > 300, JSON.stringify(counts));
  assert.equal(mergesAtOnce(['-st', 'pr', 'merge', '--auto']), true, 'the option in front takes --auto as its value once gh has taken the two command words out');
  assert.equal(mergesAtOnce(['pr', 'merge', '5', '--squash', '--auto']), false);
  assert.equal(mergesAtOnce(['pr', 'merge', '5', '--squash']), true);
  for (const once of ['--auto', '--squash --auto', '--auto -sd', '--subject --auto --auto', '-t x --auto -F f.md', '--disable-auto']) {
    assert.equal(verdict('PowerShell', `scripts/agent/gh.ps1 pr merge 5 ${once}`), 'allow', once);
  }
});

test('more ways to spell a refused command are refused', () => {
  const wrong = MORE_REFUSED.filter(([rule, tool, command]) => verdict(tool, command) !== rule);
  assert.deepEqual(wrong.map(([rule, tool, command]) => `${tool}: ${JSON.stringify(command)} should be ${rule}, gave ${verdict(tool, command)}`), []);
});

test('the nearest allowed commands stay allowed', () => {
  const wrong = MORE_ALLOWED.filter(([tool, command]) => verdict(tool, command) !== 'allow');
  assert.deepEqual(wrong.map(([tool, command]) => `${tool}: ${JSON.stringify(command)} gave ${verdict(tool, command)}`), []);
});

test('a change of folder is followed only where the shell keeps it', () => {
  const crates = path.join(ROOT, 'crates');
  const view = 'scripts/agent/gh.ps1 pr view 5';
  const expected = [
    ['Bash', `cd ..; ${view}`, 'allow'],
    ['Bash', `cd .. && ${view}`, 'allow'],
    ['Bash', `(cd ..); ${view}`, 'github'],
    ['Bash', `cd .. | cat; ${view}`, 'github'],
    ['Bash', `bash -c "cd .."; ${view}`, 'github'],
    ['Bash', `cmd //c "cd .."; ${view}`, 'github'],
    ['Bash', `cd "$elsewhere"; ${view}`, 'github'],
    ['Bash', `cd -; ${view}`, 'github'],
    ['Bash', `pushd ..; popd; ${view}`, 'github'],
    ['PowerShell', `Set-Location ..; ${view}`, 'allow'],
    ['PowerShell', `pwsh -NoProfile -Command "Set-Location .."; ${view}`, 'github'],
    ['PowerShell', `cmd /c "cd .."; ${view}`, 'github'],
    ['PowerShell', `pwsh -NoProfile -WorkingDirectory .. -File ${view}`, 'allow'],
    ['PowerShell', `pwsh -NoProfile -WorkingDirectory .. -File ${view}; ${view}`, 'github'],
    ['PowerShell', `pwsh -NoProfile -File ${view}`, 'github'],
    ['PowerShell', `Write-Output { } Set-Location ..; ${view}`, 'github'],
    ['PowerShell', `if (1) { } Set-Location ..; ${view}`, 'github'],
  ];
  for (const [tool, command, rule] of expected) assert.equal(verdict(tool, command, crates), rule, command);
});

test('a change of folder counts only when it is sure to have happened: the folder exists, and nothing makes the change conditional', () => {
  const missing = path.join(ROOT, 'no-such-folder');
  const file = path.join(ROOT, 'Cargo.toml');
  const crates = path.join(ROOT, 'crates');
  const wrapper = path.join(ROOT, 'scripts', 'agent', 'git.ps1');
  const seen = [];
  const recording = { ...io, commit: (folder, broad) => (seen.push(folder), []) };
  const elsewhere = ROOT[0].toLowerCase() === 'c' ? 'D' : 'C';
  const commits = [
    ['Bash', `cd "${missing}" || git commit -m x`],
    ['Bash', `cd "${missing}"; git commit -m x`],
    ['Bash', `cd "${missing}"\ngit commit -m x`],
    ['Bash', `cd "${missing}" 2>/dev/null; git commit -m x`],
    ['Bash', `pushd "${missing}"; git commit -m x`],
    ['Bash', `cd "${file}"; git commit -m x`],
    ['Bash', 'cd /Users; git commit -m x'],
    ['Bash', 'cd; git commit -m x'],
    ['Bash', `cd "${crates}" || git commit -m x`],
    ['Bash', `cd "${crates}" && true || git commit -m x`],
    ['Bash', `true && cd "${crates}"; git commit -m x`],
    ['Bash', `false || cd "${crates}"; git commit -m x`],
    ['Bash', `cd "${crates}" || cd "${ROOT}"; git commit -m x`],
    ['Bash', `cd "${crates}" && cd "${missing}"; git commit -m x`],
    ['Bash', `cd "${crates}" & git commit -m x`],
    ['Bash', `cd "${crates}" &&\ncd "${missing}"\ngit commit -m x`],
    ['PowerShell', `Set-Location "${missing}"; git commit -m x`],
    ['PowerShell', `cd "${missing}"; git commit -m x`],
    ['PowerShell', `Push-Location "${missing}"; git commit -m x`],
    ['PowerShell', `Set-Location -Path "${missing}" -ErrorAction SilentlyContinue; git commit -m x`],
    ['PowerShell', `Set-Location "${missing}"\ngit commit -m x`],
    ['PowerShell', `cd "${missing}" || git commit -m x`],
    ['PowerShell', `Set-Location "${crates}" || git commit -m x`],
    ['PowerShell', `Set-Location "${file}"; git commit -m x`],
    ['PowerShell', `Get-Date && Set-Location "${crates}"; git commit -m x`],
    ['PowerShell', `Set-Location "${crates}" &\ngit commit -m x`],
    ['Bash', `cmd //c "cd ${elsewhere}:\\ && git commit -m x"`],
    ['PowerShell', `cmd /c "cd ${elsewhere}:\\ && git commit -m x"`],
    ['PowerShell', `Set-Location ${elsewhere}:; git commit -m x`],
    ['PowerShell', `Set-Location ${ROOT.slice(0, 2)}crates; git commit -m x`],
  ];
  for (const [tool, command] of commits) assert.equal(verdict(tool, command, ROOT, recording), 'secrets', command);
  assert.deepEqual(seen, [], 'none of them was scanned in a folder the commit might not run in');
  const pushes = [
    ['PowerShell', `Set-Location "${missing}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['PowerShell', `Set-Location "${crates}" || pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `cd "${missing}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `cd "${crates}" || pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['PowerShell', `pwsh -NoProfile -WorkingDirectory "${missing}" -File "${wrapper}" push origin m1/x`],
    ['Bash', `false && cd "${crates}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['PowerShell', `Get-Date && Set-Location "${crates}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `true && cd "${crates}" && (echo x)\npwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `cd "${crates}" &\npwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `cd "${crates}" || true; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `cd "${crates}" | cat; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `(cd "${crates}"); pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
  ];
  for (const [tool, command] of pushes) assert.equal(verdict(tool, command), 'tags', command);
  const pushed = [
    ['Bash', `true && cd "${ROOT}"; cd "${crates}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `false || true\ncd "${crates}"\npwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `echo hi & cd "${crates}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `cd "${crates}" || exit 1; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `true && cd "${crates}" && pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `true || false; cd "${crates}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `false || true; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['Bash', `cd "${crates}"; false || true; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
    ['PowerShell', `Set-Location "${crates}" || exit 1; pwsh -NoProfile -File "${wrapper}" push origin m1/x`],
  ];
  for (const [tool, command] of pushed) assert.equal(verdict(tool, command), 'allow', command);
  const kept = [
    ['Bash', `cd "${crates}" && git commit -m x`, crates],
    ['Bash', `cd "${crates}"; git commit -m x`, crates],
    ['Bash', `cd "${crates}"\ngit commit -m x`, crates],
    ['Bash', `cd "${ROOT}" && cd crates && git commit -m x`, crates],
    ['Bash', `cd "${ROOT}"; cd crates; git commit -m x`, crates],
    ['Bash', `pushd "${crates}" && git commit -m x`, crates],
    ['PowerShell', `Set-Location "${crates}"; git commit -m x`, crates],
    ['PowerShell', `Set-Location -Path "${crates}"; git commit -m x`, crates],
    ['PowerShell', `cd "${crates}" && git commit -m x`, crates],
    ['PowerShell', `Push-Location "${ROOT}"; Set-Location crates; git commit -m x`, crates],
    ['Bash', `git -C "${crates}" commit -m x`, crates],
    ['Bash', 'git commit -m x', ROOT],
    ['Bash', `cd "${crates}" || exit 1; git commit -m x`, crates],
    ['PowerShell', `Set-Location "${crates}" || exit 1; git commit -m x`, crates],
    ['Bash', `cmd //c "cd /d ${crates} & git commit -m x"`, crates],
    ['Bash', `cmd //c "cd ${crates} && git commit -m x"`, crates],
    ['Bash', `cd /${ROOT[0].toLowerCase()}${ROOT.slice(2).replace(/\\/g, '/')}/crates && git commit -m x`, crates],
    ['Bash', `cd /${ROOT[0].toLowerCase()} && git commit -m x`, `${ROOT.slice(0, 2)}\\`],
    ...(fs.existsSync(`${elsewhere}:\\`) ? [['Bash', `cmd //c "cd /d ${elsewhere}:\\ && git commit -m x"`, `${elsewhere}:\\`]] : []),
  ];
  for (const [tool, command, folder] of kept) {
    seen.length = 0;
    assert.equal(verdict(tool, command, ROOT, recording), 'allow', command);
    assert.deepEqual(seen.map((each) => each.toLowerCase()), [folder.toLowerCase()], command);
  }
  assert.equal(verdict('PowerShell', `Set-Location "${crates}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`), 'allow');
  assert.equal(verdict('Bash', `cd "${crates}" && pwsh -NoProfile -File "${wrapper}" push origin m1/x`), 'allow');
});

test('a folder change is trusted only in the few forms that cannot fail or happen somewhere else', () => {
  const crates = path.join(ROOT, 'crates');
  const wrapper = path.join(ROOT, 'scripts', 'agent', 'git.ps1');
  const odd = fs.mkdtempSync(path.join(os.tmpdir(), 'ol-hook-'));
  fs.mkdirSync(path.join(odd, 'x[1]'));
  const seen = [];
  const recording = { ...io, commit: (folder) => (seen.push(folder), []) };
  const push = `pwsh -NoProfile -File "${wrapper}" push origin m1/x`;
  const changes = [
    ['Bash', `cd "${crates}" || exit 1 &`],
    ['Bash', `cd "${crates}" || exit &`],
    ['Bash', `cd "${crates}" || exit 1 &\n`],
    ['Bash', `cd "${crates}" && true &`],
    ['Bash', `cd "${crates}" && echo x &`],
    ['Bash', `{ cd "${crates}"; } &`],
    ['Bash', `{ cd "${crates}"; };`],
    ['Bash', `if true; then cd "${crates}"; fi &`],
    ['Bash', `if true; then\ncd "${crates}"\nfi &\n`],
    ['Bash', `if true; then\ncd "${crates}"\nfi\n`],
    ['Bash', `for d in 1; do\ncd "${crates}"\ndone &`],
    ['Bash', `while true; do\ncd "${crates}"\nbreak\ndone\n`],
    ['Bash', `case x in x) cd "${crates}";; esac;`],
    ['Bash', `cd "${crates}" extra;`],
    ['Bash', `cd -x "${crates}";`],
    ['Bash', `cd -e "${crates}";`],
    ['Bash', `cd "${crates}" < /nonexistent-file;`],
    ['Bash', `cd "${crates}" > /nonexistent-dir/x;`],
    ['Bash', `cd "${crates}" 2>/dev/null;`],
    ['Bash', `pushd -n "${crates}";`],
    ['Bash', `pushd -P "${crates}";`],
    ['Bash', `pushd -L "${crates}";`],
    ['Bash', `coproc command cd "${crates}";`],
    ['Bash', `x=1 builtin cd "${crates}";`],
    ['Bash', `time command cd "${crates}";`],
    ['Bash', `! builtin cd "${crates}";`],
    ['Bash', `cd -P "${crates}";`],
    ['Bash', `cd -L -P -- "${crates}";`],
    ['Bash', `cd "${crates}" || exit 1 > /nonexistent-dir/x;`],
    ['Bash', `cd "${crates}" || exit 1 2;`],
    ['Bash', `CDPATH=/somewhere; cd crates;`],
    ['Bash', `chdir "${crates}";`],
    ['Bash', `CD "${crates}";`],
    ['Bash', `./cd "${crates}";`],
    ['Bash', `cd.exe "${crates}";`],
    ['Bash', `command -p cd "${crates}";`],
    ['Bash', `env cd "${crates}";`],
    ['Bash', `x=1 cd "${crates}";`],
    ['Bash', `time cd "${crates}";`],
    ['Bash', `eval 'cd "${crates}"';`],
    ['Bash', `cd() { :; }; cd "${crates}";`],
    ['Bash', `function cd { :; }; cd "${crates}";`],
    ['Bash', `alias cd=true; cd "${crates}";`],
    ['PowerShell', `Set-Location "${crates}" || exit 1 &`],
    ['PowerShell', `Set-Location "${crates}" && Get-Date &`],
    ['PowerShell', `Set-Location "${crates}" extra;`],
    ['PowerShell', `Set-Location "${crates}" -Bogus;`],
    ['PowerShell', `Set-Location "${crates}" -WhatIf;`],
    ['PowerShell', `Set-Location -Path "${crates}" -ErrorAction Stopp;`],
    ['PowerShell', `Set-Location -Path "${crates}" -LiteralPath "${crates}";`],
    ['PowerShell', `Set-Location -ErrorAction SilentlyContinue "${crates}";`],
    ['PowerShell', `Set-Location "${crates}" > C:\\no-such-dir\\x.txt;`],
    ['PowerShell', `Set-Location "${path.join(odd, 'x[1]')}";`],
    ['PowerShell', `Set-Location "${crates}*";`],
    ['PowerShell', `Set-Location -Pa "${crates}";`],
    ['PowerShell', `& Set-Location "${crates}";`],
    ['PowerShell', `Set-Location "${crates}" | Out-Null;`],
    ['PowerShell', `function Set-Location { }; Set-Location "${crates}";`],
    ['PowerShell', `filter cd { }; cd "${crates}";`],
    ['PowerShell', `Set-Alias cd Get-Date; cd "${crates}";`],
    ['PowerShell', `Invoke-Expression 'Set-Location "${crates}"';`],
    ['PowerShell', `if ($true) { Set-Location "${crates}" };`],
    ['Bash', `cmd //c "pushd /d ${crates} & pwsh -NoProfile -File ${wrapper} push origin m1/x"`, true],
    ['Bash', `cmd //c "cd ${crates} extra & pwsh -NoProfile -File ${wrapper} push origin m1/x"`, true],
  ];
  const open = [];
  for (const [tool, change, whole] of changes) {
    const command = whole ? change : `${change} ${push}`;
    if (verdict(tool, command) !== 'tags') open.push(command);
  }
  assert.deepEqual(open, [], 'after each of these the folder is unknown, so the pushed name cannot be looked up');
  for (const name of ['-', '+', '+1', '-x', '-0']) fs.mkdirSync(path.join(odd, name));
  const before = [
    ['Bash', 'cd -;'],
    ['Bash', 'cd +1;'],
    ['Bash', 'cd "-";'],
    ['Bash', "cd '-';"],
    ['Bash', 'cd -- "-";'],
    ['Bash', 'cd "-x";'],
    ['Bash', 'pushd "-0";'],
    ['Bash', `cd "${odd}"; cd "-";`],
    ['Bash', `cd "${odd}" && cd "-" &&`],
    ['PowerShell', 'Set-Location "-";'],
    ['PowerShell', "Set-Location '-';"],
    ['PowerShell', "Set-Location -Path '-';"],
    ['PowerShell', 'cd "-";'],
    ['PowerShell', 'Push-Location "-";'],
    ['PowerShell', 'Set-Location "+";'],
    ['PowerShell', `Set-Location "${odd}"; Set-Location "-";`],
    ['cmd', 'cd "-"'],
  ];
  for (const [tool, change] of before) {
    const command = tool === 'cmd' ? `cmd //c "cd - & pwsh -NoProfile -File ${wrapper} push origin m1/x"` : `${change} ${push}`;
    assert.equal(verdict(tool === 'cmd' ? 'Bash' : tool, command, odd), 'tags', `${change} goes by the shell's own list of folders, not into a folder of that name`);
    if (tool !== 'cmd') assert.equal(verdict(tool, `${change} git commit -m x`, odd, recording), 'secrets', change);
  }
  assert.deepEqual(seen, []);
  for (const [tool, change, folder] of [['Bash', 'cd ./-;', path.join(odd, '-')], ['Bash', `git -C "${path.join(odd, '-')}"`, path.join(odd, '-')], ['PowerShell', 'Set-Location .\\-;', path.join(odd, '-')]]) {
    seen.length = 0;
    assert.equal(verdict(tool, change.startsWith('git') ? `${change} commit -m x` : `${change} git commit -m x`, odd, recording), 'allow', change);
    assert.deepEqual(seen, [folder], change);
  }
  seen.length = 0;
  const after = [
    ['Bash', `{ echo x; }; cd "${crates}";`],
    ['Bash', `{ echo x; }& cd "${crates}";`],
    ['Bash', `if true; then echo x; fi; cd "${crates}";`],
    ['Bash', `for i in 1; do echo x; done; cd "${crates}";`],
    ['Bash', `case x in x) echo x;; esac; cd "${crates}";`],
    ['Bash', `echo x > /dev/null; cd "${crates}";`],
    ['Bash', `cd "${crates}"; echo x &`],
    ['PowerShell', `Get-Date > $null; Set-Location "${crates}";`],
    ['PowerShell', `Set-Location "${crates}"; Get-Date &`],
    ['Bash', `bash -r -c 'echo x'; cd "${crates}";`],
  ];
  for (const [tool, change] of after) assert.equal(verdict(tool, `${change} ${push}`), 'allow', change);
  const picked = ['cd "${crates}" || exit 1 &', '{ cd "${crates}"; } &', 'cd "${crates}" extra;', 'cd -x "${crates}";', 'cd "${crates}" < /nonexistent-file;', 'coproc command cd "${crates}";', 'x=1 builtin cd "${crates}";', 'time command cd "${crates}";', 'Set-Location "${crates}" || exit 1 &', 'Set-Location "${crates}" extra;', 'Set-Location "${crates}" -Bogus;', 'Set-Location "${crates}" > C:\\no-such-dir\\x.txt;'].map((form) => form.replace('${crates}', crates));
  const chosen = changes.filter(([, change]) => picked.includes(change));
  assert.equal(chosen.length, picked.length);
  for (const [tool, change] of chosen) assert.equal(verdict(tool, `${change} git commit -m x`, ROOT, recording), 'secrets', change);
  assert.deepEqual(seen, []);
  const trusted = [
    ['Bash', `cd "${crates}" &&`],
    ['Bash', `cd "${crates}";`],
    ['Bash', `cd "${crates}"\n`],
    ['Bash', `cd -- "${crates}";`],
    ['Bash', `cd -L "${crates}";`],
    ['Bash', `cd -L -- "${crates}";`],
    ['Bash', `cd "${crates}" || exit 1;`],
    ['Bash', `cd "${crates}" || exit;`],
    ['Bash', `command cd "${crates}" &&`],
    ['Bash', `builtin cd "${crates}";`],
    ['Bash', `pushd "${crates}";`],
    ['Bash', 'cd crates &&'],
    ['Bash', `cd "${path.join(odd, 'x[1]')}";`, path.join(odd, 'x[1]')],
    ['PowerShell', `Set-Location "${crates}";`],
    ['PowerShell', `Set-Location -Path "${crates}";`],
    ['PowerShell', `Set-Location -LiteralPath "${crates}";`],
    ['PowerShell', `set-location -path "${crates}";`],
    ['PowerShell', `sl "${crates}";`],
    ['PowerShell', `chdir "${crates}";`],
    ['PowerShell', `Push-Location "${crates}";`],
    ['PowerShell', `Set-Location -LiteralPath "${path.join(odd, 'x[1]')}";`, path.join(odd, 'x[1]')],
    ['PowerShell', 'cd crates;'],
    ['PowerShell', `Set-Location "${crates}" || exit 1;`],
  ];
  for (const [tool, change, folder = crates] of trusted) {
    seen.length = 0;
    assert.equal(verdict(tool, `${change} git commit -m x`, ROOT, recording), 'allow', change);
    assert.deepEqual(seen.map((each) => each.toLowerCase()), [folder.toLowerCase()], change);
    assert.equal(verdict(tool, `${change} ${push}`), 'allow', change);
  }
  fs.rmSync(odd, { recursive: true, force: true });
});

test('the folder a program ends up in is the one bash makes of the word, and a restricted shell stays where it is', () => {
  const tw = fs.mkdtempSync(path.join(os.tmpdir(), 'ol-hook-'));
  for (const name of ['x1', 'x[1]', '{a,b}', 'a', 'b']) fs.mkdirSync(path.join(tw, name));
  const fwd = tw.replace(/\\/g, '/');
  const crates = path.join(ROOT, 'crates');
  const wrapper = path.join(ROOT, 'scripts', 'agent', 'git.ps1');
  const seen = [];
  const recording = { ...io, commit: (folder) => (seen.push(folder), []) };
  fs.symlinkSync(path.join(ROOT, '.claude'), path.join(tw, 'jlink'), 'junction');
  const expanded = [
    ['Bash', `cd -P "${fwd}/jlink/.."; git commit -m x`],
    ['Bash', `cd -P -- "${fwd}/jlink/.."; git commit -m x`],
    ['Bash', `cd -P "${fwd}/jlink/../crates"; git commit -m x`],
    ['Bash', `git -C ${fwd}/x[1] commit -m x`],
    ['Bash', `cd ${fwd}/x[1] && git commit -m x`],
    ['Bash', `cd ${fwd}/x[1]; git commit -m x`],
    ['Bash', `cd ${fwd}/"x"[1]; git commit -m x`],
    ['Bash', `cd "${fwd}/x"[1]; git commit -m x`],
    ['Bash', `cd ${fwd}/{a,b}; git commit -m x`],
    ['Bash', `git -C ${fwd}/{a,b} commit -m x`],
    ['Bash', `pwsh -NoProfile -File "${wrapper}" -C ${fwd}/x[1] commit -m x`],
    ['Bash', `cd -P "${crates}"; git commit -m x`],
    ['Bash', `cd -P "${crates}"; cd ..; git commit -m x`],
    ['Bash', `bash -r -c 'cd "${crates}"; git commit -m x'`],
    ['Bash', `bash -rc 'cd "${crates}"; git commit -m x'`],
    ['Bash', `bash --restricted -c 'cd "${crates}"; git commit -m x'`],
    ['Bash', `rbash -c 'cd "${crates}"; git commit -m x'`],
    ['PowerShell', `bash -r -c 'cd "${crates.replace(/\\/g, '/')}"; git commit -m x'`],
  ];
  for (const [tool, command] of expanded) assert.equal(verdict(tool, command, ROOT, recording), 'secrets', command);
  assert.deepEqual(seen, [], 'none was scanned in a folder the commit might not run in');
  assert.equal(verdict('Bash', `cd ${fwd}/x[1]; pwsh -NoProfile -File "${wrapper}" push origin m1/x`), 'tags');
  assert.equal(verdict('Bash', `pwsh -NoProfile -File "${wrapper}" -C ${fwd}/x[1] push origin m1/x`), 'tags');
  assert.equal(verdict('Bash', `cd -P "${crates}"; pwsh -NoProfile -File "${wrapper}" push origin m1/x`), 'tags');
  assert.equal(verdict('Bash', `cd -P "${fwd}/jlink/.."; pwsh -NoProfile -File "${wrapper}" push origin m1/x`), 'tags');
  assert.equal(verdict('Bash', "rbash -c 'tasklist'"), 'process', 'a restricted shell is read like any other');
  const kept = [
    ['Bash', `cd "${fwd}/x[1]" && git commit -m x`, path.join(tw, 'x[1]')],
    ['Bash', `git -C "${fwd}/x[1]" commit -m x`, path.join(tw, 'x[1]')],
    ['Bash', `git -C '${fwd}/{a,b}' commit -m x`, path.join(tw, '{a,b}')],
    ['Bash', `git -C ${fwd}/x1 commit -m x`, path.join(tw, 'x1')],
    ['PowerShell', `git -C ${tw}\\x[1] commit -m x`, path.join(tw, 'x[1]')],
    ['PowerShell', `git -C "${tw}\\x[1]" commit -m x`, path.join(tw, 'x[1]')],
    ['Bash', `bash -c 'cd "${crates}" && git commit -m x'`, crates],
    ['Bash', `cd -L "${crates}"; git commit -m x`, crates],
    ['Bash', `cd "${fwd}/jlink/.."; git commit -m x`, tw],
    ['Bash', `cd -L "${fwd}/jlink/.."; git commit -m x`, tw],
  ];
  for (const [tool, command, folder] of kept) {
    seen.length = 0;
    assert.equal(verdict(tool, command, ROOT, recording), 'allow', command);
    assert.deepEqual(seen.map((each) => each.toLowerCase()), [folder.toLowerCase()], command);
  }
  fs.rmSync(tw, { recursive: true, force: true });
});

test('a script named like the wrapper somewhere else is not the wrapper', () => {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'ol-hook-'));
  try {
    const agent = path.join(folder, 'scripts', 'agent');
    fs.mkdirSync(agent, { recursive: true });
    fs.mkdirSync(path.join(folder, '.git'));
    fs.writeFileSync(path.join(agent, 'gh.ps1'), 'gh @args\n');
    assert.equal(verdict('PowerShell', `& "${path.join(agent, 'gh.ps1')}" pr view 5`), 'github');
    assert.equal(verdict('Bash', `pwsh -NoProfile -File "${path.join(agent, 'gh.ps1')}" pr view 5`), 'github');
    assert.equal(verdict('PowerShell', 'scripts/agent/gh.ps1 pr view 5', folder), 'github');
    assert.equal(verdict('PowerShell', `cd "${folder}"; scripts/agent/gh.ps1 pr view 5`), 'github');
    assert.equal(verdict('PowerShell', `& "${path.join(ROOT, 'scripts', 'agent', 'gh.ps1')}" pr view 5`, folder), 'allow');
  } finally {
    fs.rmSync(folder, { recursive: true, force: true });
  }
});

test('a commit is scanned in the checkout it names, staged files only unless it stages more itself', () => {
  const seen = [];
  const recording = { ...io, commit: (folder, broad) => (seen.push([folder, broad]), []) };
  const other = path.join(ROOT, 'crates');
  for (const [tool, command] of [
    ['Bash', 'git commit -m x'],
    ['Bash', 'git commit --amend --no-edit -q'],
    ['Bash', 'git commit -am x'],
    ['Bash', 'git commit --all -m x'],
    ['Bash', 'git commit x.txt -m y'],
    ['Bash', 'git commit -m x -- x.txt'],
    ['Bash', 'git commit --only x.txt -m y'],
    ['Bash', 'git commit -p'],
    ['Bash', 'git commit --am'],
    ['Bash', 'git -C crates commit -m x'],
    ['PowerShell', `scripts/agent/git.ps1 -C "${other}" commit -m x`],
    ['Bash', `pwsh -NoProfile -File scripts/agent/git.ps1 -C "${other}" commit -m x`],
    ['Bash', 'cd crates && git commit -m x'],
  ]) {
    assert.equal(verdict(tool, command, ROOT, recording), 'allow', command);
  }
  assert.deepEqual(seen, [
    [ROOT, false],
    [ROOT, false],
    [ROOT, true],
    [ROOT, true],
    [ROOT, true],
    [ROOT, true],
    [ROOT, true],
    [ROOT, true],
    [ROOT, true],
    [other, false],
    [other, false],
    [other, false],
    [other, false],
  ]);
});

test('a commit with a finding is refused and the finding is shown masked', () => {
  const finding = 'notes.txt:3: email some…io';
  const refusal = judge({ tool: 'Bash', command: 'git commit -m x', cwd: ROOT, io: { ...io, commit: () => [finding] } });
  assert.equal(refusal.rule, 'secrets');
  assert.match(refusal.message, /notes\.txt:3: email some…io/);
});

test('a call for another tool is an error, not a pass', () => {
  assert.throws(() => judge({ tool: 'Read', command: 'tasklist', cwd: ROOT, io }));
  assert.throws(() => judge({ tool: 'Bash', command: undefined, cwd: ROOT, io }));
});

test('a very long command is judged in time', () => {
  const texts = {
    'one long argument': `git commit -m ${'a1'.repeat(150000)}`,
    'many folders up and down': `ls ${'a/../'.repeat(60000)}b`,
    'a long quoted command name': `"${' '.repeat(200000)}x" y`,
    'a long run of dots': `ls ${'.'.repeat(200000)}`,
    'a long command name full of braces': `${'{a,'.repeat(50000)} x`,
    'many words behind a wrapper': `env ${'a '.repeat(3000)}`,
    'many small commands': 'true; '.repeat(3000),
    'a long here-document': `cat <<'EOF'\n${"it's a line\n".repeat(20000)}EOF\ngit status`,
    'many expansions': `echo ${'$x '.repeat(40000)}`,
  };
  for (const [name, text] of Object.entries(texts)) {
    for (const tool of ['Bash', 'PowerShell']) {
      const from = process.hrtime.bigint();
      judge({ tool, command: text, cwd: ROOT, io });
      const ms = Number(process.hrtime.bigint() - from) / 1e6;
      assert.ok(ms < 3000, `${tool}, ${name}: ${ms.toFixed(0)} ms`);
    }
  }
});

test('a deeply nested or very long command is refused, not skipped', () => {
  let nested = 'tasklist';
  for (let k = 0; k < 12; k++) nested = `bash -c ${sh.single(nested)}`;
  assert.notEqual(verdict('Bash', nested), 'allow');
  assert.equal(verdict('Bash', `env ${'env '.repeat(40)}true`), 'unreadable');
});
