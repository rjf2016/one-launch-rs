'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { after, before, test } = require('node:test');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const SETTINGS = JSON.parse(fs.readFileSync(path.join(ROOT, '.claude', 'settings.json'), 'utf8'));
const made = [];
let main;
let tree;

after(() => {
  for (const folder of made) fs.rmSync(folder, { recursive: true, force: true });
});

function folder() {
  const created = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'ol-hook-')));
  made.push(created);
  return created;
}

function put(root, relative, text = '') {
  const file = path.join(root, relative);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
  return file;
}

function git(cwd, ...args) {
  const env = { ...process.env, GIT_CONFIG_GLOBAL: path.join(made[0], 'no-git-config'), GIT_CONFIG_NOSYSTEM: '1' };
  const identity = ['-c', 'user.name=fixture', '-c', 'user.email=fixture@example.com', '-c', 'commit.gpgsign=false', '-c', 'init.defaultBranch=main', '-c', 'core.autocrlf=false'];
  const run = spawnSync('git', [...identity, ...args], { cwd, env, encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
}

function hooksInto(root) {
  fs.cpSync(path.join(ROOT, '.claude', 'hooks'), path.join(root, '.claude', 'hooks'), { recursive: true, filter: (source) => path.basename(source) !== 'test' });
  fs.copyFileSync(path.join(ROOT, '.claude', 'settings.json'), path.join(root, '.claude', 'settings.json'));
}

function project() {
  const root = folder();
  git(root, 'init', '-q');
  hooksInto(root);
  for (const file of ['plan/tickets/M1-F6b.md', 'plan/tickets/M1-F6c.md', 'xtask/secrets.allow', 'rustfmt.toml', 'rust-toolchain.toml', '.gitignore']) {
    put(root, file, fs.readFileSync(path.join(ROOT, file), 'utf8'));
  }
  put(root, 'scripts/agent/git.ps1', 'exit 0\n');
  put(root, 'scripts/agent/gh.ps1', 'exit 0\n');
  put(root, 'docs/tasklist-notes.md', 'notes\n');
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'fixture');
  return root;
}

before(() => {
  main = project();
  tree = `${main}-tree`;
  made.push(tree);
  git(main, 'worktree', 'add', '-q', '-b', 'tree', tree);
});

function fire(event, tool, input, { home = main, env = {} } = {}) {
  const groups = SETTINGS.hooks[event].filter((group) => !group.matcher || group.matcher.split('|').includes(tool));
  const runs = groups.flatMap((group) =>
    group.hooks.map((hook) => {
      const fill = (text) => text.replaceAll('${CLAUDE_PROJECT_DIR}', home);
      const stdin = typeof input === 'string' ? input : JSON.stringify({ session_id: 's', hook_event_name: event, tool_name: tool, cwd: home, ...input });
      const run = spawnSync(fill(hook.command), hook.args.map(fill), {
        input: stdin,
        cwd: home,
        encoding: 'utf8',
        env: { ...process.env, CLAUDE_PROJECT_DIR: home, ...env },
      });
      const failed = run.error || (run.status !== 0 && run.status !== 2);
      return { status: run.status, stdout: run.stdout, stderr: run.stderr, blocked: run.status === 2 || (failed && hook.onFailure === 'block') };
    }),
  );
  return { ran: runs.length, blocked: runs.some((run) => run.blocked), status: runs.map((run) => run.status), stderr: runs.map((run) => run.stderr).join(''), stdout: runs.map((run) => run.stdout).join('') };
}

const shell = (tool, command, cwd = main, options) => fire('PreToolUse', tool, { cwd, tool_input: { command, description: 'x' } }, options);
const edit = (tool, file, cwd = main, options) => {
  const input = tool === 'NotebookEdit' ? { notebook_path: file, new_source: 'x' } : tool === 'Write' ? { file_path: file, content: 'x' } : { file_path: file, old_string: 'a', new_string: 'b' };
  return fire('PreToolUse', tool, { cwd, tool_input: input }, options);
};

const token = () => `gh${'s'}_${'Zk3'.repeat(4)}${'Qw9Lm2Xv7Tb4Nc8Rd1Fg6Hj5'}`;
const address = () => `${'someone'}@${'corp-mail'}.io`;
const profile = () => `C:\\${'Users'}\\${'jdoe'}\\AppData\\Roaming`;

test('settings.json wires each hook as node with no shell in between, and a failed tool-call hook blocks', () => {
  const { PreToolUse, PostToolUse, SubagentStop } = SETTINGS.hooks;
  assert.deepEqual(PreToolUse.map((group) => group.matcher), ['Bash|PowerShell', 'Edit|Write|NotebookEdit|MultiEdit', 'Read|Grep|Glob']);
  assert.deepEqual(PostToolUse.map((group) => group.matcher), ['Edit|Write|MultiEdit']);
  assert.deepEqual(SubagentStop.map((group) => group.matcher), [undefined]);
  const hooks = [...PreToolUse, ...PostToolUse, ...SubagentStop].flatMap((group) => group.hooks);
  assert.deepEqual(hooks.map((hook) => hook.args[3]), ['shell', 'write', 'read', 'format', 'stop']);
  for (const hook of hooks) {
    assert.equal(hook.type, 'command');
    assert.equal(hook.command, 'node');
    assert.equal(hook.args[0], '-e');
    assert.equal(hook.args[1], hooks[0].args[1]);
    assert.ok(!/["\\]/.test(hook.args[1]), 'the loader must need no quoting');
    assert.equal(hook.args[2], '${CLAUDE_PROJECT_DIR}/.claude/hooks/guard.cjs');
    assert.equal(hook.shell, undefined);
  }
  for (const group of PreToolUse) for (const hook of group.hooks) assert.equal(hook.onFailure, 'block');
  assert.equal(SETTINGS.disableAllHooks, false, 'a user-level switch must not turn these hooks off');
});

test('what an agent reads of a refusal starts with the hook\'s own line, and nothing in that line reads like a refusal', () => {
  for (const group of SETTINGS.hooks.PreToolUse) {
    const [hook] = group.hooks;
    const echoed = `${hook.command} ${hook.args.join(' ')}`;
    assert.ok(echoed.length < 320, `Claude Code prints these ${echoed.length} characters before the reason`);
    assert.deepEqual(echoed.match(/[A-Za-z]+ [A-Za-z]+ [A-Za-z]+/g), null, 'no run of three plain words, so no sentence');
    assert.ok(!/refus|could not|denied|blocked|not allowed/i.test(echoed));
  }
  const refused = shell('PowerShell', 'gh --version');
  assert.equal(refused.stderr, "Refused by the safety hook: plain gh acts with the owner's login, reads included; use scripts/agent/gh.ps1.\nRule: GitHub other than as the app (plan/design/workflow.md \"Acting as the app\").\n");
});

test('Read, Grep and Glob: the real data folder, the key folder and a .pem file are refused wherever they are', () => {
  const roaming = path.join(folder(), 'AppData', 'Roaming');
  fs.mkdirSync(path.join(roaming, 'Other'), { recursive: true });
  const env = { APPDATA: roaming };
  const read = (tool, input, cwd = main) => fire('PreToolUse', tool, { cwd, tool_input: input }, { env });
  const data = path.join(roaming, 'OneLaunch');
  const refused = [
    ['Read', { file_path: path.join(data, 'config.json') }],
    ['Read', { file_path: path.join(roaming, 'Other', '..', 'ONELAUNCH.', 'vault.bin') }],
    ['Read', { file_path: `\\\\?\\${path.join(data, 'config.json')}` }],
    ['Read', { file_path: path.join(main, 'keys', 'app.private-key.pem') }],
    ['Read', { file_path: path.join(os.tmpdir(), 'one-launch-rs-bot', 'notes.txt') }],
    ['Grep', { pattern: 'x', path: data }],
    ['Grep', { pattern: 'x', path: roaming }],
    ['Grep', { pattern: 'x', path: path.parse(roaming).root }],
    ['Grep', { pattern: 'x', path: main, glob: '**/*.pem' }],
    ['Glob', { pattern: path.join(data, '**', '*.json') }],
    ['Glob', { pattern: `${roaming}\\*\\*.json` }],
    ['Glob', { pattern: '*.json', path: data }],
    ['Glob', { pattern: '**/*.pem' }],
    ['Read', {}],
  ];
  for (const [tool, input] of refused) {
    const result = read(tool, input);
    assert.deepEqual([result.ran, result.blocked, result.status], [1, true, [2]], `${tool} ${JSON.stringify(input)}`);
    assert.match(result.stderr, /^Refused by the safety hook: /);
  }
  assert.match(read('Read', { file_path: path.join(data, 'config.json') }).stderr, /Rule: the real data folder/);
  assert.match(read('Read', { file_path: path.join(main, 'keys', 'app.private-key.pem') }).stderr, /Rule: the app's key and tokens/);
  const allowed = [
    ['Read', { file_path: path.join(main, 'docs', 'tasklist-notes.md') }],
    ...['spec/a.md', 'oracle/PIN', '.claude/settings.json', '.claude/hooks/guard.cjs', 'scripts/agent/gh.ps1', '.github/CODEOWNERS', '.git/config'].map((file) => ['Read', { file_path: path.join(tree, file) }]),
    ['Read', { file_path: path.join(path.parse(main).root, 'somewhere', 'else', 'src', 'main.ts') }],
    ['Read', { file_path: path.join(os.tmpdir(), 'OneLaunch-scratch', 'config.json') }],
    ['Read', { file_path: path.join(roaming, 'Other', 'settings.json') }],
    ['Read', { file_path: path.join(os.homedir(), '.claude', 'projects', 'x', 'memory', 'MEMORY.md') }],
    ['Grep', { pattern: 'OneLaunch|\\.pem', path: main }],
    ['Grep', { pattern: 'x', path: path.join(roaming, 'Other'), glob: '*.json' }],
    ['Grep', { pattern: 'x' }],
    ['Glob', { pattern: 'crates/**/*.rs' }],
    ['Glob', { pattern: '**/*.md', path: tree }],
  ];
  for (const [tool, input] of allowed) {
    assert.deepEqual(read(tool, input), { ran: 1, blocked: false, status: [0], stderr: '', stdout: '' }, `${tool} ${JSON.stringify(input)}`);
  }
  assert.equal(read('Grep', { pattern: 'x' }, roaming).blocked, true, 'a search with no path runs in the folder the call is made from');
  for (const tool of ['Edit', 'Write', 'NotebookEdit', 'MultiEdit']) {
    assert.equal(edit(tool, path.join(data, 'config.json'), main, { env }).blocked, true, tool);
    assert.equal(edit(tool, path.join(folder(), 'app.private-key.pem'), main, { env }).blocked, true, tool);
    assert.equal(edit(tool, path.join(roaming, 'Other', 'settings.json'), main, { env }).blocked, false, tool);
  }
  assert.equal(fire('PreToolUse', 'Glob', { tool_name: 'WebFetch', tool_input: { url: 'x' } }, { env }).blocked, true, 'another tool on this hook is an error, not a pass');
});

test('Bash and PowerShell calls: a refusal exits 2 and names the rule, an allowed call exits 0 in silence', () => {
  for (const cwd of [main, tree]) {
    const refused = shell('Bash', 'tasklist', cwd);
    assert.deepEqual([refused.ran, refused.blocked, refused.status], [1, true, [2]]);
    assert.match(refused.stderr, /^Refused by the safety hook: tasklist lists or ends processes/);
    assert.match(refused.stderr, /Rule: process tools \(CLAUDE\.md "Standing rules" \(Riot Games\) and plan\/design\/workflow\.md "Hooks"\)/);
    assert.deepEqual(shell('Bash', 'git status', cwd), { ran: 1, blocked: false, status: [0], stderr: '', stdout: '' });
    const powershell = shell('PowerShell', 'Get-Process', cwd);
    assert.deepEqual([powershell.blocked, powershell.status], [true, [2]]);
    assert.deepEqual(shell('PowerShell', 'Get-Content docs\\tasklist-notes.md', cwd), { ran: 1, blocked: false, status: [0], stderr: '', stdout: '' });
  }
});

test('the wrappers are known by where they are: in the session\'s checkout or one of its worktrees', () => {
  const inMain = path.join(main, 'scripts', 'agent');
  const inTree = path.join(tree, 'scripts', 'agent');
  assert.equal(shell('PowerShell', 'scripts/agent/gh.ps1 pr view 5', main).blocked, false);
  assert.equal(shell('PowerShell', 'scripts/agent/gh.ps1 pr view 5', tree).blocked, false);
  assert.equal(shell('Bash', `pwsh -NoProfile -File "${inTree}\\git.ps1" -C "${tree}" push origin tree`, main).blocked, false);
  assert.equal(shell('PowerShell', `& "${inMain}\\gh.ps1" pr merge 5 --auto --squash`, tree).blocked, false);
  assert.match(shell('PowerShell', `& "${inTree}\\gh.ps1" pr merge 5 --squash`, main).stderr, /Rule: merge without --auto/);
  const stranger = project();
  assert.match(shell('PowerShell', `& "${stranger}\\scripts\\agent\\gh.ps1" pr view 5`, main).stderr, /Rule: GitHub other than as the app/);
  assert.match(shell('PowerShell', 'scripts/agent/gh.ps1 pr view 5', stranger).stderr, /Rule: GitHub other than as the app/);
  assert.match(shell('Bash', 'gh pr view 5', main).stderr, /Rule: GitHub other than as the app/);
});

test('Edit, Write and NotebookEdit calls are judged by the checkout the path is in', () => {
  for (const tool of ['Edit', 'Write', 'NotebookEdit', 'MultiEdit']) {
    for (const root of [main, tree]) {
      const refused = edit(tool, path.join(root, '.claude', 'settings.json'));
      assert.deepEqual([refused.ran, refused.blocked, refused.status], [1, true, [2]], `${tool} ${root}`);
      assert.match(refused.stderr, /\.claude\/settings\.json is an owner-approved path/);
      assert.match(refused.stderr, /Rule: owner-approved paths \(plan\/design\/workflow\.md "Owner-approved paths"\)/);
      assert.deepEqual(edit(tool, path.join(root, 'crates', 'a.rs')), { ran: 1, blocked: false, status: [0], stderr: '', stdout: '' });
    }
    assert.equal(edit(tool, path.join(os.homedir(), '.claude', 'projects', 'x', 'memory', 'note.md')).blocked, false);
  }
  for (const guarded of ['spec/a.md', 'oracle/PIN', '.github/CODEOWNERS', '.claude/hooks/guard.cjs', 'deny.toml', 'scripts/agent/gh.ps1', 'xtask/src/policy/scope.rs']) {
    assert.equal(edit('Write', path.join(tree, guarded)).blocked, true, guarded);
  }
});

test('with a ticket set in a worktree, its write_scope decides there and nowhere else', () => {
  const owners = path.join('.github', 'CODEOWNERS');
  const workflow = path.join('.github', 'workflows', 'ci.yml');
  const verdicts = () => [owners, workflow].flatMap((file) => [main, tree].map((root) => edit('Edit', path.join(root, file)).blocked));
  put(tree, '.claude/ticket.local.json', '{"ticket": "M1-F6b"}');
  assert.deepEqual(verdicts(), [true, false, true, true]);
  assert.match(edit('Edit', path.join(tree, workflow)).stderr, /\.github\/workflows\/ci\.yml is outside the write_scope of M1-F6b\.\nRule: write_scope \(plan\/design\/workflow\.md "Ticket" and "Hooks"\)/);
  put(tree, '.claude/ticket.local.json', '{"ticket": "M1-F6c"}');
  assert.deepEqual(verdicts(), [true, true, true, false]);
  assert.equal(edit('Write', path.join(tree, '.claude', 'ticket.local.json')).blocked, true);
  put(tree, '.claude/ticket.local.json', 'not json');
  assert.equal(edit('Edit', path.join(tree, 'crates', 'a.rs')).blocked, true);
  fs.rmSync(path.join(tree, '.claude', 'ticket.local.json'));
  assert.deepEqual(verdicts(), [true, true, true, true]);
  assert.equal(edit('Edit', path.join(tree, 'crates', 'a.rs')).blocked, false);
});

test('a commit whose staged files hold a planted token, address or profile path is refused', () => {
  const wrapper = path.join(main, 'scripts', 'agent', 'git.ps1');
  const commits = [
    ['Bash', 'git commit -m "a step"', tree],
    ['PowerShell', 'git commit -m "a step"', tree],
    ['Bash', `git -C "${tree}" commit -m "a step"`, main],
    ['Bash', `pwsh -NoProfile -File "${wrapper}" -C "${tree}" commit -m "a step"`, main],
    ['PowerShell', `& "${wrapper}" -C "${tree}" commit -m "a step"`, main],
    ['PowerShell', `Set-Location "${tree}"; git commit -m "a step"`, main],
  ];
  const allAllowed = () => commits.map(([tool, command, cwd]) => shell(tool, command, cwd).blocked);
  put(tree, 'notes/clean.md', 'nothing to see\n');
  git(tree, 'add', '-A');
  assert.deepEqual(allAllowed(), Array(6).fill(false));
  const planted = { token: [token(), /notes\/planted\.md:2: github-token ghs_…/], address: [address(), /notes\/planted\.md:2: email some…io/], profile: [profile(), /notes\/planted\.md:2: user-folder j…\(4 chars\)/] };
  for (const [name, [value, shown]] of Object.entries(planted)) {
    put(tree, 'notes/planted.md', `first line\nsecond ${value}\n`);
    git(tree, 'add', '-A');
    for (const [tool, command, cwd] of commits) {
      const refused = shell(tool, command, cwd);
      assert.deepEqual([refused.blocked, refused.status], [true, [2]], `${name}: ${command}`);
      assert.match(refused.stderr, shown);
      assert.match(refused.stderr, /Rule: the secret scan before a commit/);
      assert.ok(!refused.stderr.includes(value), 'the value must not be shown whole');
    }
    assert.equal(shell('Bash', 'git commit -m "a step"', main).blocked, false, 'the other checkout is not affected');
  }
  put(tree, 'notes/planted.md', 'first line\nsecond line\n');
  assert.equal(shell('Bash', 'git commit -m "a step"', tree).blocked, true, 'what is staged counts, not what the folder holds');
  assert.equal(shell('Bash', 'git commit -am "a step"', tree).blocked, true);
  git(tree, 'add', '-A');
  assert.deepEqual(allAllowed(), Array(6).fill(false));
  put(tree, 'notes/clean.md', `${address()}\n`);
  assert.equal(shell('Bash', 'git commit -m "a step"', tree).blocked, false);
  assert.equal(shell('Bash', 'git commit -am "a step"', tree).blocked, true, '-a commits what is not staged yet');
  assert.equal(shell('Bash', 'git commit notes/clean.md -m "a step"', tree).blocked, true);
  assert.equal(shell('Bash', 'git add -A && git commit -m "a step"', tree).blocked, true);
  put(tree, 'notes/clean.md', 'nothing to see\n');
});

test('a push through the wrapper of a name that is a tag in that checkout is refused', () => {
  const wrapper = path.join(main, 'scripts', 'agent', 'git.ps1');
  const head = spawnSync('git', ['-C', main, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).stdout.trim();
  put(main, '.git/refs/tags/v9', `${head}\n`);
  const tagged = shell('PowerShell', `& "${wrapper}" -C "${tree}" push origin v9`, main);
  assert.deepEqual([tagged.blocked, tagged.status], [true, [2]]);
  assert.match(tagged.stderr, /v9 is a tag in this checkout\.\nRule: tags and releases/);
  assert.equal(shell('PowerShell', `& "${wrapper}" -C "${tree}" push origin HEAD:v9`, main).blocked, true);
  assert.equal(shell('PowerShell', `& "${wrapper}" -C "${tree}" push origin tree`, main).blocked, false);
  assert.equal(shell('PowerShell', `& "${wrapper}" -C "${tree}" push origin v8`, main).blocked, false);
  fs.mkdirSync(path.join(main, '.git', 'reftable'));
  assert.equal(shell('PowerShell', `& "${wrapper}" -C "${tree}" push origin v9`, main).blocked, true, 'where the files cannot tell, git is asked');
  assert.equal(shell('PowerShell', `& "${wrapper}" -C "${tree}" push origin v8`, main).blocked, false);
  fs.rmdirSync(path.join(main, '.git', 'reftable'));
  fs.rmSync(path.join(main, '.git', 'refs', 'tags', 'v9'));
});

test('the first commit of a repository is scanned like any other, and so is a staged file whose working copy has moved on', () => {
  const fresh = folder();
  git(fresh, 'init', '-q');
  put(fresh, 'notes.md', `a planted ${token()}\n`);
  put(fresh, 'sub/clean.md', 'nothing to see\n');
  git(fresh, 'add', '-A');
  const refused = shell('Bash', 'git commit -m "first"', fresh);
  assert.deepEqual([refused.blocked, refused.status], [true, [2]]);
  assert.match(refused.stderr, /notes\.md:1: github-token ghs_…/);
  put(fresh, 'notes.md', 'the working copy is clean, the staged one is not\n');
  assert.equal(shell('Bash', 'git commit -m "first"', fresh).blocked, true);
  git(fresh, 'add', '-A');
  assert.equal(shell('Bash', 'git commit -m "first"', fresh).blocked, false);
  put(fresh, 'notes.md', `the staged copy is clean, the working one holds ${token()}\n`);
  assert.equal(shell('Bash', 'git commit -m "first"', fresh).blocked, false);
  git(fresh, 'rm', '-q', '--cached', 'sub/clean.md');
  assert.equal(shell('Bash', 'git commit -m "first"', fresh).blocked, false, 'a file taken out of the commit is not read');
  git(fresh, 'commit', '-q', '-m', 'first');
  const head = spawnSync('git', ['-C', fresh, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).stdout.trim();
  git(fresh, 'update-index', '--add', '--cacheinfo', `160000,${head},vendor/sub`);
  assert.equal(shell('Bash', 'git commit -m "second"', fresh).blocked, false, 'a submodule entry is a commit id, not a file');
  const blob = spawnSync('git', ['-C', fresh, 'rev-parse', 'HEAD:notes.md'], { encoding: 'utf8' }).stdout.trim();
  const stages = [1, 2, 3].map((stage) => `100644 ${blob} ${stage}\tclash.md\n`).join('');
  assert.equal(spawnSync('git', ['-C', fresh, 'update-index', '--index-info'], { input: stages }).status, 0);
  assert.equal(shell('Bash', 'git commit -m "second"', fresh).blocked, false, 'an unmerged path is not staged content; git itself refuses that commit');
  git(fresh, 'rm', '-q', '--cached', 'clash.md');

  put(fresh, 'notes.md', `a planted ${token()}\n`);
  git(fresh, 'add', 'notes.md');
  put(fresh, 'notes.md', 'clean again in the working copy\n');
  const staged = spawnSync('git', ['-C', fresh, 'rev-parse', ':notes.md'], { encoding: 'utf8' }).stdout.trim();
  const object = path.join(fresh, '.git', 'objects', staged.slice(0, 2), staged.slice(2));
  assert.ok(fs.existsSync(object), 'a file staged a moment ago is a loose object');
  assert.equal(shell('Bash', 'git commit -m "second"', fresh).blocked, true);
  const kept = fs.readFileSync(object);
  fs.chmodSync(object, 0o644);
  fs.writeFileSync(object, 'not an object');
  assert.equal(shell('Bash', 'git commit -m "second"', fresh).blocked, true, 'a staged file that cannot be read is not passed over');
  fs.writeFileSync(object, kept);
  git(fresh, 'repack', '-a', '-d', '-q');
  git(fresh, 'prune-packed', '-q');
  assert.ok(!fs.existsSync(object), 'the staged file now lives in a pack');
  const packed = shell('Bash', 'git commit -m "second"', fresh);
  assert.equal(packed.blocked, true);
  assert.match(packed.stderr, /notes\.md:1: github-token ghs_…/);
});

test('a commit outside every checkout is refused, since nothing can be scanned', () => {
  const nowhere = folder();
  const refused = shell('Bash', 'git commit -m x', nowhere);
  assert.equal(refused.blocked, true);
  assert.match(refused.stderr, /not inside a checkout/);
});

function broken(change) {
  const root = folder();
  fs.mkdirSync(path.join(root, '.claude'));
  hooksInto(root);
  change(path.join(root, '.claude', 'hooks'));
  return root;
}

test('a hook that is missing, empty, cut short or throwing refuses the call', () => {
  const cases = {
    'guard.cjs renamed away': (hooks) => fs.renameSync(path.join(hooks, 'guard.cjs'), path.join(hooks, 'guard.cjs.away')),
    'the hooks folder gone': (hooks) => fs.rmSync(hooks, { recursive: true }),
    'guard.cjs empty': (hooks) => fs.writeFileSync(path.join(hooks, 'guard.cjs'), ''),
    'guard.cjs cut short': (hooks) => fs.writeFileSync(path.join(hooks, 'guard.cjs'), fs.readFileSync(path.join(hooks, 'guard.cjs'), 'utf8').slice(0, 900)),
    'guard.cjs throwing as it loads': (hooks) => fs.writeFileSync(path.join(hooks, 'guard.cjs'), "throw new Error('planted');\n"),
    'guard.cjs exiting 0 before it judged': (hooks) => fs.writeFileSync(path.join(hooks, 'guard.cjs'), 'process.exitCode = 0;\nnull.judge();\n'),
    'guard.cjs exiting 0 with no verdict': (hooks) => fs.writeFileSync(path.join(hooks, 'guard.cjs'), 'process.exitCode = 0;\n'),
    'a rule file missing': (hooks) => fs.rmSync(path.join(hooks, 'lib', 'rules.cjs')),
    'a rule file throwing': (hooks) => fs.writeFileSync(path.join(hooks, 'lib', 'checkout.cjs'), "throw new Error('planted');\n"),
    'the rules throwing while they judge': (hooks) => {
      fs.writeFileSync(path.join(hooks, 'lib', 'commands.cjs'), "module.exports = { judge() { throw new TypeError('planted'); } };\n");
      fs.appendFileSync(path.join(hooks, 'lib', 'checkout.cjs'), "\nmodule.exports.judgeWrite = () => { throw new TypeError('planted'); };\n");
    },
  };
  for (const [name, change] of Object.entries(cases)) {
    const home = broken(change);
    const bare = name === 'the hooks folder gone' || name === 'a rule file missing';
    const calls = [
      () => shell('Bash', 'git status', home, { home }),
      () => shell('PowerShell', 'Get-ChildItem', home, { home }),
      () => edit('Edit', path.join(home, 'crates', 'a.rs'), home, { home }),
      () => fire('PreToolUse', 'Read', { cwd: home, tool_input: { file_path: path.join(home, 'crates', 'a.rs') } }, { home }),
    ];
    for (const call of calls) {
      const result = call();
      assert.deepEqual([result.ran, result.blocked, result.status], [1, true, [2]], name);
      if (bare) assert.equal(result.stderr, 'fail closed MODULE_NOT_FOUND', name);
      else assert.match(result.stderr, /^Refused by the safety hook: the hook (could not be loaded|failed while reading this call) \(.+\)\.\nRule: fail closed/, name);
    }
  }
  const whole = broken(() => {});
  assert.equal(shell('Bash', 'git status', whole, { home: whole }).blocked, false);
  assert.equal(edit('Edit', path.join(whole, 'crates', 'a.rs'), whole, { home: whole }).blocked, false);
  const half = broken((hooks) => fs.rmSync(path.join(hooks, 'lib', 'commands.cjs')));
  assert.deepEqual(shell('Bash', 'git status', half, { home: half }).status, [2]);
  assert.deepEqual(shell('Bash', 'git commit -m x', half, { home: half }).status, [2]);
  const blind = broken((hooks) => fs.rmSync(path.join(hooks, 'lib', 'secrets.cjs')));
  fs.mkdirSync(path.join(blind, '.git'));
  assert.deepEqual(shell('Bash', 'git status', blind, { home: blind }).status, [0]);
  assert.deepEqual(shell('Bash', 'git commit -m x', blind, { home: blind }).status, [2]);
});

test('a call the hook cannot read refuses: bad JSON, no input, no command, no path, another tool', () => {
  const calls = [
    fire('PreToolUse', 'Bash', 'not json'),
    fire('PreToolUse', 'Bash', ''),
    fire('PreToolUse', 'Bash', '{"tool_name": "Bash"}'),
    fire('PreToolUse', 'Bash', { tool_input: {} }),
    fire('PreToolUse', 'Bash', { tool_input: { command: 42 } }),
    fire('PreToolUse', 'Bash', { tool_name: 'Read', tool_input: { command: 'git status' } }),
    fire('PreToolUse', 'Edit', 'not json'),
    fire('PreToolUse', 'Edit', { tool_input: {} }),
    fire('PreToolUse', 'Write', { tool_input: { file_path: 'relative.rs' } }),
  ];
  for (const [index, call] of calls.entries()) assert.deepEqual([call.blocked, call.status], [true, [2]], `call ${index}`);
});

test('a hook path that is not filled in refuses, and so does a failure to start under onFailure', () => {
  const hook = SETTINGS.hooks.PreToolUse[0].hooks[0];
  const fill = (text) => text.replaceAll('${CLAUDE_PROJECT_DIR}', main);
  for (const mode of ['nope', '', 'Shell']) {
    const args = [...hook.args.slice(0, 3).map(fill), mode];
    const odd = spawnSync(hook.command, args, { input: JSON.stringify({ tool_name: 'Bash', cwd: main, tool_input: { command: 'git status' } }), cwd: main, encoding: 'utf8' });
    assert.equal(odd.status, 2, `mode ${JSON.stringify(mode)}`);
    assert.match(odd.stderr, /unknown mode/);
  }
  const unfilled = spawnSync(hook.command, hook.args, { input: JSON.stringify({ tool_name: 'Bash', tool_input: { command: 'git status' } }), cwd: main, encoding: 'utf8' });
  assert.equal(unfilled.status, 2);
  assert.equal(unfilled.stderr, 'fail closed MODULE_NOT_FOUND');
  const noNode = fire('PreToolUse', 'Bash', { tool_input: { command: 'git status' } }, { env: { PATH: '', Path: '' } });
  assert.equal(noNode.blocked, true);
});

test('after an edit a Rust file is formatted, and nothing else is touched', () => {
  const ugly = 'fn  main( ){let x=1;println!("{}",x);}\n';
  const rust = put(tree, 'crates/a/src/main.rs', ugly);
  const sealed = put(tree, 'oracle/a.rs', ugly);
  const text = put(tree, 'crates/a/notes.md', ugly);
  const brokenRust = put(tree, 'crates/a/src/broken.rs', 'fn main( {\n');
  const outside = put(folder(), 'a.rs', ugly);
  const bare = folder();
  fs.mkdirSync(path.join(bare, '.git'));
  const unconfigured = put(bare, 'src/a.rs', ugly);
  for (const file of [rust, sealed, text, brokenRust, outside, unconfigured]) {
    for (const tool of ['Edit', 'Write']) {
      const run = fire('PostToolUse', tool, { cwd: main, tool_input: { file_path: file }, tool_response: {} });
      assert.deepEqual([run.ran, run.status, run.stdout, run.stderr], [1, [0], '', ''], file);
    }
  }
  assert.equal(fs.readFileSync(rust, 'utf8'), 'fn main() {\n    let x = 1;\n    println!("{}", x);\n}\n');
  for (const file of [sealed, text, outside, unconfigured]) assert.equal(fs.readFileSync(file, 'utf8'), ugly, file);
  assert.equal(fs.readFileSync(brokenRust, 'utf8'), 'fn main( {\n');
  assert.equal(fire('PostToolUse', 'Write', { cwd: main, tool_input: { file_path: path.join(tree, 'gone.rs') } }).status[0], 0);
});

function standIn() {
  const root = folder();
  fs.mkdirSync(path.join(root, '.git'));
  put(root, 'rust-toolchain.toml', fs.readFileSync(path.join(ROOT, 'rust-toolchain.toml'), 'utf8'));
  put(root, 'Cargo.toml', '[workspace]\nresolver = "3"\nmembers = ["xtask"]\n');
  put(root, 'Cargo.lock', 'version = 4\n\n[[package]]\nname = "xtask"\nversion = "0.0.0"\n');
  put(root, 'xtask/Cargo.toml', '[package]\nname = "xtask"\nversion = "0.0.0"\nedition = "2024"\n');
  put(
    root,
    'xtask/src/main.rs',
    'fn main() {\n    let args: Vec<String> = std::env::args().skip(1).collect();\n    println!("stand-in: {}", args.join(" "));\n    if args.last().is_some_and(|id| id.ends_with("RED")) {\n        eprintln!("planted failure");\n        std::process::exit(1);\n    }\n}\n',
  );
  return root;
}

test('when a subagent stops, the check of the ticket set in its checkout must pass', () => {
  const root = standIn();
  const env = { CARGO_TARGET_DIR: path.join(root, 'target') };
  const stop = (cwd, extra = {}) => fire('SubagentStop', '', { cwd, stop_hook_active: false, agent_id: 'a', agent_type: 'general-purpose', ...extra }, { env });

  assert.deepEqual(stop(root), { ran: 1, blocked: false, status: [0], stderr: '', stdout: '' });
  assert.ok(!fs.existsSync(path.join(root, 'target')), 'with no ticket set cargo is not run');

  put(root, '.claude/ticket.local.json', '{"ticket": "M9-GREEN"}');
  assert.deepEqual(stop(root), { ran: 1, blocked: false, status: [0], stderr: '', stdout: '' });
  assert.ok(fs.existsSync(path.join(root, 'target')), 'with a ticket set the check ran');
  assert.deepEqual(stop(path.join(root, 'xtask', 'src')).status, [0], 'the checkout is found from a folder inside it');

  put(root, '.claude/ticket.local.json', '{"ticket": "M9-RED"}');
  const red = stop(root);
  assert.deepEqual([red.status, red.stderr], [[0], '']);
  const decision = JSON.parse(red.stdout);
  assert.equal(decision.decision, 'block');
  assert.match(decision.reason, /^The check for ticket M9-RED failed with exit code 1 in /);
  assert.match(decision.reason, /cargo run --locked --package xtask -- check --ticket M9-RED\n/);
  assert.match(decision.reason, /stand-in: check --ticket M9-RED/);
  assert.match(decision.reason, /planted failure/);
  assert.match(decision.reason, /Rule: plan\/design\/workflow\.md "Hooks"/);
  assert.equal(JSON.parse(stop(root, { stop_hook_active: true }).stdout).decision, 'block');

  assert.deepEqual(stop(root, { agent_type: '' }).stdout, '', "Claude Code's own helper agents are not checked");
  assert.deepEqual(stop(main).stdout, '', 'another checkout with no ticket is not checked');
  assert.deepEqual(stop(folder()).stdout, '');

  put(root, '.claude/ticket.local.json', 'not json');
  const unreadable = stop(root);
  assert.deepEqual(unreadable.status, [2]);
  assert.match(unreadable.stderr, /ticket\.local\.json must be UTF-8 JSON/);

  put(root, '.claude/ticket.local.json', '{"ticket": "M9-GREEN"}');
  const noCargo = fire('SubagentStop', '', { cwd: root, agent_type: 'general-purpose' }, { env: { ...env, PATH: path.dirname(process.execPath), Path: path.dirname(process.execPath) } });
  assert.match(JSON.parse(noCargo.stdout).reason, /could not run \(ENOENT\)/);
});
