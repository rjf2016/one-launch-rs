'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { Refusal } = require('./rules.cjs');

const OWNER_APPROVED = ['spec/**', 'oracle/**', '.github/**', '.claude/**', 'deny.toml', 'scripts/agent/**', 'xtask/src/policy/**'];
const NEVER = ['.claude/ticket.local.json', '.git', '.git/**'];
const TICKET_FILE = path.join('.claude', 'ticket.local.json');

function canonical(file) {
  const full = path.resolve(file);
  if (full.slice(path.parse(full).root.length).includes(':')) {
    throw new Refusal('scope', 'a path with a colon after the drive names a file stream, which the hook does not follow');
  }
  const missing = [];
  let trimmed = false;
  for (let known = full; ; ) {
    let real = null;
    try {
      real = fs.realpathSync.native(known);
    } catch (error) {
      if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
    }
    if (real !== null) {
      const joined = path.join(real, ...missing.reverse());
      return trimmed ? canonical(joined) : joined;
    }
    const parent = path.dirname(known);
    if (parent === known) return full;
    const name = path.basename(known);
    const kept = name.replace(/[. ]+$/, '');
    trimmed ||= kept !== name;
    missing.push(kept);
    known = parent;
  }
}

function commonDir(root) {
  try {
    const dot = path.join(root, '.git');
    if (fs.statSync(dot).isDirectory()) return fs.realpathSync.native(dot).toLowerCase();
    const pointer = /^gitdir:\s*(.+?)\s*$/m.exec(fs.readFileSync(dot, 'utf8'));
    if (!pointer) return null;
    const gitDir = path.resolve(root, pointer[1]);
    let common = gitDir;
    try {
      common = path.resolve(gitDir, fs.readFileSync(path.join(gitDir, 'commondir'), 'utf8').trim());
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    return fs.realpathSync.native(common).toLowerCase();
  } catch {
    return null;
  }
}

function isWrapper(file, name, home) {
  let real;
  try {
    real = canonical(file);
  } catch {
    return false;
  }
  const root = path.resolve(real, '..', '..', '..');
  const common = commonDir(root);
  const here = real.toLowerCase() === path.join(root, 'scripts', 'agent', name).toLowerCase();
  return here && fs.existsSync(real) && common !== null && common === commonDir(home);
}

function nearest(folder) {
  for (let dir = path.resolve(folder); ; ) {
    if (fs.existsSync(path.join(dir, '.git'))) return dir;
    const parent = path.dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

function enclosing(file) {
  const roots = [];
  let inner = null;
  for (let dir = nearest(path.dirname(file)); dir; dir = path.dirname(dir) === dir ? null : nearest(path.dirname(dir))) {
    const common = commonDir(dir);
    if (!roots.length || common === null || common !== inner) roots.push(dir);
    inner = common;
  }
  return roots;
}

function ticket(root) {
  let raw;
  try {
    raw = fs.readFileSync(path.join(root, TICKET_FILE), 'utf8');
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
  let id;
  try {
    id = JSON.parse(raw.replace(/^\uFEFF/, '')).ticket;
  } catch {
    id = undefined;
  }
  if (typeof id !== 'string' || !/^[A-Za-z0-9]+(-[A-Za-z0-9]+)*$/.test(id)) {
    throw new Refusal('scope', '.claude/ticket.local.json must be UTF-8 JSON of the form {"ticket": "M1-F6a"}; as it stands no path can be judged');
  }
  return id;
}

function writeScope(root, id) {
  let text;
  try {
    text = fs.readFileSync(path.join(root, 'plan', 'tickets', `${id}.md`), 'utf8');
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    throw new Refusal('scope', `plan/tickets/${id}.md does not exist in this checkout, so the ticket names no path`);
  }
  const front = /^\uFEFF?---\r?\n([\s\S]*?)\r?\n---/.exec(text);
  const lines = front ? front[1].split(/\r?\n/) : [];
  const at = lines.findIndex((line) => /^write_scope:/.test(line));
  if (at < 0) throw new Refusal('scope', `plan/tickets/${id}.md has no write_scope`);
  const inline = lines[at].slice('write_scope:'.length).trim();
  const entries = [];
  if (inline.startsWith('[')) {
    entries.push(...inline.replace(/^\[/, '').replace(/\].*$/, '').split(','));
  } else {
    for (let k = at + 1; k < lines.length && !/^[A-Za-z_]/.test(lines[k]); k++) {
      const item = /^\s*-\s+(.*)$/.exec(lines[k]);
      if (item) entries.push(item[1]);
    }
  }
  return entries
    .map((entry) =>
      entry
        .replace(/\s+#.*$/, '')
        .trim()
        .replace(/^(['"])(.*)\1$/, '$2'),
    )
    .filter(Boolean);
}

function glob(pattern) {
  let source = pattern.replace(/\\/g, '/').replace(/^\.\//, '');
  if (source.endsWith('/')) source += '**';
  let out = '';
  for (let k = 0; k < source.length; k++) {
    const c = source[k];
    if (c === '*' && source[k + 1] === '*') {
      k++;
      if (source[k + 1] === '/') {
        k++;
        out += '(?:.*/)?';
      } else {
        out += '.*';
      }
    } else if (c === '*') out += '[^/]*';
    else if (c === '?') out += '[^/]';
    else out += c.replace(/[.+^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${out}$`, 'i');
}

const matches = (patterns, relative) => patterns.some((pattern) => glob(pattern).test(relative));

function judgeWrite(file) {
  if (typeof file !== 'string' || !path.isAbsolute(file)) throw new Refusal('scope', 'the call names no absolute file path');
  const full = canonical(file);
  for (const root of enclosing(full)) {
    const relative = path.relative(root, full).split(path.sep).join('/');
    if (matches(NEVER, relative)) {
      throw new Refusal('scope', `${relative} is never written through Edit or Write: it names the ticket or belongs to git`);
    }
    const id = ticket(root);
    if (id === null) {
      if (matches(OWNER_APPROVED, relative)) {
        throw new Refusal('owner', `${relative} is an owner-approved path and this checkout has no ticket set that names it`);
      }
    } else if (!matches(writeScope(root, id), relative)) {
      throw new Refusal('scope', `${relative} is outside the write_scope of ${id}`);
    }
  }
}

module.exports = { canonical, commonDir, isWrapper, nearest, ticket, writeScope, glob, judgeWrite, OWNER_APPROVED };
