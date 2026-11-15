'use strict';

const { Refusal } = require('./rules.cjs');

const MARK = '\uE000';
const LEADING = new Set(['if', 'then', 'else', 'elif', 'while', 'until', 'do', '!', 'time', 'coproc']);
const DEPTH = 200;

function lex(source, dialect, base = 0, body = false) {
  const bash = dialect === 'bash';
  const ps = dialect === 'powershell';
  const cmd = dialect === 'cmd';
  const src = ps
    ? source
        .replace(/[\u2018-\u201B]/g, "'")
        .replace(/[\u201C-\u201E]/g, '"')
        .replace(/[\u2013-\u2015]/g, '-')
        .replace(/[\u00A0\u0085\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000\f\v]/g, ' ')
    : source;
  const n = src.length;
  const escape = bash ? '\\' : ps ? '`' : '^';
  const commands = [];
  const heredocs = [];
  let i = 0;
  let level = 0;
  let math = 0;
  let cases = 0;
  let table = false;
  let assigned = 0;
  let piped = false;
  let glued = false;
  let redirect = false;
  let words = [];
  let word = null;

  const open = () => (word ??= { text: '', alt: '', quoted: false, dynamic: false, at: base + i });
  const add = (text, alt = text) => {
    const w = open();
    w.text += text;
    w.alt += alt;
  };
  const mark = () => {
    open().dynamic = true;
    add(MARK);
  };
  const keyword = () => {
    if (!bash || !word || word.counted || word.quoted || !words.every((before) => !before.quoted && LEADING.has(before.text))) return;
    if (word.text === 'case') cases++;
    else if (word.text === 'esac' && cases) cases--;
    else return;
    word.counted = true;
  };
  const endWord = () => {
    if (!word) return;
    if (redirect) {
      redirect = false;
      word = null;
      return;
    }
    keyword();
    if (word.alt === word.text) delete word.alt;
    words.push(word);
    word = null;
  };
  const endCommand = (pipe = false) => {
    endWord();
    if (words.length) commands.push({ words, at: words[0].at, level, piped: piped || pipe });
    words = [];
    piped = pipe;
    glued = false;
    redirect = false;
    assigned = 0;
  };
  const nested = (closer, keyed = false) => {
    if (level >= DEPTH) throw new Refusal('unreadable', 'brackets are nested too deep for the hook to read');
    const outer = [words, word, piped, cases, table, assigned, redirect];
    const before = commands.length;
    words = [];
    word = null;
    piped = false;
    cases = 0;
    table = keyed;
    assigned = 0;
    redirect = false;
    level++;
    run(closer);
    endCommand();
    level--;
    [words, word, piped, cases, table, assigned, redirect] = outer;
    return commands.length > before;
  };
  const assignment = () => {
    const c = src[i];
    const length = c === '=' ? 1 : '+-*/%'.includes(c) && src[i + 1] === '=' ? 2 : 0;
    const first = words[assigned] ?? word;
    if (!length || !first) return false;
    const key = table && assigned === 0 && (words.length ? words.length === 1 && !word : true);
    if (!key && first.text[0] !== MARK && first.text[0] !== '[') return false;
    endWord();
    words.push({ text: '=', quoted: false, dynamic: false, assign: true, at: base + i });
    assigned = words.length;
    i += length;
    return true;
  };
  const inner = (text, as, at, asBody) => {
    for (const command of lex(text, as, at, asBody)) commands.push({ ...command, level: command.level + level + 1 });
  };

  const dollar = (quoted) => {
    const next = src[i + 1];
    if (next === '(') {
      const arithmetic = src[i + 2] === '(';
      i += 2;
      mark();
      if (arithmetic) math++;
      nested(')');
      if (arithmetic) math--;
      return true;
    }
    if (next === '{') {
      const end = src.indexOf('}', i + 2);
      const inside = src.slice(i + 2, end < 0 ? n : end);
      if (end < 0 || (bash ? /['"`$\\{]/ : /['"`${]/).test(inside)) {
        throw new Refusal('unreadable', 'a ${...} expansion with a quote or another expansion inside it hides what follows from the hook');
      }
      i = end + 1;
      mark();
      return true;
    }
    if (bash && !quoted && next === '"') {
      i++;
      return true;
    }
    const name = (ps ? /^\$[\w:?^$]+/ : /^\$(?:\w+|[?!#@*$-])/).exec(src.slice(i, i + 256));
    if (!name) return false;
    i += name[0].length;
    mark();
    return true;
  };

  const single = () => {
    open().quoted = true;
    for (;;) {
      const end = src.indexOf("'", i);
      if (end < 0) {
        add(src.slice(i));
        i = n;
        return;
      }
      add(src.slice(i, end));
      i = end + 1;
      if (!ps || src[i] !== "'") return;
      add("'");
      i++;
    }
  };

  const ansi = () => {
    let text = '';
    let escaped = false;
    while (i < n && src[i] !== "'") {
      if (src[i] === '\\') {
        escaped = true;
        i++;
      }
      text += src[i++] ?? '';
    }
    i++;
    open().quoted = true;
    if (escaped) mark();
    else add(text);
  };

  const backtick = () => {
    const from = i;
    let text = '';
    while (i < n && src[i] !== '`') {
      if (src[i] === '\\' && '`\\$'.includes(src[i + 1])) i++;
      text += src[i++] ?? '';
    }
    i++;
    mark();
    inner(text, 'bash', base + from, false);
  };

  const double = (whole) => {
    open().quoted = true;
    while (i < n) {
      const c = src[i];
      const next = src[i + 1];
      if (c === '"' && !whole) {
        if (!bash && next === '"') {
          add('"');
          i += 2;
          continue;
        }
        i++;
        return;
      }
      if (c === escape && !cmd && next !== undefined) {
        if (bash && !'"\\$`\n'.includes(next)) {
          add(c);
          i++;
          continue;
        }
        if (next !== '\n') add(next);
        i += 2;
        continue;
      }
      if (c === '$' && !cmd && dollar(true)) continue;
      if (c === '`' && bash) {
        i++;
        backtick();
        continue;
      }
      add(c);
      i++;
    }
  };

  const hereString = () => {
    const quote = src[i + 1];
    const opening = /^[ \t]*\r?\n/.exec(src.slice(i + 2, i + 130));
    if (!opening) return false;
    const from = i + 2 + opening[0].length;
    const end = src.indexOf(`\n${quote}@`, from - 1);
    const text = src.slice(from, Math.max(from, end < 0 ? n : end)).replace(/\r$/, '');
    i = end < 0 ? n : end + 3;
    open().quoted = true;
    if (quote === '"' && text.includes('$')) {
      mark();
      inner(text, 'powershell', base + from, true);
    } else {
      add(text);
    }
    return true;
  };

  const heredoc = () => {
    i += 2;
    const strip = src[i] === '-';
    if (strip) i++;
    while (src[i] === ' ' || src[i] === '\t') i++;
    let delimiter = '';
    let literal = false;
    while (i < n && !/[\s;&|<>()]/.test(src[i])) {
      const c = src[i];
      if (c === "'" || c === '"') {
        literal = true;
        const end = src.indexOf(c, i + 1);
        delimiter += src.slice(i + 1, end < 0 ? n : end);
        i = end < 0 ? n : end + 1;
      } else if (c === '\\') {
        literal = true;
        delimiter += src[i + 1] ?? '';
        i += 2;
      } else {
        delimiter += c;
        i++;
      }
    }
    endWord();
    heredocs.push({ delimiter, literal, strip });
  };

  const bodies = () => {
    for (const { delimiter, literal, strip } of heredocs.splice(0)) {
      const from = i;
      let text = '';
      while (i < n) {
        let end = src.indexOf('\n', i);
        if (end < 0) end = n;
        const line = src.slice(i, end).replace(/\r$/, '');
        i = Math.min(end + 1, n);
        if ((strip ? line.replace(/^\t+/, '') : line) === delimiter) break;
        text += `${line}\n`;
      }
      if (!literal) inner(text, 'bash', base + from, true);
    }
  };

  const run = (closer) => {
    while (i < n) {
      const c = src[i];
      const next = src[i + 1];
      if (c === ')') keyword();
      if (c === closer && !(c === ')' && cases)) {
        i++;
        return;
      }
      if (c === '\n') {
        i++;
        endCommand();
        if (heredocs.length) bodies();
        continue;
      }
      if (c === '\r') {
        i++;
        if (next !== '\n') endCommand();
        continue;
      }
      if (c === ' ' || c === '\t' || (cmd && ',;='.includes(c))) {
        i++;
        endWord();
        glued = false;
        continue;
      }
      if (c === escape) {
        if (next === '\n') i += 2;
        else if (next === '\r' && src[i + 2] === '\n') i += 3;
        else {
          if (next !== undefined) add(next, bash ? c + next : next);
          i += 2;
        }
        continue;
      }
      if (c === '#' && !word && !glued && !cmd) {
        const end = src.indexOf('\n', i);
        i = end < 0 ? n : end;
        continue;
      }
      if (ps && !word && !glued && c === '<' && next === '#') {
        const end = src.indexOf('#>', i + 2);
        i = end < 0 ? n : end + 2;
        continue;
      }
      if (ps && !word && !glued && c === '@' && (next === "'" || next === '"') && hereString()) continue;
      if (ps && assignment()) continue;
      if (c === "'" && !cmd) {
        i++;
        single();
        continue;
      }
      if (c === '"') {
        i++;
        double(false);
        continue;
      }
      if (c === '$' && !cmd) {
        if (bash && next === "'") {
          i += 2;
          ansi();
          continue;
        }
        if (dollar(false)) continue;
      }
      if (c === '`' && bash) {
        i++;
        backtick();
        continue;
      }
      if (c === '(') {
        const subshell = bash && !word && !words.length;
        const arithmetic = bash && next === '(';
        i++;
        if (arithmetic) math++;
        const some = nested(')');
        if (arithmetic) math--;
        if (bash && cases) endCommand();
        else if (some && !subshell && !cmd) mark();
        continue;
      }
      if (c === ')' || (ps && c === '}')) {
        i++;
        endCommand();
        continue;
      }
      if (ps && c === '{') {
        i++;
        nested('}', Boolean(word?.text.endsWith('@')));
        open().block = true;
        continue;
      }
      if (bash && (c === '{' || c === '}') && !word && (next === undefined || /[\s;]/.test(next))) {
        i++;
        endCommand();
        continue;
      }
      if (c === ';') {
        i++;
        endCommand();
        continue;
      }
      if (c === '|') {
        const or = next === '|';
        i += or || (bash && next === '&') ? 2 : 1;
        endCommand(!or);
        continue;
      }
      if (c === '&') {
        const previous = src[i - 1];
        if (previous === '>' || previous === '<' || (bash && next === '>')) {
          i++;
        } else if (next === '&') {
          i += 2;
          endCommand();
        } else if (ps && !word && !words.length) {
          words.push({ text: '&', quoted: false, dynamic: false, op: true, at: base + i });
          i++;
        } else {
          i++;
          endCommand();
        }
        continue;
      }
      if (c === '<' || c === '>') {
        if (bash && c === '<' && next === '<' && src[i + 2] !== '<') {
          if (math) throw new Refusal('unreadable', 'a << inside (( )) is a shift to one reading and a here-document to another');
          heredoc();
          continue;
        }
        const stream = word !== null && !word.quoted && /^[\d*]$/.test(word.text);
        const middle = ps && word !== null && !stream;
        if (stream) word = null;
        else endWord();
        if (middle) glued = true;
        else redirect = true;
        i++;
        continue;
      }
      add(c);
      i++;
    }
  };

  if (body) {
    double(true);
    word = null;
  } else {
    run(undefined);
    endCommand();
  }
  return commands.sort((a, b) => a.at - b.at);
}

module.exports = { lex, MARK, LEADING };
