// The Electron app decides what is blank, how long a text is and what a digit is by JavaScript's rules, and launcher files are read by the same rules here.

pub const WHITESPACE_CLASS: &str = r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";

pub const fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

pub fn trim(text: &str) -> &str {
    text.trim_matches(is_whitespace)
}

pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

pub fn is_ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

// The pieces between line ends, a carriage return before a line feed counting as part of the line end.
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut pieces = text.split('\n').peekable();
    std::iter::from_fn(move || {
        let piece = pieces.next()?;
        if pieces.peek().is_some() {
            Some(piece.strip_suffix('\r').unwrap_or(piece))
        } else {
            Some(piece)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_byte_order_mark_is_blank_and_a_next_line_mark_is_not() {
        assert_eq!(trim("\u{feff} a\u{3000}\u{2028}"), "a");
        assert_eq!(trim("\u{85}a\u{180e}"), "\u{85}a\u{180e}");
    }

    #[test]
    fn a_line_keeps_a_carriage_return_that_ends_nothing() {
        let pieces: Vec<&str> = lines("a\r\nb\rc\n\r\nd\r").collect();
        assert_eq!(pieces, ["a", "b\rc", "", "d\r"]);
        assert_eq!(lines("").collect::<Vec<_>>(), [""]);
    }

    #[test]
    fn a_character_outside_the_first_plane_counts_twice() {
        assert_eq!(utf16_len("a😀"), 3);
    }
}
