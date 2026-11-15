'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Refusal } = require('./rules.cjs');
const { lex, MARK, LEADING } = require('./shell.cjs');

const set = (names) => new Set(names.split(' '));

const PROCESS = set(
  'tasklist taskkill tskill wmic qprocess pslist pskill pssuspend get-process gps ps stop-process spps kill wait-process debug-process pkill pgrep killall pidof top htop gcim gwmi iwmi icim rwmi swmi rcim scim ncim taskmgr tlist procexp procexp64 procdump procdump64 handle handle64 listdlls ciminstance wmiobject cimclass cimassociatedinstance cimsession',
);
const CIM = /^(get|set|new|remove|invoke|register)-(cim|wmi)/;
const LAUNCHERS = new Set([
  'steam',
  'steamservice',
  'steamwebhelper',
  'epicgameslauncher',
  'battle.net',
  'battle.net launcher',
  'riotclientservices',
  'riot client',
  'riotclientux',
  'riotclientuxrender',
  'leagueclient',
  'leagueclientux',
  'leagueclientuxrender',
  'league of legends',
  'valorant',
  'valorant-win64-shipping',
  'lor',
  '2xko',
  '2xko-win64-shipping',
  'vgc',
  'vgk',
  'vgtray',
]);
const PROTOCOLS = /^(steam|com\.epicgames\.launcher|battlenet|riotclient):/i;
const TOKEN_MAKERS = set('new-agenttoken new-agentjwt invoke-agentapi');
const INSTALLERS = set('msiexec wusa rustup-init add-appxpackage add-apppackage add-appxprovisionedpackage');
const INSTALLER_NAME = /setup|installer|(^|[-_. ])(un)?install([-_. ]|$)/;
const PACKAGES = /\.(msi|msix|msixbundle|appx|appxbundle|msp|appinstaller)$/;
const PACKAGE_MANAGERS = set('winget choco chocolatey scoop');
const PACKAGE_VERBS = set('install upgrade update uninstall remove import');
const REG_READS = set('query compare export save');
const REGISTRY_WRITERS = set(
  'set-itemproperty sp new-itemproperty remove-itemproperty rp rename-itemproperty rnp clear-itemproperty clp copy-itemproperty cpp move-itemproperty mp set-item si new-item ni mkdir md remove-item ri rm rmdir rd del erase rename-item rni ren clear-item cli copy-item cpi cp copy move-item mi mv move',
);
const REGISTRY_PATH = /(^|[^a-z0-9])hk(cu|lm|cr|u|cc):|registry::/;
const MOVES = set('cd chdir pushd popd set-location sl push-location pop-location');
const CHANGERS = { bash: set('cd pushd'), powershell: set('cd chdir sl set-location pushd push-location'), cmd: set('cd chdir pushd') };
const REDEFINES =
  /\b(function|filter)\s+(\w+:)?(cd|chdir|pushd|sl|set-location|push-location)\b|\b(cd|pushd)\s*\(\s*\)|\b(alias|sal|nal)\b[^;|&\n]*\b(cd|chdir|pushd|sl|set-location|push-location)\b|function:\/?(cd|chdir|pushd|sl|set-location|push-location)\b|\bcdpath\b/;
const GIT_NETWORK = set('push fetch pull clone ls-remote send-pack fetch-pack http-fetch http-push request-pull svn lfs send-email imap-send');
const GIT_FLAGS = set(
  '--version -v --help -h --html-path --man-path --info-path -p --paginate -P --no-pager --no-replace-objects --bare --literal-pathspecs --glob-pathspecs --noglob-pathspecs --icase-pathspecs --no-optional-locks --no-lazy-fetch --no-advice --exec-path',
);
const GIT_VALUES = set('--namespace --super-prefix --attr-source --list-cmds');
const CARGO_FLAGS = set('--locked --offline --frozen -q --quiet -v -vv --verbose -V --version --list -h --help');
const CARGO_VALUES = set('--config -C -Z --color --explain');
const CARGO_SETTINGS = /^(net\.[\w-]+|term\.[\w-]+|build\.(jobs|incremental)|profile\.[\w.-]+)$/;
const GIT_SETTINGS =
  /^(user\.(name|email)|(commit|tag)\.gpgsign|init\.defaultbranch|core\.(autocrlf|quotepath|longpaths|symlinks|filemode|ignorecase|abbrev|safecrlf|eol)|color\.[\w.]+|advice\.\w+|safe\.directory|gc\.auto|pull\.(rebase|ff)|merge\.ff|diff\.(renames|renamelimit)|log\.\w+|status\.\w+|column\.\w+)$/;
