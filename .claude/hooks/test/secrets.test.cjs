'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { after, test } = require('node:test');
const secrets = require('../lib/secrets.cjs');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const RUST = fs.readFileSync(path.join(ROOT, 'xtask', 'src', 'secrets.rs'), 'utf8');
const made = [];

after(() => {
  for (const folder of made) fs.rmSync(folder, { recursive: true, force: true });
});

const scanner = new secrets.Scanner(new Set());
const rules = (line, sealed = false) => scanner.scan(line, sealed).map((finding) => finding.rule);

function random(alphabet, length) {
  let state = 0x2545f491n;
  let out = '';
  for (let k = 0; k < length; k++) {
    state = (state * 6364136223846793005n + 1442695040888963407n) & 0xffffffffffffffffn;
    out += alphabet[Number((state >> 33n) % BigInt(alphabet.length))];
  }
  return out;
}

const BASE62 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
const HEX = '0123456789abcdef';
const token = () => `gh${'s'}_${random(BASE62, 36)}`;
const address = () => `${'someone'}@${'corp-mail'}.io`;
const profile = () => `C:\\${'Users'}\\${'jdoe'}\\AppData\\Roaming`;

function strings(name) {
  const list = new RegExp(`const ${name}: \\[&str; \\d+\\] = \\[([^\\]]*)\\]`).exec(RUST);
  assert.ok(list, `${name} not found in xtask/src/secrets.rs`);
  return [...list[1].matchAll(/"([^"]*)"/g)].map((found) => found[1]);
}

