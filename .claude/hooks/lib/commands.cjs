'use strict';

const os = require('node:os');
const path = require('node:path');
const { Refusal } = require('./rules.cjs');
const { lex, MARK } = require('./shell.cjs');

const set = (names) => new Set(names.split(' '));

const PROCESS = set(
  'tasklist taskkill tskill wmic qprocess pslist pskill pssuspend get-process gps ps stop-process spps kill wait-process debug-process pkill pgrep killall pidof top htop gcim gwmi iwmi icim rwmi swmi rcim scim ncim',
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
  'vgtray',
]);
const PROTOCOLS = /^(steam|com\.epicgames\.launcher|battlenet|riotclient):\/\//i;
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
const GIT_NETWORK = set('push fetch pull clone ls-remote send-pack fetch-pack http-fetch http-push request-pull svn lfs send-email imap-send');
const GITHUB_HOST = /github(usercontent)?\.com|ghcr\.io/i;
const HTTP_TOOLS = set('curl wget http https invoke-restmethod irm invoke-webrequest iwr start-bitstransfer bitsadmin certutil aria2c');
const SSH_TOOLS = set('ssh scp sftp plink pscp psftp');
const CARGO_DEPS = set('add update install remove rm upgrade generate-lockfile binstall');
const SHELLS = set('bash sh zsh dash ash ksh');
const POWERSHELLS = set('pwsh powershell pwsh-preview');
const POWERSHELL_VALUES = set('executionpolicy ex ep workingdirectory wd windowstyle w configurationname config settingsfile settings custompipename inputformat if outputformat of o psconsolefile');
const WRAPPERS = set(
  'env command builtin exec nohup nice time timeout stdbuf setsid winpty xargs watch sudo doas runas start start-process saps explorer rundll32 invoke-item ii cygstart conhost wt psexec schtasks forfiles call wsl busybox rustup set-alias new-alias sal nal find parallel start-job sajb start-threadjob invoke-command icm',
);
const BASH_WORDS = set('if then else elif while until do ! time coproc');
const ASSIGNS = set('= += -= *= /= %= ??= in');
const COMMIT_PLAIN = set(
  '--amend --no-edit --edit --quiet --verbose --signoff --no-signoff --allow-empty --allow-empty-message --reset-author --gpg-sign --no-gpg-sign --dry-run --short --branch --porcelain --long --null --status --no-status --no-post-rewrite',
);
const COMMIT_VALUES = set('--message --file --author --date --cleanup --trailer --reuse-message --reedit-message --fixup --squash --template');
const DATA_FOLDER = [/(?<!local)appdata[^a-z0-9]{0,12}(?:roaming[^a-z0-9]{0,12})?onelaunch/, /roaming[^a-z0-9]{0,12}onelaunch/];
const KEY_NAMES = [/one-launch-rs-bot/, /one_launch_agent_key_dir/, /\.pem(?![a-z0-9])/];
const BUDGET = 4000;
const DEPTH = 8;

