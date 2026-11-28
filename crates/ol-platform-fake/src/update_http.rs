use std::collections::VecDeque;
use std::io;
use std::sync::{Mutex, PoisonError};

use ol_platform_api::CancelToken;
use ol_platform_api::http::{HttpError, HttpResponse, TooLarge};
use ol_platform_api::update_http::{
    BodyEnd, DownloadError, Downloaded, UpdateAsk, UpdateHttp, UpdateRequest, copy_body,
};

const DROPPED: HttpError = HttpError::Failed("the connection failed");

// One step of an answer's body, as the wire would hand it out.
pub enum Part {
    Bytes(Vec<u8>),
    // Run once the part before it has been read and before the next is asked: to cancel, to move a fake clock, to switch a setting.
    Run(Box<dyn FnOnce() + Send>),
    // The connection drops here.
    Drop,
    // Nothing more comes, and the read times out.
    Stall,
}

impl Part {
    pub fn run(hook: impl FnOnce() + Send + 'static) -> Self {
        Self::Run(Box::new(hook))
    }
}

// An answer as the test scripts it. A declared length is held as the real client holds a Content-Length: the body ends there, and parts that end sooner are a dropped connection. Without one the body is read as a chunked one, to the end of its parts.
pub struct FakeAnswer {
    pub status: u16,
    pub content_type: Option<String>,
    pub location: Option<String>,
    pub declared: Option<u64>,
    pub parts: Vec<Part>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateCall {
    Send,
    Download,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateAsked {
    pub url: String,
    pub ask: UpdateAsk,
    // Every header the request would send beside Host.
    pub headers: Vec<(&'static str, &'static str)>,
    pub call: UpdateCall,
}

type Responder = dyn Fn(&UpdateRequest) -> Result<FakeAnswer, HttpError> + Send + Sync;

// Stands where GitHub would: every request is written down and answered in this process by the responder it was made with, and a body goes through the limit and the cancel as the real client's does: a 2xx body alone, never past the request's limit, the token looked at before each read.
pub struct ScriptedUpdates {
    respond: Box<Responder>,
    asked: Mutex<Vec<UpdateAsked>>,
}

impl ScriptedUpdates {
    pub fn new(
        respond: impl Fn(&UpdateRequest) -> Result<FakeAnswer, HttpError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            respond: Box::new(respond),
            asked: Mutex::default(),
        }
    }

    pub fn asked(&self) -> Vec<UpdateAsked> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn urls(&self) -> Vec<String> {
        self.asked().into_iter().map(|asked| asked.url).collect()
    }

    // Nothing is asked of a request whose time is already spent, as the real client sends nothing then.
    fn answer(
        &self,
        request: &UpdateRequest,
        call: UpdateCall,
    ) -> Result<(FakeAnswer, Body), HttpError> {
        if request.timeout.is_zero() {
            return Err(HttpError::Timeout);
        }
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(UpdateAsked {
                url: request.url().to_string(),
                ask: request.ask(),
                headers: request.headers().to_vec(),
                call,
            });
        let mut answer = (self.respond)(request)?;
        let body = Body {
            parts: std::mem::take(&mut answer.parts).into(),
            left: answer.declared,
        };
        Ok((answer, body))
    }
}

struct Body {
    parts: VecDeque<Part>,
    left: Option<u64>,
}

impl Body {
    fn run_hooks(&mut self) {
        while matches!(self.parts.front(), Some(Part::Run(_))) {
            if let Some(Part::Run(hook)) = self.parts.pop_front() {
                hook();
            }
        }
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, HttpError> {
        loop {
            if self.left == Some(0) {
                return Ok(0);
            }
            match self.parts.pop_front() {
                None if self.left.is_some() => return Err(DROPPED),
                None => return Ok(0),
                Some(Part::Run(hook)) => hook(),
                Some(Part::Drop) => return Err(DROPPED),
                Some(Part::Stall) => return Err(HttpError::Timeout),
                Some(Part::Bytes(mut bytes)) => {
                    let wanted = self.left.map_or(usize::MAX, |left| {
                        usize::try_from(left).unwrap_or(usize::MAX)
                    });
                    let amount = buffer.len().min(bytes.len()).min(wanted);
                    if amount == 0 {
                        continue;
                    }
                    buffer[..amount].copy_from_slice(&bytes[..amount]);
                    if let Some(left) = &mut self.left {
                        *left -= amount as u64;
                    }
                    if amount < bytes.len() {
                        bytes.drain(..amount);
                        self.parts.push_front(Part::Bytes(bytes));
                    } else {
                        self.run_hooks();
                    }
                    return Ok(amount);
                }
            }
        }
    }
}

fn is_ok(status: u16) -> bool {
    (200..=299).contains(&status)
}

impl UpdateHttp for ScriptedUpdates {
    fn send(&self, request: &UpdateRequest) -> Result<HttpResponse, HttpError> {
        let (answer, mut body) = self.answer(request, UpdateCall::Send)?;
        let read = if !is_ok(answer.status) {
            Ok(Vec::new())
        } else if let Some(declared) = answer.declared.filter(|declared| *declared > request.limit)
        {
            Err(TooLarge::Declared(declared))
        } else {
            body.run_hooks();
            let mut kept = Vec::new();
            let mut buffer = vec![0; 64 * 1024];
            loop {
                let amount = body.read(&mut buffer)?;
                if amount == 0 {
                    break Ok(kept);
                }
                if kept.len() as u64 + amount as u64 > request.limit {
                    break Err(TooLarge::Read);
                }
                kept.extend_from_slice(&buffer[..amount]);
            }
        };
        Ok(HttpResponse {
            status: answer.status,
            content_type: answer.content_type,
            location: answer.location,
            body: read,
        })
    }

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
        let (answer, mut body) = self.answer(request, UpdateCall::Download)?;
        let end = if is_ok(answer.status) {
            body.run_hooks();
            copy_body(
                request,
                answer.declared,
                &mut |buffer| body.read(buffer),
                into,
                progress,
                cancel,
            )?
        } else {
            BodyEnd::NotRead
        };
        Ok(Downloaded {
            status: answer.status,
            location: answer.location,
            declared: answer.declared,
            body: end,
        })
    }
}

pub fn status(status: u16) -> FakeAnswer {
    FakeAnswer {
        status,
        content_type: None,
        location: None,
        declared: Some(0),
        parts: Vec::new(),
    }
}

pub fn json(status: u16, text: &str) -> FakeAnswer {
    FakeAnswer {
        content_type: Some("application/json".to_owned()),
        ..file_with(status, text.as_bytes().to_vec())
    }
}

pub fn redirect(status: u16, location: &str) -> FakeAnswer {
    FakeAnswer {
        location: Some(location.to_owned()),
        ..self::status(status)
    }
}

// A 200 with its length, in one part.
pub fn file(bytes: Vec<u8>) -> FakeAnswer {
    file_with(200, bytes)
}

// A 200 whose body comes in these parts, with this length declared or none.
pub fn in_parts(declared: Option<u64>, parts: Vec<Part>) -> FakeAnswer {
    FakeAnswer {
        declared,
        parts,
        ..status(200)
    }
}

fn file_with(status: u16, bytes: Vec<u8>) -> FakeAnswer {
    FakeAnswer {
        declared: Some(bytes.len() as u64),
        parts: vec![Part::Bytes(bytes)],
        ..self::status(status)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use ol_platform_api::update_http::{InstallerName, ReleaseTag};

    use super::*;

    const WAIT: Duration = Duration::from_secs(15);

    fn installer(limit: u64) -> UpdateRequest {
        UpdateRequest::installer(
            &ReleaseTag::new("v1.2.0").unwrap(),
            &InstallerName::new("One-Launch-Setup-1.2.0.exe").unwrap(),
            limit,
            WAIT,
        )
    }

    fn bytes(text: &str) -> Part {
        Part::Bytes(text.as_bytes().to_vec())
    }

    struct Fetched {
        result: Result<Downloaded, DownloadError>,
        into: Vec<u8>,
        steps: Vec<u64>,
    }

    fn fetch(http: &ScriptedUpdates, request: &UpdateRequest, cancel: &CancelToken) -> Fetched {
        let mut into = Vec::new();
        let mut steps = Vec::new();
        let result = http.download(request, &mut into, &mut |so_far| steps.push(so_far), cancel);
        Fetched {
            result,
            into,
            steps,
        }
    }

    #[test]
    fn every_request_is_written_down_with_its_headers_and_its_call() {
        let http = ScriptedUpdates::new(|request| {
            Ok(match request.ask() {
                UpdateAsk::LatestRelease => json(200, "{\"tag_name\":\"v1.2.0\"}"),
                UpdateAsk::Manifest => {
                    redirect(302, "https://release-assets.githubusercontent.com/m")
                }
                UpdateAsk::Installer => in_parts(Some(6), vec![bytes("abc"), bytes("def")]),
            })
        });
        let latest = UpdateRequest::latest_release(1024, WAIT);
        let answered = http.send(&latest).unwrap();
        assert_eq!(
            answered.body.as_deref(),
            Ok(b"{\"tag_name\":\"v1.2.0\"}".as_slice())
        );
        assert_eq!(answered.content_type.as_deref(), Some("application/json"));

        let manifest = UpdateRequest::manifest(&ReleaseTag::new("v1.2.0").unwrap(), 1024, WAIT);
        let moved = http.send(&manifest).unwrap();
        assert_eq!(
            (moved.status, moved.location.as_deref(), moved.body),
            (
                302,
                Some("https://release-assets.githubusercontent.com/m"),
                Ok(Vec::new())
            )
        );

        let fetched = fetch(&http, &installer(6), &CancelToken::new());
        let downloaded = fetched.result.unwrap();
        assert_eq!(
            downloaded,
            Downloaded {
                status: 200,
                location: None,
                declared: Some(6),
                body: BodyEnd::Complete(6),
            }
        );
        assert_eq!(
            (fetched.into, fetched.steps),
            (b"abcdef".to_vec(), vec![3, 6])
        );

        let asked = http.asked();
        assert_eq!(
            asked[0],
            UpdateAsked {
                url: "https://github.com/rjf2016/one-launch/releases/latest".to_owned(),
                ask: UpdateAsk::LatestRelease,
                headers: vec![
                    ("accept", "application/json"),
                    ("cache-control", "no-cache"),
                    ("user-agent", "electron-builder"),
                ],
                call: UpdateCall::Send,
            }
        );
        assert_eq!(
            asked[2],
            UpdateAsked {
                url: "https://github.com/rjf2016/one-launch/releases/download/v1.2.0/One-Launch-Setup-1.2.0.exe".to_owned(),
                ask: UpdateAsk::Installer,
                headers: vec![
                    ("accept", "*/*"),
                    ("cache-control", "no-cache"),
                    ("user-agent", "electron-builder"),
                ],
                call: UpdateCall::Download,
            }
        );
        assert_eq!(http.urls().len(), 3);
    }

    #[test]
    fn a_body_past_the_limit_is_refused_when_declared_and_when_read() {
        let http = ScriptedUpdates::new(|request| {
            Ok(match request.limit {
                8 => in_parts(Some(9), vec![bytes("123456789")]),
                _ => in_parts(None, vec![bytes("12345"), bytes("6789"), bytes("0")]),
            })
        });
        let fetched = fetch(&http, &installer(8), &CancelToken::new());
        assert_eq!(
            fetched.result.unwrap().body,
            BodyEnd::TooLarge(TooLarge::Declared(9))
        );
        assert!(fetched.into.is_empty());
        let fetched = fetch(&http, &installer(7), &CancelToken::new());
        assert_eq!(
            fetched.result.unwrap().body,
            BodyEnd::TooLarge(TooLarge::Read)
        );
        assert_eq!(fetched.into, b"12345");
        let fetched = fetch(&http, &installer(10), &CancelToken::new());
        assert_eq!(fetched.result.unwrap().body, BodyEnd::Complete(10));

        assert_eq!(
            http.send(&installer(8)).unwrap().body,
            Err(TooLarge::Declared(9))
        );
        assert_eq!(http.send(&installer(7)).unwrap().body, Err(TooLarge::Read));
        assert_eq!(
            http.send(&installer(10)).unwrap().body.as_deref(),
            Ok(b"1234567890".as_slice())
        );
    }

    #[test]
    fn a_cancel_from_a_hook_between_parts_stops_the_download_before_the_next_read() {
        let cancel = CancelToken::new();
        let after = Arc::new(AtomicUsize::new(0));
        let http = ScriptedUpdates::new({
            let cancel = cancel.clone();
            let after = Arc::clone(&after);
            move |_| {
                let cancel = cancel.clone();
                let after = Arc::clone(&after);
                Ok(in_parts(
                    Some(9),
                    vec![
                        bytes("abc"),
                        bytes("def"),
                        Part::run(move || cancel.cancel()),
                        Part::run(|| {}),
                        bytes("ghi"),
                        Part::run(move || {
                            after.fetch_add(1, Ordering::SeqCst);
                        }),
                    ],
                ))
            }
        });
        let fetched = fetch(&http, &installer(9), &cancel);
        assert_eq!(fetched.result.unwrap().body, BodyEnd::Cancelled(6));
        assert_eq!(
            (fetched.into, fetched.steps),
            (b"abcdef".to_vec(), vec![3, 6])
        );
        assert_eq!(after.load(Ordering::SeqCst), 0);

        let asked = http.asked().len();
        assert_eq!(
            fetch(&http, &installer(9), &cancel).result,
            Err(DownloadError::Cancelled)
        );
        assert_eq!(http.asked().len(), asked);

        let cancel = CancelToken::new();
        let http = ScriptedUpdates::new({
            let cancel = cancel.clone();
            move |_| {
                let cancel = cancel.clone();
                Ok(in_parts(
                    None,
                    vec![Part::run(move || cancel.cancel()), bytes("abc")],
                ))
            }
        });
        let fetched = fetch(&http, &installer(9), &cancel);
        assert_eq!(fetched.result.unwrap().body, BodyEnd::Cancelled(0));
        assert!(fetched.into.is_empty());
    }

    #[test]
    fn a_dropped_connection_a_short_body_and_a_stall_are_failures() {
        for (parts, failure) in [
            (vec![bytes("abc"), Part::Drop, bytes("def")], DROPPED),
            (vec![bytes("abc")], DROPPED),
            (vec![bytes("abc"), Part::Stall], HttpError::Timeout),
        ] {
            let parts = Arc::new(Mutex::new(Some(parts)));
            let http = ScriptedUpdates::new(move |_| {
                Ok(in_parts(
                    Some(6),
                    parts.lock().unwrap().take().unwrap_or_default(),
                ))
            });
            let fetched = fetch(&http, &installer(9), &CancelToken::new());
            assert_eq!(fetched.result, Err(DownloadError::Network(failure)));
            assert_eq!(fetched.into, b"abc");
        }
        let http = ScriptedUpdates::new(|_| Ok(in_parts(None, vec![bytes("abc"), Part::Drop])));
        assert_eq!(http.send(&installer(9)), Err(DROPPED));
        let http = ScriptedUpdates::new(|_| Err(HttpError::Failed("the host was not found")));
        assert_eq!(
            fetch(&http, &installer(9), &CancelToken::new()).result,
            Err(DownloadError::Network(HttpError::Failed(
                "the host was not found"
            )))
        );
        let http = ScriptedUpdates::new(|_| Ok(in_parts(Some(3), vec![bytes("abcdef")])));
        assert_eq!(
            fetch(&http, &installer(9), &CancelToken::new()).into,
            b"abc"
        );
    }

    #[test]
    fn only_a_2xx_body_is_read_and_a_spent_request_is_not_asked() {
        let ran = Arc::new(AtomicUsize::new(0));
        let http = ScriptedUpdates::new({
            let ran = Arc::clone(&ran);
            move |request| {
                let ran = Arc::clone(&ran);
                let mut answer = in_parts(
                    Some(3),
                    vec![
                        Part::run(move || {
                            ran.fetch_add(1, Ordering::SeqCst);
                        }),
                        bytes("abc"),
                    ],
                );
                answer.status = if request.limit == 1 { 404 } else { 503 };
                Ok(answer)
            }
        });
        for limit in [1, 2] {
            let fetched = fetch(&http, &installer(limit), &CancelToken::new());
            assert_eq!(fetched.result.unwrap().body, BodyEnd::NotRead);
            assert!(fetched.into.is_empty());
            assert_eq!(http.send(&installer(limit)).unwrap().body, Ok(Vec::new()));
        }
        assert_eq!(ran.load(Ordering::SeqCst), 0);

        let mut spent = installer(9);
        spent.timeout = Duration::ZERO;
        let asked = http.asked().len();
        assert_eq!(
            fetch(&http, &spent, &CancelToken::new()).result,
            Err(DownloadError::Network(HttpError::Timeout))
        );
        assert_eq!(http.send(&spent), Err(HttpError::Timeout));
        assert_eq!(http.asked().len(), asked);
    }
}
