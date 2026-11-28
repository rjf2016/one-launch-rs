#![forbid(unsafe_code)]

use std::time::Duration;

use ol_formats::latest_yml::Yaml;
use ol_platform_api::http::HttpError;
use ol_platform_api::update_http::{ReleaseTag, UpdateAsk};
use ol_platform_fake::update_http::{
    FakeAnswer, Part, ScriptedUpdates, UpdateAsked, UpdateCall, file, in_parts, json, redirect,
    status,
};
use ol_services::updates::check::{ANSWER_LIMIT, Checked, WAIT, check};
use ol_services::updates::failure::UpdateFailure;
use semver::Version;

const RELEASES: &str = "https://github.com/rjf2016/one-launch/releases";
const LATEST: &str = "https://github.com/rjf2016/one-launch/releases/latest";
const TAG: &str = "v1.0.0-beta.4";
const REDIRECTED: &str = "the update server redirected to an address One Launch does not ask";
const UNREADABLE: &str = "the release manifest could not be read";

type Reply = Box<dyn Fn() -> Result<FakeAnswer, HttpError> + Send + Sync>;

fn reply(answer: impl Fn() -> Result<FakeAnswer, HttpError> + Send + Sync + 'static) -> Reply {
    Box::new(answer)
}

fn answer(make: impl Fn() -> FakeAnswer + Send + Sync + 'static) -> Reply {
    reply(move || Ok(make()))
}

fn server(routes: Vec<(String, Reply)>) -> ScriptedUpdates {
    ScriptedUpdates::new(move |request| {
        let url = request.url().to_string();
        let (_, reply) = routes
            .iter()
            .find(|(at, _)| *at == url)
            .unwrap_or_else(|| panic!("nothing answers {url}"));
        reply()
    })
}

fn manifest_url(tag: &str) -> String {
    format!("{RELEASES}/download/{tag}/latest.yml")
}

fn page(tag: &str) -> String {
    format!("{RELEASES}/tag/{tag}")
}

fn release_json(tag: &str) -> String {
    format!("{{\"tag_name\":\"{tag}\",\"name\":\"One Launch\",\"draft\":false}}")
}

fn latest_json() -> Reply {
    answer(|| json(200, &release_json(TAG)))
}

fn manifest_text(text: &'static str) -> Reply {
    answer(move || file(text.as_bytes().to_vec()))
}

fn good_manifest() -> Reply {
    manifest_text("version: 1.0.0-beta.4\nfiles:\n  - url: OneLaunch-1.0.0-beta.4-setup.exe\n")
}

fn running() -> Version {
    Version::parse("1.0.0-beta.3").unwrap()
}

fn checked(http: &ScriptedUpdates) -> Result<Checked, UpdateFailure> {
    check(http, &running())
}

fn newer_tag(result: Result<Checked, UpdateFailure>) -> String {
    match result {
        Ok(Checked::Newer { tag, .. }) => tag.as_str().to_owned(),
        other => panic!("expected a newer release, got {other:?}"),
    }
}

