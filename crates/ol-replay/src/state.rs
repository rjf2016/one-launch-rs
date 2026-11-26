// What a run left behind, read from the disk and the fake registry, and the rules that make two runs comparable: ids become aliases, the two folders become {launchers} and {data}, and a time becomes <ts> where the script's normalisation says so.

use std::fs;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ol_formats::battlenet::config::{parse_config, read_client_settings};
use ol_formats::epic::session::read_session_from_ini;
use ol_formats::riot::session::{SessionRead, read_session};
use ol_formats::steam::loginusers::parse_login_users;
use ol_model::{Ciphertext, LauncherId};
use ol_platform_fake::{FakeRegistry, RegistryValue};
use ol_testkit::vectors::sha256_hex;
use ol_vault::Vault;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Value, json};

use crate::ReplayError;

const STEAM_LOGIN_USERS: &str = "steam/config/loginusers.vdf";
const EPIC_SETTINGS: &str =
    "localappdata/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini";
const BATTLENET_CONFIG: &str = "appdata/Battle.net/Battle.net.config";
const RIOT_SESSION: &str = "localappdata/Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml";
// What a scripted client stamps into a file it writes: the Riot Client's two token times are the time of the write.
const STAMPS: [&str; 2] = [
    "last_token_creation_time: ",
    "original_token_creation_time: ",
];

pub struct Read {
    pub state: Value,
    // Every id the run made, with its alias.
    pub ids: Vec<(String, String)>,
}

pub struct Aliases<'a> {
    pub launchers_dir: &'a Path,
    pub data_dir: &'a Path,
    pub jobs: &'a [String],
    pub ids: Vec<(String, String)>,
}

// Every value of the fake registry, ordered by path, as the fake holds it.
pub fn registry(registry: &FakeRegistry) -> Vec<Value> {
    let mut values = registry.dump();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    values
        .into_iter()
        .map(|(path, value)| {
            let (kind, data) = match value {
                RegistryValue::Text(text) => ("REG_SZ", text),
                RegistryValue::Dword(number) => ("REG_DWORD", format!("{number:#x}")),
                RegistryValue::Binary(hex) => ("REG_BINARY", hex),
            };
            json!({ "path": path, "type": kind, "data": data })
        })
        .collect()
}

// A launcher's snapshots in the order they were taken.
pub fn snapshot_ids(data_dir: &Path, launcher: LauncherId) -> Vec<String> {
    let folder = data_dir.join("backups").join(launcher.as_str());
    let mut ids: Vec<String> = fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().join("manifest.json").is_file())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

// The ids and folders of a run, for the digest of a content once they are taken out of it.
pub struct Projector<'a> {
    ids: Vec<(String, String)>,
    launchers_dir: &'a Path,
    data_dir: &'a Path,
}

// A text with the digits after each stamp replaced, as the oracle masks what a scripted client stamps.
fn without_stamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((at, stamp)) = STAMPS
        .iter()
        .filter_map(|stamp| Some((rest.find(stamp)?, *stamp)))
        .min()
    {
        let after = at + stamp.len();
        let digits = rest[after..].bytes().take_while(u8::is_ascii_digit).count();
        out.push_str(&rest[..after]);
        if digits > 0 {
            out.push_str("<ms>");
        }
        rest = &rest[after + digits..];
    }
    out.push_str(rest);
    out
}

impl Projector<'_> {
    fn text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (id, alias) in &self.ids {
            if !id.is_empty() {
                out = out.replace(id.as_str(), alias);
            }
        }
        let out = without_folder(&out, self.launchers_dir, "{launchers}");
        without_folder(&out, self.data_dir, "{data}")
    }

    // The digest of a content with what a scripted client stamps, the two folders and the run's ids taken out: what two runs agree on where the bytes differ. Of bytes that are not text, the digest of the bytes.
    fn projection(&self, bytes: &[u8]) -> String {
        match std::str::from_utf8(bytes) {
            Ok(text) => sha256_hex(self.text(&without_stamps(text)).as_bytes()),
            Err(_) => sha256_hex(bytes),
        }
    }
}

fn sealed(value: ValueRef<'_>, vault: &dyn Vault, projector: &Projector<'_>) -> Value {
    let plain = match value {
        ValueRef::Blob(bytes) => vault.decrypt(&Ciphertext::new(bytes.to_vec())).ok(),
        _ => None,
    };
    let plain = plain.as_ref().map(|secret| secret.expose().as_bytes());
    json!({
        "present": !matches!(value, ValueRef::Null),
        "plaintextSha256": plain.map(sha256_hex),
        "projectionSha256": plain.map(|plain| projector.projection(plain)),
    })
}