test('the patterns are the ones of xtask/src/secrets.rs, in the same order', () => {
  const rust = [...RUST.matchAll(/\b(rule|whole_run)\(\s*"([a-z0-9-]+)",\s*r(#?)"([\s\S]*?)"\3,/g)].map((found) => ({
    id: found[2],
    wholeRun: found[1] === 'whole_run',
    pattern: found[4],
  }));
  assert.equal(rust.length, 11);
  assert.deepEqual(
    secrets.RULES.map(({ id, wholeRun, pattern }) => ({ id, wholeRun, pattern })),
    rust,
  );
});

test('the fixed lists are the ones of xtask/src/secrets.rs', () => {
  assert.deepEqual(secrets.SAFE_USER_FOLDERS, strings('SAFE_USER_FOLDERS'));
  assert.deepEqual(secrets.NEVER_MADE_UP, strings('NEVER_MADE_UP'));
  const reserved = /"example" \| "invalid" \| "test" \| "localhost"/.test(RUST);
  assert.ok(reserved, 'the reserved top-level names changed in xtask/src/secrets.rs');
  assert.deepEqual(secrets.RESERVED, ['example', 'invalid', 'test', 'localhost']);
  assert.match(RUST, new RegExp(`const ALLOW: &str = "${secrets.ALLOW}";`));
  assert.match(RUST, new RegExp(`const LOCK: &str = "${secrets.LOCK.replace('.', '\\.')}";`));
  assert.match(RUST, new RegExp(`const SEALED: &str = "${secrets.SEALED}";`));
  assert.match(RUST, /name\.starts_with\(\['<', '%', '\$', '\{', '\['\]\)/);
  assert.match(RUST, /&found\[0\] != "git@github\.com"/);
  assert.match(RUST, /chars\.len\(\) - most <= 4/);
  assert.match(RUST, /\(1\.\.=16\.min\(chars\.len\(\) \/ 2\)\)/);
});

test('each kind of secret is found', () => {
  assert.deepEqual(rules(`-----BEGIN ${'RSA PRIVATE'} KEY-----`), ['private-key']);
  assert.deepEqual(rules(token()), ['github-token']);
  assert.deepEqual(rules(`token: github_${'pat'}_${random(BASE62, 40)}`), ['github-token']);
  assert.deepEqual(rules(`ey${'J'}${random(BASE62, 20)}.${random(BASE62, 30)}.${random(BASE62, 20)}`), ['jwt']);
  assert.deepEqual(rules(`Authorization: Bearer ${random(BASE62, 32)}`), ['authorization']);
  assert.deepEqual(rules(`let password = "${random(BASE62, 24)}";`), ['credential']);
  assert.deepEqual(rules(`contact ${address()} today`), ['email']);
  assert.deepEqual(rules(`id ${'7656119'}${'8134509276'}`), ['steamid64']);
  const hex = random(HEX, 32);
  assert.deepEqual(rules(`account ${hex}`), ['hex32']);
  assert.deepEqual(rules(`puuid ${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`), ['uuid']);
  assert.deepEqual(rules(`key ${random(HEX, 64)}`), ['hex64']);
  assert.deepEqual(rules(profile()), ['user-folder']);
  assert.deepEqual(rules(`C:\\\\${'Users'}\\\\${'jdoe'}\\\\file.txt`), ['user-folder']);
  assert.deepEqual(rules(`cd /c/${'Users'}/${'jdoe'}/project`), ['user-folder']);
  assert.deepEqual(rules(`ls /mnt/c/${'Users'}/${'jdoe'}/project`), ['user-folder']);
});

test('what the Rust scan lets through passes here too', () => {
  for (const line of [
    'user@example.com',
    'user@mail.example.co.uk',
    'user@host.test',
    'user@box.invalid',
    '1234+name@users.noreply.github.com',
    'git@github.com:owner/repo.git',
    'cargo-deny@0.20.2',
    'actions/checkout@v5',
    random(HEX, 40),
    `0${'7656119'}${'8134509276'}`,
    '11111111-1111-4111-8111-111111111111',
    '0123456789abcdef'.repeat(2),
    `gh${'p'}_${'x'.repeat(36)}`,
    `ey${'J'}${'x'.repeat(20)}.${'x'.repeat(30)}`,
    `let password = "${'a'.repeat(24)}";`,
    `Authorization: Bearer ${'x'.repeat(24)}`,
    'C:\\Users\\<name>\\AppData',
    'C:\\Users\\%USERNAME%\\AppData',
    'C:\\Users\\someone\\AppData',
    'C:\\Users\\Public\\Desktop',
    'C:\\Users\\$env:USERNAME\\x',
  ]) {
    assert.deepEqual(rules(line), [], line);
  }
});

test('what the Rust scan still finds is found here too', () => {
  const hex = random(HEX, 32);
  assert.deepEqual(rules(`acct_${hex}_grid.png`), ['hex32']);
  assert.deepEqual(rules(`steam_${'7656120'}${'1134509276'}`), ['steamid64']);
  assert.deepEqual(rules(`token_ey${'Ai'}${random(BASE62, 20)}.${random(BASE62, 30)}`), ['jwt']);
  assert.deepEqual(rules(['1', '2', '3', '4', '5'].map((digit, k) => digit.repeat([8, 4, 4, 4, 12][k])).join('-')), ['uuid']);
  assert.deepEqual(rules(`ey${'J'}PLACEHOLDER.ey${'J'}PLACEHOLDER.PLACEHOLDER`), ['jwt']);
  for (const digits of ['0000000000', '0000000001', '8000000000', '1212121212']) {
    assert.deepEqual(rules(`id ${'7656119'}${digits}`), ['steamid64'], digits);
  }
  for (const value of [
    ['fake', 'password', 'for', 'the', 'vault'].join('-'),
    `example-${random(BASE62, 16)}`,
    random(BASE62, 11) + 'a'.repeat(11),
    random(BASE62, 16).repeat(2),
    'ab'.repeat(12),
    `${'a'.repeat(23)}b`,
    `${random(BASE62, 16)}${'a'.repeat(6)}`,
    `${random(BASE62, 20)}-test`,
  ]) {
    assert.deepEqual(rules(`let password = "${value}";`), ['credential'], value);
  }
  for (const value of ['abcd'.repeat(6), 'example-token-'.repeat(2)]) {
    assert.deepEqual(rules(`Authorization: Bearer ${value}`), ['authorization'], value);
  }
  assert.deepEqual(rules(`placeholder account ${hex}`), ['hex32']);
  assert.deepEqual(rules(`account ${random(HEX, 26)}000000`), ['hex32']);
  assert.deepEqual(rules(`ey${'J'}${random(HEX, 32)}.${random(HEX, 32)}.${random(HEX, 32)}`), ['jwt']);
});

test('the quick way to find addresses finds what the pattern itself finds, and a long line is read in time', () => {
  const email = secrets.RULES.find((rule) => rule.id === 'email');
  assert.equal(email.anchor, '@');
  assert.deepEqual(secrets.RULES.filter((rule) => rule.anchor).map((rule) => rule.id), ['email']);
  const at = (...sides) => sides.join('@');
  const pieces = ['a', 'b.c', 'x_y', '@', '@@', '.', '.io', 'corp.io', ' ', ',', '<', '>', 'é', '1', '-', '+', '%', at('a', 'b.io'), at('', 'b.io'), at('a', 'b'), at('ab', 'c-d.e.fg'), '..', at('a.', 'b.cd'), '"'];
  let state = 7n;
  const pick = () => {
    state = (state * 6364136223846793005n + 1442695040888963407n) & 0xffffffffffffffffn;
    return pieces[Number((state >> 33n) % BigInt(pieces.length))];
  };
  for (let k = 0; k < 3000; k++) {
    const line = Array.from({ length: 1 + (k % 14) }, pick).join('');
    const quick = [...secrets.matches(email, line)].map((found) => [found.index, found[0], found[1]]);
    const plain = [...line.matchAll(email.regex)].map((found) => [found.index, found[0], found[1]]);
    assert.deepEqual(quick, plain, JSON.stringify(line));
  }
  for (const line of ['0123456789abcdef'.repeat(20000), `${'a.b-c_d+e%'.repeat(30000)}@`, `${'x@'.repeat(100000)}y.io`, ' '.repeat(300000)]) {
    const from = process.hrtime.bigint();
    scanner.scan(line, false);
    const ms = Number(process.hrtime.bigint() - from) / 1e6;
    assert.ok(ms < 3000, `${line.slice(0, 24)}...: ${ms.toFixed(0)} ms`);
  }
});

test('an exact value on the allow list passes and its neighbour does not', () => {
  const base = `${'7656119'}7960265728`;
  const allowing = new secrets.Scanner(new Set([base]));
  assert.deepEqual(allowing.scan(`id ${base}`, false), []);
  assert.equal(allowing.scan(`id ${'7656119'}7960265729`, false).length, 1);
});

test('in the oracle only what is never made up is reported, and Cargo.lock is skipped', () => {
  const hex = random(HEX, 32);
  for (const line of [`account ${hex}`, `id ${'7656119'}${'8134509276'}`, profile(), `ssid: "ey${'J'}${random(BASE62, 20)}.${random(BASE62, 30)}"`]) {
    assert.deepEqual(rules(line, true), [], line);
  }
  assert.deepEqual(rules(token(), true), ['github-token']);
  assert.deepEqual(rules(`-----BEGIN ${'RSA PRIVATE'} KEY-----`, true), ['private-key']);
  assert.deepEqual(rules(`Authorization: Bearer ${random(BASE62, 32)}`, true), ['authorization']);
  assert.deepEqual(secrets.fileFindings(scanner, 'Cargo.lock', Buffer.from(`checksum = "${random(HEX, 64)}"`)), []);
  assert.equal(secrets.fileFindings(scanner, 'crates/Cargo.lock', Buffer.from(`checksum = "${random(HEX, 64)}"`)).length, 1);
  assert.deepEqual(secrets.fileFindings(scanner, 'oracle/l1/x.json', Buffer.from(`"id": "${hex}"`)), []);
  assert.equal(secrets.fileFindings(scanner, 'notes/oracle/x.json', Buffer.from(`"id": "${hex}"`)).length, 1);
});

test('a finding names the file and the line and never shows the whole value', () => {
  const secret = token();
  const hex = random(HEX, 32);
  const utf16 = Buffer.from(`first line\r\n${profile()}\r\n`, 'utf16le');
  assert.deepEqual(secrets.fileFindings(scanner, 'docs/a.md', Buffer.from(`ok\nkey ${secret}\n\n${address()}\n`)), [
    'docs/a.md:2: github-token ghs_…' + secret.slice(-2),
    'docs/a.md:4: email some…io',
  ]);
  assert.deepEqual(secrets.fileFindings(scanner, 'docs/b.txt', utf16), ['docs/b.txt:2: user-folder j…(4 chars)']);
  const named = secrets.fileFindings(scanner, `notes/${secret}/acct_${hex}_grid.png`, Buffer.alloc(0));
  assert.equal(named.length, 2);
  for (const problem of named) {
    assert.ok(problem.startsWith('notes/ghs_…'), problem);
    assert.ok(!problem.includes(secret) && !problem.includes(hex), problem);
  }
});

function git(cwd, ...args) {
  const env = { ...process.env, GIT_CONFIG_GLOBAL: path.join(cwd, '.no-git-config'), GIT_CONFIG_NOSYSTEM: '1' };
  const identity = ['-c', 'user.name=fixture', '-c', 'user.email=fixture@example.com', '-c', 'commit.gpgsign=false', '-c', 'init.defaultBranch=main', '-c', 'core.autocrlf=false'];
  const run = spawnSync('git', [...identity, ...args], { cwd, env, encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
  return run.stdout;
}

function repo(files = { 'README.md': 'fixture\n' }) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'ol-hook-')));
  made.push(root);
  git(root, 'init', '-q');
  for (const [name, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.writeFileSync(path.join(root, name), text);
  }
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'fixture');
  return root;
}

