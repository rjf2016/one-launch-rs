#![forbid(unsafe_code)]

// The app's one way to the internet: the artwork requests and the updater's, sent with ureq over TLS. What may be asked, of which host and with which headers, is the request's own (`ol_platform_api::http`, `ol_platform_api::update_http`); this client adds nothing to it but the Host line, follows no redirect, asks no proxy, keeps no cookie and reads no more of an answer than the request allows. Beside it, on an agent of its own that reaches 127.0.0.1 alone, the Riot Client's launch request (`riot`).

mod riot;

pub use riot::RiotLoopback;

use std::io::{self, Read};

use ol_platform_api::CancelToken;
use ol_platform_api::http::{HttpClient, HttpError, HttpRequest, HttpResponse, TooLarge};
use ol_platform_api::update_http::{
    self, BodyEnd, DownloadError, Downloaded, UpdateRequest, copy_body,
};
use ureq::config::Config;
use ureq::http::Response;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::typestate::WithoutBody;
use ureq::unversioned::resolver::{DefaultResolver, Resolver};
use ureq::unversioned::transport::{Connector, DefaultConnector};
use ureq::{Agent, Body, Error, RequestBuilder};

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

impl HttpsClient {
    #[expect(
        clippy::disallowed_methods,
        reason = "this is the HttpClient of ol-net, the one place a request is made"
    )]
    fn get(&self, request: &UpdateRequest) -> RequestBuilder<WithoutBody> {
        let mut call = self.agent.get(request.url().to_string());
        for (name, value) in request.headers() {
            call = call.header(name, value);
        }
        call
    }
}

fn header(answer: &Response<Body>, name: &str) -> Option<String> {
    answer
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

impl update_http::UpdateHttp for HttpsClient {
    #[expect(
        clippy::disallowed_methods,
        reason = "this is the HttpClient of ol-net, the one place a request is sent"
    )]
    fn send(&self, request: &UpdateRequest) -> Result<HttpResponse, HttpError> {
        if request.timeout.is_zero() {
            return Err(HttpError::Timeout);
        }
        let mut answer = self
            .get(request)
            .config()
            .timeout_global(Some(request.timeout))
            .build()
            .call()
            .map_err(|error| failure(&error))?;
        let status = answer.status().as_u16();
        let (content_type, location) =
            (header(&answer, "content-type"), header(&answer, "location"));
        let declared = answer.body().content_length();
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

    #[expect(
        clippy::disallowed_methods,
        reason = "this is the HttpClient of ol-net, the one place a request is sent"
    )]
    fn download(
        &self,
        request: &UpdateRequest,
        into: &mut dyn io::Write,
        progress: &mut dyn FnMut(u64),
        cancel: &CancelToken,
    ) -> Result<Downloaded, DownloadError> {
        if cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        if request.timeout.is_zero() {
            return Err(HttpError::Timeout.into());
        }
        let wait = Some(request.timeout);
        // Every step before the body has a budget of its own and every read of the body one that starts again, but nothing bounds the whole: an installer on a slow line takes as long as it takes, and a silent one ends after one wait.
        let mut answer = self
            .get(request)
            .config()
            .timeout_global(None)
            .timeout_per_call(None)
            .timeout_resolve(wait)
            .timeout_connect(wait)
            .timeout_send_request(wait)
            .timeout_recv_response(wait)
            .timeout_recv_body(None)
            .timeout_per_read(wait)
            .timeout_per_write(wait)
            .build()
            .call()
            .map_err(|error| failure(&error))?;
        let status = answer.status().as_u16();
        let location = header(&answer, "location");
        let declared = answer.body().content_length();
        let body = if (200..=299).contains(&status) {
            let mut reader = answer.body_mut().as_reader();
            copy_body(
                request,
                declared,
                &mut |buffer| {
                    reader
                        .read(buffer)
                        .map_err(|error| failure(&Error::from(error)))
                },
                into,
                progress,
                cancel,
            )?
        } else {
            BodyEnd::NotRead
        };
        Ok(Downloaded {
            status,
            location,
            declared,
            body,
        })
    }
}