fn rows(
    database: &Connection,
    sql: &str,
    vault: &dyn Vault,
    projector: &Projector<'_>,
) -> Result<Vec<Map<String, Value>>, ReplayError> {
    let mut statement = database.prepare(sql)?;
    let names: Vec<String> = statement.column_names().map(str::to_owned).collect();
    let mut found = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = found.next()? {
        let mut fields = Map::new();
        for (index, name) in names.iter().enumerate() {
            let value = row.get_ref(index)?;
            let shown = if name.ends_with("_enc") {
                sealed(value, vault, projector)
            } else if name == "fingerprint" {
                json!({ "sha256": value.as_str().unwrap_or_default() })
            } else {
                match value {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(number) => number.into(),
                    ValueRef::Real(number) => number.into(),
                    ValueRef::Text(text) => String::from_utf8_lossy(text).into_owned().into(),
                    ValueRef::Blob(bytes) => sha256_hex(bytes).into(),
                }
            };
            fields.insert(name.clone(), shown);
        }
        out.push(fields);
    }
    Ok(out)
}

// The bytes a snapshot holds of a file, as they were in the launcher's own: a copy stored encrypted is the vault's ciphertext of the file in base64.
fn original(folder: &Path, relative: &str, encrypted: bool, vault: &dyn Vault) -> Option<Vec<u8>> {
    if !encrypted {
        return fs::read(folder.join(relative)).ok();
    }
    let sealed = fs::read(folder.join(format!("{relative}.enc"))).ok()?;
    let opened = vault.decrypt(&Ciphertext::new(sealed)).ok()?;
    STANDARD.decode(opened.expose()).ok()
}

fn backups(
    data_dir: &Path,
    launcher: LauncherId,
    vault: &dyn Vault,
    projector: &Projector<'_>,
) -> Vec<Value> {
    snapshot_ids(data_dir, launcher)
        .into_iter()
        .filter_map(|id| {
            let folder = data_dir.join("backups").join(launcher.as_str()).join(&id);
            let manifest: Value =
                serde_json::from_slice(&fs::read(folder.join("manifest.json")).ok()?).ok()?;
            let files: Vec<Value> = manifest["files"]
                .as_array()?
                .iter()
                .map(|file| {
                    let relative = file["relative"].as_str().unwrap_or_default();
                    let encrypted = file["encrypted"] == true;
                    let stored = original(&folder, relative, encrypted, vault);
                    json!({
                        "relative": relative,
                        "source": file["source"],
                        "existed": file["existed"],
                        "bytes": file["bytes"],
                        "encrypted": encrypted,
                        "sha256": stored.as_deref().map(sha256_hex),
                        "projectionSha256": stored.as_deref().map(|bytes| projector.projection(bytes)),
                    })
                })
                .collect();
            let mut meta = manifest["meta"].clone();
            if let Some(tokens) = unsealed_tokens(&meta["unifiedAuth"], vault) {
                meta["unifiedAuth"] = tokens;
            }
            Some(json!({
                "id": id,
                "createdAt": manifest["createdAt"],
                "reason": manifest["reason"],
                "pinned": manifest.get("pinned").cloned().unwrap_or(Value::Null),
                "meta": meta,
                "files": files,
            }))
        })
        .collect()
}

// A Battle.net snapshot keeps the UnifiedAuth values as a JSON list in a text, each token sealed under `enc`. Shown as that list with the digest of what was sealed in the place of each `enc`.
fn unsealed_tokens(recorded: &Value, vault: &dyn Vault) -> Option<Value> {
    let list: Value = serde_json::from_str(recorded.as_str()?).ok()?;
    let shown = list.as_array()?.iter().map(|token| {
        let opened = STANDARD
            .decode(token["enc"].as_str().unwrap_or_default())
            .ok()
            .and_then(|sealed| vault.decrypt(&Ciphertext::new(sealed)).ok());
        json!({
            "name": token["name"],
            "plaintextSha256": opened.map(|token| sha256_hex(token.expose().as_bytes())),
        })
    });
    Some(shown.collect())
}

