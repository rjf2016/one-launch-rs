// Just enough of Unreal's GameUserSettings.ini: [Section] headers and key=value lines, with CRLF or LF. A rewrite touches the lines it is asked to and keeps every other byte: line ends, blank lines between sections, the order of the sections and a byte order mark.

use crate::js;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    CrLf,
    Lf,
}

impl Eol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CrLf => "\r\n",
            Self::Lf => "\n",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct IniLayout {
    pub lines: Vec<String>,
    pub eol: Eol,
    pub trailing_newline: bool,
}

pub fn split_ini(text: &str) -> IniLayout {
    let eol = if text.contains("\r\n") {
        Eol::CrLf
    } else {
        Eol::Lf
    };
    let trailing_newline = text.ends_with('\n');
    let mut body = text.to_owned();
    if trailing_newline {
        body.pop();
        if body.ends_with('\r') {
            body.pop();
        }
    }
    let lines = if body.is_empty() {
        Vec::new()
    } else {
        js::lines(&body).map(str::to_owned).collect()
    };
    IniLayout {
        lines,
        eol,
        trailing_newline,
    }
}

pub fn join_ini(layout: &IniLayout) -> String {
    let mut text = layout.lines.join(layout.eol.as_str());
    if layout.trailing_newline || layout.lines.is_empty() {
        text.push_str(layout.eol.as_str());
    }
    text
}

const fn ends_a_line(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

// The name between the brackets of a header line. A byte order mark may stand before the bracket: the header line is never rewritten, so the mark stays.
fn header(line: &str) -> Option<&str> {
    let line = line.strip_prefix('\u{feff}').unwrap_or(line);
    let inner = line.strip_prefix('[')?;
    let close = inner.rfind(']')?;
    let name = &inner[..close];
    let closed = inner[close + 1..].chars().all(js::is_whitespace);
    (closed && !name.is_empty() && !name.chars().any(ends_a_line)).then_some(name)
}

// The key and the value of an entry line. A key starts with something that is neither blank nor an equals sign.
fn entry(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let first = key.chars().next()?;
    (!js::is_whitespace(first) && !value.chars().any(ends_a_line)).then_some((key, value))
}

// No Debug: a section can be named after an account's id.
#[derive(Clone, PartialEq, Eq)]
pub struct IniSection {
    pub name: String,
    // The header's line.
    pub start: usize,
    // One past the section's last line: the next header, or the end.
    pub end: usize,
}

pub fn find_sections(lines: &[String]) -> Vec<IniSection> {
    let mut sections: Vec<IniSection> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(name) = header(line) else {
            continue;
        };
        if let Some(previous) = sections.last_mut() {
            previous.end = index;
        }
        sections.push(IniSection {
            name: name.to_owned(),
            start: index,
            end: lines.len(),
        });
    }
    sections
}

pub fn section_names(text: &str) -> Vec<String> {
    find_sections(&split_ini(text).lines)
        .into_iter()
        .map(|section| section.name)
        .collect()
}

// The value of a key in the first section of this name, the last line of the key winning. None when the section or the key is not there.
pub fn read_value(text: &str, section: &str, key: &str) -> Option<String> {
    let lines = split_ini(text).lines;
    let found = find_sections(&lines)
        .into_iter()
        .find(|candidate| candidate.name == section)?;
    lines[found.start + 1..found.end]
        .iter()
        .rev()
        .filter_map(|line| entry(line))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.to_owned())
}

// One key of a rewrite. No Debug: a value can be a saved sign-in.
#[derive(Clone, PartialEq, Eq)]
pub struct IniChange {
    pub key: String,
    // The line in the text as it stood when this change was made, counted from 1.
    pub line: usize,
    pub before: Option<String>,
    pub after: Option<String>,
}

pub struct IniRewrite {
    pub text: String,
    pub changes: Vec<IniChange>,
    pub noop: bool,
}

// Sets (a value) or removes (None) keys of one section, in the order given, and makes the section at the end of the file when it is not there. A key that is there is replaced where it stands; a new one goes after the section's last entry, before its blank lines. The values come back as they are: the caller masks them before anyone sees them.
pub fn rewrite_section(text: &str, name: &str, entries: &[(&str, Option<&str>)]) -> IniRewrite {
    let layout = split_ini(text);
    let mut lines = layout.lines.clone();
    let mut changes = Vec::new();
    let found = find_sections(&lines)
        .into_iter()
        .find(|section| section.name == name);
    let (start, mut end) = match found {
        Some(section) => (section.start, section.end),
        None => {
            if entries.iter().all(|(_, value)| value.is_none()) {
                return IniRewrite {
                    text: text.to_owned(),
                    changes,
                    noop: true,
                };
            }
            if lines.last().is_some_and(|last| !js::trim(last).is_empty()) {
                lines.push(String::new());
            }
            let start = lines.len();
            lines.push(format!("[{name}]"));
            lines.push(String::new());
            (start, lines.len())
        }
    };

    for (key, value) in entries {
        let existing: Vec<usize> = (start + 1..end)
            .filter(|index| entry(&lines[*index]).is_some_and(|(found, _)| found == *key))
            .collect();
        let value_at = |lines: &[String], index: usize| lines[index][key.len() + 1..].to_owned();
        let Some(value) = value else {
            for index in existing.into_iter().rev() {
                changes.push(IniChange {
                    key: (*key).to_owned(),
                    line: index + 1,
                    before: Some(value_at(&lines, index)),
                    after: None,
                });
                lines.remove(index);
                end -= 1;
            }
            continue;
        };
        let line = format!("{key}={value}");
        if let Some((first, repeats)) = existing.split_first() {
            let before = value_at(&lines, *first);
            if before != *value {
                changes.push(IniChange {
                    key: (*key).to_owned(),
                    line: first + 1,
                    before: Some(before),
                    after: Some((*value).to_owned()),
                });
            }
            lines[*first] = line;
            for index in repeats.iter().rev() {
                lines.remove(*index);
                end -= 1;
            }
            continue;
        }
        let mut at = end;
        while at > start + 1 && js::trim(&lines[at - 1]).is_empty() {
            at -= 1;
        }
        lines.insert(at, line);
        end += 1;
        changes.push(IniChange {
            key: (*key).to_owned(),
            line: at + 1,
            before: None,
            after: Some((*value).to_owned()),
        });
    }

    let next = join_ini(&IniLayout { lines, ..layout });
    IniRewrite {
        noop: next == text,
        text: next,
        changes,
    }
}