fn failure(result: Result<Checked, UpdateFailure>) -> String {
    match result {
        Err(failure) => failure.to_string(),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn the_eight_cases_of_describe_update_error_give_the_electron_sentences() {
    let cases: [(UpdateFailure, &str); 8] = [
        (
            UpdateFailure::from(HttpError::Failed("the host was not found")),
            "the update server could not be reached",
        ),
        (
            UpdateFailure::from(HttpError::Failed("the connection failed")),
            "the update server could not be reached",
        ),
        (
            UpdateFailure::NoRelease,
            "no release has been published yet",
        ),
        (
            UpdateFailure::NoManifest,
            "the latest release has no update manifest",
        ),
        (
            UpdateFailure::Status(404),
            "the update server answered HTTP 404",
        ),
        (
            UpdateFailure::Mismatch,
            "the downloaded file does not match the release",
        ),
        (
            UpdateFailure::Other("first line\nsecond line".to_owned()),
            "first line",
        ),
        (
            UpdateFailure::Other("x".repeat(300)),
            &format!("{}…", "x".repeat(159)),
        ),
    ];
    for (failure, sentence) in cases {
        assert_eq!(failure.to_string(), sentence, "{failure:?}");
    }
}

#[test]
fn a_text_is_cut_by_characters_trimmed_and_never_empty() {
    for (text, sentence) in [
        (String::new(), "unknown error".to_owned()),
        ("   \r\nsecond".to_owned(), "unknown error".to_owned()),
        ("  padded  \r\nsecond".to_owned(), "padded".to_owned()),
        ("x".repeat(160), "x".repeat(160)),
        ("x".repeat(161), format!("{}…", "x".repeat(159))),
        ("é".repeat(160), "é".repeat(160)),
        ("é".repeat(161), format!("{}…", "é".repeat(159))),
    ] {
        assert_eq!(
            UpdateFailure::Other(text.clone()).to_string(),
            sentence,
            "{text:?}"
        );
    }
}

#[test]
fn every_network_failure_reads_as_unreachable_but_a_failed_secure_connection() {
    for error in [
        HttpError::Timeout,
        HttpError::Failed("the host was not found"),
        HttpError::Failed("the connection failed"),
        HttpError::Failed("the answer was not HTTP"),
        HttpError::Failed("the request failed"),
    ] {
        assert_eq!(UpdateFailure::from(error), UpdateFailure::Unreachable);
    }
    assert_eq!(
        UpdateFailure::from(HttpError::Failed("TLS failed")).to_string(),
        "the secure connection to the update server failed"
    );
    assert_eq!(UpdateFailure::Redirected.to_string(), REDIRECTED);
    assert_eq!(UpdateFailure::UnreadableManifest.to_string(), UNREADABLE);
    assert_eq!(
        UpdateFailure::Disk.to_string(),
        "the update could not be saved to disk"
    );
}

#[test]
fn a_json_answer_leads_to_the_manifest_of_its_tag_with_the_fixed_headers() {
    let http = server(vec![
        (LATEST.to_owned(), latest_json()),
        (manifest_url(TAG), good_manifest()),
    ]);
    match checked(&http) {
        Ok(Checked::Newer {
            tag,
            version,
            manifest,
        }) => {
            assert_eq!(tag, ReleaseTag::new(TAG).unwrap());
            assert_eq!(version, Version::parse("1.0.0-beta.4").unwrap());
            assert_eq!(
                manifest.get("version"),
                Some(&Yaml::Text("1.0.0-beta.4".to_owned()))
            );
        }
        other => panic!("expected a newer release, got {other:?}"),
    }
    assert_eq!(
        http.asked(),
        vec![
            UpdateAsked {
                url: LATEST.to_owned(),
                ask: UpdateAsk::LatestRelease,
                headers: vec![
                    ("accept", "application/json"),
                    ("cache-control", "no-cache"),
                    ("user-agent", "electron-builder"),
                ],
                call: UpdateCall::Send,
            },
            UpdateAsked {
                url: manifest_url(TAG),
                ask: UpdateAsk::Manifest,
                headers: vec![
                    ("accept", "*/*"),
                    ("cache-control", "no-cache"),
                    ("user-agent", "electron-builder"),
                ],
                call: UpdateCall::Send,
            },
        ]
    );
}

#[test]
fn the_limits_and_the_wait_are_the_decided_ones() {
    assert_eq!(ANSWER_LIMIT, 64 * 1024);
    assert_eq!(WAIT, Duration::from_secs(30));
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let http = ScriptedUpdates::new({
        let seen = std::sync::Arc::clone(&seen);
        move |request| {
            seen.lock().unwrap().push((request.limit, request.timeout));
            Ok(match request.ask() {
                UpdateAsk::LatestRelease => json(200, &release_json(TAG)),
                _ => file(b"version: 1.0.0-beta.4\n".to_vec()),
            })
        }
    });
    newer_tag(checked(&http));
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(64 * 1024, Duration::from_secs(30)); 2]
    );
}