// Battle.net.config, and of what it says the client's settings as the app reads them. A file the app cannot read is still shown by its bytes.
fn battlenet_config(launchers_dir: &Path) -> Option<Value> {
    let bytes = fs::read(launchers_dir.join(BATTLENET_CONFIG)).ok()?;
    let parsed = parse_config(&String::from_utf8_lossy(&bytes))
        .ok()
        .map(|(doc, _)| {
            let client = read_client_settings(&doc);
            json!({
                "autoLogin": client.auto_login,
                "savedAccountNames": client.saved_account_names,
                "version": client.version,
            })
        });
    Some(json!({ "bytes": bytes.len(), "sha256": sha256_hex(&bytes), "parsed": parsed }))
}

fn login_users(launchers_dir: &Path) -> Option<Value> {
    let bytes = fs::read(launchers_dir.join(STEAM_LOGIN_USERS)).ok()?;
    let users = parse_login_users(&String::from_utf8_lossy(&bytes));
    let parsed: Option<Vec<Value>> = users.ok().map(|users| {
        users
            .iter()
        .map(|user| {
            json!({
                "steamId64": user.steam_id64,
                "accountName": user.account_name,
                "personaName": user.persona_name,
                "rememberPassword": user.remember_password,
                "mostRecent": user.most_recent,
                "timestamp": user.timestamp,
                "mostRecentKeys": user.most_recent_keys.iter().map(|key| key.as_str()).collect::<Vec<_>>(),
            })
        })
        .collect()
    });
    Some(json!({ "bytes": bytes.len(), "sha256": sha256_hex(&bytes), "parsed": parsed }))
}

// Epic's settings file, and of the session it remembers only the digests.
fn epic_settings(launchers_dir: &Path) -> Option<Value> {
    let bytes = fs::read(launchers_dir.join(EPIC_SETTINGS)).ok()?;
    let session = read_session_from_ini(&String::from_utf8_lossy(&bytes));
    let digest = |value: &Option<String>| value.as_ref().map(|value| sha256_hex(value.as_bytes()));
    Some(json!({
        "bytes": bytes.len(),
        "sha256": sha256_hex(&bytes),
        "parsed": {
            "remembered": session.remembered,
            "enable": session.enable,
            "dataSha256": digest(&session.data),
            "offlineDataSha256": digest(&session.offline_data),
        },
    }))
}

// The Riot Client's session file, and of the sign-in it holds only what the app's own reader says of it: whose it is and the digest of its token.
fn riot_session(launchers_dir: &Path, projector: &Projector<'_>) -> Option<Value> {
    let bytes = fs::read(launchers_dir.join(RIOT_SESSION)).ok()?;
    let parsed = match read_session(&String::from_utf8_lossy(&bytes)) {
        SessionRead::SignedOut => json!({ "kind": "signed-out" }),
        SessionRead::Unexpected(reason) => {
            json!({ "kind": "unexpected", "reason": reason.to_string() })
        }
        SessionRead::Session(session) => json!({
            "kind": "session",
            "session": {
                "accountId": session.account_id,
                "riotId": session.riot_id.map(|id| json!({
                    "gameName": id.game_name,
                    "tagLine": id.tag_line,
                })),
                "fingerprint": { "sha256": session.fingerprint },
                "dpopBound": session.dpop_bound,
                "writeCount": session.write_count,
            },
        }),
    };
    Some(json!({
        "bytes": bytes.len(),
        "sha256": sha256_hex(&bytes),
        "projectionSha256": projector.projection(&bytes),
        "parsed": parsed,
    }))
}

// The id of every row of a table, with what names the row.
fn named(database: &Connection, sql: &str) -> Result<Vec<Vec<String>>, ReplayError> {
    let mut statement = database.prepare(sql)?;
    let columns = statement.column_count();
    let mut found = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = found.next()? {
        let texts = (0..columns).map(|at| row.get::<_, String>(at));
        out.push(texts.collect::<Result<Vec<_>, _>>()?);
    }
    Ok(out)
}