const GITHUB_HOST = /github(usercontent)?\.com|ghcr\.io/i;
const HTTP_TOOLS = set('curl wget http https invoke-restmethod irm invoke-webrequest iwr start-bitstransfer bitsadmin certutil aria2c');
const SSH_TOOLS = set('ssh scp sftp plink pscp psftp');
const CARGO_DEPS = set('add update install remove rm upgrade generate-lockfile binstall');
const SHELLS = set('bash sh zsh dash ash ksh');
const POWERSHELLS = set('pwsh powershell pwsh-preview');
const POWERSHELL_VALUES = set('executionpolicy ex ep workingdirectory wd windowstyle w configurationname config settingsfile settings custompipename inputformat if outputformat of o psconsolefile');
const WRAPPERS = set(
  'env command builtin exec nohup nice time timeout stdbuf setsid winpty xargs watch sudo doas runas start start-process saps explorer rundll32 invoke-item ii cygstart conhost wt psexec schtasks forfiles call wsl busybox rustup set-alias new-alias sal nal parallel start-job sajb start-threadjob invoke-command icm script sc net net1 get-service gsv start-service sasv stop-service spsv restart-service suspend-service resume-service set-service new-service remove-service service',
);
const COMMIT_PLAIN = set(
  '--amend --no-edit --edit --quiet --verbose --signoff --no-signoff --allow-empty --allow-empty-message --reset-author --gpg-sign --no-gpg-sign --dry-run --short --branch --porcelain --long --null --status --no-status --no-post-rewrite',
);
const COMMIT_VALUES = set('--message --file --author --date --cleanup --trailer --reuse-message --reedit-message --fixup --squash --template');
const DATA_FOLDER = [
  /(?<!local)appdata[^a-z0-9]{0,12}(?:roaming[^a-z0-9]{0,12})?onelaunch/,
  /roaming[^a-z0-9]{0,12}onelaunch/,
  /(?<!local)appdata(?![^a-z0-9]{0,3}local)[^;|&\n]{0,40}onelaunch/,
];
const KEY_NAMES = [/one-launch-rs-bot/, /one_launch_agent_key_dir/, /\.pem(?![a-z0-9])/];
const TOKEN_SHOWN = /git_(trace_redact|config)/;
const DATA_DIR = /^--data-dir=./;
const REACHES = [
  ['process', /diagnostics\.process|\[\s*wmi(class|searcher)?\s*\]|management(object|class)/, 'this PowerShell text names a .NET or WMI type that lists, starts or ends processes, and no form of a process tool is allowed'],
  ['registry', /microsoft\.win32\.registry/, 'this PowerShell text names the .NET registry classes, which can write a key; read a key with reg query or Get-ItemProperty'],
  ['unreadable', /\[\s*scriptblock\s*\]\s*::\s*create|\.\s*(invokescript|newscriptblock)\s*\(/, 'this PowerShell text makes a script out of a string at run time, and the hook cannot read that string'],
];
const QUIET = set('cat echo printf date type get-content gc get-date write-output');
const STARTERS = set('env command builtin exec nohup nice time timeout');
const STDIN = /^(-|\/dev\/(stdin|fd\/\d+)|\/proc\/self\/fd\/\d+)$/;
const MERGE_VALUES = set('author-email body body-file match-head-commit subject repo');
const MERGE_SWITCHES = set('admin auto delete-branch disable-auto merge rebase squash help');
const MERGE_SHORT = new Map(Object.entries({ A: 'author-email', b: 'body', F: 'body-file', t: 'subject', R: 'repo', d: 'delete-branch', m: 'merge', r: 'rebase', s: 'squash', h: 'help' }));
const AS_VALUE = '; a word that is only the value of an option goes in as --option=value';
const BUDGET = 4000;
const DEPTH = 8;
const NAME_LIMIT = 260;

function flatten(text) {
  const lower = text.toLowerCase().replace(/["'`^\u2018-\u201E]/g, '');
  const tidy = (flat) =>
    flat.replace(/\S+/g, (run) => {
      const kept = [];
      for (const part of run.split('/')) {
        if ((part === '' || part === '.') && kept.length) continue;
        if (part === '..' && kept.length && kept.at(-1) !== '..' && kept.at(-1) !== '') kept.pop();
        else kept.push(part);
      }
      return kept.join('/');
    });
  return [tidy(lower.replace(/\\/g, '/')), tidy(lower.replace(/\\/g, ''))];
}

function trimmed(text) {
  let end = text.length;
  while (end > 0 && (text[end - 1] === '.' || text[end - 1] === ' ')) end--;
  return text.slice(0, end);
}

function identify(text) {
  const flat = text.replace(/\\/g, '/');
  const cut = flat.lastIndexOf('/');
  const leaf = trimmed(flat.slice(cut + 1).toLowerCase());
  const exe = /\.(exe|com|cmd|bat)$/.exec(leaf);
  const name = exe ? trimmed(leaf.slice(0, exe.index)) : leaf;
  return { name, leaf, ext: exe ? exe[1] : '', pathed: cut >= 0 };
}

function computed(word) {
  const leaf = word.text.replace(/\\/g, '/').split('/').pop();
  if (leaf.includes(MARK) || leaf.length > NAME_LIMIT || /[\0-\x1f]/.test(leaf)) return true;
  return leaf.length > 1 && !/^\[+$/.test(leaf) && /[*?]|\[.+\]|\{.*(,|\.\.).*\}/.test(leaf);
}

function into(cwd, word, dialect) {
  if (!word || word.dynamic || !word.text) return null;
  let target = word.text;
  if (/^[A-Za-z]:(?![\\/])/.test(target)) return null;
  if (target === '~' || /^~[\\/]/.test(target)) target = path.join(os.homedir(), target.slice(1));
  else if (target.startsWith('~')) return null;
  if (dialect === 'bash') {
    target = target.replace(/^\/([A-Za-z])(?=\/|$)/, '$1:').replace(/^([A-Za-z]:)$/, '$1/');
    if (target.startsWith('/')) return null;
  }
  const rooted = /^[\\/](?![\\/])/.test(target);
  if (path.isAbsolute(target) && !rooted) return path.resolve(target);
  return cwd === null ? null : path.resolve(cwd, target);
}

function head(words, dialect, mode, state) {
  let rest = words;
  if (dialect === 'powershell' && mode === 'normal') {
    const first = rest[0];
    if (first.op || (first.text === '.' && !first.quoted && !first.block && rest.length > 1)) {
      rest = rest.slice(1);
      if (!rest.length || (rest[0].block && !rest[0].text)) return null;
      const called = rest[0];
      if (called.string && called.cut < called.text.length) {
        rest = [{ ...called, text: called.text.slice(0, called.cut) }, { ...called, text: called.text.slice(called.cut), string: false }, ...rest.slice(1)];
      }
      if (computed(rest[0])) throw new Refusal('unreadable', 'the command after the call operator is computed at run time');
      return rest;
    }
    const text = first.text;
    const expression = first.string || first.block || /^[$[@!+,-]/.test(text) || text.startsWith(MARK) || /^\d+(\.\d+)?$/.test(text);
    const assign = rest.findIndex((word, k) => k > 0 && (word.assign || (expression && !word.quoted && word.text.toLowerCase() === 'in')));
    if (expression || assign === 1) {
      return assign < 0 || assign + 1 >= rest.length ? null : head(rest.slice(assign + 1), dialect, mode, state);
    }
    if (/^(return|throw|exit)$/i.test(text)) return rest.length > 1 ? head(rest.slice(1), dialect, mode, state) : null;
  }
  if (dialect !== 'powershell') {
    for (;;) {
      const word = rest[0];
      if (!word) return null;
      const assign = /^([A-Za-z_]\w*)\+?=/.exec(word.text);
      if (assign && assign[1].startsWith('GIT_')) state.gitEnv = true;
      if (!assign && (word.quoted || !LEADING.has(word.text))) break;
      rest = rest.slice(1);
      if (word.text === 'time') while (rest[0] && /^-/.test(rest[0].text)) rest = rest.slice(1);
    }
  }
  if (dialect === 'cmd' && /^[@'`]/.test(rest[0].text)) {
    const text = rest[0].text.replace(/^[@'`]+|['`]+$/g, '');
    rest = text ? [{ ...rest[0], text }, ...rest.slice(1)] : rest.slice(1);
  }
  if (!rest.length) return null;
  if (computed(rest[0])) {
    if (mode === 'suffix') return null;
    throw new Refusal('unreadable', 'the command name is computed at run time');
  }
  return rest;
}

function scoped(state, run) {
  const cwd = state.cwd;
  try {
    run();
  } finally {
    state.cwd = cwd;
  }
}

function walk(text, dialect, state, depth) {
  if (depth > DEPTH) throw new Refusal('unreadable', 'commands are nested too deep for the hook to read');
  const reach = dialect === 'powershell' && REACHES.find(([, pattern]) => pattern.test(text.toLowerCase().replace(/["'`]/g, '')));
  if (reach) throw new Refusal(reach[0], reach[2]);
  let commit = null;
  let moves = false;
  let shaky = false;
  for (const { words, level, piped, splits, wrote, joined, background, redirected, grouped } of lex(text, dialect)) {
    const plain = level === 0 && !piped && !background && !redirected && !grouped;
    if (dialect === 'powershell' && words.some((word) => word.text === '--%' && !word.quoted)) {
      throw new Refusal('unreadable', 'the stop-parsing token --% hides the rest of the line from the hook');
    }
    if (commit !== null && level <= commit) commit = null;
    if (level === 0 && !joined) {
      if (shaky) state.cwd = null;
      moves = false;
      shaky = false;
    }
    const exits = (dialect === 'powershell' ? words[0].text.toLowerCase() : words[0].text) === 'exit' && (words.length === 1 || (words.length === 2 && /^\d+$/.test(words[1].text)));
    const leaves = plain && joined === 'or' && moves && exits;
    if (level === 0 && joined === 'or' && moves && !leaves) state.cwd = null;
    const commits = state.commits;
    const moved = command(words, dialect, state, depth, 'normal', plain && joined !== 'or');
    if (moved) {
      moves = true;
      shaky ||= joined === 'and';
    }
    const inner = commit === null ? null : head(words, dialect, 'normal', state);
    if (commit !== null && (wrote || (inner && !QUIET.has(identify(inner[0].text).name)))) {
      throw new Refusal('secrets', 'a command inside the words of the commit runs after the scan and before the commit; only cat, echo, printf, date and Get-Content may build a message there, and nothing may write a file');
    }
    if (state.commits > commits && commit === null) commit = level;
    if (dialect === 'cmd') for (let k = 1; k < words.length; k++) command(words.slice(k), dialect, state, depth, 'suffix', false);
    for (const k of splits) if (k < words.length) command(words.slice(k), dialect, state, depth, 'normal', false);
    if (!moved && !leaves) state.before++;
  }
}

function readings(text, state, depth) {
  const before = state.before;
  walk(text, 'cmd', state, depth);
  const after = state.before;
  if (!text.includes('"')) return;
  state.before = before;
  walk(text.replace(/"/g, ''), 'cmd', state, depth);
  state.before = Math.max(after, state.before);
}

function literal(words, what) {
  if (words.some((word) => word.dynamic)) {
    throw new Refusal('unreadable', `${what} is put together at run time, so the hook cannot read what will run`);
  }
}

function behind(state, plain, run) {
  const outer = state.behind;
  state.behind = { plain: (outer === null || outer.plain) && plain };
  run();
  state.behind = outer;
}

function suffixes(args, dialect, state, depth, name = '') {
  literal(args, 'a word behind a command that starts another command');
  const flat = args.flatMap((word) =>
    /[\s,]/.test(word.text)
      ? word.text
          .split(/[\s,]+/)
          .filter(Boolean)
          .map((text) => ({ ...word, text, alt: undefined }))
      : [word],
  );
  for (let k = 0; k < flat.length; k++) {
    const plain = STARTERS.has(name) && flat.slice(0, k).every((word) => /^([A-Za-z_]\w*=|\d+(\.\d+)?[smhd]?$)/.test(word.text));
    behind(state, plain, () => command(flat.slice(k), dialect, state, depth + 1, 'suffix', false));
  }
}

function denied(id, args, lower) {
  const { name, leaf, ext } = id;
  const getProcess = name === 'process' && !(args[0]?.block && !args[0].text);
  if (PROCESS.has(name) || CIM.test(name) || getProcess || (name === 'query' && lower[0] === 'process')) {
    throw new Refusal('process', `${name} lists or ends processes, and no form of a process tool is allowed`);
  }
  if (LAUNCHERS.has(name)) {
    throw new Refusal('launcher', `${name} is a launcher, a Riot game or Vanguard; the owner starts and stops them, never an agent`);
  }
  const named = (ext || !/\.[a-z0-9]{1,5}$/.test(name)) && name !== 'install' && INSTALLER_NAME.test(name);
  if (INSTALLERS.has(name) || PACKAGES.test(leaf) || named || (PACKAGE_MANAGERS.has(name) && lower.some((arg) => PACKAGE_VERBS.has(arg)))) {
    throw new Refusal('installer', 'this would start an installer; installers run on GitHub-hosted runners only');
  }
  if (name === 'onelaunch' && !args.some((arg) => DATA_DIR.test(arg.text))) {
    throw new Refusal('app', 'the app binary needs --data-dir=<scratch folder> in that exact form, which the Electron build and the Rust build both read');
  }
  if (name === 'reg' && lower[0] && !lower[0].startsWith('/') && !REG_READS.has(lower[0])) {
    throw new Refusal('registry', `reg ${lower[0]} changes the registry; only reg query, compare, export and save are allowed`);
  }
  if (/^(regedit|regedt32|regini|setx)$/.test(name) || leaf.endsWith('.reg')) {
    throw new Refusal('registry', 'this changes the registry');
  }
  if (TOKEN_MAKERS.has(name)) {
    throw new Refusal('key', `${name} hands out a token of the app; only the two wrappers call it`);
  }
  if (SSH_TOOLS.has(name)) {
    throw new Refusal('github', `${name} would use the owner's SSH key; reach GitHub only through scripts/agent/git.ps1 and scripts/agent/gh.ps1`);
  }
}

function folder(target) {
  try {
    return fs.statSync(target).isDirectory() ? target : null;
  } catch {
    return null;
  }
}

function destination(name, args, dialect) {
  let rest = args;
  let literal = dialect !== 'powershell';
  if (dialect === 'powershell') {
    const option = rest.length === 2 && !rest[0].quoted ? rest[0].text.toLowerCase() : '';
    if (option === '-path' || option === '-literalpath') rest = rest.slice(1);
    literal = option === '-literalpath';
  } else if (dialect === 'cmd') {
    if (name !== 'pushd' && rest.length === 2 && /^\/d$/i.test(rest[0].text)) rest = rest.slice(1);
  } else if (name === 'cd') {
    while (rest.length > 1 && !rest[0].quoted && /^(--|-P|-L)$/.test(rest[0].text)) {
      const last = rest[0].text === '--';
      rest = rest.slice(1);
      if (last) break;
    }
  }
  const [word] = rest;
  if (rest.length !== 1 || word.block || (!word.quoted && /^[-+]/.test(word.text))) return null;
  return literal || !/[*?[\]]/.test(word.text) ? word : null;
}

function move(name, args, state, sure, dialect) {
  const named = args.find((arg) => arg.quoted || !(arg.text.startsWith('-') || /^\/d$/i.test(arg.text)));
  if (named && REGISTRY_PATH.test(named.text.toLowerCase())) {
    throw new Refusal('registry', 'a registry key as the working location hides which key a later command writes; name the key in full on a read');
  }
  const target = sure && !state.redefined ? destination(name, args, dialect) : null;
  const drive = (text) => /^[A-Za-z]:/.exec(text ?? '')?.[0].toLowerCase();
  const stays = dialect === 'cmd' && name !== 'pushd' && args.length === 1 && drive(target?.text) && drive(target.text) !== drive(state.cwd);
  state.cwd = target && !stays ? folder(into(state.cwd, target, dialect)) : null;
  return true;
}

function unknownIfMoved(state, run) {
  const before = state.cwd;
  run();
  if (state.cwd !== before) state.cwd = null;
}

function config(pair) {
  const key = pair.split('=')[0].trim().toLowerCase();
  if (key === 'core.hookspath') throw new Refusal('bypass', 'core.hooksPath moves the commit hooks away');
  if (!GIT_SETTINGS.test(key)) {
    throw new Refusal('unreadable', 'git -c sets a key the hook does not know to be harmless: an alias, a pager, an editor or a helper is a program git then runs');
  }
}

function configWrites(texts) {
  const plain = [];
  for (let k = 0; k < texts.length; k++) {
    if (/^(-f|--file|--blob|-t|--type|--default)$/.test(texts[k])) k++;
    else if (!texts[k].startsWith('-')) plain.push(texts[k].toLowerCase());
  }
  if (texts.some((text) => /^(--(unset|unset-all|add|replace-all|edit|rename-section|remove-section)|-e)$/.test(text))) return true;
  if (texts.some((text) => /^(--(get|get-all|get-regexp|get-urlmatch|list)|-l)$/.test(text)) || /^(get|list)$/.test(plain[0])) return false;
  return plain.length > 1;
}

function reaches(sub, texts) {
  if (GIT_NETWORK.has(sub)) return true;
  const plain = texts.filter((text) => !text.startsWith('-')).map((text) => text.toLowerCase());
  const flag = (...flags) => texts.some((text) => flags.includes(text));
  if (sub === 'remote') {
    return /^(update|show|prune)$/.test(plain[0] ?? '') || (plain[0] === 'set-head' && flag('-a', '--auto')) || (plain[0] === 'add' && flag('-f', '--fetch'));
  }
  if (sub === 'submodule') return plain[0] === 'add' || plain[0] === 'update';
  return sub === 'archive' && texts.some((text) => text.startsWith('--remote'));
}

function push(texts, cwd, state, wrapper) {
  const refs = [];
  for (let k = 0; k < texts.length; k++) {
    const text = texts[k];
    if (text === '--') {
      refs.push(...texts.slice(k + 1));
      break;
    }
    if (/^--(for|m)/.test(text)) throw new Refusal('bypass', 'a force-push rewrites what is on GitHub');
    if (/^--(ta|fol)/.test(text)) throw new Refusal('tags', 'this pushes tags');
    if (/^-[A-Za-z]+$/.test(text)) {
      const letters = text.slice(1);
      const option = letters.indexOf('o');
      if ((option < 0 ? letters : letters.slice(0, option)).includes('f')) throw new Refusal('bypass', 'a force-push rewrites what is on GitHub');
      if (option === letters.length - 1) k++;
    } else if (/^--(repo|push-option|receive-pack|exec)$/.test(text)) {
      k++;
    } else if (!text.startsWith('-')) {
      refs.push(text);
    }
  }
  for (const [index, ref] of refs.entries()) {
    if (ref.startsWith('+')) throw new Refusal('bypass', 'a refspec that starts with + is a force-push');
    if (ref.includes('*')) throw new Refusal('tags', 'a refspec with a wildcard can push tags');
    if (/refs\/tags/i.test(ref) || (index > 0 && ref.toLowerCase() === 'tag')) throw new Refusal('tags', 'this pushes a tag');
  }
  const sides = refs.slice(1).flatMap((ref) => ref.split(':').filter((side) => side && !side.includes(MARK)));
  if (!wrapper || !sides.length) return;
  if (cwd === null) throw new Refusal('tags', 'the hook cannot tell which checkout this push is in, so it cannot rule out a tag; name it with -C <folder>');
  for (const side of sides) if (state.io.isTag(cwd, side)) throw new Refusal('tags', `${side} is a tag in this checkout`);
}

function commit(texts, cwd, elsewhere, state) {
  let broad = false;
  for (let k = 0; k < texts.length; k++) {
    const text = texts[k];
    if (text === '--') {
      broad ||= k + 1 < texts.length;
      break;
    }
    if (text.startsWith('--')) {
      const equals = text.indexOf('=');
      const flag = equals < 0 ? text : text.slice(0, equals);
      if (COMMIT_VALUES.has(flag)) k += equals < 0 ? 1 : 0;
      else if (!COMMIT_PLAIN.has(flag)) broad = true;
    } else if (text.length > 1 && text.startsWith('-')) {
      for (let c = 1; c < text.length; c++) {
        const letter = text[c];
        if (letter === 'n') throw new Refusal('bypass', 'git commit -n skips the commit hooks');
        if ('mFCct'.includes(letter)) {
          if (c + 1 === text.length) k++;
          break;
        }
        if (letter === 'S' || letter === 'u') break;
        if (!'qvsez'.includes(letter)) broad = true;
      }
    } else {
      broad = true;
    }
  }
  if (state.before > 0) {
    throw new Refusal('secrets', 'other commands run before the commit in this call, so the scan cannot see what the commit will hold; stage in one call and commit alone in the next');
  }
  if (state.behind && !state.behind.plain) {
    throw new Refusal('secrets', 'the commit is started by another command that may run it in another folder than the one scanned; call git commit itself, or behind env, timeout, nohup, command or exec with no option of theirs');
  }
  if (elsewhere || state.gitEnv) {
    throw new Refusal('secrets', 'the commit names its own git folder or index (--git-dir, --work-tree, GIT_*), which the scan cannot follow');
  }
  if (cwd === null) throw new Refusal('secrets', 'the hook cannot tell which checkout this commit is in; name it with -C <folder>');
  const findings = state.io.commit(cwd, broad);
  if (findings.length) {
    const shown = findings.slice(0, 20).join('\n  ');
    throw new Refusal('secrets', `the commit would hold ${findings.length} finding(s) of the secret scan:\n  ${shown}\nUse a placeholder, or ask the owner to add an exact value to xtask/secrets.allow`);
  }
  state.commits++;
}

function git(args, state, wrapper, dialect, again = false) {
  let cwd = state.cwd;
  let elsewhere = false;
  let k = 0;
  for (; k < args.length; k++) {
    const text = args[k].text;
    if (text === '-C') cwd = into(cwd, args[++k], dialect);
    else if (text === '-c' || text === '--config-env') config(args[++k]?.text ?? '');
    else if (/^--config-env=/.test(text)) config(text.slice(13));
    else if (/^--(git-dir|work-tree)(=|$)/.test(text)) {
      elsewhere = true;
      if (!text.includes('=')) k++;
    } else if (GIT_VALUES.has(text)) k++;
    else if (!text.startsWith('-')) break;
    else if (!GIT_FLAGS.has(text.split('=')[0]) && !GIT_VALUES.has(text.split('=')[0])) {
      throw new Refusal('unreadable', 'git is given an option before its command that the hook does not know, so it cannot tell which command runs');
    }
  }
  const sub = (args[k]?.text ?? '').toLowerCase();
  const texts = args.slice(k + 1).map((arg) => arg.text);
  if (sub === 'tag') throw new Refusal('tags', 'git tag: tags are the owner\'s; read them with git for-each-ref refs/tags');
  if (/^(update|symbolic)-ref$/.test(sub) && texts.some((text) => /refs\/tags/i.test(text))) throw new Refusal('tags', 'this writes a tag ref');
  if (texts.some((text) => text.length >= 9 && '--no-verify'.startsWith(text))) throw new Refusal('bypass', '--no-verify skips the hooks');
  if (sub === 'config' && texts.some((text) => /core\.hookspath/i.test(text)) && configWrites(texts)) throw new Refusal('bypass', 'core.hooksPath moves the commit hooks away');
  if (sub.startsWith('credential')) throw new Refusal('key', `git ${sub} hands out a stored login or the app's token`);
  const programs =
    texts.some((text) => /^--(upload-pack|receive-pack|exec|extcmd)(=|$)/.test(text)) ||
    ((sub === 'rebase' || sub === 'difftool') && texts.includes('-x')) ||
    (sub === 'bisect' && texts[0] === 'run') ||
    (sub === 'submodule' && texts.includes('foreach')) ||
    sub === 'filter-branch';
  if (programs) throw new Refusal('unreadable', `git ${sub} is told to run a program of its own here, which the hook cannot read`);
  if (sub === 'push') push(texts, cwd, state, wrapper);
  if (!wrapper && reaches(sub, texts)) {
    throw new Refusal('github', `plain git ${sub} acts with the owner's login; use scripts/agent/git.ps1 ${sub}`);
  }
  if (sub === 'commit' && !again) commit(texts, cwd, elsewhere, state);
}

function mergeFlags(words) {
  const plain = new Set();
  const valued = new Set();
  const unknown = (word) => new Refusal('merge', `gh pr merge is given ${word}, which is no option of it that the hook knows, so it cannot tell whether --auto is set${AS_VALUE}`);
  for (let k = 0; k < words.length && words[k] !== '--'; k++) {
    const word = words[k];
    if (word.startsWith('--')) {
      const cut = word.indexOf('=');
      const name = cut < 0 ? word.slice(2) : word.slice(2, cut);
      if (MERGE_VALUES.has(name)) k += cut < 0 ? 1 : 0;
      else if (!MERGE_SWITCHES.has(name)) throw unknown(word);
      else (cut < 0 ? plain : valued).add(name);
    } else if (word.length > 1 && word[0] === '-') {
      for (let c = 1; c < word.length; c++) {
        const name = MERGE_SHORT.get(word[c]);
        if (!name) throw unknown(word);
        if (MERGE_VALUES.has(name)) {
          k += c + 1 === word.length ? 1 : 0;
          break;
        }
        if (word[c + 1] === '=') {
          valued.add(name);
          break;
        }
        plain.add(name);
      }
    }
  }
  return { plain, valued };
}

function gh(args, wrapper) {
  const words = args.map((arg) => arg.text);
  const texts = words.map((word) => word.toLowerCase());
  const bare = texts.filter((text) => !text.startsWith('-'));
  const all = texts.join(' ');
  if (bare.includes('release')) throw new Refusal('tags', `gh release: releases are the owner's${AS_VALUE}`);
  if (bare.includes('pr') && bare.includes('merge')) {
    if (args.some((arg) => !arg.quoted && /^(@?\uE000|@$)/.test(arg.text))) {
      throw new Refusal('merge', 'in gh pr merge a variable that stands as a word of its own vanishes when it is empty, and the next word then becomes the value of the option before it; quote it ("$subject") or join it to its option (--subject=$subject)');
    }
    const places = (name) => texts.flatMap((text, k) => (text === name ? [k] : []));
    for (const pr of places('pr')) {
      for (const merge of places('merge')) {
        const { plain, valued } = mergeFlags(words.filter((word, k) => k !== pr && k !== merge));
        const switched = ['auto', 'admin', 'disable-auto'].some((name) => valued.has(name));
        if (switched || plain.has('admin') || plain.has('auto') === plain.has('disable-auto')) {
          throw new Refusal('merge', 'gh pr merge needs a plain --auto as an option of its own (or a plain --disable-auto instead), no --auto=<value> and no --admin, so that GitHub merges once the checks pass');
        }
      }
    }
  }
  const api = bare.includes('api');
  const sends = texts.some((text) => /^(-x|--method|-f|--field|--raw-field|--input)(=|$)|^-[xf]./.test(text));
  if (api && /pulls\/[^/\s]+\/merge|\/merges(?![a-z0-9_-])|mergepullrequest|mergebranch/.test(all)) {
    throw new Refusal('merge', 'this API call merges a pull request or a branch');
  }
  if (api && (/\/releases|git\/tags|refs\/tags|createrelease|createref|updateref|deleteref/.test(all) || (sends && /git\/refs/.test(all)))) {
    throw new Refusal('tags', 'this API call reaches tags or releases, or writes a ref the hook cannot read; push a branch with scripts/agent/git.ps1');
  }
  if (api && bare.includes('graphql') && texts.some((text) => text.startsWith('--input') || /=@/.test(text))) {
    throw new Refusal('unreadable', 'a GraphQL call whose text comes from a file or from standard input is one the hook cannot read');
  }
  if (bare.includes('auth')) {
    throw wrapper ? new Refusal('key', `gh auth through the wrapper would show or store the app's token${AS_VALUE}`) : new Refusal('github', "gh auth works on the owner's login");
  }
  if (!wrapper) throw new Refusal('github', "plain gh acts with the owner's login, reads included; use scripts/agent/gh.ps1");
  const runs = ['alias', 'extension', 'ext', 'config'].find((name) => bare.includes(name));
  if (runs) throw new Refusal('unreadable', `gh ${runs} can make gh run a program the hook cannot read${AS_VALUE}`);
}

function cargo(args) {
  const texts = args.map((arg) => arg.text);
  const cargoArgs = texts.includes('--') ? texts.slice(0, texts.indexOf('--')) : texts;
  for (const [at, text] of cargoArgs.entries()) {
    if (text !== '--config' && !text.startsWith('--config=')) continue;
    const pair = text === '--config' ? (cargoArgs[at + 1] ?? '') : text.slice(9);
    if (!CARGO_SETTINGS.test(pair.split('=')[0].trim().toLowerCase())) {
      throw new Refusal('unreadable', 'cargo --config sets a key the hook does not know to be harmless: an alias, a runner or a wrapper is a program cargo then runs');
    }
  }
  let k = 0;
  for (; k < texts.length; k++) {
    const text = texts[k];
    if (CARGO_VALUES.has(text)) k++;
    else if (!/^[-+]/.test(text)) break;
    else if (!text.startsWith('+') && !CARGO_FLAGS.has(text) && !/^(--config=|--color=|-Z.|-C.)/.test(text)) {
      throw new Refusal('unreadable', 'cargo is given an option before its command that the hook does not know, so it cannot tell which command runs');
    }
  }
  const sub = (texts[k] ?? '').toLowerCase();
  if (CARGO_DEPS.has(sub)) throw new Refusal('deps', `cargo ${sub} changes the dependency set or installs a tool`);
  if (sub !== 'run' && sub !== 'r') return;
  const rest = args.slice(k + 1);
  const split = rest.findIndex((arg) => arg.text === '--');
  const own = (split < 0 ? rest : rest.slice(0, split)).map((arg) => arg.text.toLowerCase());
  const app = split < 0 ? [] : rest.slice(split + 1);
  const joined = app.some((arg) => DATA_DIR.test(arg.text));
  const spaced = app.some((arg, at) => arg.text === '--data-dir' && app[at + 1]?.text && !app[at + 1].text.startsWith('-'));
  if (joined || spaced) return;
  const value = (long, short) => {
    for (const [at, text] of own.entries()) {
      if (text === long || text === short) return own[at + 1] ?? '';
      if (text.startsWith(`${long}=`)) return text.slice(long.length + 1);
      if (short && text.startsWith(short) && !text.startsWith('--')) return text.slice(short.length);
    }
    return null;
  };
  const crate = value('--package', '-p');
  const bin = value('--bin', null);
  const unknown = crate === null || crate.includes(MARK) || own.some((text) => text.startsWith('--manifest-path'));
  if (unknown || crate.includes('ol-app') || (bin ?? '').includes('onelaunch')) {
    throw new Refusal('app', 'cargo run starts the app unless -p names another package; give the app -- --data-dir <scratch folder>');
  }
}

function shell(name, args, state, depth) {
  let inline = false;
  for (let k = 0; k < args.length; k++) {
    const text = args[k].text;
    if (text === '--version' || text === '--help') return;
    if (/^(--rcfile|--init-file)$/.test(text)) k++;
    else if (/^[-+][A-Za-z]+$/.test(text)) {
      inline ||= text[0] === '-' && text.includes('c');
      if (/[oO]/.test(text)) k++;
    } else if (!/^[-+]/.test(text)) {
      if (inline) {
        literal([args[k]], `the command text handed to ${name}`);
        scoped(state, () => walk(text, 'bash', state, depth + 1));
        return;
      }
      if (STDIN.test(text)) break;
      return;
    }
  }
  throw new Refusal('unreadable', `${name} with no command and no script file takes its commands from standard input`);
}

function powershell(name, args, state, depth, dialect) {
  let informational = false;
  let positional = -1;
  for (let k = 0; k < args.length; k++) {
    const arg = args[k];
    const flag = !arg.block && /^(--|[-/\u2013-\u2015])([A-Za-z]+:?|\?)$/.test(arg.text) ? arg.text.replace(/^(--|.)/, '').toLowerCase().replace(/:$/, '') : null;
    if (flag === null) {
      if (positional < 0 && arg.text !== '-') positional = k;
      continue;
    }
    const rest = args.slice(k + 1);
    if ('encodedcommand'.startsWith(flag) || flag === 'ec' || flag === 'ea' || (flag.length > 7 && 'encodedarguments'.startsWith(flag))) {
      throw new Refusal('unreadable', 'an encoded PowerShell command');
    }
    if ('command'.startsWith(flag) || flag === 'cwa' || (flag.length > 7 && 'commandwithargs'.startsWith(flag))) {
      if (!rest.length || rest[0].text === '-') break;
      literal(rest, `the command text handed to ${name}`);
      const text = rest
        .filter((word) => !word.block)
        .map((word) => word.text)
        .join(' ');
      walk(text, 'powershell', state, depth + 1);
      return;
    }
    if ('file'.startsWith(flag)) {
      if (!rest.length) break;
      command(rest, dialect, state, depth + 1, 'call', false);
      return;
    }
    if (/^(version|v|help|h|\?)$/.test(flag)) informational = true;
    if (POWERSHELL_VALUES.has(flag) || (flag.length > 2 && [...POWERSHELL_VALUES].some((full) => full.startsWith(flag)))) {
      if (flag === 'wd' || (flag.length > 1 && 'workingdirectory'.startsWith(flag))) state.cwd = folder(into(state.cwd, rest[0], dialect));
      k++;
    }
  }
  suffixes(args, 'powershell', state, depth);
  if (positional >= 0) {
    const text = args
      .slice(positional)
      .filter((word) => !word.block)
      .map((word) => word.text)
      .join(' ');
    walk(text, 'powershell', state, depth + 1);
  } else if (!informational) {
    throw new Refusal('unreadable', `${name} with no command and no script file takes its commands from standard input`);
  }
}

function cmdShell(args, state, depth) {
  const at = args.findIndex((arg) => /^\/{1,2}[cr]/i.test(arg.text));
  if (at < 0) {
    if (args.some((arg) => /^\/{1,2}\?$/.test(arg.text))) return;
    throw new Refusal('unreadable', 'cmd without /c stays open and takes its commands from standard input');
  }
  const text = [args[at].text.replace(/^\/{1,2}[cr]/i, ''), ...args.slice(at + 1).map((arg) => arg.text)].filter(Boolean).join(' ');
  literal(args.slice(at), 'the command text handed to cmd');
  if (/%[^%\s]+%/.test(text)) {
    throw new Refusal('unreadable', 'cmd puts the value of %NAME% into the line before it reads it, so the hook cannot read what will run');
  }
  scoped(state, () => readings(text, state, depth + 1));
}

function command(all, dialect, state, depth, mode, sure) {
  if (++state.count > BUDGET || depth > DEPTH) throw new Refusal('unreadable', 'this call holds more nested commands than the hook reads');
  const words = head(all, dialect, mode, state);
  if (!words) return false;
  const [first, ...args] = words;
  if (PROTOCOLS.test(first.text)) {
    throw new Refusal('launcher', 'this would open a launcher through its URL; the owner performs every real launch');
  }
  const id = identify(first.text);
  const { name } = id;
  const texts = args.map((arg) => arg.text);
  const lower = texts.map((text) => text.toLowerCase());
  const plain = (candidate) => mode === 'suffix' && candidate.pathed && !candidate.ext;
  const argument = plain(id);
  const candidates = first.alt ? [id, identify(first.alt)] : [id];
  if (dialect === 'cmd' && first.text.includes('/')) candidates.push(identify(first.text.slice(0, first.text.indexOf('/'))));
  for (const candidate of candidates) {
    if (!plain(candidate)) denied(candidate, args, lower);
  }
  if (name === 'git-credential.ps1') {
    throw new Refusal('key', 'only git may call scripts/agent/git-credential.ps1: it prints a token');
  }
  if (name === 'git.ps1' || name === 'gh.ps1') {
    const file = into(state.cwd, first, dialect);
    if (!file || !state.io.wrapper(file, name)) {
      throw new Refusal('github', `this ${name} is not scripts/agent/${name} of this repo, or the hook cannot tell which folder it is in; call it by a path the hook can read`);
    }
    const listed = dialect === 'powershell' && texts.some((text) => text.includes(','));
    const spread = args.flatMap((arg) => arg.text.split(',').filter(Boolean).map((text) => ({ ...arg, text: text.trim() })));
    for (const reading of listed ? [args, spread] : [args]) {
      if (name === 'git.ps1') git(reading, state, true, dialect, reading === spread);
      else gh(reading, true);
    }
    return false;
  }
  if (argument) return false;
  if ((first.text === '.' || name === 'source') && STDIN.test(texts[0] ?? '')) {
    throw new Refusal('unreadable', `${name} reads its commands from standard input here`);
  }

  if (REGISTRY_WRITERS.has(name)) state.registry = true;
  if (name === 'new-psdrive' && lower.some((text) => /registry|^hk(cu|lm|cr|u|cc)|^hkey_/.test(text))) {
    throw new Refusal('registry', 'a new drive on the registry hides which key a later command writes');
  }
  const typed = dialect === 'bash' ? first.text : first.text.toLowerCase();
  if (MOVES.has(name)) return move(name, args, state, sure && words === all && typed === name && CHANGERS[dialect].has(name), dialect);
  if (dialect === 'bash' && (typed === 'command' || typed === 'builtin') && CHANGERS.bash.has(texts[0])) {
    return move(texts[0], args.slice(1), state, sure && words === all, dialect);
  }
  if (name === 'git') git(args, state, false, dialect);
  else if (name === 'gh') gh(args, false);
  else if (name === 'cargo') cargo(args);
  else if (HTTP_TOOLS.has(name) && texts.some((text) => GITHUB_HOST.test(text))) {
    throw new Refusal('github', `${name} to a GitHub host goes around the app; use scripts/agent/gh.ps1 api`);
  } else if (SHELLS.has(name)) shell(name, args, state, depth);
  else if (POWERSHELLS.has(name)) scoped(state, () => powershell(name, args, state, depth, dialect));
  else if (name === 'cmd') cmdShell(args, state, depth);
  else if (name === 'invoke-expression' || name === 'iex') {
    const given = args.filter((arg) => arg.quoted || !arg.text.startsWith('-'));
    if (given.length !== 1 || given[0].dynamic || given[0].block) throw new Refusal('unreadable', 'Invoke-Expression runs text the hook cannot read');
    unknownIfMoved(state, () => walk(given[0].text, 'powershell', state, depth + 1));
  } else if (name === 'eval' || (name === 'trap' && dialect !== 'powershell')) {
    const given = name === 'trap' ? args.filter((arg) => arg.quoted || !arg.text.startsWith('-')).slice(0, 1) : args;
    literal(given, `the text ${name} runs`);
    unknownIfMoved(state, () => walk(given.map((arg) => arg.text).join(' '), 'bash', state, depth + 1));
  } else if (name === 'alias' && dialect !== 'powershell') {
    for (const text of texts) if (text.includes('=')) unknownIfMoved(state, () => walk(text.slice(text.indexOf('=') + 1), 'bash', state, depth + 1));
  } else if (name === 'find') {
    for (const [at, arg] of args.entries()) {
      if (!/^-(exec|execdir|ok|okdir)$/.test(arg.text)) continue;
      const end = args.findIndex((later, k) => k > at && (later.text === ';' || later.text === '+'));
      const run = args.slice(at + 1, end < 0 ? args.length : end);
      if (run.length) behind(state, !arg.text.endsWith('dir'), () => command(run, dialect, state, depth + 1, 'call', false));
    }
  } else if (WRAPPERS.has(name) && !(name === 'command' && /^-[vV]$/.test(texts[0] ?? ''))) suffixes(args, dialect, state, depth, name);
  return false;
}

function named(text, what) {
  const flat = flatten(text);
  if (flat.some((reading) => DATA_FOLDER.some((pattern) => pattern.test(reading)))) {
    throw new Refusal('data', `the ${what} names %APPDATA%\\OneLaunch, the real data folder; use a scratch folder`);
  }
  if (flat.some((reading) => KEY_NAMES.some((pattern) => pattern.test(reading)))) {
    throw new Refusal('key', `the ${what} names the app's key folder or a .pem file`);
  }
}

function judge({ tool, command: text, cwd, io }) {
  if (tool !== 'Bash' && tool !== 'PowerShell') throw new Error(`no rules for the tool ${tool}`);
  if (typeof text !== 'string') throw new Error('the call holds no command text');
  const state = { cwd: cwd ? path.resolve(cwd) : null, before: 0, count: 0, commits: 0, behind: null, registry: false, gitEnv: false, redefined: false, io };
  try {
    const flat = flatten(text);
    named(text, 'command');
    state.redefined = REDEFINES.test(flat[0]);
    walk(text, tool === 'PowerShell' ? 'powershell' : 'bash', state, 0);
    if (flat.some((reading) => TOKEN_SHOWN.test(reading))) {
      throw new Refusal('key', 'the command names GIT_TRACE_REDACT or a GIT_CONFIG variable, which can make git print the app\'s token or hand it to another helper');
    }
    if (state.registry && REGISTRY_PATH.test(flat[0])) {
      throw new Refusal('registry', 'this call names a registry key and runs a command that can write one; read the registry in a call of its own');
    }
    return null;
  } catch (error) {
    if (error instanceof Refusal) return error;
    throw error;
  }
}

module.exports = { judge, named };
