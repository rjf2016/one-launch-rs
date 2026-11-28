use ol_platform_api::http::HttpError;
use thiserror::Error;

const LONGEST: usize = 160;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum UpdateFailure {
    #[error("the update server could not be reached")]
    Unreachable,
    #[error("no release has been published yet")]
    NoRelease,
    #[error("the latest release has no update manifest")]
    NoManifest,
    #[error("the downloaded file does not match the release")]
    Mismatch,
    #[error("the update server answered HTTP {0}")]
    Status(u16),
    #[error("the secure connection to the update server failed")]
    SecureConnection,
    #[error("the update server redirected to an address One Launch does not ask")]
    Redirected,
    #[error("the release manifest could not be read")]
    UnreadableManifest,
    // The version as the manifest writes it, or `undefined` when it writes none that is text or a number.
    #[error("{}", first_line(&invalid_version(.0)))]
    InvalidVersion(String),
    #[error("the update could not be saved to disk")]
    Disk,
    #[error("{}", first_line(.0))]
    Other(String),
}

impl From<HttpError> for UpdateFailure {
    fn from(error: HttpError) -> Self {
        match error {
            HttpError::Failed("TLS failed") => Self::SecureConnection,
            HttpError::Timeout | HttpError::Failed(_) => Self::Unreachable,
        }
    }
}

fn invalid_version(written: &str) -> String {
    format!(
        "This file could not be downloaded, or the latest version (from update server) does not have a valid semver version: \"{written}\""
    )
}

fn first_line(text: &str) -> String {
    let first = text.split('\n').next().unwrap_or_default().trim();
    if first.is_empty() {
        return "unknown error".to_owned();
    }
    if first.chars().count() > LONGEST {
        let mut cut: String = first.chars().take(LONGEST - 1).collect();
        cut.push('…');
        return cut;
    }
    first.to_owned()
}
