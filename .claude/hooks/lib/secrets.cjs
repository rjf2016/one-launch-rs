'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ALLOW = 'xtask/secrets.allow';
const SAFE_USER_FOLDERS = ['someone', 'placeholder', 'public', 'default'];
const LOCK = 'Cargo.lock';
const SEALED = 'oracle/';
const NEVER_MADE_UP = ['private-key', 'github-token', 'authorization'];
const RESERVED = ['example', 'invalid', 'test', 'localhost'];

const whole = (found) => found[0];
const unlessNoGeneratorMadeIt = (group) => (found) => (noGeneratorMade(found[group]) ? null : found[group]);
const unlessOneCharacter = (group) => (found) => {
  const [first, ...others] = [...found[group]];
  return others.every((c) => c === first) ? null : found[group];
};

const RULES = [
  { id: 'private-key', wholeRun: false, pattern: '-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----', value: whole },
  {
    id: 'github-token',
    wholeRun: true,
    pattern: '(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})',
    value: unlessNoGeneratorMadeIt(0),
  },
  {
    id: 'jwt',
    wholeRun: true,
    pattern: '(?:eyJ|eyAi)[A-Za-z0-9_-]{10,}\\.[A-Za-z0-9_-]{10,}(?:\\.[A-Za-z0-9_-]*)?',
    value: unlessNoGeneratorMadeIt(0),
  },
  {
    id: 'authorization',
    wholeRun: false,
    pattern: '(?i)\\bauthorization["\']?\\s*[:=]\\s*["\']?(?:bearer|basic)\\s+([A-Za-z0-9._~+/=-]{20,})',
    value: unlessOneCharacter(1),
  },
  {
    id: 'credential',
    wholeRun: false,
    pattern:
      '(?i)\\b(?:refresh_token|access_token|id_token|ssid|clid|web_token|client_secret|password)["\']?\\s*[:=]\\s*["\'`]([^"\'`\\s]{20,})["\'`]',
    value: unlessOneCharacter(1),
  },
  {
    id: 'email',
    wholeRun: false,
    pattern: '[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+(?:\\.[A-Za-z0-9-]+)*\\.[A-Za-z]{2,})',
    value: (found, line, end) => {
      const after = line.codePointAt(end);
      const cutShort = after !== undefined && /[\p{Alphabetic}\p{N}]/u.test(String.fromCodePoint(after));
      return !cutShort && found[0] !== 'git@github.com' && !safeDomain(found[1]) ? found[0] : null;
    },
  },
  { id: 'steamid64', wholeRun: true, pattern: '76561(?:19|20)[0-9]{10}', value: whole },
  {
    id: 'uuid',
    wholeRun: true,
    pattern: '[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}',
    value: unlessNoGeneratorMadeIt(0),
  },
  { id: 'hex64', wholeRun: true, pattern: '[0-9A-Fa-f]{64}', value: unlessNoGeneratorMadeIt(0) },
  { id: 'hex32', wholeRun: true, pattern: '[0-9A-Fa-f]{32}', value: unlessNoGeneratorMadeIt(0) },
  {
    id: 'user-folder',
    wholeRun: false,
    pattern: '(?i)(?:\\b[a-z]:|(?:^|[^\\w.])(?:/mnt)?/[a-z])[\\\\/]{1,4}users[\\\\/]{1,4}([^\\\\/\\s"\'`*?|:;,)\\]]+)',
    value: (found) => {
      const name = found[1];
      const named = /^[<%${[]/.test(name) || SAFE_USER_FOLDERS.includes(name.toLowerCase());
      return named ? null : name;
    },
  },
];

for (const rule of RULES) {
  const insensitive = rule.pattern.startsWith('(?i)');
  const source = insensitive ? rule.pattern.slice(4) : rule.pattern;
  rule.regex = new RegExp(source, insensitive ? 'giu' : 'gu');
  const run = /^\[([^\]]+)\]\+([^\\.^$*+?()[\]{}|])/.exec(source);
  if (run) Object.assign(rule, { before: new RegExp(`[${run[1]}]`), anchor: run[2], sticky: new RegExp(source, 'yu') });
}

// A pattern that opens with a run of one class and then one fixed character is tried once per run, from the run's start: the plain search retries inside every long run and takes quadratic time.
function* matches(rule, line) {
  if (!rule.anchor) {
    yield* line.matchAll(rule.regex);
    return;
  }
  let from = 0;
  for (let at = line.indexOf(rule.anchor); at >= 0; at = line.indexOf(rule.anchor, Math.max(at + 1, from))) {
    let start = at;
    while (start > from && rule.before.test(line[start - 1])) start--;
    rule.sticky.lastIndex = start;
    const found = start < at ? rule.sticky.exec(line) : null;
    if (found) {
      yield found;
      from = start + found[0].length;
    }
  }
}

function noGeneratorMade(value) {
  const chars = [...value.toLowerCase()].filter((c) => /[a-z0-9]/.test(c));
  const counts = new Map();
  for (const c of chars) counts.set(c, (counts.get(c) ?? 0) + 1);
  const most = Math.max(0, ...counts.values());
  if (chars.length - most <= 4) return true;
  for (let period = 1; period <= Math.min(16, Math.floor(chars.length / 2)); period++) {
    if (chars.slice(period).join('') === chars.slice(0, chars.length - period).join('')) return true;
  }
  return false;
}

function safeDomain(domain) {
  const lower = domain.toLowerCase();
  const labels = lower.split('.');
  const reserved = labels.length >= 2 && RESERVED.includes(labels[labels.length - 1]);
  const at = labels.indexOf('example');
  const example = at >= 0 && at + 1 < labels.length;
  return reserved || example || lower.endsWith('users.noreply.github.com');
}

function mask(value) {
  const chars = [...value];
  return chars.length >= 12 ? `${chars.slice(0, 4).join('')}…${chars.slice(-2).join('')}` : `${chars[0]}…(${chars.length} chars)`;
}

class Scanner {
  constructor(allow) {
    this.allow = allow;
  }

  scan(line, sealed) {
    const findings = [];
    const taken = [];
    for (const rule of RULES) {
      if (sealed && !NEVER_MADE_UP.includes(rule.id)) continue;
      for (const found of matches(rule, line)) {
        const start = found.index;
        const end = start + found[0].length;
        if (taken.some(([from, to]) => start < to && end > from)) continue;
        if (rule.wholeRun && [line[start - 1], line[end]].some((c) => c !== undefined && /[A-Za-z0-9]/.test(c))) continue;
        const value = rule.value(found, line, end);
        if (value === null) continue;
        taken.push([start, end]);
        if (!this.allow.has(value)) findings.push({ rule: rule.id, masked: mask(value), span: [start, end] });
      }
    }
    return findings;
  }
}

function redacted(text, findings) {
  let shown = '';
  let from = 0;
  for (const finding of [...findings].sort((a, b) => a.span[0] - b.span[0])) {
    shown += text.slice(from, finding.span[0]) + finding.masked;
    from = finding.span[1];
  }
  return shown + text.slice(from);
}

function readable(bytes) {
  return (bytes.includes(0) ? Buffer.from(bytes.filter((byte) => byte !== 0)) : bytes).toString('utf8');
}

function fileFindings(scanner, file, bytes) {
  if (file === LOCK) return [];
  const sealed = file.startsWith(SEALED);
  const inName = scanner.scan(file, sealed);
  const shown = redacted(file, inName);
  const problems = inName.map((finding) => `${shown}: in the file name: ${finding.rule} ${finding.masked}`);
  for (const [index, line] of readable(bytes).split('\n').entries()) {
    for (const finding of scanner.scan(line.replace(/\r$/, ''), sealed)) {
      problems.push(`${shown}:${index + 1}: ${finding.rule} ${finding.masked}`);
    }
  }
  return problems;
}

function allowed(root) {
  let list = '';
  try {
    list = fs.readFileSync(path.join(root, ALLOW), 'utf8');
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  return new Set(
    list
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line && !line.startsWith('#')),
  );
}

function git(root, args, input) {
  const out = spawnSync('git', ['-C', root, ...args], { input, maxBuffer: 1 << 30, windowsHide: true });
  if (out.error || out.status !== 0) throw new Error(`git ${args[0]} did not run in the checkout the hook found`);
  return out.stdout;
}

function isTag(folder, name) {
  return git(folder, ['for-each-ref', '--count=1', '--format=%(refname)', `refs/tags/${name}`]).toString('utf8').trim() !== '';
}

function changes(root) {
  const records = git(root, ['--no-optional-locks', 'status', '--porcelain=v2', '-z', '--untracked-files=no', '--no-renames']).toString('utf8').split('\0');
  const list = [];
  for (let k = 0; k < records.length; k++) {
    const fields = records[k].split(' ');
    const kind = fields[0];
    if (kind !== '1' && kind !== '2' && kind !== 'u') continue;
    const path0 = kind === '1' ? 8 : kind === '2' ? 9 : 10;
    const link = fields[kind === 'u' ? 6 : 4] === '160000';
    const [staged, unstaged] = kind === 'u' ? ['.', 'M'] : fields[1];
    if (!link) list.push({ staged, unstaged, oid: fields[7], name: fields.slice(path0).join(' ') });
    if (kind === '2') k++;
  }
  return list;
}

function blobs(root, oids) {
  const found = new Map();
  if (!oids.length) return found;
  const out = git(root, ['cat-file', '--batch'], oids.map((oid) => `${oid}\n`).join(''));
  let at = 0;
  for (const oid of oids) {
    const end = out.indexOf(10, at);
    const [, type, size] = out.subarray(at, end).toString('utf8').split(' ');
    if (type !== 'blob' || !/^\d+$/.test(size)) throw new Error('git cat-file did not hand back a staged file');
    found.set(oid, out.subarray(end + 1, end + 1 + Number(size)));
    at = end + 2 + Number(size);
  }
  return found;
}

function blobName(bytes, like) {
  return crypto
    .createHash(like.length === 64 ? 'sha256' : 'sha1')
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest('hex');
}

function commitFindings(root, broad) {
  const scanner = new Scanner(allowed(root));
  const list = changes(root);
  const onDisk = new Map();
  const disk = (name) => {
    if (!onDisk.has(name)) onDisk.set(name, fs.readFileSync(path.join(root, name)));
    return onDisk.get(name);
  };
  const staged = list.filter((entry) => entry.staged !== '.' && entry.staged !== 'D');
  const same = (entry) => {
    try {
      return entry.unstaged === '.' && blobName(disk(entry.name), entry.oid) === entry.oid;
    } catch {
      return false;
    }
  };
  const fetched = blobs(root, staged.filter((entry) => !same(entry)).map((entry) => entry.oid));
  const problems = staged.flatMap((entry) => fileFindings(scanner, entry.name, fetched.get(entry.oid) ?? disk(entry.name)));
  if (!broad) return problems;
  for (const entry of list.filter((each) => each.unstaged !== '.' && each.unstaged !== 'D')) {
    for (const problem of fileFindings(scanner, entry.name, disk(entry.name))) {
      if (!problems.includes(problem)) problems.push(problem);
    }
  }
  return problems;
}

module.exports = { RULES, SAFE_USER_FOLDERS, NEVER_MADE_UP, RESERVED, LOCK, SEALED, ALLOW, Scanner, matches, fileFindings, commitFindings, isTag };
