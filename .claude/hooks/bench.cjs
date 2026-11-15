'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ROOT = path.resolve(__dirname, '..', '..');
const RUNS = 50;
const BUDGET_MS = 100;
const settings = JSON.parse(fs.readFileSync(path.join(ROOT, '.claude', 'settings.json'), 'utf8'));
const [shell, write, read] = settings.hooks.PreToolUse.map((group) => group.hooks[0]);

const CALLS = [
  ['Read, allowed', read, 0, { tool_name: 'Read', tool_input: { file_path: path.join(ROOT, 'Cargo.toml') } }],
  ['Read, refused', read, 2, { tool_name: 'Read', tool_input: { file_path: path.join(ROOT, 'keys', 'app.private-key.pem') } }],
  ['Grep, allowed', read, 0, { tool_name: 'Grep', tool_input: { pattern: 'fn main', path: path.join(ROOT, 'crates'), glob: '*.rs' } }],
  ['Glob, allowed', read, 0, { tool_name: 'Glob', tool_input: { pattern: 'crates/**/*.rs' } }],
  ['PowerShell, push through the wrapper', shell, 0, { tool_name: 'PowerShell', tool_input: { command: 'pwsh -NoProfile -File scripts/agent/git.ps1 push -u origin m1/some-branch' } }],
  ['Bash, commit through the wrapper', shell, null, { tool_name: 'Bash', tool_input: { command: `pwsh -NoProfile -File "${path.join(ROOT, 'scripts', 'agent', 'git.ps1')}" -C "${ROOT}" commit -q -m "a step, with a comma"` } }],
  ['Bash, allowed', shell, 0, { tool_name: 'Bash', tool_input: { command: 'cargo nextest run -p ol-model 2>&1 | tail -20' } }],
  ['Bash, refused', shell, 2, { tool_name: 'Bash', tool_input: { command: 'bash -c "tasklist | grep x"' } }],
  ['PowerShell, allowed', shell, 0, { tool_name: 'PowerShell', tool_input: { command: 'Get-ChildItem -Recurse crates | Where-Object { $_.Length -gt 1mb } | Select-Object -First 5' } }],
  ['PowerShell, wrapper call', shell, 0, { tool_name: 'PowerShell', tool_input: { command: 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr view 5' } }],
  ['Edit, allowed', write, 0, { tool_name: 'Edit', tool_input: { file_path: path.join(ROOT, 'crates', 'ol-model', 'src', 'lib.rs') } }],
  ['Write, refused', write, 2, { tool_name: 'Write', tool_input: { file_path: path.join(ROOT, 'spec', 'x.md') } }],
  ['Bash, commit with the scan of what is staged', shell, null, { tool_name: 'Bash', tool_input: { command: 'git commit -m "a step"' } }],
];

const fixture = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'ol-bench-'));
const git = (...args) => spawnSync('git', ['-C', fixture, '-c', 'user.name=fixture', '-c', 'user.email=fixture@example.com', '-c', 'commit.gpgsign=false', ...args]);
git('init', '-q');
for (let k = 0; k < 5; k++) fs.writeFileSync(path.join(fixture, `file${k}.rs`), 'fn main() {}\n'.repeat(400));
git('add', '-A');
git('commit', '-q', '-m', 'fixture');
for (let k = 0; k < 5; k++) fs.appendFileSync(path.join(fixture, `file${k}.rs`), 'fn more() {}\n');
git('add', '-A');
fs.appendFileSync(path.join(fixture, 'file0.rs'), 'fn later() {}\n');
CALLS.push(['Bash, commit of five staged files, one changed again since', shell, 0, { cwd: fixture, tool_name: 'Bash', tool_input: { command: 'git commit -m "a step"' } }]);
process.on('exit', () => fs.rmSync(fixture, { recursive: true, force: true }));

let slow = false;
for (const [name, hook, expected, input] of CALLS) {
  const fill = (text) => text.replaceAll('${CLAUDE_PROJECT_DIR}', ROOT);
  const stdin = JSON.stringify({ session_id: 'bench', hook_event_name: 'PreToolUse', cwd: ROOT, ...input });
  const times = [];
  for (let k = 0; k < RUNS; k++) {
    const from = process.hrtime.bigint();
    const run = spawnSync(fill(hook.command), hook.args.map(fill), { input: stdin, cwd: ROOT, env: { ...process.env, CLAUDE_PROJECT_DIR: ROOT } });
    times.push(Number(process.hrtime.bigint() - from) / 1e6);
    if (run.error || (expected !== null && run.status !== expected)) {
      console.error(`${name}: expected exit ${expected}, got ${run.error ?? run.status}`);
      process.exit(1);
    }
  }
  times.sort((a, b) => a - b);
  const median = (times[RUNS / 2 - 1] + times[RUNS / 2]) / 2;
  slow ||= median >= BUDGET_MS;
  console.log(`${name}: median ${median.toFixed(1)} ms, fastest ${times[0].toFixed(1)}, slowest ${times[RUNS - 1].toFixed(1)} (${RUNS} runs)`);
}
console.log(slow ? `over the budget of ${BUDGET_MS} ms` : `every median is under ${BUDGET_MS} ms`);
process.exit(slow ? 1 : 0);
