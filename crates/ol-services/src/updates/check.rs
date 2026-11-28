use std::cmp::Ordering;
use std::time::Duration;

use ol_formats::latest_yml::{self, Yaml};
use ol_platform_api::http::TooLarge;
use ol_platform_api::update_http::{ReleaseTag, UpdateHttp, UpdateRequest};
use semver::Version;
use serde_json::Value;

use super::failure::UpdateFailure;
use super::redirects::{Hop, follow};

pub const ANSWER_LIMIT: u64 = 64 * 1024;
pub const WAIT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checked {
    UpToDate,
    // Not yet checked by its signature.
    Newer {
        tag: ReleaseTag,
        version: Version,
        manifest: Yaml,
    },
}

pub fn check(http: &dyn UpdateHttp, current: &Version) -> Result<Checked, UpdateFailure> {
    let tag = latest_tag(http)?;
    let manifest = release_manifest(http, &tag)?;
    let version = manifest_version(&manifest)?;
    if version.cmp_precedence(current) == Ordering::Greater {
        Ok(Checked::Newer {
            tag,
            version,
            manifest,
        })
    } else {
        Ok(Checked::UpToDate)
    }
}

const fn is_ok(status: u16) -> bool {
    status >= 200 && status <= 299
}

fn send(
    http: &dyn UpdateHttp,
    first: UpdateRequest,
) -> Result<(u16, Result<Vec<u8>, TooLarge>), UpdateFailure> {
    follow(first, |request| {
        let answer = http.send(request)?;
        Ok::<_, UpdateFailure>(Hop {
            status: answer.status,
            location: answer.location,
            answer: answer.body,
        })
    })
}

fn latest_tag(http: &dyn UpdateHttp) -> Result<ReleaseTag, UpdateFailure> {
    let (status, body) = send(http, UpdateRequest::latest_release(ANSWER_LIMIT, WAIT))?;
    match status {
        404 => return Err(UpdateFailure::NoRelease),
        status if !is_ok(status) => return Err(UpdateFailure::Status(status)),
        _ => {}
    }
    body.ok()
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
        .and_then(|release| {
            release
                .get("tag_name")
                .and_then(Value::as_str)
                .and_then(ReleaseTag::new)
        })
        .ok_or(UpdateFailure::NoRelease)
}

fn release_manifest(http: &dyn UpdateHttp, tag: &ReleaseTag) -> Result<Yaml, UpdateFailure> {
    let (status, body) = send(http, UpdateRequest::manifest(tag, ANSWER_LIMIT, WAIT))?;
    match status {
        404 => return Err(UpdateFailure::NoManifest),
        status if !is_ok(status) => return Err(UpdateFailure::Status(status)),
        _ => {}
    }
    body.ok()
        .and_then(|body| latest_yml::read(&body).ok())
        .ok_or(UpdateFailure::UnreadableManifest)
}

// The rule of the `semver` package the Electron copy's updater parses with: surrounding white space and one leading `v` are allowed.
fn manifest_version(manifest: &Yaml) -> Result<Version, UpdateFailure> {
    let written = match manifest {
        Yaml::Map(_) => manifest.get("version"),
        _ => None,
    };
    if let Some(Yaml::Text(text)) = written {
        let trimmed = text.trim();
        let plain = trimmed.strip_prefix('v').unwrap_or(trimmed);
        if let Ok(version) = Version::parse(plain) {
            return Ok(version);
        }
    }
    Err(UpdateFailure::InvalidVersion(match written {
        Some(Yaml::Text(text) | Yaml::Number(text)) => text.clone(),
        _ => "undefined".to_owned(),
    }))
}
