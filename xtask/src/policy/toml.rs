use std::sync::LazyLock;

use regex::Regex;

static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    let key = r"(?:[A-Za-z0-9_-]+|'cfg\([^']*\)')";
    Regex::new(&format!(
        r"^(\[{{1,2}}) *({key}(?:\.{key})*) *(\]{{1,2}}) *(?:#.*)?$"
    ))
    .expect("a fixed pattern")
});
static ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*) *= *(\S.*)$").expect("a fixed pattern")
});

#[derive(Debug, PartialEq)]
pub struct Line<'a> {
    pub number: usize,
    pub table: &'a str,
    pub array: bool,
    pub entry: Option<(&'a str, &'a str)>,
}

// No TOML parser is approved, and TOML spells one thing many ways. A file is read only in the spelling where every line stands alone (a comment, a table name with plain keys, or one `key = value` that ends on its line), so a line can hide nothing from the next. The number of the first line outside that spelling is the error.
pub fn lines(text: &str) -> Result<Vec<Line<'_>>, usize> {
    let (mut table, mut array) = ("", false);
    let mut lines = Vec::new();
    for (index, line) in text.lines().map(str::trim).enumerate() {
        let number = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let header = HEADER.captures(line).and_then(|found| {
            let (open, name, close) = (found.get(1)?, found.get(2)?, found.get(3)?);
            (open.len() == close.len()).then(|| (name.as_str(), open.len() == 2))
        });
        let entry = ENTRY.captures(line).and_then(|found| {
            let (key, value) = (found.get(1)?.as_str(), found.get(2)?.as_str());
            closed(value).then_some((key, value))
        });
        match (header, entry) {
            (Some(named), _) => {
                (table, array) = named;
                lines.push(Line {
                    number,
                    table,
                    array,
                    entry: None,
                });
            }
            (None, Some(entry)) => lines.push(Line {
                number,
                table,
                array,
                entry: Some(entry),
            }),
            (None, None) => return Err(number),
        }
    }
    Ok(lines)
}

fn closed(value: &str) -> bool {
    if value.contains("\"\"\"") || value.contains("'''") {
        return false;
    }
    let (mut depth, mut quote, mut escaped) = (0_i32, None, false);
    for c in value.chars() {
        match quote {
            Some('"') if escaped => escaped = false,
            Some('"') if c == '\\' => escaped = true,
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '[' | '{' => depth += 1,
                ']' | '}' => depth -= 1,
                '#' => break,
                _ => {}
            },
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0 && quote.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_and_entries_are_read_line_by_line() {
        let text = "# a comment\n\n[[item]]\nid = \"A-1\"  \nwhy = \"a [bracket], a # hash, a \\\" quote and an = sign\"\naffects = [\"x\", 'y#z']\n\n[workspace.dependencies] # pinned\nserde = { version = \"=1.0.229\", features = [\"derive\"] }\n";
        let lines = lines(text).unwrap();
        assert_eq!(lines.len(), 6);
        assert_eq!(
            lines[0],
            Line {
                number: 3,
                table: "item",
                array: true,
                entry: None
            }
        );
        assert_eq!(lines[1].entry, Some(("id", "\"A-1\"")));
        assert_eq!(lines[3].entry, Some(("affects", "[\"x\", 'y#z']")));
        assert_eq!(
            (lines[4].table, lines[4].array, lines[4].number),
            ("workspace.dependencies", false, 8)
        );
        assert_eq!(lines[5].table, "workspace.dependencies");
    }

    #[test]
    fn a_line_that_does_not_stand_alone_is_the_error() {
        for (text, line) in [
            ("[[item]]\nwhy = \"\"\"\n[[item]]\n\"\"\"\n", 2),
            ("[[item]]\nwhy = '''\ntext'''\n", 2),
            ("[[item]]\nwhy = \"\"\"a\"\n[[item]]\n\"\"\"\n", 2),
            ("[[item]]\nwhy = '''a'\n[[item]]\n'''\n", 2),
            ("[[item]]\nwhy = \"\"\"a\"\"\"\n", 2),
            ("[[item]]\naffects = [\n  \"x\",\n]\n", 2),
            ("[[item]]\nwhy = \"open\n", 2),
            ("[[item]]\nwhy =\n", 2),
            ("[[item]]\nrow = { a = 1,\n}\n", 2),
            ("[[item]\n", 1),
            ("[item]]\n", 1),
            ("[\"item\"]\n", 1),
            ("[workspace . dependencies]\n", 1),
            ("[[item]]\n\"id\" = \"A-1\"\n", 2),
            ("[[item]]\nid . x = \"A-1\"\n", 2),
            ("\u{feff}[[item]]\n", 1),
            ("[[item]]\njust words\n", 2),
        ] {
            assert_eq!(lines(text).unwrap_err(), line, "{text}");
        }
    }
}
