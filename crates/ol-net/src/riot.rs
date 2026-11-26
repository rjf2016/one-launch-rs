// The one request One Launch sends the Riot Client's own local server: the launch request its Play button sends (Riot's rule 4). The host is this PC and nothing else, the port and the password come from the client's lockfile at the moment of the call, and the bytes read, their text, the password, the credentials and the header's text are wiped when the call returns; the header value ureq holds, ureq's and rustls's buffers and the file port's are not. Nothing here logs, and no error can carry what the lockfile holds.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ol_formats::riot::lockfile::parse_lockfile;
use ol_platform_api::{FileReader, RiotLaunchError, RiotLaunchRequest, RiotLocalLaunch};
use ureq::config::Config;
use ureq::http::header::{ACCEPT, AUTHORIZATION, CONNECTION};
use ureq::http::{HeaderValue, Uri};
use ureq::tls::{TlsConfig, TlsProvider};
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{Connector, NextTimeout, RustlsConnector, TcpConnector};
use ureq::{Agent, Error};
use zeroize::Zeroizing;

const HOST: &str = "127.0.0.1";
const LIMIT: Duration = Duration::from_secs(10);
// A lockfile is one short line; one that fills this is not the client's.
const LOCKFILE_LIMIT: u64 = 4096;

pub struct RiotLoopback {
    files: Arc<dyn FileReader>,
    agent: Agent,
}

impl RiotLoopback {
    pub fn new(files: Arc<dyn FileReader>) -> Self {
        Self::over(
            files,
            ().chain(TcpConnector::default())
                .chain(RustlsConnector::default()),
            LIMIT,
        )
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "this is the Riot loopback channel of ol-net, the one place its agent is made"
    )]
    fn over(files: Arc<dyn FileReader>, connector: impl Connector, limit: Duration) -> Self {
        Self {
            files,
            agent: Agent::with_parts(config(limit), connector, LoopbackOnly),
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the one agent that does not check certificates, and it reaches 127.0.0.1 alone"
)]
fn config(limit: Duration) -> Config {
    Agent::config_builder()
        .https_only(true)
        // ureq's default reads HTTPS_PROXY and ALL_PROXY, and the request would go wherever the environment says.
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .max_idle_connections(0)
        .max_idle_connections_per_host(0)
        .timeout_global(Some(limit))
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::Rustls)
                // The client's server presents a certificate of its own, as the Electron app accepts it; this agent reaches nothing but 127.0.0.1.
                .disable_verification(true)
                .build(),
        )
        .user_agent("")
        .accept("")
        .accept_encoding("")
        .build()
}

// Answers 127.0.0.1 and the port the address names, for an https address on 127.0.0.1 and nothing else, and looks no name up.
#[derive(Debug)]
struct LoopbackOnly;

impl Resolver for LoopbackOnly {
    fn resolve(&self, uri: &Uri, _: &Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, Error> {
        let authority = uri.authority().ok_or(Error::HostNotFound)?;
        let port = authority
            .port_u16()
            .filter(|port| *port != 0)
            .ok_or(Error::HostNotFound)?;
        if uri.scheme_str() != Some("https")
            || authority.host() != HOST
            || authority.as_str().contains('@')
        {
            return Err(Error::HostNotFound);
        }
        let mut found = self.empty();
        found.push(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        Ok(found)
    }
}

fn is_id_part(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

impl RiotLocalLaunch for RiotLoopback {
    #[expect(
        clippy::disallowed_methods,
        reason = "this is the Riot loopback channel of ol-net, the one place its request is sent"
    )]
    fn launch(&self, request: &RiotLaunchRequest<'_>) -> Result<(), RiotLaunchError> {
        if request.cancel.is_cancelled() {
            return Err(RiotLaunchError::Stopped);
        }
        if !is_id_part(request.product) || !is_id_part(request.patchline) {
            return Err(RiotLaunchError::ProductShape);
        }
        let bytes = Zeroizing::new(
            self.files
                .read_range(request.lockfile_path, 0, LOCKFILE_LIMIT)
                .map_err(|_| RiotLaunchError::NoLockfile)?,
        );
        if bytes.len() as u64 >= LOCKFILE_LIMIT {
            return Err(RiotLaunchError::LockfileShape);
        }
        let text = Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned());
        let lock = parse_lockfile(&text).ok_or(RiotLaunchError::LockfileShape)?;
        let port = lock.port;
        let authorization = basic(&lock.password.take())?;
        let url = format!(
            "https://{HOST}:{port}/product-launcher/v1/products/{}/patchlines/{}",
            request.product, request.patchline
        );
        let answer = self
            .agent
            .post(url)
            .header(AUTHORIZATION, authorization)
            .header(ACCEPT, "application/json")
            .header(CONNECTION, "close")
            .send_empty()
            .map_err(|error| unreachable(&error))?;
        let status = answer.status().as_u16();
        if (200..=299).contains(&status) {
            Ok(())
        } else {
            Err(RiotLaunchError::Status(status))
        }
    }
}

