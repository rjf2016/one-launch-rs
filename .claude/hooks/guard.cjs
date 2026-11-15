'use strict';

process.exitCode = 2;

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { Refusal, explain } = require('./lib/rules.cjs');
const checkout = require('./lib/checkout.cjs');

const HOME = path.resolve(__dirname, '..', '..');
const CHECK = ['run', '--locked', '--package', 'xtask', '--', 'check', '--ticket'];

const io = {
  wrapper: (file, name) => checkout.isWrapper(file, name, HOME),
  isTag: (folder, name) => require('./lib/secrets.cjs').isTag(folder, name),
  commit(folder, broad) {
    const root = checkout.nearest(folder);
    if (!root) throw new Refusal('secrets', 'the commit is not inside a checkout the hook can find');
    return require('./lib/secrets.cjs').commitFindings(root, broad);
  },
};

function shell(input) {
  const { judge } = require('./lib/commands.cjs');
  const refusal = judge({ tool: input.tool_name, command: input.tool_input?.command, cwd: input.cwd, io });
  if (refusal) throw refusal;
}

function write(input) {
  checkout.judgeWrite(input.tool_input?.file_path ?? input.tool_input?.notebook_path);
}

function format(input) {
  const file = input.tool_input?.file_path;
  if (typeof file !== 'string' || !/\.rs$/i.test(file) || !fs.existsSync(file)) return;
  const root = checkout.nearest(path.dirname(file));
  if (!root || !fs.existsSync(path.join(root, 'rustfmt.toml'))) return;
  if (/^(oracle|spec)\//i.test(path.relative(root, file).split(path.sep).join('/'))) return;
  spawnSync('rustfmt', [file], { cwd: root, timeout: 20000, windowsHide: true });
}

function stop(input) {
  if (input.agent_type === '') return null;
  const root = checkout.nearest(input.cwd ?? '.');
  const id = root && checkout.ticket(root);
  if (!id) return null;
  const run = spawnSync('cargo', [...CHECK, id], { cwd: root, encoding: 'utf8', maxBuffer: 1 << 28, timeout: 1500000, windowsHide: true });
  if (!run.error && run.status === 0) return null;
  const tail = `${run.stdout ?? ''}\n${run.stderr ?? ''}`.trim().split(/\r?\n/).slice(-40).join('\n').slice(-6000);
  const how = run.error ? `could not run (${run.error.code ?? run.error.message})` : `failed with exit code ${run.status}`;
  return `The check for ticket ${id} ${how} in ${root}: cargo ${[...CHECK, id].join(' ')}\nMake it pass before stopping, or report why it cannot pass.\nRule: plan/design/workflow.md "Hooks" and "Definition of done".\n\n${tail}`;
}

function main() {
  const mode = process.argv[2];
  const input = JSON.parse(fs.readFileSync(0, 'utf8'));
  if (mode === 'shell') shell(input);
  else if (mode === 'write') write(input);
  else if (mode === 'format') format(input);
  else if (mode === 'stop') {
    const reason = stop(input);
    if (reason) fs.writeSync(1, JSON.stringify({ decision: 'block', reason }));
  } else throw new Error(`unknown mode ${mode}`);
  process.exitCode = 0;
}

try {
  main();
} catch (error) {
  fs.writeSync(2, explain(error));
  process.exitCode = 2;
}

module.exports = true;