// The final state, as the app left it once it had quit.
pub fn read(
    data_dir: &Path,
    launchers_dir: &Path,
    vault: &dyn Vault,
    registry: &[Value],
) -> Result<Read, ReplayError> {
    let database = Connection::open_with_flags(
        data_dir.join("onelaunch.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let version: i64 = database.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    // Every id of the run comes first: the digest of a content is taken with them out of it.
    let mut ids = Vec::new();
    for account in named(&database, "SELECT id, launcher, login FROM accounts")? {
        ids.push((
            account[0].clone(),
            format!("@acct:{}:{}", account[1], account[2]),
        ));
    }
    for game in named(
        &database,
        "SELECT id, launcher, launcher_game_id FROM games",
    )? {
        ids.push((game[0].clone(), format!("@game:{}:{}", game[1], game[2])));
    }
    let launches = named(&database, "SELECT id FROM launch_history ORDER BY rowid")?;
    for (index, launch) in launches.iter().enumerate() {
        ids.push((launch[0].clone(), format!("@history:{}", index + 1)));
    }
    for launcher in LauncherId::ALL {
        for (index, id) in snapshot_ids(data_dir, launcher).into_iter().enumerate() {
            ids.push((id, format!("@backup:{}:{}", launcher.as_str(), index + 1)));
        }
    }
    let mut by_length = ids.clone();
    by_length.sort_by_key(|(id, _)| std::cmp::Reverse(id.len()));
    let projector = Projector {
        ids: by_length,
        launchers_dir,
        data_dir,
    };
    let table = |sql: &str| rows(&database, sql, vault, &projector);
    let accounts = table(
        "SELECT id, launcher, display_name, login, secret_enc, launcher_uid, tags, notes, last_used_at, created_at, updated_at
         FROM accounts ORDER BY launcher, login",
    )?;
    let games = table(
        "SELECT id, launcher, launcher_game_id, name, install_path, installed, last_seen_at, needs_update, last_played_at
         FROM games ORDER BY launcher, launcher_game_id",
    )?;
    let history = table(
        "SELECT id, game_id, account_id, launched_at, result FROM launch_history ORDER BY rowid",
    )?;
    let settings =
        table("SELECT key, value FROM settings WHERE key <> 'window.bounds' ORDER BY key")?;
    let sessions = table(
        "SELECT s.account_id, s.launcher, s.launcher_uid, s.blob_enc, s.fingerprint, s.captured_at, s.verified_at, s.stale_reason
         FROM launcher_sessions s JOIN accounts a ON a.id = s.account_id ORDER BY a.launcher, a.login",
    )?;

    let artwork = table(
        "SELECT a.game_id, a.source, a.grid_path, a.hero_path, a.logo_path, a.fetched_at
         FROM artwork a JOIN games g ON g.id = a.game_id ORDER BY g.launcher, g.launcher_game_id",
    )?;
    let attempts = table(
        "SELECT t.game_id, t.attempted_at, t.error
         FROM artwork_attempts t JOIN games g ON g.id = t.game_id ORDER BY g.launcher, g.launcher_game_id",
    )?;

    let mut snapshots = Map::new();
    let copies = fs::read_dir(data_dir.join("backups").join("db"))
        .map(Iterator::count)
        .unwrap_or(0);
    snapshots.insert("db".to_owned(), json!({ "copies": copies }));
    for launcher in LauncherId::ALL {
        snapshots.insert(
            launcher.as_str().to_owned(),
            backups(data_dir, launcher, vault, &projector).into(),
        );
    }

    let journal = fs::read(data_dir.join("switch.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    let mut launcher_files = Map::new();
    if let Some(file) = login_users(launchers_dir) {
        launcher_files.insert(STEAM_LOGIN_USERS.to_owned(), file);
    }
    if let Some(file) = epic_settings(launchers_dir) {
        launcher_files.insert(EPIC_SETTINGS.to_owned(), file);
    }
    if let Some(file) = battlenet_config(launchers_dir) {
        launcher_files.insert(BATTLENET_CONFIG.to_owned(), file);
    }
    if let Some(file) = riot_session(launchers_dir, &projector) {
        launcher_files.insert(RIOT_SESSION.to_owned(), file);
    }
    let as_values = |rows: Vec<Map<String, Value>>| -> Value {
        rows.into_iter()
            .map(Value::Object)
            .collect::<Vec<_>>()
            .into()
    };
    let state = json!({
        "db": {
            "user_version": version,
            "tables": {
                "accounts": as_values(accounts),
                "artwork": as_values(artwork),
                "artwork_attempts": as_values(attempts),
                "games": as_values(games),
                "launch_history": as_values(history),
                "launcher_sessions": as_values(sessions),
                "settings": as_values(settings),
            },
        },
        "journal": journal,
        "backups": snapshots,
        "launcherFiles": launcher_files,
        "registry": registry,
    });
    Ok(Read { state, ids })
}

// A folder as text can be written three ways in what the app leaves behind: as it is, with forward slashes, and with every backslash doubled. Launchers also write it in lower case.
fn without_folder(text: &str, folder: &Path, name: &str) -> String {
    let plain = folder.to_string_lossy().into_owned();
    let mut out = text.to_owned();
    for spelling in [
        plain.replace('\\', "\\\\"),
        plain.clone(),
        plain.replace('\\', "/"),
    ] {
        let lower = spelling.to_ascii_lowercase();
        let mut replaced = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(at) = rest.to_ascii_lowercase().find(&lower) {
            // Lower-casing ASCII alone keeps every byte where it was, so the place is the same in both.
            replaced.push_str(&rest[..at]);
            replaced.push_str(name);
            rest = &rest[at + spelling.len()..];
        }
        replaced.push_str(rest);
        out = replaced;
    }
    out
}

fn rewrite(value: &mut Value, change: &dyn Fn(&str) -> String) {
    match value {
        Value::String(text) => *text = change(text),
        Value::Array(items) => items.iter_mut().for_each(|item| rewrite(item, change)),
        Value::Object(fields) => fields.values_mut().for_each(|field| rewrite(field, change)),
        _ => {}
    }
}

fn at_path(value: &mut Value, path: &[&str], change: &dyn Fn(&mut Value)) {
    let Some((first, rest)) = path.split_first() else {
        change(value);
        return;
    };
    match value {
        Value::Array(items) if *first == "*" => {
            items
                .iter_mut()
                .for_each(|item| at_path(item, rest, change));
        }
        Value::Object(fields) if *first == "*" => {
            fields
                .values_mut()
                .for_each(|field| at_path(field, rest, change));
        }
        Value::Object(fields) => {
            if let Some(field) = fields.get_mut(*first) {
                at_path(field, rest, change);
            }
        }
        _ => {}
    }
}

// Applies the oracle's rules to a report: every id and both folders wherever they appear, and <ts> at the places the script's normalisation file lists under the rule "ts". The write of a snapshot's manifest loses its digest, because a manifest holds its own time.
pub fn normalise(report: &mut Value, aliases: &Aliases<'_>, rules: &Value) {
    let mut ids = aliases.ids.clone();
    for (index, job) in aliases.jobs.iter().enumerate() {
        ids.push((job.clone(), format!("@job:{}", index + 1)));
    }
    ids.sort_by_key(|(id, _)| std::cmp::Reverse(id.len()));
    rewrite(report, &|text| {
        let mut out = text.to_owned();
        for (id, alias) in &ids {
            if !id.is_empty() {
                out = out.replace(id.as_str(), alias);
            }
        }
        let out = without_folder(&out, aliases.launchers_dir, "{launchers}");
        without_folder(&out, aliases.data_dir, "{data}")
    });
    if let Some(events) = report["events"].as_array_mut() {
        for event in events {
            let manifest = event["op"] == "file.write"
                && event["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("\\manifest.json"));
            if manifest && let Some(fields) = event.as_object_mut() {
                fields.remove("sha256");
            }
        }
    }
    let timed = rules["normalised"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|rule| rule["rule"] == "ts")
        .filter_map(|rule| rule["path"].as_str());
    for path in timed {
        let parts: Vec<&str> = path.split('/').skip(1).collect();
        at_path(report, &parts, &|value| {
            if value.is_string() {
                *value = "<ts>".into();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_whose_lower_case_is_longer_does_not_move_the_folder() {
        let folder = Path::new(r"D:\Scratch\Data");
        let text = r"İ D:\scratch\data\onelaunch.db";
        assert_eq!(
            without_folder(text, folder, "{data}"),
            r"İ {data}\onelaunch.db"
        );
    }

    // A launcher file the app cannot read is still told apart from a missing one and from another unreadable one.
    #[test]
    fn a_launcher_file_that_does_not_parse_is_reported_by_its_bytes() {
        let scratch = ol_testkit::Scratch::new("replay-unparsed");
        for (relative, read) in [
            (
                BATTLENET_CONFIG,
                battlenet_config as fn(&Path) -> Option<Value>,
            ),
            (STEAM_LOGIN_USERS, login_users),
        ] {
            let file = scratch.join(relative);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, b"{ not what the app reads").unwrap();
            let reported = read(&scratch.join("")).expect("the file is reported");
            assert_eq!(reported["bytes"], 24);
            assert_eq!(reported["sha256"], sha256_hex(b"{ not what the app reads"));
            assert!(reported["parsed"].is_null());
        }
    }
}
