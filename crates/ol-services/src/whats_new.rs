use std::cmp::Ordering;

use ol_model::{SettingKey, Settings, is_version};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Success,
    Warning,
    Danger,
    Steam,
    Epic,
    Battlenet,
    Riot,
}

impl Tone {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accent => "accent",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::Steam => "steam",
            Self::Epic => "epic",
            Self::Battlenet => "battlenet",
            Self::Riot => "riot",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Item {
    pub lead: &'static str,
    pub detail: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Section {
    pub heading: &'static str,
    pub tone: Option<Tone>,
    pub items: &'static [Item],
}

#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    pub file: &'static str,
    pub alt: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Release {
    pub version: &'static str,
    pub date: &'static str,
    pub intro: Option<&'static str>,
    pub image: Option<Image>,
    pub sections: &'static [Section],
}

pub static RELEASES: &[Release] = &[Release {
    version: "1.0.0-beta.3",
    date: "2026-10-15",
    intro: None,
    image: Some(Image {
        file: "beta-3.png",
        alt: Some("The setup guide on a new install"),
    }),
    sections: &[
        Section {
            heading: "New Features",
            tone: Some(Tone::Success),
            items: &[Item {
                lead: "One Launch now tells you what changed.",
                detail: Some(
                    "After an update, this card shows up to inform you of any new quirks or features.",
                ),
            }],
        },
        Section {
            heading: "Improved",
            tone: Some(Tone::Warning),
            items: &[Item {
                lead: "A clearer welcome for new installs.",
                detail: Some(
                    "The setup guide opens with the same tagline as the rest of the app and gets to the point faster.",
                ),
            }],
        },
    ],
}];

fn js_space(c: char) -> bool {
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

// As JavaScript's `Number()`, which the Electron app compares the parts of a version by.
fn js_number(text: &str) -> f64 {
    let text = text.trim_matches(js_space);
    if text.is_empty() {
        return 0.;
    }
    let radix = match text.get(..2) {
        Some("0x" | "0X") => Some(16),
        Some("0o" | "0O") => Some(8),
        Some("0b" | "0B") => Some(2),
        _ => None,
    };
    if let Some(radix) = radix {
        let digits = &text[2..];
        if digits.is_empty() {
            return f64::NAN;
        }
        return digits
            .chars()
            .try_fold(0_f64, |sum, digit| {
                digit
                    .to_digit(radix)
                    .map(|value| sum * f64::from(radix) + f64::from(value))
            })
            .unwrap_or(f64::NAN);
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if unsigned == "Infinity" {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let mantissa_ok =
        digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty());
    let exponent_ok = exponent.is_none_or(|exponent| {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });
    if !mantissa_ok || !exponent_ok {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

fn identifiers(version: &str) -> (Vec<f64>, Vec<&str>) {
    let (core, pre) = match version.find('-') {
        Some(dash) => (&version[..dash], version[dash + 1..].split('.').collect()),
        None => (version, Vec::new()),
    };
    (core.split('.').map(js_number).collect(), pre)
}

fn numeric(identifier: &str) -> bool {
    !identifier.is_empty() && identifier.bytes().all(|byte| byte.is_ascii_digit())
}

// JavaScript's `a < b` compares UTF-16 code units.
fn code_units(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn sign(difference: f64) -> Ordering {
    if difference == 0. {
        Ordering::Equal
    } else if difference < 0. {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

fn compare_identifiers(a: &str, b: &str) -> Ordering {
    match (numeric(a), numeric(b)) {
        (true, true) => sign(js_number(a) - js_number(b)),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => code_units(a, b),
    }
}

pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (a_core, a_pre) = identifiers(a);
    let (b_core, b_pre) = identifiers(b);
    for at in 0..3 {
        let part = |core: &[f64]| core.get(at).copied().unwrap_or(0.);
        let order = sign(part(&a_core) - part(&b_core));
        if order != Ordering::Equal {
            return order;
        }
    }
    match (a_pre.is_empty(), b_pre.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for at in 0..a_pre.len().max(b_pre.len()) {
        let order = match (a_pre.get(at), b_pre.get(at)) {
            (None, _) => Ordering::Less,
            (_, None) => Ordering::Greater,
            (Some(a), Some(b)) => compare_identifiers(a, b),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

pub fn pick(versions: &[&str], current: Option<&str>, last_seen: Option<&str>) -> Option<usize> {
    let current = current.filter(|current| is_version(current))?;
    let seen = last_seen.filter(|seen| is_version(seen));
    let mut best: Option<usize> = None;
    for (at, version) in versions.iter().enumerate() {
        if !is_version(version) || compare_versions(version, current) == Ordering::Greater {
            continue;
        }
        if seen.is_some_and(|seen| compare_versions(version, seen) != Ordering::Greater) {
            continue;
        }
        if best.is_none_or(|best| compare_versions(version, versions[best]) == Ordering::Greater) {
            best = Some(at);
        }
    }
    best
}

pub fn pick_whats_new(current: &str, last_seen: Option<&str>) -> Option<&'static Release> {
    let versions: Vec<&str> = RELEASES.iter().map(|release| release.version).collect();
    pick(&versions, Some(current), last_seen).map(|at| &RELEASES[at])
}

pub fn latest_whats_new(current: &str) -> Option<&'static Release> {
    pick_whats_new(current, None)
}

#[derive(Debug, PartialEq, Eq)]
pub enum AtStart {
    Nothing,
    // So turning the setting on later never shows old notes.
    RecordSeen,
    Show(&'static Release),
}

pub fn at_start(settings: &Settings, version: &str, recovery_pending: bool) -> AtStart {
    if recovery_pending {
        return AtStart::Nothing;
    }
    let last_seen = settings.get(SettingKey::WhatsNewLastSeen);
    if settings.get(SettingKey::WhatsNewEnabled) == Some("false") {
        let advances = is_version(version)
            && last_seen
                .filter(|seen| is_version(seen))
                .is_none_or(|seen| compare_versions(version, seen) == Ordering::Greater);
        return if advances {
            AtStart::RecordSeen
        } else {
            AtStart::Nothing
        };
    }
    pick_whats_new(version, last_seen).map_or(AtStart::Nothing, AtStart::Show)
}

pub fn shows_setup_guide(settings: &Settings, accounts: usize) -> bool {
    settings.get(SettingKey::FirstRunDismissed) != Some("true") && accounts == 0
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

// From its parts, so the day never shifts with the time zone.
pub fn release_date(date: &str) -> String {
    let mut parts = date.split('-').map(|part| part.parse::<u32>().ok());
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Some(year)), Some(Some(month @ 1..=12)), Some(Some(day @ 1..=31)), None)
            if year > 0 =>
        {
            format!("{} {day}, {year}", MONTHS[month as usize - 1])
        }
        _ => date.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_date_reads_as_the_electron_app_writes_it_on_an_english_pc() {
        assert_eq!(release_date("2026-10-15"), "October 15, 2026");
        assert_eq!(release_date("2026-01-01"), "January 1, 2026");
        assert_eq!(release_date("2026-13-01"), "2026-13-01");
        assert_eq!(release_date("soon"), "soon");
    }

    #[test]
    fn a_start_shows_the_notes_once_and_never_while_a_recovery_is_due_or_the_setting_is_off() {
        let mut settings = Settings::default();
        assert_eq!(
            at_start(&settings, "1.0.0-beta.3", false),
            AtStart::Show(&RELEASES[0])
        );
        assert_eq!(at_start(&settings, "1.0.0-beta.3", true), AtStart::Nothing);
        settings.insert(SettingKey::WhatsNewLastSeen, "1.0.0-beta.3".to_owned());
        assert_eq!(at_start(&settings, "1.0.0-beta.3", false), AtStart::Nothing);
        settings.insert(SettingKey::WhatsNewLastSeen, "1.0.0-beta.2".to_owned());
        settings.insert(SettingKey::WhatsNewEnabled, "false".to_owned());
        assert_eq!(
            at_start(&settings, "1.0.0-beta.3", false),
            AtStart::RecordSeen
        );
        settings.insert(SettingKey::WhatsNewLastSeen, "1.0.0-beta.3".to_owned());
        assert_eq!(at_start(&settings, "1.0.0-beta.3", false), AtStart::Nothing);
        assert_eq!(
            at_start(&Settings::default(), "0.0.0", false),
            AtStart::Nothing
        );
    }

    #[test]
    fn with_the_notes_off_an_older_build_leaves_the_newer_mark_and_a_newer_build_moves_it() {
        let mut settings = Settings::default();
        settings.insert(SettingKey::WhatsNewEnabled, "false".to_owned());
        assert_eq!(
            at_start(&settings, "1.0.0-beta.3", false),
            AtStart::RecordSeen
        );
        settings.insert(SettingKey::WhatsNewLastSeen, "1.0.0-beta.4".to_owned());
        assert_eq!(at_start(&settings, "1.0.0-beta.3", false), AtStart::Nothing);
        assert_eq!(
            at_start(&settings, "1.0.0-beta.5", false),
            AtStart::RecordSeen
        );
        assert_eq!(
            at_start(&settings, "not-a-version", false),
            AtStart::Nothing
        );
        settings.insert(SettingKey::WhatsNewLastSeen, "soon".to_owned());
        assert_eq!(
            at_start(&settings, "1.0.0-beta.3", false),
            AtStart::RecordSeen
        );
    }

    #[test]
    fn the_guide_opens_for_a_folder_nobody_has_set_up() {
        let mut settings = Settings::default();
        assert!(shows_setup_guide(&settings, 0));
        assert!(!shows_setup_guide(&settings, 1));
        settings.insert(SettingKey::FirstRunDismissed, "false".to_owned());
        assert!(shows_setup_guide(&settings, 0));
        settings.insert(SettingKey::FirstRunDismissed, "true".to_owned());
        assert!(!shows_setup_guide(&settings, 0));
    }
}
