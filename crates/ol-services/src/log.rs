pub mod file;
pub mod redact;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ol_formats::js::iso_from_millis;
use ol_model::LogLevel;
use ol_model::redact::with_registered_names;
use serde_json::Value;

use self::file::FileSink;
pub use self::file::{KEEP, LOG_NAME, MAX_BYTES};
use self::redact::format_line;

// Past this many lines waiting for the disk, a line is dropped and counted rather than hold up its caller.
const QUEUE: usize = 4096;
const ROOT_SCOPE: &str = "main";

// Logged as warnings, so an error line always means something unexpected.
pub const REFUSAL_CODES: [&str; 14] = [
    "BUSY",
    "CONFIRM_REQUIRED",
    "DRY_RUN",
    "DUPLICATE",
    "GAME_NOT_PLAYABLE",
    "GAME_RUNNING",
    "LAUNCHER_RUNNING",
    "NOT_FOUND",
    "NOT_IMPLEMENTED",
    "NOT_INSTALLED",
    "NOT_READY",
    "PROCESS_ACCESS_DENIED",
    "SWITCH_BLOCKED",
    "UNAVAILABLE",
];

pub fn is_refusal(code: &str) -> bool {
    REFUSAL_CODES.contains(&code)
}

const fn rank(level: LogLevel) -> u8 {
    match level {
        LogLevel::Debug => 10,
        LogLevel::Info => 20,
        LogLevel::Warn => 30,
        LogLevel::Error => 40,
    }
}

const fn of_rank(rank: u8) -> LogLevel {
    match rank {
        10 => LogLevel::Debug,
        30 => LogLevel::Warn,
        40 => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogOptions {
    pub level: LogLevel,
    pub mirror: bool,
    pub max_bytes: u64,
    pub keep: u32,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            mirror: false,
            max_bytes: MAX_BYTES,
            keep: KEEP,
        }
    }
}

enum Message {
    Line(String),
    Flush(mpsc::Sender<()>),
}

struct Shared {
    threshold: AtomicU8,
    mirror: bool,
    lines: RwLock<Option<SyncSender<Message>>>,
    dropped: AtomicU64,
}

