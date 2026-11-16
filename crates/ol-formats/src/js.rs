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

// A time in milliseconds since 1970 as JavaScript's Date prints it, or None for one a Date cannot hold. A year outside 0 to 9999 is written with a sign and six digits.
pub fn iso_from_millis(milliseconds: f64) -> Option<String> {
    const DAY: i64 = 86_400_000;
    if !milliseconds.is_finite() || milliseconds.abs() > 8.64e15 {
        return None;
    }
    let whole = milliseconds.trunc() as i64;
    let (days, in_day) = (whole.div_euclid(DAY), whole.rem_euclid(DAY));
    // Days to a date in the calendar of today, counted from 1 March of year 0 in eras of 400 years.
    let shifted = days + 719_468;
    let (era, day_of_era) = (shifted.div_euclid(146_097), shifted.rem_euclid(146_097));
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { '-' } else { '+' }, year.abs())
    };
    Some(format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_printed_as_a_date_prints_it() {
        let printed = [
            (0.0, "1970-01-01T00:00:00.000Z"),
            (1_771_729_603_000.0, "2026-02-22T03:06:43.000Z"),
            (951_782_400_999.9, "2000-02-29T00:00:00.999Z"),
            (-1.0, "1969-12-31T23:59:59.999Z"),
            (253_402_300_800_000.0, "+010000-01-01T00:00:00.000Z"),
            (8.64e15, "+275760-09-13T00:00:00.000Z"),
            (-62_198_755_200_000.0, "-000001-01-01T00:00:00.000Z"),
        ];
        for (milliseconds, text) in printed {
            assert_eq!(iso_from_millis(milliseconds).as_deref(), Some(text));
        }
        assert_eq!(iso_from_millis(8.64e15 + 1.0), None);
        assert_eq!(iso_from_millis(f64::INFINITY), None);
        assert_eq!(iso_from_millis(f64::NAN), None);
    }

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
