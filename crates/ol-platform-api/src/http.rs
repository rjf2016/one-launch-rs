// The only requests the app sends to the internet: the artwork lookups and the image downloads. A request can be made for a host of the two lists below and for no other, over https on the default port, and the SteamGridDB key rides in one header of a request to that API alone. The real client and the fakes are handed the same request and send or record its `headers()`, so what a test sees is what would be sent.

use std::fmt;
use std::time::Duration;

use thiserror::Error;
use zeroize::Zeroizing;

// Steam's two searches, which map a game's name to its Steam id, and the SteamGridDB API.
pub const LOOKUP_HOSTS: [&str; 3] = ["store.steampowered.com", "steamcommunity.com", KEY_HOST];

// Where a picture comes from, first request or redirect. The second Steam host answers with a redirect to the third.
pub const IMAGE_HOSTS: [&str; 4] = [
    "cdn.cloudflare.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
    "shared.steamstatic.com",
    "cdn2.steamgriddb.com",
];

// The one host the SteamGridDB key is ever sent to.
pub const KEY_HOST: &str = "www.steamgriddb.com";

// What every request carries beside Host: what the Electron app's fetch sends for a request without headers of its own, less Accept-Encoding, because this app decodes no encoding.
pub const FIXED_HEADERS: [(&str, &str); 4] = [
    ("accept", "*/*"),
    ("accept-language", "*"),
    ("sec-fetch-mode", "cors"),
    ("user-agent", "node"),
];

pub const KEY_HEADER: &str = "authorization";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum NotAllowed {
    #[error("not one of the hosts One Launch asks")]
    Host,
    #[error("the SteamGridDB key goes to the SteamGridDB API alone")]
    Key,
}

// An https address on one of the hosts above, default port, no user, no fragment.
#[derive(Clone, PartialEq, Eq)]
pub struct ArtworkUrl {
    host: &'static str,
    // From the first slash on, the query included.
    path: String,
}

impl ArtworkUrl {
    pub fn lookup(url: &str) -> Option<Self> {
        Self::parse(url, &LOOKUP_HOSTS)
    }

    pub fn image(url: &str) -> Option<Self> {
        Self::parse(url, &IMAGE_HOSTS)
    }

    fn parse(url: &str, hosts: &[&'static str]) -> Option<Self> {
        let rest = strip_prefix_ignoring_case(url, "https://")?;
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, rest) = rest.split_at(end);
        let named = authority.strip_suffix(":443").unwrap_or(authority);
        let host = hosts
            .iter()
            .copied()
            .find(|host| host.eq_ignore_ascii_case(named))?;
        let rest = rest.split('#').next().unwrap_or_default();
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (rest, None),
        };
        Self::on(host, path, query)
    }

    fn on(host: &'static str, path: &str, query: Option<&str>) -> Option<Self> {
        let mut whole = without_dot_segments(path);
        if let Some(query) = query {
            whole.push('?');
            whole.push_str(query);
        }
        let plain = whole
            .bytes()
            .all(|byte| matches!(byte, 0x21..=0x7e) && byte != b'\\');
        plain.then_some(Self { host, path: whole })
    }

    // Where a redirect's Location leads from here, when that is an image host again.
    pub fn redirected(&self, location: &str) -> Option<Self> {
        if has_scheme(location) {
            return Self::image(location);
        }
        if location.starts_with("//") {
            return Self::image(&format!("https:{location}"));
        }
        if !IMAGE_HOSTS.contains(&self.host) {
            return None;
        }
        let location = location.split('#').next().unwrap_or_default();
        let (here, query_here) = match self.path.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (self.path.as_str(), None),
        };
        let (path, query) = match location.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (location, None),
        };
        if path.is_empty() {
            return Self::on(self.host, here, query.or(query_here));
        }
        if path.starts_with('/') {
            return Self::on(self.host, path, query);
        }
        let folder = here.rfind('/').map_or("/", |slash| &here[..=slash]);
        Self::on(self.host, &format!("{folder}{path}"), query)
    }

    pub const fn host(&self) -> &'static str {
        self.host
    }

    pub fn path_and_query(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for ArtworkUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "https://{}{}", self.host, self.path)
    }
}

impl fmt::Debug for ArtworkUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