#[test]
fn a_redirect_to_the_release_page_is_followed_to_its_json() {
    for location in [page(TAG), format!("/rjf2016/one-launch/releases/tag/{TAG}")] {
        let http = server(vec![
            (LATEST.to_owned(), answer(move || redirect(302, &location))),
            (page(TAG), latest_json()),
            (manifest_url(TAG), good_manifest()),
        ]);
        assert_eq!(newer_tag(checked(&http)), TAG);
        assert_eq!(
            http.urls(),
            vec![LATEST.to_owned(), page(TAG), manifest_url(TAG)]
        );
        let asked = http.asked();
        assert_eq!(asked[1].ask, UpdateAsk::LatestRelease);
        assert_eq!(asked[1].headers[0], ("accept", "application/json"));
    }
}

#[test]
fn the_manifest_is_followed_to_the_file_hosts_and_the_signed_query_arrives_unchanged() {
    let assets = "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r&sv=2018-11-09&sr=b&spr=https&se=2026-11-27T10%3A00%3A00Z&rscd=attachment%3B+filename%3Dlatest.yml&rsct=application%2Foctet-stream&skoid=placeholder&sig=placeholdersignature%2Bslash%2Fequals%3D";
    let objects = "https://objects.githubusercontent.com/github-production-release-asset-2e65be/3/4?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=PLACEHOLDER%2F20261127%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20261127T000000Z&X-Amz-Expires=300&X-Amz-Signature=placeholder&X-Amz-SignedHeaders=host&actor_id=0&key_id=0&repo_id=1&response-content-disposition=attachment%3B%20filename%3Dlatest.yml&response-content-type=application%2Foctet-stream";
    for host in [assets, objects] {
        let http = server(vec![
            (LATEST.to_owned(), latest_json()),
            (manifest_url(TAG), answer(move || redirect(302, host))),
            (host.to_owned(), good_manifest()),
        ]);
        assert_eq!(newer_tag(checked(&http)), TAG);
        let asked = http.asked();
        assert_eq!(
            http.urls(),
            vec![LATEST.to_owned(), manifest_url(TAG), host.to_owned()]
        );
        assert_eq!(asked[2].ask, UpdateAsk::Manifest);
        assert_eq!(asked[2].headers[0], ("accept", "*/*"));
    }
}

#[test]
fn a_redirect_the_port_refuses_is_not_followed() {
    let cases: Vec<(String, String)> = vec![
        (LATEST.to_owned(), "https://example.com/release".to_owned()),
        (
            LATEST.to_owned(),
            "https://objects.githubusercontent.com/latest".to_owned(),
        ),
        (
            LATEST.to_owned(),
            format!("http://github.com/rjf2016/one-launch/releases/tag/{TAG}"),
        ),
        (
            manifest_url(TAG),
            "https://githubusercontent.com.example.com/latest.yml".to_owned(),
        ),
        (
            manifest_url(TAG),
            "http://release-assets.githubusercontent.com/latest.yml".to_owned(),
        ),
        (
            manifest_url(TAG),
            "https://release-assets.githubusercontent.com:8443/latest.yml".to_owned(),
        ),
    ];
    for (at, location) in cases {
        let first_hop = at == LATEST;
        let moved = location.clone();
        let mut routes = vec![(at.clone(), answer(move || redirect(302, &moved)))];
        if !first_hop {
            routes.push((LATEST.to_owned(), latest_json()));
        }
        let http = server(routes);
        assert_eq!(failure(checked(&http)), REDIRECTED, "{location}");
        let expected = if first_hop {
            vec![LATEST.to_owned()]
        } else {
            vec![LATEST.to_owned(), manifest_url(TAG)]
        };
        assert_eq!(http.urls(), expected, "{location}");
    }
}

fn hop(step: usize) -> String {
    format!("{RELEASES}/tag/hop{step}")
}