// A word for the log and never ureq's own sentence, which can quote the address asked.
fn failure(error: &Error) -> HttpError {
    match error {
        Error::Timeout(_) => HttpError::Timeout,
        Error::HostNotFound => HttpError::Failed("the host was not found"),
        // rustls hands a refused handshake or a bad record to ureq as an io error of this kind, and with no proxy nothing else here makes one.
        Error::Io(error) if error.kind() == io::ErrorKind::InvalidData => {
            HttpError::Failed("TLS failed")
        }
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

    use ol_platform_api::http::{ArtworkUrl, IMAGE_HOSTS, KEY_HOST, LOOKUP_HOSTS, STORE_API_HOST};
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
        Parts(VecDeque<Part>),
        Stalls,
        Drops,
    }

    // One step of an answer that comes in parts: each wait for input gets the next one.
    enum Part {
        Bytes(Vec<u8>),
        Stall,
        Drop,
    }

    // What stands where the network would: every name asked of the resolver, every connection opened, every byte written, and the answers to give.
    #[derive(Default)]
    struct Wire {
        looked_up: Vec<String>,
        connected: Vec<String>,
        sent: Vec<u8>,
        answers: VecDeque<Answer>,
        // What each wait for input of an answer in parts was allowed, and how many of its parts were handed out.
        waits: Vec<NextTimeout>,
        handed: usize,
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
                Answer::Parts(parts) => {
                    let mut wire = self.wire.lock().unwrap();
                    wire.waits.push(timeout);
                    match parts.front_mut() {
                        None => Err(Error::Io(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "ended",
                        ))),
                        Some(Part::Stall) => Err(Error::Timeout(timeout.for_read().reason)),
                        Some(Part::Drop) => Err(Error::Io(io::Error::new(
                            io::ErrorKind::ConnectionReset,
                            "dropped",
                        ))),
                        Some(Part::Bytes(left)) => {
                            let input = self.buffers.input_append_buf();
                            let amount = input.len().min(left.len());
                            input[..amount].copy_from_slice(&left[..amount]);
                            left.drain(..amount);
                            if left.is_empty() {
                                parts.pop_front();
                                wire.handed += 1;
                            }
                            self.buffers.input_appended(amount);
                            Ok(true)
                        }
                    }
                }
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

        let (client, wire) = self::client(vec![ok()]);
        let store = HttpRequest::steam_store_items(2_807_960, 1024, WAIT).unwrap();
        client.send(&store).unwrap();
        assert_eq!(
            lines(&wire),
            [
                format!(
                    "get {} http/1.1",
                    store.url().path_and_query().to_lowercase()
                ),
                "accept-language: *".to_owned(),
                "accept: */*".to_owned(),
                format!("host: {STORE_API_HOST}"),
                "sec-fetch-mode: cors".to_owned(),
                "user-agent: node".to_owned(),
            ]
        );
        let wire = wire.lock().unwrap();
        assert_eq!(wire.looked_up, [STORE_API_HOST]);
        assert_eq!(wire.connected, [store.url().to_string()]);
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

    #[derive(Debug)]
    struct HandshakeRefused;

    impl std::fmt::Display for HandshakeRefused {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("invalid peer certificate: UnknownIssuer")
        }
    }

    impl std::error::Error for HandshakeRefused {}

    #[test]
    fn a_tls_failure_inside_an_io_error_is_told_as_tls_and_the_rest_keep_their_words() {
        let refused = Error::from(io::Error::new(io::ErrorKind::InvalidData, HandshakeRefused));
        assert!(matches!(refused, Error::Io(_)));
        assert_eq!(failure(&refused), HttpError::Failed("TLS failed"));
        let told = format!("{} {:?}", failure(&refused), failure(&refused));
        assert!(!told.contains("UnknownIssuer"), "{told}");

        for kind in [
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::UnexpectedEof,
        ] {
            assert_eq!(
                failure(&Error::from(io::Error::new(kind, "dropped"))),
                HttpError::Failed("the connection failed"),
                "{kind:?}"
            );
        }
        assert_eq!(
            failure(&Error::Timeout(ureq::Timeout::Connect)),
            HttpError::Timeout
        );
        assert_eq!(
            failure(&Error::from(io::Error::other(Error::Timeout(
                ureq::Timeout::PerRead
            )))),
            HttpError::Timeout
        );
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

    use ol_platform_api::update_http::{InstallerName, ReleaseTag};

    fn installer(limit: u64) -> UpdateRequest {
        UpdateRequest::installer(
            &ReleaseTag::new("v1.2.0").unwrap(),
            &InstallerName::new("One-Launch-Setup-1.2.0.exe").unwrap(),
            limit,
            WAIT,
        )
    }

    fn parts(head: &str, body: &[&[u8]], then: Option<Part>) -> Answer {
        let mut parts: VecDeque<Part> =
            body.iter().map(|part| Part::Bytes(part.to_vec())).collect();
        match parts.front_mut() {
            Some(Part::Bytes(first)) => {
                first.splice(0..0, head.bytes());
            }
            _ => parts.push_front(Part::Bytes(head.as_bytes().to_vec())),
        }
        parts.extend(then);
        Answer::Parts(parts)
    }

    struct Fetched {
        result: Result<Downloaded, DownloadError>,
        into: Vec<u8>,
        steps: Vec<u64>,
    }

    fn fetch(client: &HttpsClient, request: &UpdateRequest, cancel_at: Option<u64>) -> Fetched {
        let cancel = CancelToken::new();
        let mut into = Vec::new();
        let mut steps = Vec::new();
        let result = update_http::UpdateHttp::download(
            client,
            request,
            &mut into,
            &mut |so_far| {
                steps.push(so_far);
                if cancel_at.is_some_and(|at| so_far >= at) {
                    cancel.cancel();
                }
            },
            &cancel,
        );
        Fetched {
            result,
            into,
            steps,
        }
    }

    fn ok(length: usize, body: &[u8]) -> Answer {
        answer(
            &format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n\r\n"),
            body,
        )
    }

    struct Full;

    impl io::Write for Full {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::StorageFull, "full"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_updaters_requests_go_on_the_wire_with_the_host_and_the_three_fixed_headers() {
        let (client, wire) = client(vec![ok(2, b"{}")]);
        let latest = UpdateRequest::latest_release(1024, WAIT);
        let answered = update_http::UpdateHttp::send(&client, &latest).unwrap();
        assert_eq!(answered.body.as_deref(), Ok(b"{}".as_slice()));
        let sent = String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap();
        assert!(sent.starts_with("GET /rjf2016/one-launch/releases/latest HTTP/1.1\r\n"));
        assert_eq!(
            lines(&wire),
            [
                "get /rjf2016/one-launch/releases/latest http/1.1",
                "accept: application/json",
                "cache-control: no-cache",
                "host: github.com",
                "user-agent: electron-builder",
            ]
        );
        assert_eq!(wire.lock().unwrap().connected, [latest.url().to_string()]);

        let (client, wire) = self::client(vec![ok(3, b"exe")]);
        let request = installer(3);
        let fetched = fetch(&client, &request, None);
        assert_eq!(fetched.result.unwrap().body, BodyEnd::Complete(3));
        let sent = String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap();
        assert!(sent.starts_with(
            "GET /rjf2016/one-launch/releases/download/v1.2.0/One-Launch-Setup-1.2.0.exe HTTP/1.1\r\n"
        ));
        assert_eq!(
            lines(&wire)[1..],
            [
                "accept: */*",
                "cache-control: no-cache",
                "host: github.com",
                "user-agent: electron-builder",
            ]
        );
        let lower = sent.to_lowercase();
        for absent in ["cookie", "authorization", "accept-encoding", "ureq"] {
            assert!(!lower.contains(absent), "{absent}");
        }

        let moved = request
            .redirected("https://release-assets.githubusercontent.com/a/b?sp=r&sig=x%2By")
            .unwrap();
        let (client, wire) = self::client(vec![ok(3, b"exe")]);
        assert_eq!(fetch(&client, &moved, None).into, b"exe");
        let sent = String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap();
        assert!(sent.starts_with("GET /a/b?sp=r&sig=x%2By HTTP/1.1\r\n"));
        assert!(sent.contains("\r\nhost: release-assets.githubusercontent.com\r\n"));
        assert_eq!(
            wire.lock().unwrap().looked_up,
            ["release-assets.githubusercontent.com"]
        );
    }

    #[test]
    fn a_download_in_parts_arrives_whole_and_progress_sees_each_step() {
        for (head, body) in [
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n",
                [b"abc".as_slice(), b"def", b"ghi"],
            ),
            (
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
                [
                    b"3\r\nabc\r\n".as_slice(),
                    b"3\r\ndef\r\n",
                    b"3\r\nghi\r\n0\r\n\r\n",
                ],
            ),
            (
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
                [b"abc".as_slice(), b"def", b"ghi"],
            ),
        ] {
            let (client, _) = client(vec![parts(head, &body, None)]);
            let fetched = fetch(&client, &installer(9), None);
            let downloaded = fetched.result.unwrap();
            assert_eq!(downloaded.status, 200, "{head}");
            assert_eq!(downloaded.body, BodyEnd::Complete(9), "{head}");
            assert_eq!(fetched.into, b"abcdefghi", "{head}");
            assert_eq!(fetched.steps, [3, 6, 9], "{head}");
            assert_eq!(
                downloaded.declared,
                head.contains("Content-Length").then_some(9),
                "{head}"
            );
        }
    }

    #[test]
    fn a_stall_ends_in_a_timeout_and_a_download_has_no_total_limit() {
        let head = "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n";
        let (client, wire) = client(vec![parts(head, &[b"abc"], Some(Part::Stall))]);
        let fetched = fetch(&client, &installer(6), None);
        assert_eq!(
            fetched.result,
            Err(DownloadError::Network(HttpError::Timeout))
        );
        assert_eq!(fetched.into, b"abc");
        let waits = wire.lock().unwrap().waits.clone();
        let (head_wait, body_wait) = (waits[0], *waits.last().unwrap());
        assert_eq!(head_wait.per_read, Some(WAIT.into()));
        assert!(!head_wait.after.is_not_happening());
        assert!(*head_wait.after <= WAIT);
        assert_eq!(body_wait.per_read, Some(WAIT.into()));
        assert!(body_wait.after.is_not_happening(), "{body_wait:?}");
        assert_eq!(body_wait.for_read().reason, ureq::Timeout::PerRead);

        let (client, _) = self::client(vec![Answer::Parts(VecDeque::from([Part::Stall]))]);
        assert_eq!(
            fetch(&client, &installer(6), None).result,
            Err(DownloadError::Network(HttpError::Timeout))
        );

        let (client, wire) = self::client(vec![parts(head, &[b"abc"], Some(Part::Stall))]);
        let latest = UpdateRequest::latest_release(1024, WAIT);
        assert_eq!(
            update_http::UpdateHttp::send(&client, &latest),
            Err(HttpError::Timeout)
        );
        for wait in &wire.lock().unwrap().waits {
            assert!(*wait.after <= WAIT, "{wait:?}");
            assert_eq!(wait.per_read, None);
        }
    }

    #[test]
    fn a_body_that_ends_early_is_a_failure_and_never_a_short_success() {
        let length = "HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n";
        let chunked = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        for (head, body, dropped) in [
            (length, [b"abc".as_slice(), b"def"], true),
            (length, [b"abc".as_slice(), b"def"], false),
            (chunked, [b"3\r\nabc\r\n".as_slice(), b"3\r\ndef\r\n"], true),
            (
                chunked,
                [b"3\r\nabc\r\n".as_slice(), b"3\r\ndef\r\n"],
                false,
            ),
        ] {
            let then = || dropped.then_some(Part::Drop);
            let (client, _) = client(vec![parts(head, &body, then())]);
            let fetched = fetch(&client, &installer(9), None);
            assert_eq!(
                fetched.result,
                Err(DownloadError::Network(HttpError::Failed(
                    "the connection failed"
                ))),
                "{head} {dropped}"
            );
            assert_eq!(fetched.into, b"abcdef", "{head}");

            let (client, _) = self::client(vec![parts(head, &body, then())]);
            let latest = UpdateRequest::latest_release(1024, WAIT);
            assert_eq!(
                update_http::UpdateHttp::send(&client, &latest),
                Err(HttpError::Failed("the connection failed")),
                "{head} {dropped}"
            );
        }
    }

    #[test]
    fn a_body_past_the_limit_is_refused_when_declared_and_when_read() {
        let (client, wire) = client(vec![parts(
            "HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n",
            &[b"abc", b"def", b"ghi"],
            None,
        )]);
        let fetched = fetch(&client, &installer(8), None);
        let downloaded = fetched.result.unwrap();
        assert_eq!(downloaded.body, BodyEnd::TooLarge(TooLarge::Declared(9)));
        assert_eq!(downloaded.declared, Some(9));
        assert!(fetched.into.is_empty() && fetched.steps.is_empty());
        assert_eq!(wire.lock().unwrap().handed, 1);

        let (client, wire) = self::client(vec![parts(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            &[
                b"3\r\nabc\r\n",
                b"3\r\ndef\r\n",
                b"3\r\nghi\r\n",
                b"3\r\njkl\r\n0\r\n\r\n",
            ],
            None,
        )]);
        let fetched = fetch(&client, &installer(8), None);
        assert_eq!(
            fetched.result.unwrap().body,
            BodyEnd::TooLarge(TooLarge::Read)
        );
        assert_eq!(fetched.into, b"abcdef");
        assert_eq!(wire.lock().unwrap().handed, 3);

        let (client, _) = self::client(vec![parts(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            &[b"abcdefgh", b"i"],
            None,
        )]);
        let fetched = fetch(&client, &installer(8), None);
        assert_eq!(
            fetched.result.unwrap().body,
            BodyEnd::TooLarge(TooLarge::Read)
        );
        assert_eq!(fetched.into, b"abcdefgh");

        let latest = UpdateRequest::latest_release(8, WAIT);
        let (client, _) = self::client(vec![ok(9, b"123456789")]);
        assert_eq!(
            update_http::UpdateHttp::send(&client, &latest)
                .unwrap()
                .body,
            Err(TooLarge::Declared(9))
        );
        let (client, _) = self::client(vec![answer(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"9\r\n123456789\r\n0\r\n\r\n",
        )]);
        assert_eq!(
            update_http::UpdateHttp::send(&client, &latest)
                .unwrap()
                .body,
            Err(TooLarge::Read)
        );
    }

    #[test]
    fn a_cancel_between_parts_ends_the_download_and_nothing_more_is_read() {
        let (client, wire) = client(vec![parts(
            "HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n",
            &[b"abc", b"def", b"ghi"],
            None,
        )]);
        let fetched = fetch(&client, &installer(9), Some(6));
        assert_eq!(fetched.result.unwrap().body, BodyEnd::Cancelled(6));
        assert_eq!(fetched.into, b"abcdef");
        assert_eq!(fetched.steps, [3, 6]);
        assert_eq!(wire.lock().unwrap().handed, 2);

        let (client, wire) = self::client(vec![ok(3, b"abc")]);
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = update_http::UpdateHttp::download(
            &client,
            &installer(9),
            &mut Vec::new(),
            &mut |_| {},
            &cancel,
        );
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert!(wire.lock().unwrap().connected.is_empty());
    }

    #[test]
    fn a_redirect_comes_back_with_its_location_and_is_not_followed() {
        let moved = "HTTP/1.1 302 Found\r\nLocation: https://release-assets.githubusercontent.com/a?sig=1\r\nContent-Length: 5\r\n\r\n";
        let (client, wire) = client(vec![answer(moved, b"moved"), ok(2, b"no")]);
        let request = installer(9);
        let fetched = fetch(&client, &request, None);
        let downloaded = fetched.result.unwrap();
        assert_eq!(downloaded.status, 302);
        assert_eq!(
            downloaded.location.as_deref(),
            Some("https://release-assets.githubusercontent.com/a?sig=1")
        );
        assert_eq!(downloaded.body, BodyEnd::NotRead);
        assert!(fetched.into.is_empty() && fetched.steps.is_empty());
        assert_eq!(wire.lock().unwrap().connected, [request.url().to_string()]);
        assert_eq!(wire.lock().unwrap().looked_up, ["github.com"]);

        let (client, wire) = self::client(vec![answer(moved, b"moved"), ok(2, b"no")]);
        let manifest = UpdateRequest::manifest(&ReleaseTag::new("v1").unwrap(), 1024, WAIT);
        let answered = update_http::UpdateHttp::send(&client, &manifest).unwrap();
        assert_eq!(answered.status, 302);
        assert_eq!(
            answered.location.as_deref(),
            Some("https://release-assets.githubusercontent.com/a?sig=1")
        );
        assert_eq!(answered.body, Ok(Vec::new()));
        assert_eq!(wire.lock().unwrap().connected.len(), 1);
    }

    #[test]
    fn a_404_and_a_503_come_back_with_no_body_read() {
        for status in ["404 Not Found", "503 Service Unavailable"] {
            let head = format!("HTTP/1.1 {status}\r\nContent-Length: 9\r\n\r\n");
            let (client, wire) = client(vec![parts(&head, &[b"not", b" found"], None)]);
            let fetched = fetch(&client, &installer(1024), None);
            let downloaded = fetched.result.unwrap();
            assert_eq!(downloaded.status.to_string(), &status[..3]);
            assert_eq!(downloaded.body, BodyEnd::NotRead);
            assert!(fetched.into.is_empty());
            assert_eq!(wire.lock().unwrap().handed, 1, "{status}");

            let (client, _) = self::client(vec![answer(&head, b"not found")]);
            let latest = UpdateRequest::latest_release(1024, WAIT);
            let answered = update_http::UpdateHttp::send(&client, &latest).unwrap();
            assert_eq!(answered.status.to_string(), &status[..3]);
            assert_eq!(answered.body, Ok(Vec::new()));
        }
    }

    #[test]
    fn a_failed_write_is_told_apart_from_a_network_failure() {
        let (client, _) = client(vec![ok(3, b"abc")]);
        let result = update_http::UpdateHttp::download(
            &client,
            &installer(9),
            &mut Full,
            &mut |_| {},
            &CancelToken::new(),
        );
        assert_eq!(
            result,
            Err(DownloadError::Write(io::ErrorKind::StorageFull))
        );

        let (client, wire) = self::client(vec![]);
        let mut spent = installer(9);
        spent.timeout = Duration::ZERO;
        assert_eq!(
            fetch(&client, &spent, None).result,
            Err(DownloadError::Network(HttpError::Timeout))
        );
        assert_eq!(
            update_http::UpdateHttp::send(&client, &spent),
            Err(HttpError::Timeout)
        );
        assert!(wire.lock().unwrap().connected.is_empty());
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test asks the client's own agent for plain http to see it refused"
    )]
    fn plain_http_is_refused_by_the_request_and_by_the_client() {
        assert!(installer(9).redirected("http://github.com/a").is_none());
        let (client, wire) = client(vec![ok(2, b"no")]);
        let refused = client
            .agent
            .get("http://github.com/rjf2016/one-launch/releases/latest")
            .call();
        assert!(matches!(refused, Err(Error::RequireHttpsOnly(_))));
        let wire = wire.lock().unwrap();
        assert!(wire.connected.is_empty() && wire.looked_up.is_empty());
    }
}