fn strip_prefix_ignoring_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn has_scheme(text: &str) -> bool {
    let Some((scheme, _)) = text.split_once(':') else {
        return false;
    };
    let mut letters = scheme.bytes();
    letters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && letters.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn without_dot_segments(path: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.iter().enumerate() {
        match *segment {
            "." | ".." => {
                if *segment == ".." {
                    kept.pop();
                }
                if index == last {
                    kept.push("");
                }
            }
            segment => kept.push(segment),
        }
    }
    format!("/{}", kept.join("/"))
}

pub struct HttpRequest {
    url: ArtworkUrl,
    key: Option<Zeroizing<String>>,
    // The most bytes of the answer's body that are read.
    pub limit: u64,
    // For the whole request, the reading of the body included.
    pub timeout: Duration,
}

impl HttpRequest {
    pub fn lookup(url: &str, limit: u64, timeout: Duration) -> Result<Self, NotAllowed> {
        let url = ArtworkUrl::lookup(url).ok_or(NotAllowed::Host)?;
        Ok(Self {
            url,
            key: None,
            limit,
            timeout,
        })
    }

    pub fn image(url: ArtworkUrl, limit: u64, timeout: Duration) -> Result<Self, NotAllowed> {
        if !IMAGE_HOSTS.contains(&url.host) {
            return Err(NotAllowed::Host);
        }
        Ok(Self {
            url,
            key: None,
            limit,
            timeout,
        })
    }

    pub fn with_key(mut self, key: &str) -> Result<Self, NotAllowed> {
        // A key is a run of visible characters: anything else could not be one header's value.
        let plain = !key.is_empty() && key.bytes().all(|byte| matches!(byte, 0x21..=0x7e));
        if self.url.host != KEY_HOST || !plain {
            return Err(NotAllowed::Key);
        }
        self.key = Some(Zeroizing::new(format!("Bearer {key}")));
        Ok(self)
    }

    pub const fn url(&self) -> &ArtworkUrl {
        &self.url
    }

    pub const fn has_key(&self) -> bool {
        self.key.is_some()
    }

    // Every header a client sends beside Host, and the only ones.
    pub fn headers(&self) -> Vec<(&'static str, Zeroizing<String>)> {
        let mut headers: Vec<(&'static str, Zeroizing<String>)> = FIXED_HEADERS
            .iter()
            .map(|(name, value)| (*name, Zeroizing::new((*value).to_owned())))
            .collect();
        if let Some(key) = &self.key {
            headers.push((KEY_HEADER, key.clone()));
        }
        headers
    }
}

// Never the key.
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpRequest")
            .field("url", &self.url)
            .field("key", &self.key.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TooLarge {
    // The answer said how long its body is, and that is past the limit: nothing of it was read.
    Declared(u64),
    // The body went past the limit while it was read, and the rest was left unread.
    Read,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub location: Option<String>,
    // Read only for a 2xx answer; empty for any other.
    pub body: Result<Vec<u8>, TooLarge>,
}

impl HttpResponse {
    pub const fn is_ok(&self) -> bool {
        self.status >= 200 && self.status <= 299
    }
}

// Worded as the Electron app's fetch words them, because the sentence is kept as the reason a cover was not found. The word of `Failed` says where it failed, for the log; none can carry a header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum HttpError {
    #[error("The operation was aborted due to timeout")]
    Timeout,
    #[error("fetch failed")]
    Failed(&'static str),
}

// A redirect is never followed: the answer comes back as it is, with its Location.
pub trait HttpClient: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: u64 = 1024;
    const WAIT: Duration = Duration::from_secs(15);

    #[test]
    fn a_request_can_be_made_for_the_listed_hosts_and_no_other() {
        for host in LOOKUP_HOSTS {
            let url = format!("https://{host}/api/x?term=a%20b&l=english");
            let request = HttpRequest::lookup(&url, LIMIT, WAIT).unwrap();
            assert_eq!(request.url().to_string(), url);
            assert_eq!(request.url().host(), host);
            assert!(ArtworkUrl::image(&url).is_none(), "{host} is no image host");
        }
        for host in IMAGE_HOSTS {
            let url = format!("https://{host}/steam/apps/252950/logo.png");
            let parsed = ArtworkUrl::image(&url).unwrap();
            assert_eq!(parsed.to_string(), url);
            assert!(HttpRequest::image(parsed, LIMIT, WAIT).is_ok());
            assert!(HttpRequest::lookup(&url, LIMIT, WAIT).is_err(), "{host}");
        }
        for url in [
            "http://store.steampowered.com/api/storesearch/",
            "https://store.steampowered.com:8443/api/storesearch/",
            "https://store.steampowered.com:80/api/storesearch/",
            "https://store.steampowered.com./api/storesearch/",
            "https://store.steampowered.com@example.com/",
            "https://example.com/?https://store.steampowered.com/",
            "https://example.com/store.steampowered.com",
            "https://store.steampowered.com.example.com/",
            "https://evilstore.steampowered.com/",
            "https://steampowered.com/",
            "https://127.0.0.1/",
            "https://localhost/",
            "https://[::1]/",
            "https://store.steampowered.com\\@example.com/",
            "https://store.steampowered.com/a b",
            "https://store.steampowered.com/a\nb",
            "https://store.steampowered.com/\u{e9}",
            "https:/store.steampowered.com/",
            "https:store.steampowered.com/",
            "//store.steampowered.com/",
            "store.steampowered.com/",
            "ftp://store.steampowered.com/",
            "file:///C:/Windows/win.ini",
            " https://store.steampowered.com/",
            "",
        ] {
            assert!(HttpRequest::lookup(url, LIMIT, WAIT).is_err(), "{url:?}");
            assert!(ArtworkUrl::image(url).is_none(), "{url:?}");
        }
        // A name, or a name and a password, in front of a listed host.
        for user in ["user", "user:pass"] {
            for host in LOOKUP_HOSTS.into_iter().chain(IMAGE_HOSTS) {
                let url = format!("https://{user}{}{host}/x", '@');
                assert!(HttpRequest::lookup(&url, LIMIT, WAIT).is_err(), "{url:?}");
                assert!(ArtworkUrl::image(&url).is_none(), "{url:?}");
            }
        }
    }

    #[test]
    fn an_address_is_kept_as_written_but_for_its_host_its_port_and_its_fragment() {
        for (written, kept) in [
            (
                "HTTPS://CDN2.SteamGridDB.com/grid/Ab.png",
                "https://cdn2.steamgriddb.com/grid/Ab.png",
            ),
            (
                "https://cdn2.steamgriddb.com:443/grid/a.png",
                "https://cdn2.steamgriddb.com/grid/a.png",
            ),
            (
                "https://cdn2.steamgriddb.com",
                "https://cdn2.steamgriddb.com/",
            ),
            (
                "https://cdn2.steamgriddb.com?x=1",
                "https://cdn2.steamgriddb.com/?x=1",
            ),
            (
                "https://cdn2.steamgriddb.com/a.png#top",
                "https://cdn2.steamgriddb.com/a.png",
            ),
            (
                "https://cdn2.steamgriddb.com/a/./b/../c.png?x=../y",
                "https://cdn2.steamgriddb.com/a/c.png?x=../y",
            ),
            (
                "https://cdn2.steamgriddb.com/../../a/..",
                "https://cdn2.steamgriddb.com/",
            ),
            (
                "https://cdn2.steamgriddb.com/a/b/.",
                "https://cdn2.steamgriddb.com/a/b/",
            ),
        ] {
            assert_eq!(
                ArtworkUrl::image(written)
                    .map(|url| url.to_string())
                    .as_deref(),
                Some(kept),
                "{written}"
            );
        }
    }

    #[test]
    fn a_redirect_leads_to_an_image_host_or_nowhere() {
        let from = ArtworkUrl::image(
            "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/1/logo.png?t=1",
        )
        .unwrap();
        for (location, lands) in [
            (
                "https://shared.steamstatic.com/store_item_assets/steam/apps/1/logo.png",
                Some("https://shared.steamstatic.com/store_item_assets/steam/apps/1/logo.png"),
            ),
            (
                "//shared.steamstatic.com/a.png",
                Some("https://shared.steamstatic.com/a.png"),
            ),
            (
                "/other/logo.png?v=2",
                Some("https://shared.cloudflare.steamstatic.com/other/logo.png?v=2"),
            ),
            (
                "logo_2x.png",
                Some(
                    "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/1/logo_2x.png",
                ),
            ),
            (
                "../2/logo.png",
                Some(
                    "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/2/logo.png",
                ),
            ),
            (
                "?t=2",
                Some(
                    "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/1/logo.png?t=2",
                ),
            ),
            (
                "#frag",
                Some(
                    "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/1/logo.png?t=1",
                ),
            ),
            ("http://shared.steamstatic.com/a.png", None),
            ("https://example.com/a.png", None),
            ("//example.com/a.png", None),
            ("https://www.steamgriddb.com/api/v2/grids/steam/1", None),
            ("https://store.steampowered.com/a.png", None),
            ("https://127.0.0.1/a.png", None),
            ("https://192.168.1.1/a.png", None),
            ("https://localhost/a.png", None),
            ("file:///C:/a.png", None),
            ("javascript:alert(1)", None),
            ("data:image/png;base64,AAAA", None),
            ("/a b.png", None),
        ] {
            assert_eq!(
                from.redirected(location)
                    .map(|url| url.to_string())
                    .as_deref(),
                lands,
                "{location}"
            );
        }
    }

    #[test]
    fn the_key_goes_in_its_one_header_and_to_its_one_host() {
        let key = "placeholderkey0000000000";
        let api = "https://www.steamgriddb.com/api/v2/grids/steam/252950?types=static";
        let request = HttpRequest::lookup(api, LIMIT, WAIT)
            .unwrap()
            .with_key(key)
            .unwrap();
        assert!(request.has_key());
        let headers = request.headers();
        let with_key: Vec<&str> = headers
            .iter()
            .filter(|(_, value)| value.contains(key))
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(with_key, [KEY_HEADER]);
        assert_eq!(
            headers.last().map(|(_, value)| value.as_str()),
            Some("Bearer placeholderkey0000000000")
        );
        assert!(!request.url().to_string().contains(key));
        assert!(!format!("{request:?}").contains(key));
        assert!(!format!("{request:?}").contains("Bearer"));

        for host in LOOKUP_HOSTS.into_iter().filter(|host| *host != KEY_HOST) {
            let refused = HttpRequest::lookup(&format!("https://{host}/x"), LIMIT, WAIT)
                .unwrap()
                .with_key(key);
            assert_eq!(refused.err(), Some(NotAllowed::Key), "{host}");
        }
        for host in IMAGE_HOSTS {
            let url = ArtworkUrl::image(&format!("https://{host}/x.png")).unwrap();
            let refused = HttpRequest::image(url, LIMIT, WAIT).unwrap().with_key(key);
            assert_eq!(refused.err(), Some(NotAllowed::Key), "{host}");
        }
        for odd in [
            "",
            "a b",
            "key\r\nx-other: 1",
            "key\n",
            "k\u{e9}y",
            "key\u{0}",
        ] {
            let refused = HttpRequest::lookup(api, LIMIT, WAIT).unwrap().with_key(odd);
            assert_eq!(refused.err(), Some(NotAllowed::Key), "{odd:?}");
        }
    }

    #[test]
    fn a_request_without_a_key_carries_the_four_fixed_headers_and_nothing_else() {
        let request = HttpRequest::lookup(
            "https://steamcommunity.com/actions/SearchApps/x",
            LIMIT,
            WAIT,
        )
        .unwrap();
        let headers: Vec<(&str, String)> = request
            .headers()
            .into_iter()
            .map(|(name, value)| (name, value.to_string()))
            .collect();
        assert_eq!(
            headers,
            [
                ("accept", "*/*".to_owned()),
                ("accept-language", "*".to_owned()),
                ("sec-fetch-mode", "cors".to_owned()),
                ("user-agent", "node".to_owned()),
            ]
        );
    }

    // Riot's rule: nothing is ever sent to a host of Riot's.
    #[test]
    fn no_host_on_either_list_is_riots() {
        for host in LOOKUP_HOSTS.into_iter().chain(IMAGE_HOSTS) {
            for riot in [
                "riot",
                "valorant",
                "leagueoflegends",
                "pvp.net",
                "vanguard",
                "lolesports",
                "rgpub",
            ] {
                assert!(!host.contains(riot), "{host}");
            }
        }
        assert_eq!(LOOKUP_HOSTS.len() + IMAGE_HOSTS.len(), 7);
    }
}
