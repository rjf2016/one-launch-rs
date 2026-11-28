// What the updater asks of the internet, and nothing else: GitHub's latest release of One Launch, a release's `latest.yml` and a release's installer, asked as the installed Electron copies ask them. The address of each first request is built whole here, a redirect leads only where `UpdateRequest::redirected` allows, and the headers are fixed and carry nothing of the install or of the user. The real client and the fake are handed the same request and send or record its `headers()`.

use std::fmt;
use std::io;
use std::time::Duration;

use thiserror::Error;

use crate::CancelToken;
use crate::http::{
    HttpError, HttpResponse, TooLarge, has_scheme, plain, relative_target,
    strip_prefix_ignoring_case, without_dot_segments,
};

// The Electron app's `RELEASE_FEED`: the repository rjf2016/one-launch on GitHub.
pub const FEED_HOST: &str = "github.com";
pub const FEED_REPOSITORY: &str = "/rjf2016/one-launch";

// GitHub hands a release file's bytes out from a host under this domain, and has renamed that host before. The domain is safe to take whole because the manifest is checked by its signature and the installer by its signed SHA-512.
pub const FILE_DOMAIN: &str = "githubusercontent.com";

pub const MANIFEST_FILE: &str = "latest.yml";

pub const ACCEPT_JSON: &str = "application/json";
pub const ACCEPT_ANY: &str = "*/*";
pub const CACHE_CONTROL: &str = "no-cache";
// What the Electron app's updater sends, so this copy looks no different to GitHub than an installed Electron copy.
pub const USER_AGENT: &str = "electron-builder";

const TAG_LONGEST: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAsk {
    LatestRelease,
    Manifest,
    Installer,
}

// A release's tag as GitHub names it, in the one shape that cannot change the path it goes into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseTag(String);

