#![forbid(unsafe_code)]

// The app's one way to the internet: the artwork requests, sent with ureq over TLS. What may be asked, of which host and with which headers, is the request's own (`ol_platform_api::http`); this client adds nothing to it but the Host line, follows no redirect, asks no proxy, keeps no cookie and reads no more of an answer than the request allows.

use ol_platform_api::http::{HttpClient, HttpError, HttpRequest, HttpResponse, TooLarge};
use ureq::config::Config;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::unversioned::resolver::{DefaultResolver, Resolver};
use ureq::unversioned::transport::{Connector, DefaultConnector};
use ureq::{Agent, Error};

pub struct HttpsClient {
    agent: Agent,
}

impl Default for HttpsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpsClient {
    // The system's name lookup and its sockets, with rustls and Mozilla's root certificates over them.
    pub fn new() -> Self {
        Self::over(DefaultConnector::new(), DefaultResolver::default())
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "this is the HttpClient of ol-net, the one place an agent is made"
    )]
    fn over(connector: impl Connector, resolver: impl Resolver) -> Self {
        Self {
            agent: Agent::with_parts(config(), connector, resolver),
        }
    }
}

fn config() -> Config {
    Agent::config_builder()
        .https_only(true)
        // ureq's default reads HTTPS_PROXY and ALL_PROXY, and a request would go wherever the environment says.
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::Rustls)
                .root_certs(RootCerts::WebPki)
                .build(),
        )
        // No header of ureq's own: the request names every one.
        .user_agent("")
        .accept("")
        .accept_encoding("")
        .build()
}

impl HttpClient for HttpsClient {
    #[expect(
        clippy::disallowed_methods,
        reason = "this is the HttpClient of ol-net, the one place a request is sent"
    )]
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        if request.timeout.is_zero() {
            return Err(HttpError::Timeout);
        }
        let mut call = self.agent.get(request.url().to_string());
        for (name, value) in request.headers() {
            call = call.header(name, value.as_str());
        }
        let mut answer = call
            .config()
            .timeout_global(Some(request.timeout))
            .build()
            .call()
            .map_err(|error| failure(&error))?;
        let status = answer.status().as_u16();
        let header = |name: &str| {
            answer
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let (content_type, location) = (header("content-type"), header("location"));
        let declared = header("content-length").and_then(|length| length.trim().parse().ok());
        let body = if !(200..=299).contains(&status) {
            Ok(Vec::new())
        } else if let Some(declared) = declared.filter(|declared| *declared > request.limit) {
            Err(TooLarge::Declared(declared))
        } else {
            match answer
                .body_mut()
                .with_config()
                .limit(request.limit)
                .read_to_vec()
            {
                Ok(body) => Ok(body),
                Err(Error::BodyExceedsLimit(_)) => Err(TooLarge::Read),
                Err(error) => return Err(failure(&error)),
            }
        };
        Ok(HttpResponse {
            status,
            content_type,
            location,
            body,
        })
    }
}