function flatten(text) {
  const lower = text.toLowerCase().replace(/["'`^‘-„]/g, '');
  const tidy = (flat) => {
    let out = flat.replace(/\/+/g, '/').replace(/\/\.(?=\/)/g, '');
    for (let before = ''; before !== out; ) {
      before = out;
      out = out.replace(/(?!\.\.\/)[^/\s]+\/\.\.(\/|(?=\s|$))/, '');
    }
    return out;
  };
  return [tidy(lower.replace(/\\/g, '/')), tidy(lower.replace(/\\/g, ''))];
}

function identify(text) {
  const flat = text.replace(/\\/g, '/');
  const cut = flat.lastIndexOf('/');
  const leaf = flat
    .slice(cut + 1)
    .toLowerCase()
    .replace(/[. ]+$/, '');
  const exe = /\.(exe|com|cmd|bat)$/.exec(leaf);
  const name = exe ? leaf.slice(0, exe.index).replace(/[. ]+$/, '') : leaf;
  return { name, leaf, ext: exe ? exe[1] : '', pathed: cut >= 0 };
}

function computed(word) {
  const leaf = word.text.replace(/\\/g, '/').split('/').pop();
  if (leaf.includes(MARK)) return true;
  return leaf.length > 1 && !/^\[+$/.test(leaf) && /[*?]|\[.+\]|\{.*,.*\}/.test(leaf);
}

function into(cwd, word, dialect) {
  if (!word || word.dynamic || !word.text) return null;
  let target = word.text;
  if (target === '~' || /^~[\\/]/.test(target)) target = path.join(os.homedir(), target.slice(1));
  else if (target.startsWith('~')) return null;
  if (dialect === 'bash') {
    target = target.replace(/^\/([A-Za-z])(?=\/|$)/, '$1:');
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
      if (computed(rest[0])) throw new Refusal('unreadable', 'the command after the call operator is computed at run time');
      return rest;
    }
    const text = first.text;
    const expression = first.quoted || first.block || /^[$[@!+-]/.test(text) || text.startsWith(MARK) || /^\d+(\.\d+)?$/.test(text);
    const assign = rest.findIndex((word, k) => k > 0 && !word.quoted && ASSIGNS.has(word.text.toLowerCase()));
    if (expression || assign === 1) {
      return assign < 0 || assign + 1 >= rest.length ? null : head(rest.slice(assign + 1), dialect, mode, state);
    }
    if (/^(return|throw)$/i.test(text)) return rest.length > 1 ? head(rest.slice(1), dialect, mode, state) : null;
  }
  if (dialect !== 'powershell') {
    for (;;) {
      const word = rest[0];
      if (!word) return null;
      const assign = /^([A-Za-z_]\w*)\+?=/.exec(word.text);
      if (assign && assign[1].startsWith('GIT_')) state.gitEnv = true;
      if (!assign && (word.quoted || !BASH_WORDS.has(word.text))) break;
      rest = rest.slice(1);
    }
  }
  if (dialect === 'cmd' && rest[0].text.startsWith('@')) {
    const text = rest[0].text.replace(/^@+/, '');
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
  for (const { words, level, piped } of lex(text, dialect)) {
    if (dialect === 'powershell' && words.some((word) => word.text === '--%' && !word.quoted)) {
      throw new Refusal('unreadable', 'the stop-parsing token --% hides the rest of the line from the hook');
    }
    const moved = command(words, dialect, state, depth, 'normal', level === 0 && !piped);
    if (dialect === 'cmd') for (let k = 1; k < words.length; k++) command(words.slice(k), dialect, state, depth, 'suffix', false);
    if (!moved) state.before++;
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

function suffixes(args, dialect, state, depth) {
  const flat = args.flatMap((word) =>
    /\s/.test(word.text)
      ? word.text
          .split(/\s+/)
          .filter(Boolean)
          .map((text) => ({ ...word, text, alt: undefined }))
      : [word],
  );
  for (let k = 0; k < flat.length; k++) command(flat.slice(k), dialect, state, depth + 1, 'suffix', false);
}

function denied(id, args, lower) {
  const { name, leaf, ext } = id;
  if (PROCESS.has(name) || CIM.test(name) || (name === 'query' && lower[0] === 'process')) {
    throw new Refusal('process', `${name} lists or ends processes, and no form of a process tool is allowed`);
  }
  if (LAUNCHERS.has(name)) {
    throw new Refusal('launcher', 'this would start a launcher or a Riot game; the owner performs every real launch');
  }
  const named = (ext || !/\.[a-z0-9]{1,5}$/.test(name)) && name !== 'install' && INSTALLER_NAME.test(name);
  if (INSTALLERS.has(name) || PACKAGES.test(leaf) || named || (PACKAGE_MANAGERS.has(name) && lower.some((arg) => PACKAGE_VERBS.has(arg)))) {
    throw new Refusal('installer', 'this would start an installer; installers run on GitHub-hosted runners only');
  }
  if (name === 'onelaunch' && !args.some((arg) => /^--data-dir=./i.test(arg.text))) {
    throw new Refusal('app', 'the app binary needs --data-dir=<scratch folder> in that exact form, which the Electron build and the Rust build both read');
  }
  if (name === 'reg' && lower[0] && !lower[0].startsWith('/') && !REG_READS.has(lower[0])) {
    throw new Refusal('registry', `reg ${lower[0]} changes the registry; only reg query, compare, export and save are allowed`);
  }
  if (/^(regedit|regedt32|regini)$/.test(name) || leaf.endsWith('.reg')) {
    throw new Refusal('registry', 'this changes the registry');
  }
  if (SSH_TOOLS.has(name)) {
    throw new Refusal('github', `${name} would use the owner's SSH key; reach GitHub only through scripts/agent/git.ps1 and scripts/agent/gh.ps1`);
  }
}

function move(args, state, sure, dialect) {
  const target = args.find((arg) => arg.quoted || !(arg.text.startsWith('-') || /^\/d$/i.test(arg.text)));
  if (target && REGISTRY_PATH.test(target.text.toLowerCase())) {
    throw new Refusal('registry', 'a registry key as the working location hides which key a later command writes; name the key in full on a read');
  }
  state.cwd = sure ? into(state.cwd, target, dialect) : null;
  return true;
}

function config(pair) {
  const key = pair.split('=')[0].toLowerCase();
  if (key === 'core.hookspath') throw new Refusal('bypass', 'core.hooksPath moves the commit hooks away');
  if (key.startsWith('alias.')) throw new Refusal('unreadable', 'a git alias given on the command line runs what the hook cannot read');
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
    if (/^--for/.test(text) || text === '--mirror') throw new Refusal('bypass', 'a force-push rewrites what is on GitHub');
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
  if (elsewhere || state.gitEnv) {
    throw new Refusal('secrets', 'the commit names its own git folder or index (--git-dir, --work-tree, GIT_*), which the scan cannot follow');
  }
  if (cwd === null) throw new Refusal('secrets', 'the hook cannot tell which checkout this commit is in; name it with -C <folder>');
  const findings = state.io.commit(cwd, broad);
  if (findings.length) {
    const shown = findings.slice(0, 20).join('\n  ');
    throw new Refusal('secrets', `the commit would hold ${findings.length} finding(s) of the secret scan:\n  ${shown}\nUse a placeholder, or ask the owner to add an exact value to xtask/secrets.allow`);
  }
}

function git(args, state, wrapper, dialect) {
  let cwd = state.cwd;
  let elsewhere = false;
  let k = 0;
  for (; k < args.length; k++) {
    const text = args[k].text;
    if (text === '-C') cwd = into(cwd, args[++k], dialect);
    else if (text === '-c' || text === '--config-env') config(args[++k]?.text ?? '');
    else if (/^--config-env=/.test(text)) config(text.slice(13));
    else if (/^--(git-dir|work-tree)/.test(text)) {
      elsewhere = true;
      if (!text.includes('=')) k++;
    } else if (/^--(namespace|super-prefix)$/.test(text)) k++;
    else if (!text.startsWith('-')) break;
  }
  const sub = (args[k]?.text ?? '').toLowerCase();
  const texts = args.slice(k + 1).map((arg) => arg.text);
  if (sub === 'tag') throw new Refusal('tags', 'git tag: tags are the owner\'s; read them with git for-each-ref refs/tags');
  if (/^(update|symbolic)-ref$/.test(sub) && texts.some((text) => /refs\/tags/i.test(text))) throw new Refusal('tags', 'this writes a tag ref');
  if (texts.some((text) => text.length >= 9 && '--no-verify'.startsWith(text))) throw new Refusal('bypass', '--no-verify skips the hooks');
  if (sub === 'config' && texts.some((text) => /core\.hookspath/i.test(text))) throw new Refusal('bypass', 'core.hooksPath moves the commit hooks away');
  if (sub.startsWith('credential')) throw new Refusal('key', `git ${sub} hands out a stored login or the app's token`);
  if (sub === 'push') push(texts, cwd, state, wrapper);
  if (!wrapper && reaches(sub, texts)) {
    throw new Refusal('github', `plain git ${sub} acts with the owner's login; use scripts/agent/git.ps1 ${sub}`);
  }
  if (sub === 'commit') commit(texts, cwd, elsewhere, state);
}

function gh(args, wrapper) {
  const texts = args.map((arg) => arg.text.toLowerCase());
  const plain = [];
  for (let k = 0; k < texts.length; k++) {
    if (/^(-r|--repo|--hostname)$/.test(texts[k])) k++;
    else if (!texts[k].startsWith('-')) plain.push(texts[k]);
  }
  const [group, verb] = plain;
  const all = texts.join(' ');
  if (group === 'release') throw new Refusal('tags', 'gh release: releases are the owner\'s');
  if (group === 'pr' && verb === 'merge' && (!texts.some((text) => text === '--auto' || text === '--auto=true') || texts.some((text) => text.startsWith('--admin')))) {
    throw new Refusal('merge', 'gh pr merge needs --auto and no --admin, so that GitHub merges once the checks pass');
  }
  if (group === 'api' && /pulls\/[^/\s]+\/merge|mergepullrequest|mergebranch/.test(all)) {
    throw new Refusal('merge', 'this API call merges a pull request');
  }
  if (group === 'api' && /\/releases|git\/tags|refs\/tags|createrelease|createref/.test(all)) {
    throw new Refusal('tags', 'this API call reaches tags or releases');
  }
  if (group === 'auth') {
    throw wrapper ? new Refusal('key', 'gh auth through the wrapper would show or store the app\'s token') : new Refusal('github', 'gh auth works on the owner\'s login');
  }
  if (!wrapper) throw new Refusal('github', 'plain gh acts with the owner\'s login, reads included; use scripts/agent/gh.ps1');
}

function cargo(args) {
  const texts = args.map((arg) => arg.text);
  let k = 0;
  for (; k < texts.length; k++) {
    if (/^(--config|-C|-Z|--color|--explain)$/.test(texts[k])) k++;
    else if (!/^[-+]/.test(texts[k])) break;
  }
  const sub = (texts[k] ?? '').toLowerCase();
  if (CARGO_DEPS.has(sub)) throw new Refusal('deps', `cargo ${sub} changes the dependency set or installs a tool`);
  if (sub !== 'run' && sub !== 'r') return;
  const rest = args.slice(k + 1);
  const split = rest.findIndex((arg) => arg.text === '--');
  const own = (split < 0 ? rest : rest.slice(0, split)).map((arg) => arg.text.toLowerCase());
  const app = split < 0 ? [] : rest.slice(split + 1);
  const folder = app.some((arg, at) => /^--data-dir=./i.test(arg.text) || (/^--data-dir$/i.test(arg.text) && app[at + 1]?.text && !app[at + 1].text.startsWith('-')));
  if (folder) return;
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
  for (let k = 0; k < args.length; k++) {
    const text = args[k].text;
    if (text === '--version' || text === '--help') return;
    if (!args[k].quoted && /^-[A-Za-z]*c[A-Za-z]*$/.test(text)) {
      const script = args.slice(k + 1).find((arg) => arg.quoted || !/^[-+]/.test(arg.text));
      if (!script) break;
      scoped(state, () => walk(script.text, 'bash', state, depth + 1));
      return;
    }
    if (/^([-+][oO]|--rcfile|--init-file)$/.test(text)) k++;
    else if (args[k].quoted || !/^[-+]/.test(text)) return;
  }
  throw new Refusal('unreadable', `${name} with no command and no script file takes its commands from standard input`);
}

function powershell(name, args, state, depth, dialect) {
  let informational = false;
  let positional = false;
  for (let k = 0; k < args.length; k++) {
    const arg = args[k];
    const flag = !arg.quoted && !arg.block && /^[-/–-―]([A-Za-z]+:?|\?)$/.test(arg.text) ? arg.text.slice(1).toLowerCase().replace(/:$/, '') : null;
    if (flag === null) {
      positional ||= arg.text !== '-';
      continue;
    }
    const rest = args.slice(k + 1);
    if ('encodedcommand'.startsWith(flag) || flag === 'ec' || flag === 'ea' || (flag.length > 7 && 'encodedarguments'.startsWith(flag))) {
      throw new Refusal('unreadable', 'an encoded PowerShell command');
    }
    if ('command'.startsWith(flag) || flag === 'cwa' || (flag.length > 7 && 'commandwithargs'.startsWith(flag))) {
      if (!rest.length || rest[0].text === '-') break;
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
      if (flag === 'wd' || (flag.length > 1 && 'workingdirectory'.startsWith(flag))) state.cwd = into(state.cwd, rest[0], dialect);
      k++;
    }
  }
  suffixes(args, 'powershell', state, depth);
  if (!informational && !positional) {
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
  for (const candidate of first.alt ? [id, identify(first.alt)] : [id]) {
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
    if (name === 'git.ps1') git(args, state, true, dialect);
    else gh(args, true);
    return false;
  }
  if (argument) return false;

  if (REGISTRY_WRITERS.has(name)) state.registry = true;
  if (name === 'new-psdrive' && lower.some((text) => /registry|^hk(cu|lm|cr|u|cc)|^hkey_/.test(text))) {
    throw new Refusal('registry', 'a new drive on the registry hides which key a later command writes');
  }
  if (MOVES.has(name)) return move(args, state, sure, dialect);
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
    walk(given[0].text, 'powershell', state, depth + 1);
  } else if (name === 'eval' || name === 'trap') {
    const given = name === 'trap' ? args.filter((arg) => arg.quoted || !arg.text.startsWith('-')).slice(0, 1) : args;
    if (given.some((arg) => arg.dynamic)) throw new Refusal('unreadable', `${name} runs text the hook cannot read`);
    walk(given.map((arg) => arg.text).join(' '), 'bash', state, depth + 1);
  } else if (name === 'alias' && dialect !== 'powershell') {
    for (const text of texts) if (text.includes('=')) walk(text.slice(text.indexOf('=') + 1), 'bash', state, depth + 1);
  } else if (WRAPPERS.has(name)) suffixes(args, dialect, state, depth);
  return false;
}

function judge({ tool, command: text, cwd, io }) {
  if (tool !== 'Bash' && tool !== 'PowerShell') throw new Error(`no rules for the tool ${tool}`);
  if (typeof text !== 'string') throw new Error('the call holds no command text');
  const state = { cwd: cwd ? path.resolve(cwd) : null, before: 0, count: 0, registry: false, gitEnv: false, io };
  try {
    const flat = flatten(text);
    if (flat.some((reading) => DATA_FOLDER.some((pattern) => pattern.test(reading)))) {
      throw new Refusal('data', 'the command names %APPDATA%\\OneLaunch, the real data folder; use a scratch folder');
    }
    if (flat.some((reading) => KEY_NAMES.some((pattern) => pattern.test(reading)))) {
      throw new Refusal('key', 'the command names the app\'s key folder or a .pem file');
    }
    walk(text, tool === 'PowerShell' ? 'powershell' : 'bash', state, 0);
    if (state.registry && REGISTRY_PATH.test(flat[0])) {
      throw new Refusal('registry', 'this call names a registry key and runs a command that can write one; read the registry in a call of its own');
    }
    return null;
  } catch (error) {
    if (error instanceof Refusal) return error;
    throw error;
  }
}

module.exports = { judge };
