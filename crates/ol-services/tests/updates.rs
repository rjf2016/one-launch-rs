#![forbid(unsafe_code)]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use ol_formats::latest_yml::{self, Yaml};
use ol_services::updates::manifest::{
    ManifestFile, Refusal, RefusalReason, ReleaseKey, Verified, canonical_payload,
    ed25519_public_key, key_id_of, sha512_of_file, verify_manifest,
};
use ol_services::updates::release_keys::RELEASE_KEYS;
use ol_testkit::vectors::{Answer, UNPAIRED, bytes, plain, run};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};

const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

const SIZE_FORMS: [&str; 6] = [
    "accepted-size-hex",
    "accepted-size-float",
    "accepted-size-exponent",
    "size-plus-sign",
    "size-leading-zero",
    "size-octal",
];

const SIZE_FORM_REASON: &str =
    "triage X-19: the size is plain decimal digits only, so the port refuses this form";

struct Pair {
    signing: SigningKey,
    spki: String,
    id: String,
}

impl Pair {
    fn new(seed: u8) -> Self {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let mut der = ED25519_SPKI_PREFIX.to_vec();
        der.extend(signing.verifying_key().to_bytes());
        Self {
            signing,
            spki: STANDARD.encode(&der),
            id: key_id_of(&der),
        }
    }

    fn key(&self) -> ReleaseKey<'_> {
        ReleaseKey {
            id: &self.id,
            spki: &self.spki,
        }
    }
}

struct Keys {
    primary: Pair,
    backup: Pair,
    stranger: Pair,
}

impl Keys {
    fn new() -> Self {
        Self {
            primary: Pair::new(1),
            backup: Pair::new(2),
            stranger: Pair::new(3),
        }
    }

    fn pinned(&self) -> [ReleaseKey<'_>; 2] {
        [self.primary.key(), self.backup.key()]
    }
}

fn sha512_base64(bytes: &[u8]) -> String {
    STANDARD.encode(Sha512::digest(bytes))
}

fn file() -> ManifestFile {
    ManifestFile {
        url: "OneLaunch-1.0.0-beta.2-setup.exe".to_owned(),
        sha512: sha512_base64(b"installer bytes"),
        size: 112_000_000,
    }
}

fn text(value: &str) -> Yaml {
    Yaml::Text(value.to_owned())
}