impl ReleaseTag {
    pub fn new(tag: &str) -> Option<Self> {
        let mut bytes = tag.bytes();
        let first = bytes.next()?;
        let fits = tag.len() <= TAG_LONGEST
            && first.is_ascii_alphanumeric()
            && bytes.all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
            });
        fits.then(|| Self(tag.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// The installer's file name, by the manifest check's rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallerName(String);

impl InstallerName {
    pub fn new(name: &str) -> Option<Self> {
        let stem = name.strip_suffix(".exe")?;
        let mut bytes = stem.bytes();
        let first = bytes.next()?;
        let fits = first.is_ascii_alphanumeric()
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        fits.then(|| Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn allowed_host(host: &str, ask: UpdateAsk) -> bool {
    let named = host.split('.').all(|label| {
        let bytes = label.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 63
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
            && bytes.first() != Some(&b'-')
            && bytes.last() != Some(&b'-')
    });
    let file_host = host == FILE_DOMAIN
        || host
            .strip_suffix(FILE_DOMAIN)
            .is_some_and(|head| head.ends_with('.'));
    named && (host == FEED_HOST || (ask != UpdateAsk::LatestRelease && file_host))
}

// An https address on the default port, without user or fragment, on a host the request it belongs to may reach.
#[derive(Clone, PartialEq, Eq)]
pub struct UpdateUrl {
    ask: UpdateAsk,
    host: String,
    // From the first slash on, the query included.
    path: String,
}

impl UpdateUrl {
    fn first(ask: UpdateAsk, path: String) -> Self {
        Self {
            ask,
            host: FEED_HOST.to_owned(),
            path,
        }
    }

    fn parse(url: &str, ask: UpdateAsk) -> Option<Self> {
        let rest = strip_prefix_ignoring_case(url, "https://")?;
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, rest) = rest.split_at(end);
        let host = authority
            .strip_suffix(":443")
            .unwrap_or(authority)
            .to_ascii_lowercase();
        if !allowed_host(&host, ask) {
            return None;
        }
        let rest = rest.split('#').next().unwrap_or_default();
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (rest, None),
        };
        Self::on(ask, host, path, query)
    }

    fn on(ask: UpdateAsk, host: String, path: &str, query: Option<&str>) -> Option<Self> {
        let mut whole = without_dot_segments(path);
        if let Some(query) = query {
            whole.push('?');
            whole.push_str(query);
        }
        plain(&whole).then_some(Self {
            ask,
            host,
            path: whole,
        })
    }

    fn redirected(&self, location: &str) -> Option<Self> {
        if has_scheme(location) {
            return Self::parse(location, self.ask);
        }
        if location.starts_with("//") {
            return Self::parse(&format!("https:{location}"), self.ask);
        }
        let (path, query) = relative_target(&self.path, location);
        Self::on(self.ask, self.host.clone(), &path, query)
    }

    pub const fn ask(&self) -> UpdateAsk {
        self.ask
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn path_and_query(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for UpdateUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "https://{}{}", self.host, self.path)
    }
}

impl fmt::Debug for UpdateUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateRequest {
    url: UpdateUrl,
    // The most bytes of the answer's body that are read; for the installer, the size the signed manifest states.
    pub limit: u64,
    // For `send`, the whole request. For `download`, each step before the body and each read of it: a line silent this long ends the download, a slow one that keeps sending does not.
    pub timeout: Duration,
}

impl UpdateRequest {
    pub fn latest_release(limit: u64, timeout: Duration) -> Self {
        let path = format!("{FEED_REPOSITORY}/releases/latest");
        Self::first(UpdateAsk::LatestRelease, path, limit, timeout)
    }

    pub fn manifest(tag: &ReleaseTag, limit: u64, timeout: Duration) -> Self {
        let path = format!(
            "{FEED_REPOSITORY}/releases/download/{}/{MANIFEST_FILE}",
            tag.as_str()
        );
        Self::first(UpdateAsk::Manifest, path, limit, timeout)
    }

    pub fn installer(
        tag: &ReleaseTag,
        file: &InstallerName,
        limit: u64,
        timeout: Duration,
    ) -> Self {
        let path = format!(
            "{FEED_REPOSITORY}/releases/download/{}/{}",
            tag.as_str(),
            file.as_str()
        );
        Self::first(UpdateAsk::Installer, path, limit, timeout)
    }

    fn first(ask: UpdateAsk, path: String, limit: u64, timeout: Duration) -> Self {
        Self {
            url: UpdateUrl::first(ask, path),
            limit,
            timeout,
        }
    }

    // Where a redirect's Location leads from here, when this request may go there; the limit and the timeout go with it.
    pub fn redirected(&self, location: &str) -> Option<Self> {
        Some(Self {
            url: self.url.redirected(location)?,
            limit: self.limit,
            timeout: self.timeout,
        })
    }

    pub const fn url(&self) -> &UpdateUrl {
        &self.url
    }

    pub const fn ask(&self) -> UpdateAsk {
        self.url.ask
    }

    // Every header a client sends beside Host, and the only ones.
    pub const fn headers(&self) -> [(&'static str, &'static str); 3] {
        let accept = match self.url.ask {
            UpdateAsk::LatestRelease => ACCEPT_JSON,
            UpdateAsk::Manifest | UpdateAsk::Installer => ACCEPT_ANY,
        };
        [
            ("accept", accept),
            ("cache-control", CACHE_CONTROL),
            ("user-agent", USER_AGENT),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyEnd {
    // Not a 2xx answer: nothing of the body was read.
    NotRead,
    // The whole body, this many bytes, went into the writer.
    Complete(u64),
    // What was written is not the whole body and is not to be kept.
    TooLarge(TooLarge),
    // The token was cancelled after this many bytes were written, and nothing more was read.
    Cancelled(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downloaded {
    pub status: u16,
    pub location: Option<String>,
    // The length the answer gave for its body, when it gave one.
    pub declared: Option<u64>,
    pub body: BodyEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum DownloadError {
    #[error(transparent)]
    Network(#[from] HttpError),
    #[error("the download could not be written to disk ({0})")]
    Write(io::ErrorKind),
    #[error("the download was stopped before it was asked")]
    Cancelled,
}

const PART: usize = 64 * 1024;

// The one way a 2xx body goes into the writer, for the real client and the fake alike: refused unread when the declared length is past the limit, then read part by part, each part written and told to `progress` as the bytes so far, the token looked at before each read, and refused once the body goes past the limit. `read` gives the next part of the body, and 0 at its end.
pub fn copy_body(
    request: &UpdateRequest,
    declared: Option<u64>,
    read: &mut dyn FnMut(&mut [u8]) -> Result<usize, HttpError>,
    into: &mut dyn io::Write,
    progress: &mut dyn FnMut(u64),
    cancel: &CancelToken,
) -> Result<BodyEnd, DownloadError> {
    if let Some(declared) = declared.filter(|declared| *declared > request.limit) {
        return Ok(BodyEnd::TooLarge(TooLarge::Declared(declared)));
    }
    let mut buffer = vec![0; PART];
    let mut written: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            return Ok(BodyEnd::Cancelled(written));
        }
        let amount = read(&mut buffer)?;
        if amount == 0 {
            return Ok(BodyEnd::Complete(written));
        }
        let total = written.saturating_add(amount as u64);
        if total > request.limit {
            return Ok(BodyEnd::TooLarge(TooLarge::Read));
        }
        into.write_all(&buffer[..amount])
            .map_err(|error| DownloadError::Write(error.kind()))?;
        written = total;
        progress(written);
    }
}

// A redirect is never followed: the answer comes back with its status and its Location, and `UpdateRequest::redirected` says where that may lead.
pub trait UpdateHttp: Send + Sync {
    // A small answer, in memory: the body of a 2xx answer alone, and never past the request's limit.
    fn send(&self, request: &UpdateRequest) -> Result<HttpResponse, HttpError>;

    // A file, into `into` through `copy_body`.
    fn download(
        &self,
        request: &UpdateRequest,
        into: &mut dyn io::Write,
        progress: &mut dyn FnMut(u64),
        cancel: &CancelToken,
    ) -> Result<Downloaded, DownloadError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: u64 = 1024;
    const WAIT: Duration = Duration::from_secs(15);
    const RELEASE: &str = "https://github.com/rjf2016/one-launch/releases";

    fn tag(text: &str) -> ReleaseTag {
        ReleaseTag::new(text).unwrap()
    }

    fn file(text: &str) -> InstallerName {
        InstallerName::new(text).unwrap()
    }

    fn manifest() -> UpdateRequest {
        UpdateRequest::manifest(&tag("v1.2.0"), LIMIT, WAIT)
    }

    fn installer() -> UpdateRequest {
        UpdateRequest::installer(
            &tag("v1.2.0"),
            &file("One-Launch-Setup-1.2.0.exe"),
            LIMIT,
            WAIT,
        )
    }

    fn latest() -> UpdateRequest {
        UpdateRequest::latest_release(LIMIT, WAIT)
    }

    fn lands(from: &UpdateRequest, location: &str) -> Option<String> {
        from.redirected(location)
            .map(|request| request.url().to_string())
    }

    #[test]
    fn the_three_first_requests_are_built_whole_on_github() {
        let latest = latest();
        assert_eq!(latest.url().to_string(), format!("{RELEASE}/latest"));
        assert_eq!(latest.ask(), UpdateAsk::LatestRelease);
        let manifest = manifest();
        assert_eq!(
            manifest.url().to_string(),
            format!("{RELEASE}/download/v1.2.0/latest.yml")
        );
        assert_eq!(manifest.ask(), UpdateAsk::Manifest);
        let installer = installer();
        assert_eq!(
            installer.url().to_string(),
            format!("{RELEASE}/download/v1.2.0/One-Launch-Setup-1.2.0.exe")
        );
        assert_eq!(installer.ask(), UpdateAsk::Installer);
        for request in [&latest, &manifest, &installer] {
            assert_eq!(request.url().host(), FEED_HOST);
            assert!(
                request
                    .url()
                    .path_and_query()
                    .starts_with("/rjf2016/one-launch/releases/")
            );
            assert_eq!((request.limit, request.timeout), (LIMIT, WAIT));
        }
        assert_eq!(
            UpdateRequest::manifest(&tag("1.0.0-beta.3+build_7"), LIMIT, WAIT)
                .url()
                .to_string(),
            format!("{RELEASE}/download/1.0.0-beta.3+build_7/latest.yml")
        );
    }

    #[test]
    fn every_request_carries_the_three_fixed_headers_and_nothing_else() {
        assert_eq!(
            latest().headers(),
            [
                ("accept", "application/json"),
                ("cache-control", "no-cache"),
                ("user-agent", "electron-builder"),
            ]
        );
        for request in [manifest(), installer()] {
            assert_eq!(
                request.headers(),
                [
                    ("accept", "*/*"),
                    ("cache-control", "no-cache"),
                    ("user-agent", "electron-builder"),
                ]
            );
        }
        let moved = installer()
            .redirected("https://release-assets.githubusercontent.com/x?sig=1")
            .unwrap();
        assert_eq!(moved.headers(), installer().headers());
        let moved = latest()
            .redirected("/rjf2016/one-launch/releases/tag/v1")
            .unwrap();
        assert_eq!(moved.headers(), latest().headers());
    }

    #[test]
    fn a_tag_is_one_to_sixty_four_safe_characters_starting_with_a_letter_or_digit() {
        for good in [
            "v1",
            "1",
            "v1.2.0",
            "V1.0.0-beta.3",
            "1.0.0+build.7",
            "release_2026",
            "a..b",
            &"a".repeat(64),
        ] {
            assert_eq!(ReleaseTag::new(good).unwrap().as_str(), good);
        }
        for bad in [
            "",
            &"a".repeat(65),
            ".",
            "..",
            ".v1",
            "-v1",
            "_v1",
            "+v1",
            "v1/../../x",
            "v1/latest",
            "v1%2F..",
            "v1?x=1",
            "v1#x",
            "v1\\x",
            "v 1",
            "v1\n",
            "v1\u{0}",
            "v1:2",
            "v1@x",
            "v1~",
            "v\u{e9}1",
            "v1;x",
        ] {
            assert!(ReleaseTag::new(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn an_installer_name_is_a_plain_exe_name() {
        for good in [
            "One-Launch-Setup-1.2.0.exe",
            "a.exe",
            "0.exe",
            "One_Launch.1.exe",
            "a.exe.exe",
        ] {
            assert_eq!(InstallerName::new(good).unwrap().as_str(), good);
        }
        for bad in [
            "",
            ".exe",
            "exe",
            "a.EXE",
            "a.exe ",
            "a.msi",
            "a.exe/b.exe",
            "../a.exe",
            "-a.exe",
            "_a.exe",
            ".a.exe",
            "a b.exe",
            "a+b.exe",
            "a%2F.exe",
            "a?.exe",
            "a#.exe",
            "a\\b.exe",
            "a\n.exe",
            "\u{e9}.exe",
            "a.exe?x=1",
        ] {
            assert!(InstallerName::new(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_file_may_be_redirected_to_github_or_to_a_host_under_githubusercontent() {
        for from in [manifest(), installer()] {
            for (location, landing) in [
                (
                    "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r&sig=a%2Bb%3D&X-Amz-Signature=0a",
                    Some(
                        "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r&sig=a%2Bb%3D&X-Amz-Signature=0a",
                    ),
                ),
                (
                    "https://objects.githubusercontent.com/a/b?x=../y&z=%2e%2e",
                    Some("https://objects.githubusercontent.com/a/b?x=../y&z=%2e%2e"),
                ),
                (
                    "https://githubusercontent.com/a",
                    Some("https://githubusercontent.com/a"),
                ),
                (
                    "https://a.b-c.githubusercontent.com:443/a",
                    Some("https://a.b-c.githubusercontent.com/a"),
                ),
                (
                    "HTTPS://Objects.GitHubUserContent.COM/A",
                    Some("https://objects.githubusercontent.com/A"),
                ),
                (
                    "https://github.com/rjf2016/one-launch/x",
                    Some("https://github.com/rjf2016/one-launch/x"),
                ),
                (
                    "https://objects.githubusercontent.com/a#github.com",
                    Some("https://objects.githubusercontent.com/a"),
                ),
                (
                    "//objects.githubusercontent.com/a?q=1",
                    Some("https://objects.githubusercontent.com/a?q=1"),
                ),
                ("http://objects.githubusercontent.com/a", None),
                ("https://objects.githubusercontent.com:8443/a", None),
                ("https://objects.githubusercontent.com:80/a", None),
                ("https://objects.githubusercontent.com:/a", None),
                ("https://github.com@evil.example/a", None),
                ("https://objects.githubusercontent.com@evil.example/a", None),
                ("https://evil.example#github.com", None),
                ("https://evil.example/#objects.githubusercontent.com", None),
                ("https://evil.example/?u=https://github.com/", None),
                ("https://evilgithubusercontent.com/a", None),
                ("https://githubusercontent.com.evil.example/a", None),
                ("https://github.com.evil.example/a", None),
                ("https://evilgithub.com/a", None),
                ("https://api.github.com/a", None),
                ("https://objects.githubusercontent.com./a", None),
                ("https://.githubusercontent.com/a", None),
                ("https://a..githubusercontent.com/a", None),
                ("https://-a.githubusercontent.com/a", None),
                ("https://a_b.githubusercontent.com/a", None),
                ("https://ob\u{e9}cts.githubusercontent.com/a", None),
                ("https://140.82.112.3/a", None),
                ("https://[::1]/a", None),
                ("https://localhost/a", None),
                ("//evil.example/a", None),
                ("https://github.com\\@evil.example/a", None),
                ("https:\\\\evil.example\\a", None),
                ("https:objects.githubusercontent.com/a", None),
                ("https:/objects.githubusercontent.com/a", None),
                ("https://objects.githubusercontent.com/a\\b", None),
                ("https://objects.githubusercontent.com/a b", None),
                ("https://objects.githubusercontent.com/a\nb", None),
                ("https://objects.githubusercontent.com/a\u{0}", None),
                ("https://objects.githubusercontent.com/a?\u{7f}", None),
                ("https://objects.githubusercontent.com\r\n/a", None),
                ("ftp://objects.githubusercontent.com/a", None),
                ("file:///C:/Windows/win.ini", None),
                ("javascript:alert(1)", None),
                (" https://objects.githubusercontent.com/a", None),
                ("\\\\evil.example\\a", None),
            ] {
                assert_eq!(lands(&from, location).as_deref(), landing, "{location:?}");
            }
            // A name, or a name and a password, in front of a host a file may come from.
            for user in ["user", "user:pass"] {
                for host in [FEED_HOST, "objects.githubusercontent.com"] {
                    let location = format!("https://{user}{}{host}/a", '@');
                    assert_eq!(lands(&from, &location), None, "{location:?}");
                }
            }
        }
    }

    #[test]
    fn a_relative_location_stays_on_the_host_of_the_request() {
        let from = installer();
        for (location, landing) in [
            (
                "/rjf2016/one-launch/releases/download/v1.2.1/x.exe?t=1",
                "https://github.com/rjf2016/one-launch/releases/download/v1.2.1/x.exe?t=1",
            ),
            (
                "other.exe",
                "https://github.com/rjf2016/one-launch/releases/download/v1.2.0/other.exe",
            ),
            (
                "../v1.1.0/a.exe",
                "https://github.com/rjf2016/one-launch/releases/download/v1.1.0/a.exe",
            ),
            (
                "?sig=1",
                "https://github.com/rjf2016/one-launch/releases/download/v1.2.0/One-Launch-Setup-1.2.0.exe?sig=1",
            ),
            (
                "evil.example#github.com",
                "https://github.com/rjf2016/one-launch/releases/download/v1.2.0/evil.example",
            ),
            ("/../../../x", "https://github.com/x"),
        ] {
            assert_eq!(
                lands(&from, location).as_deref(),
                Some(landing),
                "{location}"
            );
        }
        for location in ["/a b", "/a\\b", "a\tb", "\\evil.example\\a", "/a\u{e9}"] {
            assert!(lands(&from, location).is_none(), "{location:?}");
        }
        let there = from
            .redirected("https://objects.githubusercontent.com/p/q?sig=a")
            .unwrap();
        assert_eq!(
            lands(&there, "r?sig=b").as_deref(),
            Some("https://objects.githubusercontent.com/p/r?sig=b")
        );
        assert_eq!(
            lands(&there, "https://github.com/a").as_deref(),
            Some("https://github.com/a")
        );
        assert_eq!(there.ask(), UpdateAsk::Installer);
        assert_eq!((there.limit, there.timeout), (LIMIT, WAIT));
    }

    #[test]
    fn the_latest_release_is_never_redirected_off_github() {
        let from = latest();
        for (location, landing) in [
            (
                "https://github.com/rjf2016/one-launch/releases/tag/v1.2.0",
                "https://github.com/rjf2016/one-launch/releases/tag/v1.2.0",
            ),
            (
                "/rjf2016/one-launch/releases/tag/v1.2.0",
                "https://github.com/rjf2016/one-launch/releases/tag/v1.2.0",
            ),
            (
                "tag/v1",
                "https://github.com/rjf2016/one-launch/releases/tag/v1",
            ),
            ("//GitHub.com:443/a", "https://github.com/a"),
        ] {
            assert_eq!(
                lands(&from, location).as_deref(),
                Some(landing),
                "{location}"
            );
        }
        for location in [
            "https://objects.githubusercontent.com/a",
            "https://release-assets.githubusercontent.com/a",
            "https://githubusercontent.com/a",
            "//objects.githubusercontent.com/a",
            "https://api.github.com/repos/rjf2016/one-launch/releases/latest",
            "https://github.com.evil.example/a",
            "https://evil.example/a",
            "http://github.com/a",
        ] {
            assert!(lands(&from, location).is_none(), "{location}");
        }
        let moved = from
            .redirected("/rjf2016/one-launch/releases/tag/v1")
            .unwrap();
        assert!(
            moved
                .redirected("https://objects.githubusercontent.com/a")
                .is_none()
        );
    }

    // Riot's rule: nothing is ever sent to a host of Riot's.
    #[test]
    fn no_host_the_updater_can_reach_is_riots() {
        let riots = [
            "riot",
            "valorant",
            "leagueoflegends",
            "pvp.net",
            "vanguard",
            "lolesports",
            "rgpub",
        ];
        for host in [FEED_HOST, FILE_DOMAIN] {
            for riot in riots {
                assert!(!host.contains(riot), "{host}");
            }
        }
        for from in [latest(), manifest(), installer()] {
            for host in [
                "riotgames.com",
                "www.riotgames.com",
                "auth.riotgames.com",
                "valorant.secure.dyn.riotcdn.net",
                "lol.secure.dyn.riotcdn.net",
                "leagueoflegends.com",
                "pvp.net",
                "riotgames.githubusercontent.com.riotgames.com",
                "githubusercontent.riotgames.com",
                "github.com.riotgames.com",
            ] {
                assert!(
                    lands(&from, &format!("https://{host}/a")).is_none(),
                    "{host}"
                );
            }
        }
    }

    struct Parts(Vec<Result<Vec<u8>, HttpError>>);

    impl Parts {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, HttpError> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let part = self.0.remove(0)?;
            buffer[..part.len()].copy_from_slice(&part);
            Ok(part.len())
        }
    }

    fn copied(
        request: &UpdateRequest,
        declared: Option<u64>,
        parts: Vec<Result<Vec<u8>, HttpError>>,
        cancel_after: Option<u64>,
    ) -> (Result<BodyEnd, DownloadError>, Vec<u8>, Vec<u64>, usize) {
        let mut parts = Parts(parts);
        let mut into = Vec::new();
        let mut steps = Vec::new();
        let cancel = CancelToken::new();
        let end = copy_body(
            request,
            declared,
            &mut |buffer| parts.read(buffer),
            &mut into,
            &mut |so_far| {
                steps.push(so_far);
                if cancel_after.is_some_and(|after| so_far >= after) {
                    cancel.cancel();
                }
            },
            &cancel,
        );
        (end, into, steps, parts.0.len())
    }

    #[test]
    fn a_body_is_copied_part_by_part_up_to_the_limit_and_no_further() {
        let mut request = installer();
        request.limit = 8;
        let (end, into, steps, left) = copied(
            &request,
            Some(8),
            vec![Ok(b"123".to_vec()), Ok(b"45".to_vec()), Ok(b"678".to_vec())],
            None,
        );
        assert_eq!(end, Ok(BodyEnd::Complete(8)));
        assert_eq!(
            (into.as_slice(), steps, left),
            (b"12345678".as_slice(), vec![3, 5, 8], 0)
        );

        let (end, into, _, left) = copied(&request, Some(9), vec![Ok(b"123456789".to_vec())], None);
        assert_eq!(end, Ok(BodyEnd::TooLarge(TooLarge::Declared(9))));
        assert_eq!((into.len(), left), (0, 1));

        let (end, into, steps, left) = copied(
            &request,
            None,
            vec![
                Ok(b"12345".to_vec()),
                Ok(b"6789".to_vec()),
                Ok(b"0".to_vec()),
            ],
            None,
        );
        assert_eq!(end, Ok(BodyEnd::TooLarge(TooLarge::Read)));
        assert_eq!(
            (into.as_slice(), steps, left),
            (b"12345".as_slice(), vec![5], 1)
        );
    }

    #[test]
    fn a_cancel_stops_the_copy_before_the_next_read() {
        let (end, into, steps, left) = copied(
            &installer(),
            None,
            vec![Ok(b"ab".to_vec()), Ok(b"cd".to_vec()), Ok(b"ef".to_vec())],
            Some(4),
        );
        assert_eq!(end, Ok(BodyEnd::Cancelled(4)));
        assert_eq!(
            (into.as_slice(), steps, left),
            (b"abcd".as_slice(), vec![2, 4], 1)
        );
    }

    #[test]
    fn a_failed_read_and_a_failed_write_are_errors_of_their_own() {
        let (end, into, _, _) = copied(
            &installer(),
            None,
            vec![
                Ok(b"ab".to_vec()),
                Err(HttpError::Failed("the connection failed")),
            ],
            None,
        );
        assert_eq!(
            end,
            Err(DownloadError::Network(HttpError::Failed(
                "the connection failed"
            )))
        );
        assert_eq!(into, b"ab");

        struct Full;
        impl io::Write for Full {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::StorageFull, "full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut parts = Parts(vec![Ok(b"ab".to_vec())]);
        let end = copy_body(
            &installer(),
            None,
            &mut |buffer| parts.read(buffer),
            &mut Full,
            &mut |_| {},
            &CancelToken::new(),
        );
        assert_eq!(end, Err(DownloadError::Write(io::ErrorKind::StorageFull)));
    }
}