fn chain(redirects: usize, at_the_end: Reply) -> ScriptedUpdates {
    let mut routes = vec![(LATEST.to_owned(), {
        let next = hop(1);
        answer(move || redirect(302, &next))
    })];
    for step in 1..redirects {
        let next = hop(step + 1);
        routes.push((hop(step), answer(move || redirect(307, &next))));
    }
    routes.push((hop(redirects), at_the_end));
    routes.push((manifest_url(TAG), good_manifest()));
    server(routes)
}

#[test]
fn five_redirects_in_a_row_are_followed_and_a_sixth_is_refused() {
    let http = chain(5, latest_json());
    assert_eq!(newer_tag(checked(&http)), TAG);
    assert_eq!(http.urls().len(), 7);

    let http = chain(6, latest_json());
    assert_eq!(failure(checked(&http)), REDIRECTED);
    let mut expected = vec![LATEST.to_owned()];
    expected.extend((1..=5).map(hop));
    assert_eq!(http.urls(), expected);
}

#[test]
fn a_redirect_without_a_location_is_refused() {
    for code in [301, 302, 303, 307, 308] {
        let http = server(vec![(LATEST.to_owned(), answer(move || status(code)))]);
        assert_eq!(failure(checked(&http)), REDIRECTED);
        assert_eq!(http.urls(), vec![LATEST.to_owned()]);
        let http = server(vec![
            (LATEST.to_owned(), latest_json()),
            (manifest_url(TAG), answer(move || status(code))),
        ]);
        assert_eq!(failure(checked(&http)), REDIRECTED);
        assert_eq!(http.urls().len(), 2);
    }
}

#[test]
fn a_bad_status_at_each_step_and_at_a_later_hop_says_what_it_was() {
    let assets = "https://release-assets.githubusercontent.com/latest.yml";
    for (code, release, manifest) in [
        (
            404,
            "no release has been published yet",
            "the latest release has no update manifest",
        ),
        (
            500,
            "the update server answered HTTP 500",
            "the update server answered HTTP 500",
        ),
        (
            403,
            "the update server answered HTTP 403",
            "the update server answered HTTP 403",
        ),
    ] {
        let http = server(vec![(LATEST.to_owned(), answer(move || status(code)))]);
        assert_eq!(failure(checked(&http)), release);
        assert_eq!(http.urls(), vec![LATEST.to_owned()]);

        let http = server(vec![
            (LATEST.to_owned(), answer(|| redirect(302, &page(TAG)))),
            (page(TAG), answer(move || status(code))),
        ]);
        assert_eq!(failure(checked(&http)), release);
        assert_eq!(http.urls(), vec![LATEST.to_owned(), page(TAG)]);

        let http = server(vec![
            (LATEST.to_owned(), latest_json()),
            (manifest_url(TAG), answer(move || status(code))),
        ]);
        assert_eq!(failure(checked(&http)), manifest);
        assert_eq!(http.urls().len(), 2);

        let http = server(vec![
            (LATEST.to_owned(), latest_json()),
            (manifest_url(TAG), answer(move || redirect(302, assets))),
            (assets.to_owned(), answer(move || status(code))),
        ]);
        assert_eq!(failure(checked(&http)), manifest);
        assert_eq!(http.urls().len(), 3);
    }
}

