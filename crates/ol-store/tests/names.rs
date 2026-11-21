#![forbid(unsafe_code)]

use ol_model::redact::with_registered_names;
use ol_store::Store;
use ol_testkit::Scratch;

#[test]
fn an_account_read_from_the_database_is_masked_from_then_on() {
    let scratch = Scratch::new("store-names");
    let path = scratch.join("onelaunch.db");
    drop(Store::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let scenario: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            ol_testkit::oracle("fixtures/dev-only/fake-launchers/all-four").join("scenario.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let steam_id = scenario["seed"]["accounts"][0]["launcherUid"]
        .as_str()
        .unwrap()
        .to_owned();
    conn.execute(
        "INSERT INTO accounts (id, launcher, display_name, login, launcher_uid, created_at, updated_at)
         VALUES ('a1', 'steam', 'Weekend Smurf', 'crowbar_77', ?1, 't', 't')",
        [&steam_id],
    )
    .unwrap();
    drop(conn);
    let text = format!("crowbar_77, Weekend Smurf, {steam_id}");
    let text = text.as_str();
    assert_eq!(
        with_registered_names(|names| names.mask(text).into_owned()),
        text
    );

    let (store, _) = Store::open(&path).unwrap();
    assert_eq!(store.accounts().unwrap().len(), 1);
    assert_eq!(
        with_registered_names(|names| names.mask(text).into_owned()),
        "[ACCOUNT], [ACCOUNT], [ACCOUNT]"
    );
}
