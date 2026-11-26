#![forbid(unsafe_code)]

// Every case of oracle/l1 for the order of versions and the choice of the notes a start shows, and the notes themselves against the ones the Electron app ships.

use ol_services::whats_new::{RELEASES, compare_versions, pick};
use ol_testkit::vectors::{Answer, UNPAIRED, plain, run};
use serde_json::{Value, json};

#[test]
fn versions_order_as_in_the_electron_app() {
    let skipped = [
        ("lone-surrogate-pre.ab", UNPAIRED),
        ("lone-surrogate-pre.ba", UNPAIRED),
    ];
    run("l1/core/compareVersions.json", &skipped, |case| {
        let (a, b) = (plain(&case.args[0]), plain(&case.args[1]));
        let order = compare_versions(a.as_str().expect("text"), b.as_str().expect("text"));
        Answer::Out((order as i8).into())
    });
}

#[test]
fn a_start_picks_the_notes_the_electron_app_picks() {
    run("l1/core/pickWhatsNew.json", &[], |case| {
        let releases = plain(&case.args[0]);
        let releases = releases.as_array().expect("releases");
        let versions: Vec<&str> = releases
            .iter()
            .map(|release| release["version"].as_str().expect("a version"))
            .collect();
        let (current, seen) = (plain(&case.args[1]), plain(&case.args[2]));
        let picked = pick(&versions, current.as_str(), seen.as_str());
        Answer::Out(picked.map_or(Value::Null, |at| releases[at].clone()))
    });
}

#[test]
fn the_notes_are_the_ones_the_electron_app_ships() {
    let recorded: Value = serde_json::from_str(
        &std::fs::read_to_string(ol_testkit::oracle("assets/whats-new.json")).expect("the notes"),
    )
    .expect("JSON");
    let ours: Vec<Value> = RELEASES
        .iter()
        .map(|release| {
            let mut value = json!({
                "version": release.version,
                "date": release.date,
                "sections": release.sections.iter().map(|section| {
                    let mut value = json!({
                        "heading": section.heading,
                        "items": section.items.iter().map(|item| {
                            let mut value = json!({ "lead": item.lead });
                            if let Some(detail) = item.detail {
                                value["detail"] = detail.into();
                            }
                            value
                        }).collect::<Vec<_>>(),
                    });
                    if let Some(tone) = section.tone {
                        value["tone"] = tone.as_str().into();
                    }
                    value
                }).collect::<Vec<_>>(),
            });
            if let Some(intro) = release.intro {
                value["intro"] = intro.into();
            }
            if let Some(image) = &release.image {
                value["image"] = json!({ "file": image.file });
                if let Some(alt) = image.alt {
                    value["image"]["alt"] = alt.into();
                }
            }
            value
        })
        .collect();
    assert_eq!(Value::Array(ours), recorded["releases"]);
}