const write = (root, name, text) => {
  fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
  fs.writeFileSync(path.join(root, name), text);
};

test('a staged token, address or profile path is found, each on its line', () => {
  const root = repo();
  write(root, 'notes/a.txt', `clean\n${token()}\n`);
  write(root, 'notes/b.txt', `${address()}\n`);
  write(root, 'notes/c.txt', `one\ntwo\n${profile()}\n`);
  write(root, 'notes/clean.txt', 'nothing here\n');
  assert.deepEqual(secrets.commitFindings(root, false), []);
  git(root, 'add', '-A');
  const found = secrets.commitFindings(root, false);
  assert.equal(found.length, 3, found.join('\n'));
  assert.match(found[0], /^notes\/a\.txt:2: github-token ghs_…/);
  assert.equal(found[1], 'notes/b.txt:1: email some…io');
  assert.equal(found[2], 'notes/c.txt:3: user-folder j…(4 chars)');
});

test('the scan reads what is staged, not what the folder holds now', () => {
  const root = repo();
  write(root, 'a.txt', `${token()}\n`);
  git(root, 'add', 'a.txt');
  write(root, 'a.txt', 'cleaned up afterwards\n');
  assert.equal(secrets.commitFindings(root, false).length, 1);
  git(root, 'add', 'a.txt');
  assert.deepEqual(secrets.commitFindings(root, false), []);
  write(root, 'README.md', `${address()}\n`);
  assert.deepEqual(secrets.commitFindings(root, false), []);
  assert.deepEqual(secrets.commitFindings(root, true), ['README.md:1: email some…io']);
  write(root, 'untracked.txt', `${address()}\n`);
  assert.equal(secrets.commitFindings(root, true).length, 1);
});