#[test]
fn a_release_answer_without_a_usable_tag_reads_as_no_release() {
    let long = format!("v{}", "1".repeat(64));
    let bodies: Vec<String> = vec![
        "<html>not json</html>".to_owned(),
        String::new(),
        "{\"tag_name\":".to_owned(),
        "[]".to_owned(),
        "\"v1.0.0\"".to_owned(),
        "{\"name\":\"v1.0.0-beta.4\"}".to_owned(),
        "{\"tag_name\":104}".to_owned(),
        "{\"tag_name\":null}".to_owned(),
        "{\"tag_name\":[\"v1.0.0\"]}".to_owned(),
        release_json("../x"),
        release_json("v1/x"),
        release_json("v1.0.0 beta"),
        release_json(""),
        release_json(&long),
    ];
    for body in bodies {
        let text = body.clone();
        let http = server(vec![(LATEST.to_owned(), answer(move || json(200, &text)))]);
        assert_eq!(
            failure(checked(&http)),
            "no release has been published yet",
            "{body}"
        );
        assert_eq!(http.urls(), vec![LATEST.to_owned()]);
    }
    let longest = format!("v{}", "1".repeat(63));
    let http = server(vec![
        (LATEST.to_owned(), {
            let body = release_json(&longest);
            answer(move || json(200, &body))
        }),
        (manifest_url(&longest), good_manifest()),
    ]);
    assert_eq!(newer_tag(checked(&http)), longest);
}

#[test]
fn a_release_answer_past_the_limit_reads_as_no_release() {
    let padding = " ".repeat(64 * 1024);
    let declared = format!("{{\"tag_name\":\"{TAG}\"{padding}}}");
    let http = server(vec![(
        LATEST.to_owned(),
        answer(move || json(200, &declared)),
    )]);
    assert_eq!(failure(checked(&http)), "no release has been published yet");

    let http = server(vec![(
        LATEST.to_owned(),
        answer(|| {
            in_parts(
                None,
                vec![
                    Part::Bytes(format!("{{\"tag_name\":\"{TAG}\"").into_bytes()),
                    Part::Bytes(vec![b' '; 64 * 1024]),
                    Part::Bytes(b"}".to_vec()),
                ],
            )
        }),
    )]);
    assert_eq!(failure(checked(&http)), "no release has been published yet");
    assert_eq!(http.urls(), vec![LATEST.to_owned()]);
}

#[test]
fn a_manifest_past_the_limit_or_unreadable_cannot_be_read() {
    let replies: Vec<Reply> = vec![
        answer(|| file(vec![b'#'; 64 * 1024 + 1])),
        answer(|| {
            in_parts(
                None,
                vec![
                    Part::Bytes(b"version: 1.0.0-beta.4\n".to_vec()),
                    Part::Bytes(vec![b' '; 64 * 1024]),
                ],
            )
        }),
        manifest_text("version: 1.0.0-beta.4 # a comment\n"),
        manifest_text("version: [1, 0]\n"),
        manifest_text("version:\t1.0.0\n"),
        manifest_text("version: 'open\n"),
    ];
    for reply in replies {
        let http = server(vec![
            (LATEST.to_owned(), latest_json()),
            (manifest_url(TAG), reply),
        ]);
        assert_eq!(failure(checked(&http)), UNREADABLE);
        assert_eq!(http.urls().len(), 2);
    }
}

fn with_manifest(text: &'static str) -> Result<Checked, UpdateFailure> {
    let http = server(vec![
        (LATEST.to_owned(), latest_json()),
        (manifest_url(TAG), manifest_text(text)),
    ]);
    let result = checked(&http);
    assert_eq!(http.urls().len(), 2);
    result
}

