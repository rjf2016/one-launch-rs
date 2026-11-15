'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { after, test } = require('node:test');
const { Refusal } = require('../lib/rules.cjs');
const { judgeWrite, glob, writeScope, OWNER_APPROVED } = require('../lib/checkout.cjs');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const made = [];

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

function checkout(ticket) {
  const root = folder();
  fs.mkdirSync(path.join(root, '.git'));
  for (const id of ['M1-F6a', 'M1-F6b', 'M1-F6c']) {
    put(root, `plan/tickets/${id}.md`, fs.readFileSync(path.join(ROOT, 'plan', 'tickets', `${id}.md`), 'utf8'));
  }
  put(root, '.claude/settings.json', '{}');
  put(root, 'deny.toml');
  if (ticket) put(root, '.claude/ticket.local.json', JSON.stringify({ ticket }));
  return root;
}

function git(cwd, ...args) {
  const quiet = path.join(made[0] ?? os.tmpdir(), 'no-git-config');
  const env = { ...process.env, GIT_CONFIG_GLOBAL: quiet, GIT_CONFIG_NOSYSTEM: '1' };
  const identity = ['-c', 'user.name=fixture', '-c', 'user.email=fixture@example.com', '-c', 'commit.gpgsign=false', '-c', 'init.defaultBranch=main'];
  const run = spawnSync('git', [...identity, ...args], { cwd, env, encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
}

function verdict(file) {
  try {
    judgeWrite(file);
    return 'allow';
  } catch (error) {
    if (error instanceof Refusal) return error.rule;
    throw error;
  }
}

const verdicts = (root, paths) => Object.fromEntries(paths.map((relative) => [relative, verdict(path.join(root, relative))]));

test('with no ticket set, the owner-approved paths are refused and the rest is allowed', () => {
  const root = checkout(null);
  assert.deepEqual(
    verdicts(root, [
      'spec/x.md',
      'oracle/PIN',
      '.github/workflows/ci.yml',
      '.github/CODEOWNERS',
      '.claude/settings.json',
      '.claude/hooks/guard.cjs',
      '.claude/skills/x/SKILL.md',
      'deny.toml',
      'scripts/agent/git.ps1',
      'xtask/src/policy/scope.rs',
    ]),
    {
      'spec/x.md': 'owner',
      'oracle/PIN': 'owner',
      '.github/workflows/ci.yml': 'owner',
      '.github/CODEOWNERS': 'owner',
      '.claude/settings.json': 'owner',
      '.claude/hooks/guard.cjs': 'owner',
      '.claude/skills/x/SKILL.md': 'owner',
      'deny.toml': 'owner',
      'scripts/agent/git.ps1': 'owner',
      'xtask/src/policy/scope.rs': 'owner',
    },
  );
  assert.deepEqual(
    Object.values(
      verdicts(root, [
        'crates/ol-model/src/lib.rs',
        'plan/tickets/M9-X.md',
        'README.md',
        'CLAUDE.md',
        'xtask/src/main.rs',
        'xtask/src/secrets.rs',
        'deny.toml.bak',
        'specs/x.md',
        'docs/spec/x.md',
        'scripts/other/x.ps1',
        'crates/a/.github/x.yml',
      ]),
    ),
    Array(11).fill('allow'),
  );
});

test('the list of owner-approved paths is the one of the ticket', () => {
  assert.deepEqual(OWNER_APPROVED, ['spec/**', 'oracle/**', '.github/**', '.claude/**', 'deny.toml', 'scripts/agent/**', 'xtask/src/policy/**']);
});

test('another spelling of an owner-approved path is the same path', () => {
  const root = checkout(null);
  put(root, 'crates/a.rs');
  fs.symlinkSync(path.join(root, '.claude'), path.join(root, 'link'), 'junction');
  const spelled = {
    'two dots': path.join(root, 'crates') + '\\..\\.claude\\settings.json',
    'two dots through a folder that does not exist': `${root}\\nowhere\\..\\deny.toml`,
    'forward slashes': `${root.replace(/\\/g, '/')}/.claude/settings.json`,
    'mixed slashes': `${root}/.claude\\hooks/guard.cjs`,
    'doubled slashes': `${root}\\\\.claude\\\\settings.json`,
    'upper case': `${root}\\.CLAUDE\\SETTINGS.JSON`,
    'upper case, new file': `${root}\\SPEC\\New.md`,
    'upper case, file at the root': `${root}\\Deny.TOML`,
    'a trailing dot on the folder': `${root}\\.claude.\\settings.json`,
    'a trailing dot on the file': `${root}\\deny.toml.`,
    'a trailing space on the file': `${root}\\deny.toml `,
    'trailing dots and spaces on a new file': `${root}\\spec\\new.md. .`,
    'a junction to the folder': `${root}\\link\\settings.json`,
    'a junction with a trailing dot': `${root}\\link.\\settings.json`,
    'the drive letter in lower case': `${root[0].toLowerCase()}${root.slice(1)}\\deny.toml`,
  };
  const got = Object.fromEntries(Object.entries(spelled).map(([name, file]) => [name, verdict(file)]));
  assert.deepEqual(got, Object.fromEntries(Object.keys(spelled).map((name) => [name, 'owner'])));
  assert.equal(verdict(`${root}\\deny.toml::$DATA`), 'scope');
  assert.equal(verdict(`${root}\\crates\\a.rs:stream`), 'scope');
  assert.equal(verdict(`${root}\\crates\\..\\crates\\a.rs`), 'allow');
});

test('a ticket allows exactly what its write_scope names, owner-approved paths included', () => {
  const paths = ['.github/CODEOWNERS', '.github/workflows/ci.yml', 'CLAUDE.md', 'docs/cookbook/README.md', 'xtask/src/main.rs', 'xtask/src/policy/a/b.rs', 'xtask/src/secrets.rs', 'crates/a.rs'];
  assert.deepEqual(verdicts(checkout('M1-F6b'), paths), {
    '.github/CODEOWNERS': 'allow',
    '.github/workflows/ci.yml': 'scope',
    'CLAUDE.md': 'allow',
    'docs/cookbook/README.md': 'allow',
    'xtask/src/main.rs': 'scope',
    'xtask/src/policy/a/b.rs': 'scope',
    'xtask/src/secrets.rs': 'scope',
    'crates/a.rs': 'scope',
  });
  assert.deepEqual(verdicts(checkout('M1-F6c'), paths), {
    '.github/CODEOWNERS': 'scope',
    '.github/workflows/ci.yml': 'allow',
    'CLAUDE.md': 'scope',
    'docs/cookbook/README.md': 'scope',
    'xtask/src/main.rs': 'allow',
    'xtask/src/policy/a/b.rs': 'allow',
    'xtask/src/secrets.rs': 'scope',
    'crates/a.rs': 'scope',
  });
  assert.deepEqual(verdicts(checkout('M1-F6a'), ['.claude/settings.json', '.claude/hooks/lib/x.cjs', '.claude/settings.local.json', '.claude/agents/x.md', 'deny.toml']), {
    '.claude/settings.json': 'allow',
    '.claude/hooks/lib/x.cjs': 'allow',
    '.claude/settings.local.json': 'scope',
    '.claude/agents/x.md': 'scope',
    'deny.toml': 'scope',
  });
});

test('no ticket lets an agent write the file that names the ticket, the ticket itself, or git\'s own files', () => {
  for (const ticket of [null, 'M1-F6a', 'M1-F6b']) {
    const root = checkout(ticket);
    assert.equal(verdict(path.join(root, '.claude', 'ticket.local.json')), 'scope', String(ticket));
    assert.equal(verdict(path.join(root, '.CLAUDE', 'Ticket.Local.JSON')), 'scope', String(ticket));
    assert.equal(verdict(path.join(root, '.git', 'config')), 'scope', String(ticket));
    assert.equal(verdict(path.join(root, '.git')), 'scope', String(ticket));
  }
  assert.equal(verdict(path.join(checkout('M1-F6b'), 'plan', 'tickets', 'M1-F6b.md')), 'scope');
  assert.equal(verdict(path.join(checkout(null), 'plan', 'tickets', 'M1-F6b.md')), 'allow');
});

test('a ticket file that cannot be read refuses every write in its checkout', () => {
  const cases = {
    'not JSON': '{ticket: M1-F6b}',
    'no ticket field': '{"id": "M1-F6b"}',
    'a ticket that is a path': '{"ticket": "../../x"}',
    'a ticket with no file': '{"ticket": "M9-NONE"}',
    'UTF-16': Buffer.from('\uFEFF{"ticket": "M1-F6b"}', 'utf16le'),
    empty: '',
  };
  for (const [name, content] of Object.entries(cases)) {
    const root = checkout(null);
    put(root, '.claude/ticket.local.json', content);
    assert.equal(verdict(path.join(root, 'CLAUDE.md')), 'scope', name);
    assert.equal(verdict(path.join(root, 'crates', 'a.rs')), 'scope', name);
  }
  const marked = checkout(null);
  put(marked, '.claude/ticket.local.json', '\uFEFF{"ticket": "M1-F6b", "round": 2}\r\n');
  assert.equal(verdict(path.join(marked, 'CLAUDE.md')), 'allow');
  const scopeless = checkout(null);
  put(scopeless, 'plan/tickets/M9-BARE.md', '---\nid: M9-BARE\n---\n');
  put(scopeless, '.claude/ticket.local.json', '{"ticket": "M9-BARE"}');
  assert.equal(verdict(path.join(scopeless, 'CLAUDE.md')), 'scope');
});

test('the checkout is found from the path that is written, not from where the session runs', () => {
  const first = checkout('M1-F6b');
  const second = checkout('M1-F6c');
  const none = checkout(null);
  assert.equal(verdict(path.join(first, '.github', 'CODEOWNERS')), 'allow');
  assert.equal(verdict(path.join(second, '.github', 'CODEOWNERS')), 'scope');
  assert.equal(verdict(path.join(none, '.github', 'CODEOWNERS')), 'owner');
  assert.equal(verdict(path.join(none, 'crates', 'a.rs')), 'allow');
});

test('a path outside every checkout is not judged: the user\'s own .claude folder is not the repo\'s', () => {
  const home = folder();
  assert.equal(verdict(path.join(home, '.claude', 'projects', 'x', 'memory', 'MEMORY.md')), 'allow');
  assert.equal(verdict(path.join(home, '.claude', 'settings.json')), 'allow');
  assert.equal(verdict(path.join(home, 'spec', 'x.md')), 'allow');
});

test('a repository made inside a protected folder does not take the folder out of its checkout', () => {
  const root = checkout(null);
  fs.mkdirSync(path.join(root, '.claude', '.git'));
  assert.equal(verdict(path.join(root, '.claude', 'settings.json')), 'owner');
  const real = checkout(null);
  git(real, 'init', '-q');
  git(path.join(real, '.claude'), 'init', '-q');
  assert.equal(verdict(path.join(real, '.claude', 'settings.json')), 'owner');
  const ticketed = checkout('M1-F6b');
  fs.mkdirSync(path.join(ticketed, 'crates', '.git'), { recursive: true });
  assert.equal(verdict(path.join(ticketed, 'crates', 'a.rs')), 'scope');
});

test('a worktree is its own checkout, nested inside the main one or beside it', () => {
  const main = folder();
  git(main, 'init', '-q');
  for (const id of ['M1-F6b', 'M1-F6c']) {
    put(main, `plan/tickets/${id}.md`, fs.readFileSync(path.join(ROOT, 'plan', 'tickets', `${id}.md`), 'utf8'));
  }
  put(main, '.gitignore', '*.local.*\n.claude/worktrees/\n');
  git(main, 'add', '-A');
  git(main, 'commit', '-q', '-m', 'fixture');
  const inside = path.join(main, '.claude', 'worktrees', 'w');
  const beside = `${main}-beside`;
  made.push(beside);
  git(main, 'worktree', 'add', '-q', '-b', 'inside', inside);
  git(main, 'worktree', 'add', '-q', '-b', 'beside', beside);
  put(main, '.claude/ticket.local.json', '{"ticket": "M1-F6b"}');
  put(inside, '.claude/ticket.local.json', '{"ticket": "M1-F6c"}');

  assert.equal(verdict(path.join(main, '.github', 'CODEOWNERS')), 'allow');
  assert.equal(verdict(path.join(main, '.github', 'workflows', 'ci.yml')), 'scope');
  assert.equal(verdict(path.join(inside, '.github', 'workflows', 'ci.yml')), 'allow');
  assert.equal(verdict(path.join(inside, '.github', 'CODEOWNERS')), 'scope');
  assert.equal(verdict(path.join(inside, '.git')), 'scope');
  assert.equal(verdict(path.join(beside, '.github', 'workflows', 'ci.yml')), 'owner');
  assert.equal(verdict(path.join(beside, 'crates', 'a.rs')), 'allow');
  assert.equal(verdict(path.join(main, '.git', 'worktrees', 'inside', 'HEAD')), 'scope');
});

test('a call with no usable path is refused', () => {
  for (const file of [undefined, null, '', 42, 'relative/path.rs']) assert.equal(verdict(file), 'scope', String(file));
});

test('write_scope globs match whole paths with forward slashes', () => {
  const yes = [
    ['.claude/hooks/**', '.claude/hooks/a.cjs'],
    ['.claude/hooks/**', '.claude/hooks/lib/deep/a.cjs'],
    ['.claude/hooks/**', '.CLAUDE/Hooks/a.cjs'],
    ['a/**/b.rs', 'a/b.rs'],
    ['a/**/b.rs', 'a/x/y/b.rs'],
    ['crates/*/src/lib.rs', 'crates/ol-model/src/lib.rs'],
    ['*.md', 'README.md'],
    ['docs/', 'docs/a/b.md'],
    ['a?c.rs', 'abc.rs'],
    ['./deny.toml', 'deny.toml'],
    ['a.b+c(d)[e]{f}^$|.rs', 'a.b+c(d)[e]{f}^$|.rs'],
  ];
  const no = [
    ['.claude/hooks/**', '.claude/hooks'],
    ['.claude/hooks/**', '.claude/hooksx/a.cjs'],
    ['.claude/hooks/**', '.claude/settings.json'],
    ['.claude/settings.json', '.claude/settings.json.bak'],
    ['.claude/settings.json', 'x/.claude/settings.json'],
    ['a/**/b.rs', 'a/xb.rs'],
    ['crates/*/src/lib.rs', 'crates/a/b/src/lib.rs'],
    ['*.md', 'docs/README.md'],
    ['a?c.rs', 'a/c.rs'],
    ['deny.toml', 'denyxtoml'],
  ];
  for (const [pattern, file] of yes) assert.ok(glob(pattern).test(file), `${pattern} should match ${file}`);
  for (const [pattern, file] of no) assert.ok(!glob(pattern).test(file), `${pattern} should not match ${file}`);
});

test('write_scope is read from the ticket in each way a ticket may write it', () => {
  const root = folder();
  const ticket = (id, body) => put(root, `plan/tickets/${id}.md`, body);
  ticket('A', '---\nid: A\nwrite_scope:\n  - a/**\n  - "b c.rs"   # why\n  - d.rs # pushed by the owner\nread:\n  - not/this\n---\n- nor/this\n');
  ticket('B', '---\r\nwrite_scope: [a.rs, \'b.rs\', "c/**"]\r\nread: []\r\n---\r\n');
  ticket('C', '---\nwrite_scope:\n- a.rs\n\n- b.rs\nkind: fix\n---\n');
  ticket('D', '---\nwrite_scope: []\n---\n');
  assert.deepEqual(writeScope(root, 'A'), ['a/**', 'b c.rs', 'd.rs']);
  assert.deepEqual(writeScope(root, 'B'), ['a.rs', 'b.rs', 'c/**']);
  assert.deepEqual(writeScope(root, 'C'), ['a.rs', 'b.rs']);
  assert.deepEqual(writeScope(root, 'D'), []);
  assert.deepEqual(writeScope(ROOT, 'M1-F6a'), ['.claude/settings.json', '.claude/hooks/**']);
  assert.deepEqual(writeScope(ROOT, 'M1-F6c'), ['xtask/src/policy/**', 'xtask/src/main.rs', 'xtask/tests/policy/**', '.github/workflows/ci.yml']);
});