// A word for the log and never ureq's own sentence, which can quote the address asked.
fn failure(error: &Error) -> HttpError {
    match error {
        Error::Timeout(_) => HttpError::Timeout,
        Error::HostNotFound => HttpError::Failed("the host was not found"),
        Error::Io(_) | Error::ConnectionFailed => HttpError::Failed("the connection failed"),
        Error::Tls(_) | Error::Rustls(_) | Error::Pem(_) | Error::TlsRequired => {
            HttpError::Failed("TLS failed")
        }
        Error::Protocol(_) | Error::Http(_) => HttpError::Failed("the answer was not HTTP"),
        _ => HttpError::Failed("the request failed"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use ol_platform_api::http::{ArtworkUrl, IMAGE_HOSTS, KEY_HOST, LOOKUP_HOSTS};
    use ureq::http::Uri;
    use ureq::unversioned::resolver::ResolvedSocketAddrs;
    use ureq::unversioned::transport::{
        Buffers, ConnectionDetails, LazyBuffers, NextTimeout, Transport,
    };

    use super::*;

    const WAIT: Duration = Duration::from_secs(15);
    const KEY: &str = "placeholderkey0000000000";

    enum Answer {
        Bytes(Vec<u8>),
        Stalls,
        Drops,
    }

    // What stands where the network would: every name asked of the resolver, every connection opened, every byte written, and the answers to give.
    #[derive(Default)]
    struct Wire {
        looked_up: Vec<String>,
        connected: Vec<String>,
        sent: Vec<u8>,
        answers: VecDeque<Answer>,
    }

    type Shared = Arc<Mutex<Wire>>;

    #[derive(Debug)]
    struct NoLookup(Shared);

    impl std::fmt::Debug for Wire {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Wire")
        }
    }

    impl Resolver for NoLookup {
        fn resolve(
            &self,
            uri: &Uri,
            _: &Config,
            _: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, Error> {
            let host = uri.host().unwrap_or_default().to_owned();
            self.0.lock().unwrap().looked_up.push(host);
            let mut found = self.empty();
            found.push("127.0.0.1:443".parse().unwrap());
            Ok(found)
        }
    }

    #[derive(Debug)]
    struct InProcess(Shared);

    impl<In: Transport> Connector<In> for InProcess {
        type Out = Canned;

        fn connect(
            &self,
            details: &ConnectionDetails,
            _: Option<In>,
        ) -> Result<Option<Canned>, Error> {
            let mut wire = self.0.lock().unwrap();
            wire.connected.push(details.uri.to_string());
            let answer = wire.answers.pop_front().unwrap_or(Answer::Drops);
            Ok(Some(Canned {
                buffers: LazyBuffers::new(
                    details.config.input_buffer_size(),
                    details.config.output_buffer_size(),
                ),
                wire: Arc::clone(&self.0),
                answer,
            }))
        }
    }

    #[derive(Debug)]
    struct Canned {
        buffers: LazyBuffers,
        wire: Shared,
        answer: Answer,
    }

    impl std::fmt::Debug for Answer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Answer")
        }
    }

    impl Transport for Canned {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }

        fn transmit_output(&mut self, amount: usize, _: NextTimeout) -> Result<(), Error> {
            let written = self.buffers.output()[..amount].to_vec();
            self.wire.lock().unwrap().sent.extend(written);
            Ok(())
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
            match &mut self.answer {
                Answer::Stalls => Err(Error::Timeout(timeout.reason)),
                Answer::Drops => Err(Error::Io(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "dropped",
                ))),
                Answer::Bytes(left) => {
                    let input = self.buffers.input_append_buf();
                    let amount = input.len().min(left.len());
                    input[..amount].copy_from_slice(&left[..amount]);
                    left.drain(..amount);
                    self.buffers.input_appended(amount);
                    if amount == 0 {
                        return Err(Error::Io(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "ended",
                        )));
                    }
                    Ok(true)
                }
            }
        }

        fn is_open(&mut self) -> bool {
            false
        }

        fn is_tls(&self) -> bool {
            true
        }
    }

    fn client(answers: Vec<Answer>) -> (HttpsClient, Shared) {
        let wire: Shared = Arc::new(Mutex::new(Wire {
            answers: answers.into(),
            ..Wire::default()
        }));
        let client = HttpsClient::over(InProcess(Arc::clone(&wire)), NoLookup(Arc::clone(&wire)));
        (client, wire)
    }

    fn answer(head: &str, body: &[u8]) -> Answer {
        Answer::Bytes([head.as_bytes(), body].concat())
    }

    fn lookup(url: &str) -> HttpRequest {
        HttpRequest::lookup(url, 1024, WAIT).unwrap()
    }

    fn image(url: &str, limit: u64) -> HttpRequest {
        HttpRequest::image(ArtworkUrl::image(url).unwrap(), limit, WAIT).unwrap()
    }

    fn lines(wire: &Shared) -> Vec<String> {
        let sent = String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap();
        let (head, body) = sent.split_once("\r\n\r\n").expect("a request has a head");
        assert_eq!(body, "", "a request has no body");
        let mut lines: Vec<String> = head.split("\r\n").map(str::to_lowercase).collect();
        lines[1..].sort();
        lines
    }

    #[test]
    fn what_goes_on_the_wire_is_the_request_line_the_host_and_the_four_fixed_headers() {
        let (client, wire) = client(vec![answer(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n",
            b"{\"items\":[]}",
        )]);
        let url =
            "https://store.steampowered.com/api/storesearch/?term=Rocket%20League&l=english&cc=US";
        let answered = client.send(&lookup(url)).unwrap();
        assert_eq!(answered.status, 200);
        assert_eq!(answered.content_type.as_deref(), Some("application/json"));
        assert_eq!(answered.body.as_deref(), Ok(b"{\"items\":[]}".as_slice()));
        assert_eq!(
            lines(&wire),
            [
                "get /api/storesearch/?term=rocket%20league&l=english&cc=us http/1.1",
                "accept-language: *",
                "accept: */*",
                "host: store.steampowered.com",
                "sec-fetch-mode: cors",
                "user-agent: node",
            ]
        );
        let sent = String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap();
        assert!(sent.starts_with(
            "GET /api/storesearch/?term=Rocket%20League&l=english&cc=US HTTP/1.1\r\n"
        ));
        let wire = wire.lock().unwrap();
        assert_eq!(wire.looked_up, ["store.steampowered.com"]);
        assert_eq!(wire.connected, [url]);
    }

    #[test]
    fn the_key_is_written_once_in_its_header_and_only_to_the_steamgriddb_api() {
        let ok = || answer("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n", b"{}");
        let (client, wire) = client(vec![ok()]);
        let api = format!("https://{KEY_HOST}/api/v2/grids/steam/252950?types=static");
        client.send(&lookup(&api).with_key(KEY).unwrap()).unwrap();
        let sent = String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap();
        assert_eq!(sent.matches(KEY).count(), 1);
        assert!(sent.contains(&format!("\r\nauthorization: Bearer {KEY}\r\n")));
        assert!(!sent.lines().next().unwrap().contains(KEY));
        assert_eq!(wire.lock().unwrap().connected, [api]);

        for host in LOOKUP_HOSTS.into_iter().chain(IMAGE_HOSTS) {
            let (client, wire) = self::client(vec![ok()]);
            let url = format!("https://{host}/x");
            let request = match ArtworkUrl::image(&url) {
                Some(url) => HttpRequest::image(url, 1024, WAIT).unwrap(),
                None => lookup(&url),
            };
            client.send(&request).unwrap();
            let sent = String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap();
            assert!(!sent.to_lowercase().contains("authorization"), "{host}");
            assert!(!sent.to_lowercase().contains("cookie"), "{host}");
        }
    }

    #[test]
    fn a_redirect_comes_back_as_it_is_and_is_not_followed() {
        let (client, wire) = client(vec![
            answer(
                "HTTP/1.1 302 Found\r\nLocation: https://example.com/elsewhere.png\r\nContent-Length: 5\r\n\r\n",
                b"moved",
            ),
            answer("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n", b"no"),
        ]);
        let url =
            "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/1/logo.png";
        let answered = client.send(&image(url, 1024)).unwrap();
        assert_eq!(answered.status, 302);
        assert_eq!(
            answered.location.as_deref(),
            Some("https://example.com/elsewhere.png")
        );
        assert_eq!(answered.body, Ok(Vec::new()));
        let wire = wire.lock().unwrap();
        assert_eq!(wire.connected, [url]);
        assert_eq!(wire.looked_up, ["shared.cloudflare.steamstatic.com"]);
    }

    #[test]
    fn a_body_is_read_up_to_the_limit_and_no_further() {
        let url = "https://cdn.cloudflare.steamstatic.com/steam/apps/1/logo.png";
        let (client, _) = client(vec![answer(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 8\r\n\r\n",
            b"12345678",
        )]);
        let exact = client.send(&image(url, 8)).unwrap();
        assert_eq!(exact.body.as_deref(), Ok(b"12345678".as_slice()));

        let (client, _) = self::client(vec![answer(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 9\r\n\r\n",
            b"123456789",
        )]);
        let declared = client.send(&image(url, 8)).unwrap();
        assert_eq!(declared.body, Err(TooLarge::Declared(9)));
        assert_eq!(declared.content_type.as_deref(), Some("image/png"));

        let (client, _) = self::client(vec![answer(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"9\r\n123456789\r\n0\r\n\r\n",
        )]);
        let undeclared = client.send(&image(url, 8)).unwrap();
        assert_eq!(undeclared.body, Err(TooLarge::Read));

        let (client, _) = self::client(vec![answer(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"8\r\n12345678\r\n0\r\n\r\n",
        )]);
        let chunked = client.send(&image(url, 8)).unwrap();
        assert_eq!(chunked.body.as_deref(), Ok(b"12345678".as_slice()));
    }

    #[test]
    fn only_the_body_of_a_2xx_answer_is_read() {
        for status in [
            "404 Not Found",
            "401 Unauthorized",
            "500 Internal Server Error",
        ] {
            let (client, _) = client(vec![answer(
                &format!("HTTP/1.1 {status}\r\nContent-Length: 9\r\n\r\n"),
                b"not found",
            )]);
            let answered = client
                .send(&lookup("https://steamcommunity.com/actions/SearchApps/x"))
                .unwrap();
            assert_eq!(answered.status.to_string(), &status[..3]);
            assert_eq!(answered.body, Ok(Vec::new()), "{status}");
        }
    }

    #[test]
    fn a_failure_is_told_in_fixed_words() {
        let url = "https://steamcommunity.com/actions/SearchApps/x";
        let (client, _) = client(vec![Answer::Stalls]);
        assert_eq!(client.send(&lookup(url)), Err(HttpError::Timeout));
        let (client, _) = self::client(vec![Answer::Drops]);
        assert_eq!(
            client.send(&lookup(url)),
            Err(HttpError::Failed("the connection failed"))
        );
        let (client, _) = self::client(vec![answer("not http at all\r\n\r\n", b"")]);
        assert!(matches!(
            client.send(&lookup(url)),
            Err(HttpError::Failed(_))
        ));
        let (client, wire) = self::client(vec![]);
        let mut spent = lookup(url);
        spent.timeout = Duration::ZERO;
        assert_eq!(client.send(&spent), Err(HttpError::Timeout));
        assert!(wire.lock().unwrap().connected.is_empty());
        for error in [
            HttpError::Timeout,
            HttpError::Failed("the connection failed"),
        ] {
            let told = format!("{error} {error:?}");
            assert!(!told.contains("steamcommunity"), "{told}");
        }
    }

    const PROXIES: [&str; 6] = [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ];

    // Run by the test below, in a process of its own whose environment names a proxy.
    #[test]
    #[ignore = "run by a_proxy_named_in_the_environment_is_not_used"]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test shows that the environment's proxy is there and not taken"
    )]
    fn the_client_names_no_proxy() {
        for name in PROXIES {
            assert!(std::env::var_os(name).is_some(), "{name} is not set");
        }
        assert!(ureq::Proxy::try_from_env().is_some());
        assert!(config().proxy().is_none());
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a test cannot change its own environment, so it runs itself again with another"
    )]
    fn a_proxy_named_in_the_environment_is_not_used() {
        let ran = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::the_client_names_no_proxy", "--ignored"])
            .envs(PROXIES.map(|name| (name, "http://127.0.0.1:9")))
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&ran.stdout);
        assert!(ran.status.success(), "{said}");
        assert!(said.contains("1 passed"), "{said}");
    }

    #[test]
    fn the_client_is_set_up_for_https_alone_without_a_proxy_and_without_redirects() {
        let config = config();
        assert!(config.https_only());
        assert!(config.proxy().is_none());
        assert_eq!(config.max_redirects(), 0);
        assert!(!config.http_status_as_error());
        assert!(matches!(
            config.tls_config().provider(),
            TlsProvider::Rustls
        ));
        assert!(matches!(
            config.tls_config().root_certs(),
            RootCerts::WebPki
        ));
        assert!(!config.tls_config().disable_verification());
        // Made, and nothing asked of it: making a client sends nothing.
        let _ = HttpsClient::new();
    }
}