fn map(entries: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn entry(file: &ManifestFile) -> Yaml {
    entry_with(file, Vec::new())
}

fn entry_with(file: &ManifestFile, extra: Vec<(&str, Yaml)>) -> Yaml {
    let mut entries = vec![
        ("url", text(&file.url)),
        ("sha512", text(&file.sha512)),
        ("size", Yaml::Number(file.size.to_string())),
    ];
    entries.extend(extra);
    map(entries)
}

fn sign(version: &str, file: &ManifestFile, signer: &Pair) -> (String, String) {
    let signature = signer
        .signing
        .sign(canonical_payload(version, file).as_bytes());
    (signer.id.clone(), STANDARD.encode(signature.to_bytes()))
}

fn manifest_signed(
    overrides: Vec<(&str, Option<Yaml>)>,
    signer: &Pair,
    file: &ManifestFile,
) -> Yaml {
    let (key_id, signature) = sign("1.0.0-beta.2", file, signer);
    let mut entries: Vec<(String, Yaml)> = vec![
        ("version".to_owned(), text("1.0.0-beta.2")),
        ("files".to_owned(), Yaml::List(vec![entry(file)])),
        ("path".to_owned(), text(&file.url)),
        ("sha512".to_owned(), text(&file.sha512)),
        ("releaseDate".to_owned(), text("2026-10-01T00:00:00.000Z")),
        ("keyId".to_owned(), Yaml::Text(key_id)),
        ("signature".to_owned(), Yaml::Text(signature)),
    ];
    for (name, value) in overrides {
        let at = entries.iter().position(|(key, _)| key == name);
        match (at, value) {
            (Some(at), Some(value)) => entries[at].1 = value,
            (Some(at), None) => {
                entries.remove(at);
            }
            (None, Some(value)) => entries.push((name.to_owned(), value)),
            (None, None) => {}
        }
    }
    Yaml::Map(entries)
}

fn manifest(keys: &Keys, overrides: Vec<(&str, Option<Yaml>)>) -> Yaml {
    manifest_signed(overrides, &keys.primary, &file())
}

fn reason(answer: Result<Verified, Refusal>) -> RefusalReason {
    answer.expect_err("a refusal").reason
}

fn as_json(answer: &Result<Verified, Refusal>) -> Value {
    match answer {
        Ok(verified) => json!({
            "ok": true,
            "version": verified.version,
            "file": {
                "url": verified.file.url,
                "sha512": verified.file.sha512,
                "size": verified.file.size,
            },
            "keyId": verified.key_id,
        }),
        Err(refusal) => json!({
            "ok": false,
            "reason": refusal.reason.as_str(),
            "detail": refusal.detail,
        }),
    }
}

#[test]
fn every_recorded_manifest_gets_the_recorded_answer() {
    let skipped: Vec<(&str, &str)> = SIZE_FORMS
        .iter()
        .map(|id| (*id, SIZE_FORM_REASON))
        .collect();
    run("l1/core/verifyManifest.json", &skipped, |case| {
        let manifest = match latest_yml::read(&bytes(&case.args[0])) {
            Ok(manifest) => manifest,
            Err(unreadable) => {
                return Answer::Error(json!({ "unreadable": unreadable.to_string() }));
            }
        };
        let trusted: Vec<(String, String)> = case.args[1]
            .as_array()
            .expect("a list of keys")
            .iter()
            .map(|key| {
                (
                    key["id"].as_str().expect("an id").to_owned(),
                    key["spki"].as_str().expect("an spki").to_owned(),
                )
            })
            .collect();
        let keys: Vec<ReleaseKey<'_>> = trusted
            .iter()
            .map(|(id, spki)| ReleaseKey { id, spki })
            .collect();
        let answer = verify_manifest(&manifest, &keys);
        if SIZE_FORMS.contains(&case.id.as_str()) {
            assert_eq!(
                case.out().expect("a recorded answer")["ok"],
                true,
                "{}: the Electron app accepts this size form",
                case.id
            );
            assert_eq!(
                answer,
                Err(Refusal {
                    reason: RefusalReason::Malformed,
                    detail: "the release file has no size".to_owned(),
                }),
                "{}",
                case.id
            );
            return Answer::Skip(SIZE_FORM_REASON);
        }
        Answer::Out(as_json(&answer))
    });
}

#[test]
fn every_recorded_payload_is_built_as_recorded() {
    let skipped = [
        (
            "size-exponent",
            "a size of 1e21, past what a u64 holds and what the size check passes",
        ),
        (
            "size-fraction",
            "a size that is not a whole number, which the size check refuses",
        ),
        (
            "size-tiny",
            "a size that is not a whole number, which the size check refuses",
        ),
        (
            "size-nan",
            "a size of NaN, which only a JavaScript number holds",
        ),
        (
            "size-string",
            "a size given as text, which the size check refuses",
        ),
        (
            "missing-fields",
            "fields that are undefined, which only JavaScript has",
        ),
        ("lone-surrogate", UNPAIRED),
    ];
    run("l1/core/canonicalPayload.json", &skipped, |case| {
        if let Some((_, reason)) = skipped.iter().find(|(id, _)| *id == case.id) {
            return Answer::Skip(reason);
        }
        let version = plain(&case.args[0]);
        let entry = plain(&case.args[1]);
        let file = ManifestFile {
            url: entry["url"].as_str().expect("a url").to_owned(),
            sha512: entry["sha512"].as_str().expect("a hash").to_owned(),
            size: entry["size"].as_u64().expect("a whole size"),
        };
        Answer::Out(canonical_payload(version.as_str().expect("a version"), &file).into())
    });
}

#[test]
fn accepts_a_manifest_signed_by_a_pinned_key_and_reports_what_it_covers() {
    let keys = Keys::new();
    assert_eq!(
        verify_manifest(&manifest(&keys, Vec::new()), &keys.pinned()),
        Ok(Verified {
            version: "1.0.0-beta.2".to_owned(),
            file: file(),
            key_id: keys.primary.id.clone(),
        })
    );
}

#[test]
fn accepts_the_backup_key_so_the_primary_can_be_retired() {
    let keys = Keys::new();
    let signed = manifest_signed(Vec::new(), &keys.backup, &file());
    assert_eq!(
        verify_manifest(&signed, &keys.pinned()).map(|verified| verified.key_id),
        Ok(keys.backup.id.clone())
    );
}

#[test]
fn signs_a_fixed_newline_separated_payload() {
    let file = ManifestFile {
        url: "a.exe".to_owned(),
        sha512: "h".to_owned(),
        size: 7,
    };
    assert_eq!(
        canonical_payload("1.2.3", &file),
        "one-launch-update-v1\n1.2.3\na.exe\nh\n7\n"
    );
}

#[test]
fn refuses_a_manifest_whose_version_changed_after_signing() {
    let keys = Keys::new();
    let changed = manifest(&keys, vec![("version", Some(text("1.0.0-beta.3")))]);
    assert_eq!(
        reason(verify_manifest(&changed, &keys.pinned())),
        RefusalReason::BadSignature
    );
}

#[test]
fn refuses_a_manifest_whose_file_name_changed_after_signing() {
    let keys = Keys::new();
    let renamed = ManifestFile {
        url: "OneLaunch-evil-setup.exe".to_owned(),
        ..file()
    };
    let changed = manifest(
        &keys,
        vec![("files", Some(Yaml::List(vec![entry(&renamed)])))],
    );
    assert_eq!(
        reason(verify_manifest(&changed, &keys.pinned())),
        RefusalReason::BadSignature
    );
}

#[test]
fn refuses_a_manifest_whose_hash_changed_after_signing() {
    let keys = Keys::new();
    let rehashed = ManifestFile {
        sha512: sha512_base64(b"other"),
        ..file()
    };
    let changed = manifest(
        &keys,
        vec![("files", Some(Yaml::List(vec![entry(&rehashed)])))],
    );
    assert_eq!(
        reason(verify_manifest(&changed, &keys.pinned())),
        RefusalReason::BadSignature
    );
}

#[test]
fn refuses_a_manifest_whose_size_changed_after_signing() {
    let keys = Keys::new();
    let resized = ManifestFile {
        size: file().size + 1,
        ..file()
    };
    let changed = manifest(
        &keys,
        vec![("files", Some(Yaml::List(vec![entry(&resized)])))],
    );
    assert_eq!(
        reason(verify_manifest(&changed, &keys.pinned())),
        RefusalReason::BadSignature
    );
}

#[test]
fn refuses_a_key_it_does_not_pin_even_when_it_names_a_pinned_key_id() {
    let keys = Keys::new();
    let foreign = manifest_signed(Vec::new(), &keys.stranger, &file());
    assert_eq!(
        verify_manifest(&foreign, &keys.pinned()),
        Err(Refusal {
            reason: RefusalReason::UnknownKey,
            detail: format!(
                "the release is signed with a key this build does not trust ({})",
                keys.stranger.id
            ),
        })
    );
    let forged = manifest_signed(
        vec![("keyId", Some(text(&keys.primary.id)))],
        &keys.stranger,
        &file(),
    );
    assert_eq!(
        reason(verify_manifest(&forged, &keys.pinned())),
        RefusalReason::BadSignature
    );
}

#[test]
fn refuses_a_signature_by_one_pinned_key_that_names_another() {
    let keys = Keys::new();
    let crossed = manifest_signed(
        vec![("keyId", Some(text(&keys.primary.id)))],
        &keys.backup,
        &file(),
    );
    assert_eq!(
        reason(verify_manifest(&crossed, &keys.pinned())),
        RefusalReason::BadSignature
    );
}

#[test]
fn refuses_everything_when_the_build_pins_no_key() {
    let keys = Keys::new();
    assert_eq!(
        verify_manifest(&manifest(&keys, Vec::new()), &[]),
        Err(Refusal {
            reason: RefusalReason::NoKeys,
            detail: "this build trusts no release key".to_owned(),
        })
    );
}

#[test]
fn refuses_an_unsigned_manifest_and_malformed_signatures() {
    let keys = Keys::new();
    let pinned = keys.pinned();
    let unsigned = manifest(&keys, vec![("signature", None), ("keyId", None)]);
    assert_eq!(
        reason(verify_manifest(&unsigned, &pinned)),
        RefusalReason::Unsigned
    );
    let short = manifest(&keys, vec![("signature", Some(text("abc")))]);
    assert_eq!(
        reason(verify_manifest(&short, &pinned)),
        RefusalReason::Malformed
    );
    let no_signature = manifest(&keys, vec![("signature", None)]);
    assert_eq!(
        reason(verify_manifest(&no_signature, &pinned)),
        RefusalReason::Malformed
    );
    let not_hex = manifest(&keys, vec![("keyId", Some(text("NOT-HEX")))]);
    assert_eq!(
        reason(verify_manifest(&not_hex, &pinned)),
        RefusalReason::Malformed
    );
}

#[test]
fn refuses_shapes_the_updater_would_treat_differently_from_what_was_signed() {
    let keys = Keys::new();
    let pinned = keys.pinned();
    let file = file();
    let two = manifest(
        &keys,
        vec![("files", Some(Yaml::List(vec![entry(&file), entry(&file)])))],
    );
    assert_eq!(
        reason(verify_manifest(&two, &pinned)),
        RefusalReason::Unsupported
    );
    let none = manifest(&keys, vec![("files", Some(Yaml::List(Vec::new())))]);
    assert_eq!(
        reason(verify_manifest(&none, &pinned)),
        RefusalReason::Unsupported
    );
    let admin = entry_with(&file, vec![("isAdminRightsRequired", Yaml::Bool(true))]);
    let admin = manifest(&keys, vec![("files", Some(Yaml::List(vec![admin])))]);
    assert_eq!(
        reason(verify_manifest(&admin, &pinned)),
        RefusalReason::Unsupported
    );
    let packages = map(vec![(
        "x64",
        map(vec![("path", text("p.7z")), ("sha512", text(&file.sha512))]),
    )]);
    let web = manifest(&keys, vec![("packages", Some(packages))]);
    assert_eq!(
        reason(verify_manifest(&web, &pinned)),
        RefusalReason::Unsupported
    );
}

#[test]
fn refuses_fields_that_could_smuggle_a_second_line_into_the_signed_payload() {
    let keys = Keys::new();
    let pinned = keys.pinned();
    let tricky = ManifestFile {
        url: "a.exe\n1.0.0".to_owned(),
        ..file()
    };
    let smuggled = manifest_signed(Vec::new(), &keys.primary, &tricky);
    assert_eq!(
        reason(verify_manifest(&smuggled, &pinned)),
        RefusalReason::Malformed
    );
    let version = manifest(&keys, vec![("version", Some(text("1.0.0\n")))]);
    assert_eq!(
        reason(verify_manifest(&version, &pinned)),
        RefusalReason::Malformed
    );
    let climbing = ManifestFile {
        url: "../OneLaunch.exe".to_owned(),
        ..file()
    };
    let climbing = manifest(
        &keys,
        vec![("files", Some(Yaml::List(vec![entry(&climbing)])))],
    );
    assert_eq!(
        reason(verify_manifest(&climbing, &pinned)),
        RefusalReason::Malformed
    );
    let mut negative = entry(&file());
    if let Yaml::Map(entries) = &mut negative {
        entries[2].1 = Yaml::Number("-1".to_owned());
    }
    let negative = manifest(&keys, vec![("files", Some(Yaml::List(vec![negative])))]);
    assert_eq!(
        reason(verify_manifest(&negative, &pinned)),
        RefusalReason::Malformed
    );
    assert_eq!(
        reason(verify_manifest(&Yaml::Null, &pinned)),
        RefusalReason::Malformed
    );
}

#[test]
fn pins_only_well_formed_ed25519_keys_whose_ids_match_their_bytes() {
    assert!(!RELEASE_KEYS.is_empty());
    for key in RELEASE_KEYS {
        let der = STANDARD.decode(key.spki).expect("base64");
        assert!(
            ed25519_public_key(&der).is_some(),
            "{} is not an Ed25519 key",
            key.id
        );
        assert_eq!(key.id, key_id_of(&der));
    }
}

#[test]
fn hashes_the_same_bytes_the_same_way_streamed_and_in_one_go() {
    let scratch = ol_testkit::Scratch::new("updates-hash");
    let path = scratch.join("setup.exe");
    let bytes = vec![7_u8; 3 * 1024 * 1024 + 17];
    std::fs::write(&path, &bytes).expect("the file is written");
    assert_eq!(
        sha512_of_file(&path).expect("the file is read"),
        sha512_base64(&bytes)
    );
}

#[test]
fn a_file_that_cannot_be_read_is_an_error() {
    let scratch = ol_testkit::Scratch::new("updates-hash-missing");
    assert!(sha512_of_file(&scratch.join("missing.exe")).is_err());
}

#[test]
fn only_an_ed25519_spki_gives_a_public_key() {
    let pair = Pair::new(9);
    let der = STANDARD.decode(&pair.spki).expect("base64");
    assert_eq!(
        ed25519_public_key(&der).map(|key| key.to_bytes()),
        Some(pair.signing.verifying_key().to_bytes())
    );
    assert_eq!(ed25519_public_key(&der[..43]), None);
    let mut longer = der.clone();
    longer.push(0);
    assert_eq!(ed25519_public_key(&longer), None);
    let mut other = der.clone();
    other[8] = 0x71;
    assert_eq!(ed25519_public_key(&other), None);
    assert_eq!(ed25519_public_key(b"garbage"), None);
}

#[test]
fn a_size_must_be_plain_decimal_digits_within_the_safe_range() {
    let keys = Keys::new();
    let pinned = keys.pinned();
    for size in ["50", "9007199254740991"] {
        let file = ManifestFile {
            size: size.parse().expect("a number"),
            ..file()
        };
        let signed = manifest_signed(Vec::new(), &keys.primary, &file);
        assert!(verify_manifest(&signed, &pinned).is_ok(), "{size}");
    }
    let refused = [
        Yaml::Number("0".to_owned()),
        Yaml::Number("-1".to_owned()),
        Yaml::Number("050".to_owned()),
        Yaml::Number("+50".to_owned()),
        Yaml::Number("0x32".to_owned()),
        Yaml::Number("0o62".to_owned()),
        Yaml::Number("5e1".to_owned()),
        Yaml::Number("50.0".to_owned()),
        Yaml::Number(".inf".to_owned()),
        Yaml::Number("9007199254740992".to_owned()),
        Yaml::Number("99999999999999999999999".to_owned()),
        text("50"),
        Yaml::Null,
    ];
    for size in refused {
        let mut sized = entry(&file());
        if let Yaml::Map(entries) = &mut sized {
            entries[2].1 = size.clone();
        }
        let manifest = manifest(&keys, vec![("files", Some(Yaml::List(vec![sized])))]);
        assert_eq!(
            verify_manifest(&manifest, &pinned),
            Err(Refusal {
                reason: RefusalReason::Malformed,
                detail: "the release file has no size".to_owned(),
            }),
            "{size:?}"
        );
    }
}

#[test]
fn a_signature_is_read_as_node_reads_base64() {
    let keys = Keys::new();
    let signed = manifest(&keys, Vec::new());
    let Some(Yaml::Text(signature)) = signed.get("signature") else {
        panic!("a signature");
    };
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let last = signature.as_bytes()[85];
    let at = alphabet
        .iter()
        .position(|c| *c == last)
        .expect("a base64 character");
    let mut loose = signature.clone();
    loose.replace_range(85..86, &char::from(alphabet[at ^ 1]).to_string());
    assert_ne!(&loose, signature);
    assert_eq!(STANDARD.decode(&loose).ok(), None);
    let loose = manifest(&keys, vec![("signature", Some(Yaml::Text(loose)))]);
    assert!(verify_manifest(&loose, &keys.pinned()).is_ok());
}