// Sized up front and encoded in place, so the credentials and the header's text leave no copy of ours behind; the HeaderValue made from them is ureq's and is not wiped.
fn basic(password: &str) -> Result<HeaderValue, RiotLaunchError> {
    let mut credentials = Zeroizing::new(String::with_capacity(5 + password.len()));
    credentials.push_str("riot:");
    credentials.push_str(password);
    let encoded =
        base64::encoded_len(credentials.len(), true).ok_or(RiotLaunchError::LockfileShape)?;
    let mut header = Zeroizing::new(vec![0u8; 6 + encoded]);
    header[..6].copy_from_slice(b"Basic ");
    STANDARD
        .encode_slice(credentials.as_bytes(), &mut header[6..])
        .map_err(|_| RiotLaunchError::LockfileShape)?;
    let mut value = HeaderValue::from_bytes(&header).map_err(|_| RiotLaunchError::LockfileShape)?;
    value.set_sensitive(true);
    Ok(value)
}

// A few fixed words and never ureq's own sentence.
fn unreachable(error: &Error) -> RiotLaunchError {
    RiotLaunchError::Unreachable(match error {
        Error::Timeout(_) => "timed out",
        Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => "refused",
        // rustls reports a handshake or a record it cannot take as InvalidData.
        Error::Io(io) if io.kind() == std::io::ErrorKind::InvalidData => "TLS failed",
        Error::Tls(_) | Error::Rustls(_) | Error::Pem(_) | Error::TlsRequired => "TLS failed",
        _ => "error",
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::io::{self, Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::Mutex;
    use std::thread;

    use ol_platform_api::CancelToken;
    use time::OffsetDateTime;
    use ureq::unversioned::transport::{Buffers, ConnectionDetails, LazyBuffers, Transport};

    use super::*;

    const PASSWORD: &str = "placeholder-lockfile-password";
    const PATH: &str = r"C:\placeholder\Riot Client\Config\lockfile";
    const PORT: u16 = 51234;
    const SHORT: Duration = Duration::from_millis(500);

    fn encoded() -> String {
        STANDARD.encode(format!("riot:{PASSWORD}"))
    }

    fn lockfile(port: u16) -> Vec<u8> {
        format!("Riot Client:4242:{port}:{PASSWORD}:https\r\n").into_bytes()
    }

    // The files it holds, and every question the channel asked of it.
    #[derive(Default)]
    struct Lockfiles {
        files: HashMap<String, Vec<u8>>,
        asked: Mutex<Vec<String>>,
    }

    impl Lockfiles {
        fn with(path: &str, bytes: Vec<u8>) -> Arc<Self> {
            Arc::new(Self {
                files: HashMap::from([(path.to_owned(), bytes)]),
                ..Self::default()
            })
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }

        fn ask(&self, what: String) {
            self.asked.lock().unwrap().push(what);
        }

        fn file(&self, path: &str) -> io::Result<&Vec<u8>> {
            self.files
                .get(path)
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }
    }

    impl FileReader for Lockfiles {
        fn exists(&self, path: &str) -> bool {
            self.ask(format!("exists {path}"));
            self.files.contains_key(path)
        }

        fn size(&self, path: &str) -> io::Result<u64> {
            self.ask(format!("size {path}"));
            Ok(self.file(path)?.len() as u64)
        }

        fn modified(&self, path: &str) -> io::Result<OffsetDateTime> {
            self.ask(format!("modified {path}"));
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }

        fn read(&self, path: &str) -> io::Result<Vec<u8>> {
            self.ask(format!("read {path}"));
            self.file(path).cloned()
        }

        fn read_range(&self, path: &str, from: u64, length: u64) -> io::Result<Vec<u8>> {
            self.ask(format!("read_range {path} {from} {length}"));
            let bytes = self.file(path)?;
            let from = usize::try_from(from).unwrap().min(bytes.len());
            let to = from
                .saturating_add(usize::try_from(length).unwrap())
                .min(bytes.len());
            Ok(bytes[from..to].to_vec())
        }

        fn list(&self, folder: &str) -> io::Result<Vec<String>> {
            self.ask(format!("list {folder}"));
            Ok(Vec::new())
        }

        fn resolved(&self, folder: &str) -> Option<String> {
            self.ask(format!("resolved {folder}"));
            None
        }
    }

    enum Answer {
        Bytes(Vec<u8>),
        Stalls,
        Drops,
        Refused,
    }

    impl std::fmt::Debug for Answer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Answer")
        }
    }

    // What stands where the socket would: each connection opened, with the addresses the resolver gave it, every byte written, and the answers to give.
    #[derive(Default)]
    struct Wire {
        connected: Vec<(String, Vec<SocketAddr>)>,
        sent: Vec<u8>,
        answers: VecDeque<Answer>,
        stays_open: bool,
    }

    impl std::fmt::Debug for Wire {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Wire")
        }
    }

    type Shared = Arc<Mutex<Wire>>;

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
            wire.connected
                .push((details.uri.to_string(), details.addrs.to_vec()));
            let answer = wire.answers.pop_front().unwrap_or(Answer::Drops);
            if matches!(answer, Answer::Refused) {
                return Err(Error::Io(io::Error::from(io::ErrorKind::ConnectionRefused)));
            }
            Ok(Some(Canned {
                buffers: LazyBuffers::new(
                    details.config.input_buffer_size(),
                    details.config.output_buffer_size(),
                ),
                open: wire.stays_open,
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
        open: bool,
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
                Answer::Drops | Answer::Refused => Err(Error::Io(io::Error::new(
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
            self.open
        }

        fn is_tls(&self) -> bool {
            true
        }
    }

    fn answer(head: &str) -> Answer {
        Answer::Bytes(head.as_bytes().to_vec())
    }

    fn ok() -> Answer {
        answer("HTTP/1.1 204 No Content\r\n\r\n")
    }

    fn channel(files: Arc<Lockfiles>, answers: Vec<Answer>) -> (RiotLoopback, Shared) {
        let wire: Shared = Arc::new(Mutex::new(Wire {
            answers: answers.into(),
            ..Wire::default()
        }));
        let channel = RiotLoopback::over(files, InProcess(Arc::clone(&wire)), SHORT);
        (channel, wire)
    }

    fn launch(channel: &RiotLoopback) -> Result<(), RiotLaunchError> {
        launch_as(channel, "valorant", "live", &CancelToken::new())
    }

    fn launch_as(
        channel: &RiotLoopback,
        product: &str,
        patchline: &str,
        cancel: &CancelToken,
    ) -> Result<(), RiotLaunchError> {
        channel.launch(&RiotLaunchRequest {
            product,
            patchline,
            lockfile_path: PATH,
            cancel,
        })
    }

    fn sent(wire: &Shared) -> String {
        String::from_utf8(wire.lock().unwrap().sent.clone()).unwrap()
    }

    fn told(error: RiotLaunchError) -> String {
        format!("{error} {error:?} {error:#?}")
    }

    fn assert_holds_no_secret(error: RiotLaunchError) {
        let told = told(error);
        assert!(
            !told.contains(PASSWORD) && !told.contains(&encoded()),
            "the password is in an error"
        );
    }

    #[test]
    fn the_request_is_one_post_to_127_0_0_1_with_the_basic_header_once_and_nothing_else() {
        let files = Lockfiles::with(PATH, lockfile(PORT));
        let (channel, wire) = channel(Arc::clone(&files), vec![ok()]);
        assert_eq!(launch(&channel), Ok(()));

        let sent = sent(&wire);
        let (head, body) = sent.split_once("\r\n\r\n").expect("a request has a head");
        assert_eq!(body, "", "the request has no body");
        let mut lines: Vec<&str> = head.split("\r\n").collect();
        assert_eq!(
            lines[0],
            "POST /product-launcher/v1/products/valorant/patchlines/live HTTP/1.1"
        );
        lines[1..].sort_unstable();
        let basic = format!("authorization: Basic {}", encoded());
        assert_eq!(
            lines[1..],
            [
                "accept: application/json",
                basic.as_str(),
                "connection: close",
                "content-length: 0",
                "host: 127.0.0.1:51234",
            ]
        );
        assert_eq!(sent.matches(&encoded()).count(), 1);
        assert!(!sent.contains(PASSWORD));

        let wire = wire.lock().unwrap();
        assert_eq!(
            wire.connected,
            [(
                "https://127.0.0.1:51234/product-launcher/v1/products/valorant/patchlines/live"
                    .to_owned(),
                vec![SocketAddr::from(([127, 0, 0, 1], PORT))]
            )]
        );
        assert_eq!(files.asked(), [format!("read_range {PATH} 0 4096")]);
    }

    #[test]
    fn a_2xx_is_taken_and_any_other_status_is_told_without_the_body() {
        for head in [
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}",
            "HTTP/1.1 204 No Content\r\n\r\n",
        ] {
            let (channel, _) = channel(Lockfiles::with(PATH, lockfile(PORT)), vec![answer(head)]);
            assert_eq!(launch(&channel), Ok(()), "{head}");
        }
        for (head, status) in [
            (
                "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nnot found",
                404,
            ),
            (
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\n\r\noops",
                500,
            ),
            (
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n",
                401,
            ),
        ] {
            let (channel, wire) =
                channel(Lockfiles::with(PATH, lockfile(PORT)), vec![answer(head)]);
            let refused = launch(&channel).unwrap_err();
            assert_holds_no_secret(refused);
            assert_eq!(refused, RiotLaunchError::Status(status));
            assert!(!told(refused).contains("not found"));
            assert_eq!(wire.lock().unwrap().connected.len(), 1);
        }
    }

    #[test]
    fn a_redirect_comes_back_as_its_status_after_one_connection() {
        for location in [
            "https://127.0.0.1:51234/product-launcher/v1/products/valorant/patchlines/pbe",
            "https://example.com/elsewhere",
        ] {
            let (channel, wire) = channel(
                Lockfiles::with(PATH, lockfile(PORT)),
                vec![
                    answer(&format!(
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
                    )),
                    ok(),
                ],
            );
            assert_eq!(launch(&channel), Err(RiotLaunchError::Status(302)));
            let wire = wire.lock().unwrap();
            assert_eq!(wire.connected.len(), 1, "{location}");
            assert_eq!(wire.answers.len(), 1, "{location}");
            let sent = String::from_utf8(wire.sent.clone()).unwrap();
            assert_eq!(sent.matches("POST ").count(), 1, "{location}");
        }
    }

    #[test]
    fn no_connection_is_kept_for_a_later_call() {
        let wire: Shared = Arc::new(Mutex::new(Wire {
            answers: vec![
                answer("HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: 0\r\n\r\n"),
                ok(),
            ]
            .into(),
            stays_open: true,
            ..Wire::default()
        }));
        let channel = RiotLoopback::over(
            Lockfiles::with(PATH, lockfile(PORT)),
            InProcess(Arc::clone(&wire)),
            SHORT,
        );
        assert_eq!(launch(&channel), Ok(()));
        assert_eq!(launch(&channel), Ok(()));
        assert_eq!(wire.lock().unwrap().connected.len(), 2);
    }

    #[test]
    fn a_failure_is_told_in_fixed_words() {
        for (answers, words) in [
            (vec![Answer::Stalls], "timed out"),
            (vec![Answer::Drops], "error"),
            (vec![Answer::Refused], "refused"),
            (vec![answer("not http at all\r\n\r\n")], "error"),
        ] {
            let (channel, _) = channel(Lockfiles::with(PATH, lockfile(PORT)), answers);
            let failed = launch(&channel).unwrap_err();
            assert_holds_no_secret(failed);
            assert_eq!(failed, RiotLaunchError::Unreachable(words));
        }
    }

    #[test]
    fn the_resolver_answers_127_0_0_1_and_the_port_named_and_nothing_else() {
        let config = config(SHORT);
        let resolve = |url: &str| {
            let uri: Uri = url.parse().unwrap();
            LoopbackOnly
                .resolve(&uri, &config, NextTimeout::default())
                .map(|found| found.to_vec())
        };
        assert_eq!(
            resolve("https://127.0.0.1:51234/x").unwrap(),
            [SocketAddr::from(([127, 0, 0, 1], 51234))]
        );
        assert_eq!(HOST, Ipv4Addr::LOCALHOST.to_string());
        for other in [
            "https://localhost:51234/x",
            "https://example.com:51234/x",
            "https://example.com/x",
            "https://127.0.0.2:51234/x",
            "https://127.1:51234/x",
            "https://0.0.0.0:51234/x",
            "https://[::1]:51234/x",
            "https://127.0.0.1/x",
            "https://127.0.0.1:0/x",
            "http://127.0.0.1:51234/x",
            "https://riot@127.0.0.1:51234/x",
            "https://127.0.0.1.example.com:51234/x",
        ] {
            assert!(
                matches!(resolve(other), Err(Error::HostNotFound)),
                "{other}"
            );
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test asks the channel's own agent for another host, which the channel never does"
    )]
    fn the_channels_agent_refuses_any_other_host_before_it_connects() {
        for url in [
            "https://localhost:51234/x",
            "https://example.invalid:51234/x",
            "https://127.0.0.2:51234/x",
        ] {
            let (channel, wire) = channel(Lockfiles::with(PATH, lockfile(PORT)), vec![ok()]);
            let refused = channel.agent.post(url).send_empty();
            assert!(matches!(refused, Err(Error::HostNotFound)), "{url}");
            assert!(wire.lock().unwrap().connected.is_empty(), "{url}");
            assert!(wire.lock().unwrap().sent.is_empty(), "{url}");
        }
    }

    #[test]
    fn a_bad_product_or_patchline_is_refused_before_the_lockfile_is_asked_for() {
        for (product, patchline) in [
            ("VALORANT", "live"),
            ("valorant", "Live"),
            ("", "live"),
            ("valorant", ""),
            ("valorant/../x", "live"),
            ("valorant", "live?x=1"),
            ("valorant", "live#x"),
            ("val orant", "live"),
            ("valorant.live", "live"),
            ("valorant", "live%2f"),
        ] {
            let files = Lockfiles::with(PATH, lockfile(PORT));
            let (channel, wire) = channel(Arc::clone(&files), vec![ok()]);
            assert_eq!(
                launch_as(&channel, product, patchline, &CancelToken::new()),
                Err(RiotLaunchError::ProductShape),
                "{product}.{patchline}"
            );
            assert!(files.asked().is_empty());
            assert!(wire.lock().unwrap().connected.is_empty());
        }
        let (channel, _) = channel(Lockfiles::with(PATH, lockfile(PORT)), vec![ok()]);
        assert_eq!(
            launch_as(&channel, "league_of_legends", "pbe", &CancelToken::new()),
            Ok(())
        );
    }

    #[test]
    fn a_lockfile_missing_too_large_or_not_in_shape_sends_nothing() {
        let padded = |length: usize| {
            let mut bytes = lockfile(PORT);
            bytes.resize(length, b' ');
            bytes
        };
        let not_in_shape = [
            padded(5000),
            padded(4096),
            b"Riot Client:4242:51234::https".to_vec(),
            format!("Riot Client:4242:51234:{PASSWORD}:http").into_bytes(),
            format!("LeagueClient:4242:51234:{PASSWORD}:https").into_bytes(),
            format!("Riot Client:4242:0:{PASSWORD}:https").into_bytes(),
            vec![0xff, 0xfe, 0x00, b':'],
        ];
        let mut cases = vec![(Arc::new(Lockfiles::default()), RiotLaunchError::NoLockfile)];
        for bytes in not_in_shape {
            cases.push((Lockfiles::with(PATH, bytes), RiotLaunchError::LockfileShape));
        }
        for (files, refusal) in cases {
            let (channel, wire) = channel(Arc::clone(&files), vec![ok()]);
            let refused = launch(&channel).unwrap_err();
            assert_holds_no_secret(refused);
            assert_eq!(refused, refusal);
            assert_eq!(files.asked(), [format!("read_range {PATH} 0 4096")]);
            assert!(wire.lock().unwrap().connected.is_empty());
            assert!(wire.lock().unwrap().sent.is_empty());
        }
        let (channel, _) = channel(Lockfiles::with(PATH, padded(4095)), vec![ok()]);
        assert_eq!(launch(&channel), Ok(()));
    }

    #[test]
    fn a_stopped_job_reads_nothing_and_sends_nothing() {
        let files = Lockfiles::with(PATH, lockfile(PORT));
        let (channel, wire) = channel(Arc::clone(&files), vec![ok()]);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            launch_as(&channel, "valorant", "live", &cancel),
            Err(RiotLaunchError::Stopped)
        );
        assert!(files.asked().is_empty());
        assert!(wire.lock().unwrap().connected.is_empty());
    }

    fn on_this_pc(port: u16, limit: Duration) -> RiotLoopback {
        RiotLoopback::over(
            Lockfiles::with(PATH, lockfile(port)),
            ().chain(TcpConnector::default())
                .chain(RustlsConnector::default()),
            limit,
        )
    }

    // A test connects only to a port whose listener it holds open until the connection is taken, so nothing else on this PC can be the one that answers.
    fn listener(serve: impl FnOnce(TcpStream) + Send + 'static) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream);
        });
        (port, served)
    }

    fn read_record(stream: &mut TcpStream) -> Vec<u8> {
        let mut head = [0u8; 5];
        stream.read_exact(&mut head).unwrap();
        let mut body = vec![0u8; usize::from(u16::from_be_bytes([head[3], head[4]]))];
        stream.read_exact(&mut body).unwrap();
        [head.as_slice(), &body].concat()
    }

    #[test]
    fn the_server_is_greeted_with_a_tls_handshake_and_a_close_ends_in_fixed_words() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        let (port, served) = listener(move |mut stream| {
            *record.lock().unwrap() = read_record(&mut stream);
            stream.shutdown(Shutdown::Both).unwrap();
        });
        let failed = launch(&on_this_pc(port, LIMIT)).unwrap_err();
        served.join().unwrap();
        let hello = seen.lock().unwrap().clone();
        assert_eq!(hello[0], 0x16, "a TLS handshake record");
        assert_eq!(hello[5], 0x01, "a ClientHello");
        assert!(
            !hello
                .windows(PASSWORD.len())
                .any(|at| at == PASSWORD.as_bytes()),
            "the password is in the handshake"
        );
        assert_holds_no_secret(failed);
        assert_eq!(failed, RiotLaunchError::Unreachable("error"));
    }

    #[test]
    fn a_server_that_answers_in_plain_http_fails_tls() {
        let (port, served) = listener(|mut stream| {
            let _ = read_record(&mut stream);
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
            let _ = stream.shutdown(Shutdown::Write);
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest);
        });
        let failed = launch(&on_this_pc(port, LIMIT)).unwrap_err();
        served.join().unwrap();
        assert_holds_no_secret(failed);
        assert_eq!(failed, RiotLaunchError::Unreachable("TLS failed"));
    }

    #[test]
    fn a_server_that_never_answers_times_out() {
        let (port, served) = listener(|mut stream| {
            let _ = read_record(&mut stream);
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest);
        });
        let failed = launch(&on_this_pc(port, SHORT)).unwrap_err();
        served.join().unwrap();
        assert_holds_no_secret(failed);
        assert_eq!(failed, RiotLaunchError::Unreachable("timed out"));
    }

    #[test]
    fn the_agent_is_set_up_for_127_0_0_1_alone_within_ten_seconds() {
        let config = config(LIMIT);
        assert!(config.https_only());
        assert!(config.proxy().is_none());
        assert_eq!(config.max_redirects(), 0);
        assert!(!config.http_status_as_error());
        assert_eq!(config.max_idle_connections(), 0);
        assert_eq!(config.max_idle_connections_per_host(), 0);
        assert_eq!(config.timeouts().global, Some(Duration::from_secs(10)));
        assert!(matches!(
            config.tls_config().provider(),
            TlsProvider::Rustls
        ));
        assert!(config.tls_config().disable_verification());
        let _ = RiotLoopback::new(Arc::new(Lockfiles::default()));
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
    fn the_channel_names_no_proxy() {
        for name in PROXIES {
            assert!(std::env::var_os(name).is_some(), "{name} is not set");
        }
        assert!(ureq::Proxy::try_from_env().is_some());
        assert!(config(LIMIT).proxy().is_none());
        let (channel, wire) = channel(Lockfiles::with(PATH, lockfile(PORT)), vec![ok()]);
        assert_eq!(launch(&channel), Ok(()));
        assert_eq!(
            wire.lock().unwrap().connected,
            [(
                "https://127.0.0.1:51234/product-launcher/v1/products/valorant/patchlines/live"
                    .to_owned(),
                vec![SocketAddr::from(([127, 0, 0, 1], PORT))]
            )]
        );
        assert!(!sent(&wire).starts_with("CONNECT"));
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a test cannot change its own environment, so it runs itself again with another"
    )]
    fn a_proxy_named_in_the_environment_is_not_used() {
        let ran = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "riot::tests::the_channel_names_no_proxy",
                "--ignored",
            ])
            .envs(PROXIES.map(|name| (name, "http://127.0.0.1:9")))
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&ran.stdout);
        assert!(ran.status.success(), "{said}");
        assert!(said.contains("1 passed"), "{said}");
    }
}