test('a staged file that git was told to stop comparing is still read from the index', () => {
  const root = repo({ 'a.txt': 'plain\n' });
  write(root, 'a.txt', `${token()}\n`);
  git(root, 'add', 'a.txt');
  git(root, 'update-index', '--assume-unchanged', 'a.txt');
  write(root, 'a.txt', 'cleaned up afterwards\n');
  const found = secrets.commitFindings(root, false);
  assert.equal(found.length, 1, found.join('\n'));
  assert.match(found[0], /^a\.txt:1: github-token ghs_…/);
});

test('a staged change to a line beside an old finding, a rename, a removal and a binary file are handled', () => {
  const old = `${address()}\nsecond\n`;
  const root = repo({ 'old.txt': old, 'gone.txt': `${address()}\n`, 'moved.txt': 'plain\n' });
  write(root, 'old.txt', `${address()}\nsecond, edited\n`);
  fs.rmSync(path.join(root, 'gone.txt'));
  fs.renameSync(path.join(root, 'moved.txt'), path.join(root, `acct_${random(HEX, 32)}.txt`));
  fs.writeFileSync(path.join(root, 'blob.bin'), Buffer.concat([Buffer.from([0, 1, 2, 0xff]), Buffer.from(token()), Buffer.from([0, 0])]));
  git(root, 'add', '-A');
  const found = secrets.commitFindings(root, false);
  assert.equal(found.length, 3, found.join('\n'));
  assert.match(found[0], /^acct_[0-9a-f]{4}…[0-9a-f]{2}\.txt: in the file name: hex32 /);
  assert.match(found[1], /^blob\.bin:1: github-token ghs_…/);
  assert.equal(found[2], 'old.txt:1: email some…io');
});

test('the allow list of the checkout is honoured and a first commit can be scanned', () => {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'ol-hook-')));
  made.push(root);
  git(root, 'init', '-q');
  const allowed = `${'7656119'}7960265728`;
  write(root, 'xtask/secrets.allow', `# base\n${allowed}\n`);
  write(root, 'a.txt', `id ${allowed}\nid ${'7656119'}7960265729\n`);
  git(root, 'add', '-A');
  assert.deepEqual(secrets.commitFindings(root, false), ['a.txt:2: steamid64 7656…29']);
});

test('a folder that is no checkout is an error, not a clean scan', () => {
  const plain = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'ol-hook-')));
  made.push(plain);
  assert.throws(() => secrets.commitFindings(plain, false));
  assert.throws(() => secrets.isTag(plain, 'v1'));
});

test('a tag is told from a branch by asking git', () => {
  const root = repo();
  const head = git(root, 'rev-parse', 'HEAD').trim();
  fs.mkdirSync(path.join(root, '.git', 'refs', 'tags'), { recursive: true });
  fs.writeFileSync(path.join(root, '.git', 'refs', 'tags', 'v1'), `${head}\n`);
  assert.equal(secrets.isTag(root, 'v1'), true);
  assert.equal(secrets.isTag(root, 'main'), false);
  assert.equal(secrets.isTag(root, 'v2'), false);
  assert.equal(secrets.isTag(root, 'HEAD~1'), false);
});
