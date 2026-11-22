use std::borrow::Cow;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

pub const PATH_MASK: &str = "[PATH]";
pub const DATA_FOLDER_MASK: &str = "<data folder>";

static START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[a-z]:[\\/]|\\\\[a-z0-9._$-]+[\\/]").expect("a fixed pattern")
});

pub struct Paths {
    data_folder: Option<Regex>,
}

impl Paths {
    pub const fn none() -> Self {
        Self { data_folder: None }
    }

    pub fn for_data_folder(dir: &Path) -> Self {
        let text = dir.to_string_lossy();
        let segments: Vec<String> = text
            .split(['\\', '/'])
            .filter(|segment| !segment.is_empty() && *segment != "?")
            .map(regex::escape)
            .collect();
        let data_folder = (!segments.is_empty())
            .then(|| {
                Regex::new(&format!(
                    r"(?i){}(?P<after>[\\/]|$|[^\w.-])",
                    segments.join(r"[\\/]+")
                ))
                .ok()
            })
            .flatten();
        Self { data_folder }
    }

    pub fn mask<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let text = match &self.data_folder {
            Some(folder) => folder.replace_all(text, format!("{DATA_FOLDER_MASK}$after").as_str()),
            None => Cow::Borrowed(text),
        };
        match mask_absolute(&text) {
            Cow::Borrowed(_) => text,
            Cow::Owned(masked) => Cow::Owned(masked),
        }
    }

    pub fn mask_value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.mask(text).into_owned()),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.mask_value(item)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, field)| (key.clone(), self.mask_value(field)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

pub fn in_data_folder(path: &Path, data_dir: &Path) -> String {
    path.strip_prefix(data_dir).map_or_else(
        |_| PATH_MASK.to_owned(),
        |relative| relative.to_string_lossy().into_owned(),
    )
}

fn mask_absolute(text: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = START.find_at(text, from) {
        let end = path_end(text, found.end());
        out.push_str(&text[copied..found.start()]);
        out.push_str(PATH_MASK);
        copied = end;
        from = end;
    }
    if copied == 0 {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

// A path may hold spaces, so it runs on until a quote, a line's end or a sign that ends a sentence; masking a few words too many is the safe way to be wrong.
fn path_end(text: &str, from: usize) -> usize {
    let mut characters = text[from..].char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        let next_is_space = characters
            .peek()
            .is_none_or(|(_, next)| next.is_whitespace());
        let ends = matches!(
            character,
            '"' | '\'' | '`' | '<' | '>' | '|' | '*' | '?' | '\r' | '\n' | '\t'
        ) || (matches!(character, '.' | ',' | ';' | ':' | ')' | ']') && next_is_space);
        if ends {
            return from + at;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_path_is_masked_and_the_data_folder_named() {
        let paths = Paths::for_data_folder(Path::new(
            r"D:\Alice Smith\OneDrive - Contoso Ltd\OneLaunch",
        ));
        let cases = [
            (
                r"D:\Alice Smith\OneDrive - Contoso Ltd\OneLaunch\backups\db\x.db",
                r"<data folder>\backups\db\x.db",
            ),
            (
                r"d:/alice smith/onedrive - contoso ltd/onelaunch",
                "<data folder>",
            ),
            (
                r"\\?\D:\Alice Smith\OneDrive - Contoso Ltd\OneLaunch\logs",
                r"\\?\<data folder>\logs",
            ),
            (
                r"names a file that is not one of steam's: c:\program files (x86)\steam\config\loginusers.vdf",
                "names a file that is not one of steam's: [PATH]",
            ),
            (
                r"could not read E:\Jo Bloggs\Games\Steam. Close it first.",
                "could not read [PATH]. Close it first.",
            ),
            (r#"{"dir":"\\server\share$\jo\x"}"#, r#"{"dir":"[PATH]"}"#),
            (
                "steam://rungameid/1 at 12:30 on https://example.com/a",
                "steam://rungameid/1 at 12:30 on https://example.com/a",
            ),
            (
                r"backups\steam\2026-11-07T03-23-28.574Z",
                r"backups\steam\2026-11-07T03-23-28.574Z",
            ),
        ];
        for (text, masked) in cases {
            assert_eq!(paths.mask(text), masked, "{text}");
        }
        assert_eq!(Paths::none().mask(r"D:\Alice Smith\x"), PATH_MASK);
        let data = Path::new(r"D:\Alice Smith\OneLaunch");
        assert_eq!(
            in_data_folder(&data.join(r"backups\db\x.db"), data),
            r"backups\db\x.db"
        );
        assert_eq!(
            in_data_folder(Path::new(r"E:\elsewhere\x.db"), data),
            PATH_MASK
        );
    }
}
