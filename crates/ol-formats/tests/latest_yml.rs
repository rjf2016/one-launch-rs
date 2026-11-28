#![forbid(unsafe_code)]

use ol_formats::latest_yml::{MAX_BYTES, MAX_DEPTH, Unreadable, Yaml, read};

const SHA512: &str =
    "TrVrOgassNbwx5uV5nUThQ5r3VAp7aMC1XIirwvmxC5hdsYQ0tnXsAOVXxa/O1nBsIoTsP03pTuYmyM0S2R/qw==";
const SIGNATURE: &str =
    "WmaNrmSbkp1CA6SZqh1JKrl+0wZxwCx88sNKpz5iCKZIcx1YC6TyeqBeaipegc2nAQ6gtrynYf6GS2v/bVScCA==";
const INSTALLER: &str = "OneLaunch-1.0.0-beta.4-setup.exe";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(ol_testkit::oracle(&format!("fixtures/updates/{name}"))).expect("the fixture")
}

fn text(value: &str) -> Yaml {
    Yaml::Text(value.to_owned())
}

fn number(value: &str) -> Yaml {
    Yaml::Number(value.to_owned())
}

fn map(entries: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn unsigned_tree() -> Vec<(&'static str, Yaml)> {
    vec![
        ("version", text("1.0.0-beta.4")),
        (
            "files",
            Yaml::List(vec![map(vec![
                ("url", text(INSTALLER)),
                ("sha512", text(SHA512)),
                ("size", number("50")),
            ])]),
        ),
        ("path", text(INSTALLER)),
        ("sha512", text(SHA512)),
        ("releaseDate", text("2026-11-20T00:00:00.000Z")),
    ]
}

fn signed_tree() -> Yaml {
    let mut entries = unsigned_tree();
    entries.push(("signature", text(SIGNATURE)));
    entries.push(("keyId", text("3376dec30c48c71f")));
    map(entries)
}

fn value_of(scalar: &str) -> Result<Yaml, Unreadable> {
    match read(format!("a: {scalar}\n").as_bytes())? {
        Yaml::Map(mut entries) if entries.len() == 1 => Ok(entries.remove(0).1),
        other => panic!("{scalar:?} did not read as one entry: {other:?}"),
    }
}

#[test]
fn the_signed_fixture_reads_to_its_tree() {
    assert_eq!(read(&fixture("latest.yml")), Ok(signed_tree()));
}

#[test]
fn the_unsigned_fixture_reads_to_its_tree() {
    assert_eq!(
        read(&fixture("latest-unsigned.yml")),
        Ok(map(unsigned_tree()))
    );
}

#[test]
fn crlf_line_ends_read_as_lf() {
    let lf = String::from_utf8(fixture("latest.yml")).expect("UTF-8");
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(read(crlf.as_bytes()), Ok(signed_tree()));
}

#[test]
fn one_leading_byte_order_mark_is_dropped() {
    let mut bytes = "\u{feff}".as_bytes().to_vec();
    bytes.extend(fixture("latest.yml"));
    assert_eq!(read(&bytes), Ok(signed_tree()));
}

#[test]
fn a_missing_last_newline_changes_nothing() {
    let mut bytes = fixture("latest.yml");
    assert_eq!(bytes.pop(), Some(b'\n'));
    assert_eq!(read(&bytes), Ok(signed_tree()));
}

#[test]
fn bytes_that_are_not_utf8_become_replacement_characters() {
    assert_eq!(
        value_of("One\u{0}Launch").map(|_| ()),
        Err(Unreadable::ControlCharacter(1))
    );
    let bytes = b"url: One\xffLaunch.exe\n";
    assert_eq!(
        read(bytes),
        Ok(map(vec![("url", text("One\u{fffd}Launch.exe"))]))
    );
}

#[test]
fn an_empty_document_is_null() {
    assert_eq!(read(b""), Ok(Yaml::Null));
    assert_eq!(read(b"\n  \n"), Ok(Yaml::Null));
    assert_eq!(read("\u{feff}".as_bytes()), Ok(Yaml::Null));
}

#[test]
fn a_document_may_be_one_scalar_or_a_list() {
    assert_eq!(read(b"null\n"), Ok(Yaml::Null));
    assert_eq!(read(b"42\n"), Ok(number("42")));
    assert_eq!(read(b"false\n"), Ok(Yaml::Bool(false)));
    assert_eq!(read(b"version 1.0.0\n"), Ok(text("version 1.0.0")));
    assert_eq!(
        read(b"- 1\n- 2\n"),
        Ok(Yaml::List(vec![number("1"), number("2")]))
    );
}

#[test]
fn plain_scalars_are_typed_as_js_yaml_types_them() {
    let cases: &[(&str, Yaml)] = &[
        ("~", Yaml::Null),
        ("null", Yaml::Null),
        ("Null", Yaml::Null),
        ("NULL", Yaml::Null),
        ("nULL", text("nULL")),
        ("true", Yaml::Bool(true)),
        ("True", Yaml::Bool(true)),
        ("TRUE", Yaml::Bool(true)),
        ("false", Yaml::Bool(false)),
        ("False", Yaml::Bool(false)),
        ("FALSE", Yaml::Bool(false)),
        ("yes", text("yes")),
        ("50", number("50")),
        ("0", number("0")),
        ("-0", number("-0")),
        ("-1", number("-1")),
        ("+50", number("+50")),
        ("050", number("050")),
        ("0x32", number("0x32")),
        ("0xFf", number("0xFf")),
        ("0o62", number("0o62")),
        ("0b110010", number("0b110010")),
        ("0b12", text("0b12")),
        ("0x", text("0x")),
        ("0o8", text("0o8")),
        ("5_0", text("5_0")),
        ("50.0", number("50.0")),
        ("50.5", number("50.5")),
        ("5e1", number("5e1")),
        ("1.", number("1.")),
        ("1.e5", number("1.e5")),
        (".5", number(".5")),
        ("+.5", text("+.5")),
        ("1e999", text("1e999")),
        (".inf", number(".inf")),
        ("-.Inf", number("-.Inf")),
        ("+.INF", number("+.INF")),
        (".nan", number(".nan")),
        (".NaN", number(".NaN")),
        ("-.nan", text("-.nan")),
        ("3376123012345678", number("3376123012345678")),
        ("12345678901234e5", number("12345678901234e5")),
        ("0b01010101010101", number("0b01010101010101")),
        ("9007199254740993", number("9007199254740993")),
        ("1.0", number("1.0")),
        ("1.0.0", text("1.0.0")),
        ("1.0.0-beta.4+b1", text("1.0.0-beta.4+b1")),
        ("v1.0.0", text("v1.0.0")),
        (
            "https://example.com/a.exe",
            text("https://example.com/a.exe"),
        ),
        ("a#b", text("a#b")),
        ("a, b]", text("a, b]")),
        ("One Launch.exe", text("One Launch.exe")),
        ("OneLaunch.exe   ", text("OneLaunch.exe")),
        ("[]", Yaml::List(Vec::new())),
        ("[  ]", Yaml::List(Vec::new())),
    ];
    for (scalar, expected) in cases {
        assert_eq!(value_of(scalar).as_ref(), Ok(expected), "{scalar:?}");
    }
    let digits = "9".repeat(400);
    assert_eq!(value_of(&digits), Ok(text(&digits)));
    let hex = format!("0x{}", "f".repeat(300));
    assert_eq!(value_of(&hex), Ok(text(&hex)));
}

#[test]
fn quoted_scalars_are_text() {
    let cases: &[(&str, &str)] = &[
        ("'50'", "50"),
        ("'true'", "true"),
        ("'~'", "~"),
        ("''", ""),
        ("'it''s'", "it's"),
        ("'a: b # c'", "a: b # c"),
        ("'OneLaunch.exe '", "OneLaunch.exe "),
        ("\"\"", ""),
        ("\"null\"", "null"),
        (r#""1.0.0\n""#, "1.0.0\n"),
        (r#""a\tb\\c\"d\/e""#, "a\tb\\c\"d/e"),
        (r#""\x41\u00e9\U0001F600""#, "A\u{e9}\u{1f600}"),
        (
            r#""\0\a\b\e\f\r\v\ \N\_\L\P""#,
            "\0\x07\x08\x1b\x0c\r\x0b \u{85}\u{a0}\u{2028}\u{2029}",
        ),
        ("'2026-11-20'", "2026-11-20"),
    ];
    for (scalar, expected) in cases {
        assert_eq!(value_of(scalar), Ok(text(expected)), "{scalar}");
    }
}

#[test]
fn quoted_keys_and_nested_blocks_read() {
    assert_eq!(
        read(b"'a b': 1\n\"c\": x\n"),
        Ok(map(vec![("a b", number("1")), ("c", text("x"))]))
    );
    assert_eq!(
        read(b"packages:\n  x64:\n    path: p.7z\n"),
        Ok(map(vec![(
            "packages",
            map(vec![("x64", map(vec![("path", text("p.7z"))]))])
        )]))
    );
    assert_eq!(
        read(b"a:\nb: 1\n"),
        Ok(map(vec![("a", Yaml::Null), ("b", number("1"))]))
    );
    assert_eq!(read(b"a:\n  b\n"), Ok(map(vec![("a", text("b"))])));
    assert_eq!(
        read(b"files:\n  - null\n  - []\n  - -\n  - - x\n    - y\n  -\n    k: v\n"),
        Ok(map(vec![(
            "files",
            Yaml::List(vec![
                Yaml::Null,
                Yaml::List(Vec::new()),
                Yaml::List(vec![Yaml::Null]),
                Yaml::List(vec![text("x"), text("y")]),
                map(vec![("k", text("v"))]),
            ])
        )]))
    );
    assert_eq!(
        read(b"-\n- a\n"),
        Ok(Yaml::List(vec![Yaml::Null, text("a")]))
    );
    assert_eq!(read(b"a  : 1\n"), Ok(map(vec![("a", number("1"))])));
}

#[test]
fn each_construct_outside_the_subset_is_refused() {
    let cases: &[(&str, Unreadable)] = &[
        ("a: &x 1\n", Unreadable::NodeProperty(1)),
        ("a: *x\n", Unreadable::NodeProperty(1)),
        ("a: !!str 1\n", Unreadable::NodeProperty(1)),
        ("&x a: 1\n", Unreadable::NodeProperty(1)),
        ("- !x 1\n", Unreadable::NodeProperty(1)),
        ("a: |\n  x\n", Unreadable::BlockScalar(1)),
        ("a: >-\n  x\n", Unreadable::BlockScalar(1)),
        ("a: {}\n", Unreadable::Flow(1)),
        ("a: {b: 1}\n", Unreadable::Flow(1)),
        ("a: [1]\n", Unreadable::Flow(1)),
        ("a: [] x\n", Unreadable::Flow(1)),
        ("[a]: 1\n", Unreadable::Flow(1)),
        ("? a\n: b\n", Unreadable::ComplexKey(1)),
        ("a: ? b\n", Unreadable::ComplexKey(1)),
        ("a: 1\n---\nb: 2\n", Unreadable::Document(2)),
        ("---\na: 1\n", Unreadable::Document(1)),
        ("%YAML 1.2\n---\na: 1\n", Unreadable::Document(1)),
        ("a: 1\n...\n", Unreadable::Document(2)),
        ("a:\n\tb: 1\n", Unreadable::Tab(2)),
        ("a:\t1\n", Unreadable::Tab(1)),
        ("a: 1\na: 2\n", Unreadable::DuplicateKey(2)),
        ("a: 1\n'a': 2\n", Unreadable::DuplicateKey(2)),
        ("- a: 1\n  b: 2\n  a: 3\n", Unreadable::DuplicateKey(3)),
        ("# note\na: 1\n", Unreadable::Comment(1)),
        ("a: 1\n  # note\n", Unreadable::Comment(2)),
        ("a: 1 # note\n", Unreadable::Comment(1)),
        ("a: 'x' # note\n", Unreadable::Comment(1)),
        ("a: 'x\n", Unreadable::UnclosedQuote(1)),
        ("a: 'x\n  y'\n", Unreadable::UnclosedQuote(1)),
        ("a: \"x\n", Unreadable::UnclosedQuote(1)),
        ("a: \"x\\\n", Unreadable::UnclosedQuote(1)),
        ("a: \"\\q\"\n", Unreadable::Escape(1)),
        ("a: \"\\x4\"\n", Unreadable::Escape(1)),
        ("a: \"\\ud800\"\n", Unreadable::Escape(1)),
        ("a: \"\\U00110000\"\n", Unreadable::Escape(1)),
        ("a: 'x' y\n", Unreadable::Syntax(1)),
        ("a: 1\n  b: 2\n", Unreadable::Indentation(2)),
        ("a:\n  b: 1\n c: 2\n", Unreadable::Indentation(3)),
        ("  a: 1\nb: 2\n", Unreadable::Indentation(2)),
        ("a\nb\n", Unreadable::Indentation(2)),
        ("a: b\n  c\n", Unreadable::Indentation(2)),
        ("- a\n - b\n", Unreadable::Indentation(2)),
        ("files:\n- a\n", Unreadable::Syntax(2)),
        ("a: 1\n- b\n", Unreadable::Syntax(2)),
        ("- a\nb: 1\n", Unreadable::Syntax(2)),
        ("a: b: c\n", Unreadable::Syntax(1)),
        ("a: b:\n", Unreadable::Syntax(1)),
        ("a: - b\n", Unreadable::Syntax(1)),
        ("a: -\n", Unreadable::Syntax(1)),
        ("a: : b\n", Unreadable::Syntax(1)),
        ("a: @b\n", Unreadable::Syntax(1)),
        ("a: `b\n", Unreadable::Syntax(1)),
        ("a: %b\n", Unreadable::Syntax(1)),
        ("a: ,b\n", Unreadable::Syntax(1)),
        ("a: }\n", Unreadable::Syntax(1)),
        ("-a\n-b\n", Unreadable::Indentation(2)),
        ("1: a\n", Unreadable::KeyNotText(1)),
        ("null: a\n", Unreadable::KeyNotText(1)),
        ("~: a\n", Unreadable::KeyNotText(1)),
        ("true: a\n", Unreadable::KeyNotText(1)),
        ("0x10: a\n", Unreadable::KeyNotText(1)),
        ("<<: a\n", Unreadable::Merge(1)),
        ("a: <<\n", Unreadable::Merge(1)),
        ("a: 2026-11-20\n", Unreadable::Timestamp(1)),
        ("a: 2026-1-2 3:04:05\n", Unreadable::Timestamp(1)),
        ("a: 2026-11-20T00:00:00.000Z\n", Unreadable::Timestamp(1)),
        ("2026-11-20: a\n", Unreadable::Timestamp(1)),
        ("a: \u{1}\n", Unreadable::ControlCharacter(1)),
        ("a: \u{7f}\n", Unreadable::ControlCharacter(1)),
        ("a: \u{85}\n", Unreadable::ControlCharacter(1)),
        ("a: '\u{9f}'\n", Unreadable::ControlCharacter(1)),
        ("a: \u{fffe}\n", Unreadable::ControlCharacter(1)),
        ("a: \u{2028}\n", Unreadable::ControlCharacter(1)),
        ("a: 1\n\u{feff}b: 2\n", Unreadable::ControlCharacter(2)),
        ("a: 1\rb: 2\n", Unreadable::CarriageReturn(1)),
        ("a: 1\r\r\n", Unreadable::CarriageReturn(1)),
    ];
    for (source, expected) in cases {
        assert_eq!(
            read(source.as_bytes()).as_ref(),
            Err(expected),
            "{source:?}"
        );
    }
}

#[test]
fn nesting_deeper_than_the_cap_is_refused() {
    let within = format!("{}x\n", "- ".repeat(MAX_DEPTH - 1));
    assert!(read(within.as_bytes()).is_ok());
    let past = format!("{}x\n", "- ".repeat(MAX_DEPTH));
    assert_eq!(read(past.as_bytes()), Err(Unreadable::TooDeep(1)));
    let mut keys = String::new();
    for depth in 0..=MAX_DEPTH {
        keys.push_str(&format!("{}k:\n", " ".repeat(depth)));
    }
    assert_eq!(
        read(keys.as_bytes()),
        Err(Unreadable::TooDeep(MAX_DEPTH + 1))
    );
    let flat = "- ".repeat(MAX_BYTES / 2);
    assert!(matches!(
        read(&flat.as_bytes()[..MAX_BYTES]),
        Err(Unreadable::TooDeep(_))
    ));
}

#[test]
fn input_past_the_cap_is_refused_unread() {
    let mut bytes = fixture("latest.yml");
    let line = b"x: 1\n";
    bytes.extend(b"padding:\n");
    let mut at = 0;
    while bytes.len() < MAX_BYTES {
        let key = format!("  k{at}: 1\n");
        bytes.extend(key.as_bytes());
        at += 1;
    }
    bytes.truncate(MAX_BYTES);
    while bytes.last() != Some(&b'\n') {
        bytes.pop();
    }
    assert!(read(&bytes).is_ok());
    let mut past = vec![b' '; MAX_BYTES - line.len() + 1];
    past.extend(line);
    assert_eq!(past.len(), MAX_BYTES + 1);
    assert_eq!(read(&past), Err(Unreadable::TooLarge));
    let mut fits = vec![b' '; MAX_BYTES - line.len()];
    fits.extend(line);
    assert_eq!(read(&fits), Ok(map(vec![("x", number("1"))])));
}

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

#[test]
fn no_input_makes_it_panic_or_hang() {
    let source = fixture("latest.yml");
    for end in 0..=source.len() {
        let _ = read(&source[..end]);
    }
    let alphabet = b" \n\r\t-:'\"#&*!|>?[]{},%@`~\\.0123456789abexoAZ+_\xef\xbb\xbf\xff\xc3";
    let mut generator = Generator(0x9e37_79b9_7f4a_7c15);
    for _ in 0..4000 {
        let length = generator.below(160);
        let bytes: Vec<u8> = (0..length)
            .map(|_| {
                if generator.below(8) == 0 {
                    generator.next() as u8
                } else {
                    alphabet[generator.below(alphabet.len())]
                }
            })
            .collect();
        let _ = read(&bytes);
    }
    for _ in 0..2000 {
        let mut bytes = source.clone();
        for _ in 0..1 + generator.below(4) {
            let at = generator.below(bytes.len());
            bytes[at] = alphabet[generator.below(alphabet.len())];
        }
        let _ = read(&bytes);
    }
}
