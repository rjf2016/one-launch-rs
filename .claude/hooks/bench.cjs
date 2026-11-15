'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ROOT = path.resolve(__dirname, '..', '..');
const RUNS = 50;
const BUDGET_MS = 100;
const settings = JSON.parse(fs.readFileSync(path.join(ROOT, '.claude', 'settings.json'), 'utf8'));
const [shell, write] = settings.hooks.PreToolUse.map((group) => group.hooks[0]);

const CALLS = [
  ['Bash, allowed', shell, 0, { tool_name: 'Bash', tool_input: { command: 'cargo nextest run -p ol-model 2>&1 | tail -20' } }],
  ['Bash, refused', shell, 2, { tool_name: 'Bash', tool_input: { command: 'bash -c "tasklist | grep x"' } }],
  ['PowerShell, allowed', shell, 0, { tool_name: 'PowerShell', tool_input: { command: 'Get-ChildItem -Recurse crates | Where-Object { $_.Length -gt 1mb } | Select-Object -First 5' } }],
  ['PowerShell, wrapper call', shell, 0, { tool_name: 'PowerShell', tool_input: { command: 'pwsh -NoProfile -File scripts/agent/gh.ps1 pr view 5' } }],
  ['Edit, allowed', write, 0, { tool_name: 'Edit', tool_input: { file_path: path.join(ROOT, 'crates', 'ol-model', 'src', 'lib.rs') } }],
  ['Write, refused', write, 2, { tool_name: 'Write', tool_input: { file_path: path.join(ROOT, 'spec', 'x.md') } }],
  ['Bash, commit with the scan of what is staged', shell, null, { tool_name: 'Bash', tool_input: { command: 'git commit -m "a step"' } }],
];

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
