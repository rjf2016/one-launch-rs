// The Riot Client's remembered session: the `riot-client` block under `psl` › `authorization` in RiotGamesPrivateSettings.yaml. The block holds bearer tokens, so nothing here prints a value, no reason for a refusal quotes the file, and what a user is shown of a sign-in is the start of a SHA-256. The reader fails closed: a block that is not in the shape a real client writes reads as unexpected, never as a session or a sign-out. It understands space-indented maps, one scalar on a line and one-line list items, and nothing more of YAML, on purpose.

use ol_model::redact::{register_account_ids, register_account_names};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroize;

pub use crate::epic::session::short_fingerprint;
use crate::js;
use crate::js_json::{JsObject, JsValue, parse, quote_text, quoted_capacity};

const BLOCK_PATH: [(&str, &str); 3] = [
    ("psl", "psl"),
    ("authorization", "psl.authorization"),
    ("riot-client", "psl.authorization.riot-client"),
];
const MAX_NAME_LENGTH: usize = 64;

// The Riot ID an id token names. No Debug: it is a name, and is never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct RiotId {
    pub game_name: String,
    pub tag_line: String,
}

impl RiotId {
    pub fn text(&self) -> String {
        format!("{}#{}", self.game_name, self.tag_line)
    }
}

// What the file says about the sign-in it holds, and none of its tokens. No Debug: it names the account.
#[derive(Clone, PartialEq, Eq)]
pub struct RiotSession {
    // The Riot account id: the id token's `sub`, in lower case. The same across token rotations.
    pub account_id: String,
    pub riot_id: Option<RiotId>,
    // SHA-256 of the refresh token, in hex. The client replaces the token at every start, so a stored copy with another fingerprint is out of date.
    pub fingerprint: String,
    // True would bind the tokens to a device key that no copy of the file carries.
    pub dpop_bound: Option<bool>,
    pub write_count: Option<u32>,
}

// Why a file is not one the reader knows. Every reason is a fixed sentence: none can carry a byte of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum Unexpected {
    #[error("no {0} block")]
    NoBlock(&'static str),
    #[error("riot-client is neither null nor a block")]
    NeitherNullNorBlock,
    #[error("riot-client is empty")]
    EmptyBlock,
    #[error("riot-client has no id_token")]
    NoIdToken,
    #[error("riot-client has no refresh_token")]
    NoRefreshToken,
    #[error("id_token is not a readable token")]
    UnreadableIdToken,
    #[error("id_token names no account id")]
    NoAccountId,
    #[error("a comment at the left of the riot-client block cuts it")]
    CommentInBlock,
}

#[derive(Clone, PartialEq, Eq)]
pub enum SessionRead {
    Session(RiotSession),
    // `riot-client: null`, the client's own signed-out state.
    SignedOut,
    Unexpected(Unexpected),
}

// What One Launch keeps for an account: the whole file, as the client left it. No Debug, and wiped when dropped. The file as it is read for a plan or a switch is held in one of these too.
#[derive(Clone, PartialEq, Eq)]
pub struct RiotSessionBlob {
    pub file: String,
}