impl Shared {
    fn sender(&self) -> Option<SyncSender<Message>> {
        self.lines
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub struct Fields(Option<Value>);

impl From<()> for Fields {
    fn from((): ()) -> Self {
        Self(None)
    }
}

impl From<Value> for Fields {
    fn from(value: Value) -> Self {
        Self(Some(value))
    }
}

#[derive(Clone)]
pub struct Log {
    shared: Option<Arc<Shared>>,
    scope: Arc<str>,
}

impl Default for Log {
    fn default() -> Self {
        Self::off()
    }
}

impl Log {
    pub fn off() -> Self {
        Self {
            shared: None,
            scope: Arc::from(ROOT_SCOPE),
        }
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn child(&self, sub: &str) -> Self {
        Self {
            shared: self.shared.clone(),
            scope: Arc::from(format!("{}:{sub}", self.scope)),
        }
    }

    pub fn is_off(&self) -> bool {
        self.shared.is_none()
    }

    pub fn enabled(&self, level: LogLevel) -> bool {
        self.shared
            .as_ref()
            .is_some_and(|shared| rank(level) >= shared.threshold.load(Ordering::Relaxed))
    }

    pub fn set_level(&self, level: LogLevel) {
        if let Some(shared) = &self.shared {
            shared.threshold.store(rank(level), Ordering::Relaxed);
        }
    }

    pub fn level(&self) -> Option<LogLevel> {
        self.shared
            .as_ref()
            .map(|shared| of_rank(shared.threshold.load(Ordering::Relaxed)))
    }

    pub fn debug(&self, message: &str, fields: impl Into<Fields>) {
        self.write(LogLevel::Debug, message, fields.into());
    }

    pub fn info(&self, message: &str, fields: impl Into<Fields>) {
        self.write(LogLevel::Info, message, fields.into());
    }

    pub fn warn(&self, message: &str, fields: impl Into<Fields>) {
        self.write(LogLevel::Warn, message, fields.into());
    }

    pub fn error(&self, message: &str, fields: impl Into<Fields>) {
        self.write(LogLevel::Error, message, fields.into());
    }

    pub fn at(&self, level: LogLevel, message: &str, fields: impl Into<Fields>) {
        self.write(level, message, fields.into());
    }

    fn write(&self, level: LogLevel, message: &str, fields: Fields) {
        let Some(shared) = &self.shared else {
            return;
        };
        if rank(level) < shared.threshold.load(Ordering::Relaxed) {
            return;
        }
        let line = with_registered_names(|names| {
            format_line(
                names,
                &timestamp(),
                level,
                &self.scope,
                message,
                fields.0.as_ref(),
            )
        });
        if shared.mirror {
            mirror(level, &line);
        }
        let Some(lines) = shared.sender() else {
            return;
        };
        if let Err(TrySendError::Full(_)) = lines.try_send(Message::Line(line)) {
            shared.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn mirror(level: LogLevel, line: &str) {
    let _ = if matches!(level, LogLevel::Warn | LogLevel::Error) {
        std::io::stderr().lock().write_all(line.as_bytes())
    } else {
        std::io::stdout().lock().write_all(line.as_bytes())
    };
}

pub fn timestamp() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_millis() as f64);
    iso_from_millis(millis).unwrap_or_default()
}

pub struct Logger {
    log: Log,
    dir: PathBuf,
    writer: Mutex<Option<(JoinHandle<()>, Receiver<()>)>>,
}

impl Logger {
    pub fn for_data_folder(data_dir: &Path, options: LogOptions) -> Self {
        Self::open(&data_dir.join("logs"), options)
    }

    pub fn open(dir: &Path, options: LogOptions) -> Self {
        let (lines, queue) = mpsc::sync_channel(QUEUE);
        let shared = Arc::new(Shared {
            threshold: AtomicU8::new(rank(options.level)),
            mirror: options.mirror,
            lines: RwLock::new(Some(lines)),
            dropped: AtomicU64::new(0),
        });
        let (done, finished) = mpsc::channel();
        let sink = FileSink::new(dir, options.max_bytes, options.keep);
        let writing = Arc::clone(&shared);
        let writer = thread::Builder::new()
            .name("log".to_owned())
            .spawn(move || {
                write_until_closed(&queue, sink, &writing);
                let _ = done.send(());
            })
            .ok();
        if writer.is_none() {
            *shared.lines.write().unwrap_or_else(PoisonError::into_inner) = None;
        }
        Self {
            log: Log {
                shared: Some(shared),
                scope: Arc::from(ROOT_SCOPE),
            },
            dir: dir.to_path_buf(),
            writer: Mutex::new(writer.map(|writer| (writer, finished))),
        }
    }

    pub fn log(&self) -> Log {
        self.log.clone()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn current_path(&self) -> PathBuf {
        self.dir.join(format!("{LOG_NAME}.log"))
    }

    pub fn flush(&self, timeout: Duration) -> bool {
        let Some(shared) = &self.log.shared else {
            return false;
        };
        let Some(lines) = shared.sender() else {
            return false;
        };
        let deadline = Instant::now() + timeout;
        let (ack, acked) = mpsc::channel();
        let mut message = Message::Flush(ack);
        loop {
            match lines.try_send(message) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) if Instant::now() < deadline => {
                    message = back;
                    thread::sleep(Duration::from_millis(2));
                }
                Err(_) => return false,
            }
        }
        acked
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    }

    pub fn close(&self, timeout: Duration) {
        if let Some(shared) = &self.log.shared {
            *shared.lines.write().unwrap_or_else(PoisonError::into_inner) = None;
        }
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some((writer, finished)) = writer {
            match finished.recv_timeout(timeout) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                    let _ = writer.join();
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        self.close(Duration::from_secs(2));
    }
}

fn write_until_closed(queue: &Receiver<Message>, mut sink: FileSink, shared: &Shared) {
    while let Ok(first) = queue.recv() {
        let mut lines = Vec::new();
        let mut acks = Vec::new();
        let mut next = Some(first);
        while let Some(message) = next {
            match message {
                Message::Line(line) => lines.push(line),
                Message::Flush(ack) => acks.push(ack),
            }
            next = (lines.len() < QUEUE)
                .then(|| queue.try_recv().ok())
                .flatten();
        }
        let dropped = shared.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            lines.push(with_registered_names(|names| {
                format_line(
                    names,
                    &timestamp(),
                    LogLevel::Warn,
                    "main:log",
                    &format!("{dropped} log lines were dropped: the disk could not keep up"),
                    None,
                )
            }));
        }
        let _ = sink.write_lines(&lines);
        for ack in acks {
            let _ = ack.send(());
        }
    }
}