#[test]
fn the_version_rule_takes_only_a_greater_version_by_precedence() {
    let cases: [(&'static str, bool); 12] = [
        ("version: 1.0.0-beta.4\n", true),
        ("version: 1.0.0\n", true),
        ("version: 1.0.0-beta.3\n", false),
        ("version: 1.0.0-beta.3+x\n", false),
        ("version: 1.0.0-beta.4+build.7\n", true),
        ("version: 1.0.0-beta.2\n", false),
        ("version: 0.9.9\n", false),
        ("version: v1.0.0-beta.4\n", true),
        ("version: v1.0.0-beta.3\n", false),
        ("version: '  1.0.0-beta.4  '\n", true),
        ("version: ' v1.0.0 '\n", true),
        ("version: 1.0.0-beta.10\n", true),
    ];
    for (text, newer) in cases {
        match with_manifest(text) {
            Ok(Checked::Newer { .. }) if newer => {}
            Ok(Checked::UpToDate) if !newer => {}
            other => panic!("{text:?} gave {other:?}"),
        }
    }
}

#[test]
fn a_build_tag_on_the_running_version_does_not_count_either() {
    let http = server(vec![
        (LATEST.to_owned(), latest_json()),
        (
            manifest_url(TAG),
            manifest_text("version: 1.0.0-beta.3+other\n"),
        ),
    ]);
    let running = Version::parse("1.0.0-beta.3+build.9").unwrap();
    assert!(matches!(check(&http, &running), Ok(Checked::UpToDate)));
}

fn invalid(version: &str) -> String {
    format!(
        "This file could not be downloaded, or the latest version (from update server) does not have a valid semver version: \"{version}\""
    )
}

#[test]
fn a_version_that_is_not_semver_is_refused_with_the_library_sentence() {
    let cases: [(&'static str, &str); 14] = [
        ("not a manifest\n", "undefined"),
        ("files: []\n", "undefined"),
        ("- version: 1.0.0\n", "undefined"),
        ("version: 1.5\n", "1.5"),
        ("version: 2\n", "2"),
        ("version: banana\n", "banana"),
        ("version: 1.0\n", "1.0"),
        ("version: 1.0.0.0\n", "1.0.0.0"),
        ("version: vv1.0.0\n", "vv1.0.0"),
        ("version: 01.0.0\n", "01.0.0"),
        ("version: 1.0.0-beta.04\n", "1.0.0-beta.04"),
        ("version: V1.0.0\n", "V1.0.0"),
        ("version: ~\n", "undefined"),
        ("version: true\n", "undefined"),
    ];
    for (text, written) in cases {
        assert_eq!(failure(with_manifest(text)), invalid(written), "{text:?}");
    }
}

#[test]
fn the_recorded_not_an_object_run_gets_its_exact_sentence() {
    assert_eq!(
        failure(with_manifest("not a manifest\n")),
        "This file could not be downloaded, or the latest version (from update server) does not have a valid semver version: \"undefined\""
    );
}

#[test]
fn a_network_failure_at_each_step_ends_the_check_there() {
    let errors = [
        (
            HttpError::Failed("the connection failed"),
            "the update server could not be reached",
        ),
        (
            HttpError::Failed("the host was not found"),
            "the update server could not be reached",
        ),
        (HttpError::Timeout, "the update server could not be reached"),
        (
            HttpError::Failed("TLS failed"),
            "the secure connection to the update server failed",
        ),
    ];
    for (error, sentence) in errors {
        let http = server(vec![(LATEST.to_owned(), reply(move || Err(error)))]);
        assert_eq!(failure(checked(&http)), sentence);
        assert_eq!(http.urls(), vec![LATEST.to_owned()]);

        let http = server(vec![
            (LATEST.to_owned(), answer(|| redirect(302, &page(TAG)))),
            (page(TAG), reply(move || Err(error))),
        ]);
        assert_eq!(failure(checked(&http)), sentence);
        assert_eq!(http.urls().len(), 2);

        let http = server(vec![
            (LATEST.to_owned(), latest_json()),
            (manifest_url(TAG), reply(move || Err(error))),
        ]);
        assert_eq!(failure(checked(&http)), sentence);
        assert_eq!(http.urls().len(), 2);
    }
    for broken in [Part::Drop, Part::Stall] {
        let parts = std::sync::Mutex::new(Some(broken));
        let http = server(vec![
            (LATEST.to_owned(), latest_json()),
            (
                manifest_url(TAG),
                answer(move || {
                    in_parts(
                        Some(40),
                        vec![
                            Part::Bytes(b"version: ".to_vec()),
                            parts.lock().unwrap().take().unwrap_or(Part::Drop),
                        ],
                    )
                }),
            ),
        ]);
        assert_eq!(
            failure(checked(&http)),
            "the update server could not be reached"
        );
        assert_eq!(http.urls().len(), 2);
    }
}