impl Drop for RiotSessionBlob {
    fn drop(&mut self) {
        self.file.zeroize();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("not UTF-8 text")]
pub struct NotText;

impl RiotSessionBlob {
    // The file as the text it is. Bytes that are not UTF-8 are refused and wiped: read as text they would not come back as they were in a write.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, NotText> {
        match String::from_utf8(bytes) {
            Ok(file) => Ok(Self { file }),
            Err(refused) => {
                refused.into_bytes().zeroize();
                Err(NotText)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum BlobError {
    #[error("the stored Riot sign-in is not readable")]
    NotReadable,
    #[error("the stored Riot sign-in is not in a known shape")]
    UnknownShape,
    #[error("the stored Riot sign-in holds no session ({0})")]
    NoSession(Unexpected),
    #[error("the stored Riot sign-in holds no session (signed out)")]
    SignedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("{0}; the file is left alone")]
pub struct LeftAlone(pub Unexpected);

pub struct SignOutWrite {
    pub text: String,
    pub noop: bool,
}

pub fn fingerprint_of(refresh_token: &str) -> String {
    Sha256::digest(refresh_token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct Entry<'a> {
    key: &'a str,
    line: usize,
    indent: usize,
    // What follows `key:` on the line, or None when a block follows, or nothing.
    value: Option<&'a str>,
}

// The file's lines, each by where its content starts and ends in the text. A line end is a line feed, with the carriage return before it when there is one.
struct Lines<'a> {
    text: &'a str,
    spans: Vec<(usize, usize)>,
}

fn indent_of(line: &str) -> usize {
    line.bytes().take_while(|byte| *byte == b' ').count()
}

fn is_blank(line: &str) -> bool {
    js::trim(line).is_empty()
}

// `key:` or `key: value`, behind spaces only. A list item never is one: no colon follows its dash.
fn key_line(line: &str) -> Option<(usize, &str, Option<&str>)> {
    let indent = indent_of(line);
    let rest = &line[indent..];
    let length = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(byte))
        .count();
    let after = rest[length..].strip_prefix(':')?;
    if length == 0 {
        return None;
    }
    if after.is_empty() {
        return Some((indent, &rest[..length], None));
    }
    if !after.starts_with([' ', '\t']) || after.contains(['\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let value = after.trim_matches([' ', '\t']);
    Some((
        indent,
        &rest[..length],
        Some(value).filter(|v| !v.is_empty()),
    ))
}

impl<'a> Lines<'a> {
    fn of(text: &'a str) -> Self {
        let start = if text.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        let bytes = text.as_bytes();
        let mut spans = Vec::new();
        let mut from = start;
        for (at, byte) in bytes.iter().enumerate().skip(start) {
            if *byte == b'\n' {
                let returned = at > from && bytes[at - 1] == b'\r';
                spans.push((from, if returned { at - 1 } else { at }));
                from = at + 1;
            }
        }
        spans.push((from, text.len()));
        Self { text, spans }
    }

    fn line(&self, at: usize) -> &'a str {
        let (start, end) = self.spans[at];
        &self.text[start..end]
    }

    // The keys directly inside the block that follows `parent`, or at the top of the file: the lines after it, as far as one indented no deeper than it, that sit at the indent of the block's first line. Deeper lines and list items are passed over.
    fn entries_of(&self, parent: Option<&Entry<'_>>) -> Vec<Entry<'a>> {
        let mut entries = Vec::new();
        let mut block_indent = None;
        let first = parent.map_or(0, |parent| parent.line + 1);
        for at in first..self.spans.len() {
            let line = self.line(at);
            if is_blank(line) {
                continue;
            }
            let indent = indent_of(line);
            if parent.is_some_and(|parent| indent <= parent.indent) {
                break;
            }
            if *block_indent.get_or_insert(indent) != indent {
                continue;
            }
            if let Some((indent, key, value)) = key_line(line) {
                entries.push(Entry {
                    key,
                    line: at,
                    indent,
                    value,
                });
            }
        }
        entries
    }

    // The `riot-client` entry, walked to from the top, or the dotted path of the first key that is not there.
    fn session_block(&self) -> Result<Entry<'a>, &'static str> {
        let mut block: Option<Entry<'a>> = None;
        for (key, path) in BLOCK_PATH {
            let entries = self.entries_of(block.as_ref());
            block = Some(
                entries
                    .into_iter()
                    .find(|entry| entry.key == key)
                    .ok_or(path)?,
            );
        }
        block.ok_or(BLOCK_PATH[0].1)
    }

    // The line after the last line of the block that follows `entry`. Blank lines behind it are not part of it.
    fn block_end(&self, entry: &Entry<'_>) -> usize {
        let mut end = entry.line + 1;
        for at in entry.line + 1..self.spans.len() {
            let line = self.line(at);
            if is_blank(line) {
                continue;
            }
            if indent_of(line) <= entry.indent {
                break;
            }
            end = at + 1;
        }
        end
    }

    // A comment no deeper than `entry` with lines of the block below it. For YAML the block goes on past the comment; for this reader and for a splice it would end there, and a sign-out would leave the rest of the block behind, which the client cannot read.
    fn cut_by_comment(&self, entry: &Entry<'_>) -> bool {
        let mut after_comment = false;
        for at in entry.line + 1..self.spans.len() {
            let line = self.line(at);
            if is_blank(line) {
                continue;
            }
            let shallow = indent_of(line) <= entry.indent;
            if js::trim(line).starts_with('#') {
                after_comment |= shallow;
                continue;
            }
            if shallow {
                return false;
            }
            if after_comment {
                return true;
            }
        }
        false
    }
}

// The file with the `riot-client` block replaced: by `riot-client: null`, the client's own signed-out state, when there is no body, else by the given lines one level in. Every byte outside the block stays as it is: each other line keeps its own line end, and a byte order mark stays. The new lines end as the file's lines do, CRLF when it holds one.
pub fn splice_session_block(text: &str, body: Option<&[&str]>) -> Result<String, Unexpected> {
    let lines = Lines::of(text);
    let entry = lines.session_block().map_err(Unexpected::NoBlock)?;
    if lines.cut_by_comment(&entry) {
        return Err(Unexpected::CommentInBlock);
    }
    let end = lines.block_end(&entry);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let pad = " ".repeat(entry.indent);
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..lines.spans[entry.line].0]);
    out.push_str(&pad);
    out.push_str("riot-client:");
    match body {
        None => out.push_str(" null"),
        Some(body) => {
            for line in body {
                out.push_str(eol);
                out.push_str(&pad);
                out.push_str("    ");
                out.push_str(line);
            }
        }
    }
    out.push_str(&text[lines.spans[end - 1].1..]);
    Ok(out)
}

// The sign-out write: the session block becomes `null` and nothing else changes. Nothing to do for a file that is signed out already; a file the reader does not know is left alone.
pub fn plan_sign_out(text: &str) -> Result<SignOutWrite, LeftAlone> {
    match read_session(text) {
        SessionRead::Unexpected(reason) => Err(LeftAlone(reason)),
        SessionRead::SignedOut => Ok(SignOutWrite {
            text: text.to_owned(),
            noop: true,
        }),
        SessionRead::Session(_) => Ok(SignOutWrite {
            text: splice_session_block(text, None).map_err(LeftAlone)?,
            noop: false,
        }),
    }
}

fn unquote(value: Option<&str>) -> Option<&str> {
    let value = value?;
    let within = |quote: char| value.strip_prefix(quote)?.strip_suffix(quote);
    let text = within('"').or_else(|| within('\'')).unwrap_or(value);
    Some(text).filter(|text| !text.is_empty())
}

fn is_null(value: &str) -> bool {
    matches!(value, "null" | "~" | "Null" | "NULL")
}

fn is_token_part(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

// Base64 in the URL alphabet as Node reads it: without padding, and with the bits left over at the end dropped.
fn base64url(part: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(part.len() * 3 / 4);
    let (mut held, mut bits) = (0_u32, 0_u32);
    for byte in part.bytes() {
        let six = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            _ => 63,
        };
        held = (held << 6) | u32::from(six);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((held >> bits) as u8);
            held &= (1 << bits) - 1;
        }
    }
    bytes
}

// The payload of the id token, read for its fields only: its signature is not checked and the token goes nowhere.
fn id_token_claims(token: &str) -> Option<JsValue> {
    let mut parts = token.split('.');
    let (head, payload, signature) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || ![head, payload, signature].iter().all(|p| is_token_part(p)) {
        return None;
    }
    let mut decoded = base64url(payload);
    let claims = parse(&String::from_utf8_lossy(&decoded)).ok();
    decoded.zeroize();
    claims.filter(|claims| claims.as_object().is_some())
}

fn text_of(value: Option<&JsValue>) -> Option<String> {
    String::from_utf16(value?.as_string()?.units()).ok()
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(at, byte)| {
            if matches!(at, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn name_of(value: Option<&JsValue>) -> Option<String> {
    let text = text_of(value)?;
    let name = js::trim(&text);
    (1..=MAX_NAME_LENGTH)
        .contains(&js::utf16_len(name))
        .then(|| name.to_owned())
}

fn riot_id_of(claims: &JsObject) -> Option<RiotId> {
    let account = claims.get("acct")?.as_object()?;
    let id = RiotId {
        game_name: name_of(account.get("game_name"))?,
        tag_line: name_of(account.get("tag_line"))?,
    };
    register_account_names([id.game_name.clone(), id.text()]);
    Some(id)
}

// Reads the session block. The rest of the file, the device cookie and the cookies of older builds, is never looked at.
pub fn read_session(text: &str) -> SessionRead {
    let lines = Lines::of(text);
    let block = match lines.session_block() {
        Ok(block) => block,
        Err(missing) => return SessionRead::Unexpected(Unexpected::NoBlock(missing)),
    };
    if let Some(value) = block.value {
        return if is_null(value) {
            SessionRead::SignedOut
        } else {
            SessionRead::Unexpected(Unexpected::NeitherNullNorBlock)
        };
    }
    if lines.cut_by_comment(&block) {
        return SessionRead::Unexpected(Unexpected::CommentInBlock);
    }
    let fields = lines.entries_of(Some(&block));
    if fields.is_empty() {
        return SessionRead::Unexpected(Unexpected::EmptyBlock);
    }
    // Of a key that is there twice the later one counts.
    let field = |key: &str| {
        let named = fields.iter().rev().find(|entry| entry.key == key);
        named.and_then(|entry| entry.value)
    };
    let Some(id_token) = unquote(field("id_token")) else {
        return SessionRead::Unexpected(Unexpected::NoIdToken);
    };
    let Some(refresh_token) = unquote(field("refresh_token")) else {
        return SessionRead::Unexpected(Unexpected::NoRefreshToken);
    };
    let Some(mut claims) = id_token_claims(id_token) else {
        return SessionRead::Unexpected(Unexpected::UnreadableIdToken);
    };
    let named = claims.as_object().and_then(|claims| {
        let account_id = text_of(claims.get("sub")).filter(|sub| is_uuid(sub))?;
        register_account_ids([&account_id]);
        Some((account_id.to_ascii_lowercase(), riot_id_of(claims)))
    });
    claims.wipe();
    let Some((account_id, riot_id)) = named else {
        return SessionRead::Unexpected(Unexpected::NoAccountId);
    };
    SessionRead::Session(RiotSession {
        account_id,
        riot_id,
        fingerprint: fingerprint_of(refresh_token),
        dpop_bound: match field("is_dpop_bound") {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        },
        write_count: field("refresh_token_write_count")
            .filter(|count| count.len() <= 9 && js::is_ascii_digits(count))
            .and_then(|count| count.parse().ok()),
    })
}

const BLOB_HEAD: &str = "{\"v\":1,\"file\":";

// The JSON that is encrypted and stored, byte for byte the Electron app's. It is the session: the caller seals it at once.
pub fn serialize_blob(blob: &RiotSessionBlob) -> String {
    // Written straight into a buffer sized for the longest it can be: one that grew would leave a part of the sign-in behind where nothing wipes it.
    let mut written = String::with_capacity(BLOB_HEAD.len() + quoted_capacity(&blob.file) + 1);
    written.push_str(BLOB_HEAD);
    quote_text(&mut written, &blob.file);
    written.push('}');
    written
}

// The stored copy back from its JSON. Only its shape is checked here: whether the file holds a session is `session_of_blob`'s question.
pub fn parse_blob(json: &str) -> Result<RiotSessionBlob, BlobError> {
    let mut raw = parse(json).map_err(|_| BlobError::NotReadable)?;
    let file = raw.as_object().and_then(|root| {
        let version = root.get("v")?.as_number()?;
        (version == 1.0).then(|| text_of(root.get("file")))?
    });
    raw.wipe();
    file.map(|file| RiotSessionBlob { file })
        .ok_or(BlobError::UnknownShape)
}

// The session a stored copy holds. A stored file the reader does not know is refused: nothing is written from it.
pub fn session_of_blob(blob: &RiotSessionBlob) -> Result<RiotSession, BlobError> {
    match read_session(&blob.file) {
        SessionRead::Session(session) => Ok(session),
        SessionRead::SignedOut => Err(BlobError::SignedOut),
        SessionRead::Unexpected(reason) => Err(BlobError::NoSession(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js_json::{JsString, stringify};

    const SIGNED_OUT: &str = "psl:\r\n    authorization:\r\n        riot-client: null\r\nrso-authenticator:\r\n    tdid:\r\n        value: \"placeholder\"\r\n";

    fn token(payload: &str) -> String {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in payload.as_bytes().chunks(3) {
            let held = chunk
                .iter()
                .fold(0_u32, |held, byte| (held << 8) | u32::from(*byte))
                << (8 * (3 - chunk.len()));
            for at in 0..=chunk.len() {
                out.push(alphabet[(held >> (18 - 6 * at)) as usize & 63] as char);
            }
        }
        format!("placeholderhead.{out}.placeholdersignature")
    }

    fn signed_in(sub: &str, refresh_token: &str) -> String {
        let id_token = token(&format!(
            r#"{{"sub":"{sub}","acct":{{"game_name":" Placeholder ","tag_line":"0000"}}}}"#
        ));
        let body = [
            format!("id_token: \"{id_token}\""),
            "is_dpop_bound: false".to_owned(),
            format!("refresh_token: \"{refresh_token}\""),
            "refresh_token_write_count: 7".to_owned(),
            "scopes:".to_owned(),
            "- \"openid\"".to_owned(),
        ];
        let body: Vec<&str> = body.iter().map(String::as_str).collect();
        splice_session_block(SIGNED_OUT, Some(&body)).unwrap()
    }

    #[test]
    fn a_sign_in_is_read_for_whose_it_is_and_a_digest_of_its_token() {
        let file = signed_in(
            "FFFFFFFF-FFFF-4FFF-8FFF-FFFFFFFFFFFF",
            "placeholder-refresh",
        );
        let SessionRead::Session(session) = read_session(&file) else {
            panic!("the file holds a session");
        };
        assert_eq!(session.account_id, "ffffffff-ffff-4fff-8fff-ffffffffffff");
        assert_eq!(session.fingerprint, fingerprint_of("placeholder-refresh"));
        assert_eq!(session.dpop_bound, Some(false));
        assert_eq!(session.write_count, Some(7));
        let riot_id = session.riot_id.expect("the token names a Riot ID");
        assert_eq!(riot_id.text(), "Placeholder#0000");
        assert!(read_session(SIGNED_OUT) == SessionRead::SignedOut);
    }

    #[test]
    fn a_file_the_reader_does_not_know_is_neither_a_session_nor_a_sign_out() {
        let refused = |text: &str, reason: Unexpected| {
            assert!(read_session(text) == SessionRead::Unexpected(reason));
            assert!(plan_sign_out(text).err() == Some(LeftAlone(reason)));
        };
        refused("", Unexpected::NoBlock("psl"));
        refused(
            "psl:\n  other: 1\n",
            Unexpected::NoBlock("psl.authorization"),
        );
        refused(
            "psl:\n  authorization:\n    riot-client: {}\n",
            Unexpected::NeitherNullNorBlock,
        );
        refused(
            "psl:\n  authorization:\n    riot-client:\n      refresh_token: \"r\"\n",
            Unexpected::NoIdToken,
        );
        refused(
            "psl:\n  authorization:\n    riot-client:\n      id_token: \"a.b.c\"\n      refresh_token: \"r\"\n",
            Unexpected::UnreadableIdToken,
        );
        let no_sub = signed_in("not-an-account-id", "placeholder-refresh");
        refused(&no_sub, Unexpected::NoAccountId);
    }

    #[test]
    fn a_sign_out_changes_the_block_and_no_other_byte_whatever_the_line_ends() {
        let file = signed_in(
            "11111111-1111-4111-8111-111111111111",
            "placeholder-refresh",
        );
        let mixed = file.replacen("rso-authenticator:\r\n", "rso-authenticator:\n", 1);
        let write = plan_sign_out(&mixed).unwrap();
        assert!(!write.noop);
        assert_eq!(
            write.text,
            SIGNED_OUT.replacen("rso-authenticator:\r\n", "rso-authenticator:\n", 1)
        );
        let again = plan_sign_out(&write.text).unwrap();
        assert!(again.noop && again.text == write.text);
        let with_mark = format!("\u{feff}{file}");
        assert_eq!(
            plan_sign_out(&with_mark).unwrap().text,
            format!("\u{feff}{SIGNED_OUT}")
        );
    }

    #[test]
    fn a_comment_at_the_left_that_cuts_the_session_block_is_refused_and_never_written() {
        let file = signed_in(
            "11111111-1111-4111-8111-111111111111",
            "placeholder-refresh",
        );
        let cut = file.replacen("            scopes:", "# a note\r\n            scopes:", 1);
        assert!(cut != file);
        assert!(read_session(&cut) == SessionRead::Unexpected(Unexpected::CommentInBlock));
        assert!(plan_sign_out(&cut).err() == Some(LeftAlone(Unexpected::CommentInBlock)));
        assert_eq!(
            splice_session_block(&cut, None).err(),
            Some(Unexpected::CommentInBlock)
        );
        // One that stands between the block and what follows it cuts nothing.
        let after = file.replacen("rso-authenticator:", "# a note\r\nrso-authenticator:", 1);
        assert!(matches!(read_session(&after), SessionRead::Session(_)));
        let write = plan_sign_out(&after).unwrap();
        assert!(
            write
                .text
                .contains("riot-client: null\r\n# a note\r\nrso-authenticator:")
        );
        let inside = file.replacen(
            "            scopes:",
            "            # a note\r\n            scopes:",
            1,
        );
        assert!(matches!(read_session(&inside), SessionRead::Session(_)));
    }

    // Every file the oracle recorded a write for, written both ways: what stands before the block's first line and after its last is the file's own, byte for byte.
    #[test]
    fn a_write_keeps_every_byte_outside_the_block_of_every_recorded_file() {
        use ol_testkit::vectors::{Vectors, text};
        let body = ["id_token: \"x.y.z\"", "refresh_token: \"r\""];
        let mut written = 0;
        for file in [
            "l1/riot/spliceSessionBlock.json",
            "l1/riot/planSignOut.json",
        ] {
            for case in Vectors::load(file).cases {
                if case.has_unpaired_surrogate() {
                    continue;
                }
                let before = text(&case.args[0]);
                let lines = Lines::of(&before);
                let Ok(entry) = lines.session_block() else {
                    continue;
                };
                let start = lines.spans[entry.line].0;
                let end = lines.spans[lines.block_end(&entry) - 1].1;
                for body in [None, Some(body.as_slice())] {
                    let after = splice_session_block(&before, body).unwrap();
                    let (head, tail) = (&before[..start], &before[end..]);
                    assert!(after.len() >= head.len() + tail.len(), "{}", case.id);
                    assert!(after.starts_with(head), "{}", case.id);
                    assert!(after.ends_with(tail), "{}", case.id);
                    let block = &after[head.len()..after.len() - tail.len()];
                    assert!(block.trim_start_matches(' ').starts_with("riot-client:"));
                    written += 1;
                }
            }
        }
        assert!(written > 400, "{written} writes were checked");
    }

    #[test]
    fn a_stored_copy_is_the_whole_file_and_reads_back_as_it_was_stored() {
        let file = signed_in(
            "11111111-1111-4111-8111-111111111111",
            "placeholder-refresh",
        );
        let stored = serialize_blob(&RiotSessionBlob { file: file.clone() });
        assert!(stored.starts_with("{\"v\":1,\"file\":\"psl:\\r\\n"));
        let blob = parse_blob(&stored).unwrap();
        assert!(blob.file == file);
        assert!(session_of_blob(&blob).is_ok());
        assert_eq!(parse_blob("not json").err(), Some(BlobError::NotReadable));
        assert_eq!(
            parse_blob(r#"{"v":2,"file":"x"}"#).err(),
            Some(BlobError::UnknownShape)
        );
        let signed_out = RiotSessionBlob {
            file: SIGNED_OUT.to_owned(),
        };
        assert_eq!(
            session_of_blob(&signed_out).err(),
            Some(BlobError::SignedOut)
        );
    }

    #[test]
    fn a_stored_copy_is_written_as_json_stringify_writes_it_into_a_buffer_that_never_grows() {
        let odd = "\"quoted\" back\\slash\r\n\ttab \u{8}\u{c}\u{1}\u{1f}\u{7f} caf\u{e9} \u{2028} \u{1f600}";
        for file in [odd.to_owned(), "\u{1}".repeat(64), String::new()] {
            let stored = serialize_blob(&RiotSessionBlob { file: file.clone() });
            let mut shaped = JsObject::default();
            shaped.insert(JsString::from("v"), JsValue::Number(1.0));
            shaped.insert(
                JsString::from("file"),
                JsValue::String(JsString::from(file.as_str())),
            );
            assert_eq!(stored, stringify(&JsValue::Object(shaped), 0));
            assert!(stored.len() <= BLOB_HEAD.len() + quoted_capacity(&file) + 1);
            assert!(parse_blob(&stored).unwrap().file == file);
        }
        let longest = serialize_blob(&RiotSessionBlob {
            file: "\u{1}".repeat(64),
        });
        assert_eq!(longest.len(), BLOB_HEAD.len() + 6 * 64 + 2 + 1);
    }
}
