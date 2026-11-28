use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ol_platform_api::update_http::{
    BodyEnd, DownloadError, InstallerName, ReleaseTag, UpdateHttp, UpdateRequest,
};
use ol_platform_api::{CancelToken, Clock};
use sha2::{Digest, Sha512};
use time::OffsetDateTime;

use super::check::WAIT;
use super::failure::UpdateFailure;
use super::manifest::{ManifestFile, sha512_of_file};
use super::redirects::{Hop, follow};

const REPORT_EVERY: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    Ready { path: PathBuf, sha512: String },
    Cancelled,
}

enum Stop {
    Failed(UpdateFailure),
    Cancelled,
}

impl From<UpdateFailure> for Stop {
    fn from(failure: UpdateFailure) -> Self {
        Self::Failed(failure)
    }
}

pub fn download(
    http: &dyn UpdateHttp,
    clock: &dyn Clock,
    pending: &Path,
    tag: &ReleaseTag,
    file: &ManifestFile,
    progress: &mut dyn FnMut(u8),
    cancel: &CancelToken,
) -> Result<Fetched, UpdateFailure> {
    if cancel.is_cancelled() {
        return Ok(Fetched::Cancelled);
    }
    let Some(name) = InstallerName::new(&file.url) else {
        return Err(UpdateFailure::Other(
            "the release file is not a plain .exe name".to_owned(),
        ));
    };
    fs::create_dir_all(pending).map_err(|_| UpdateFailure::Disk)?;
    let installer = pending.join(name.as_str());
    if is_plain_file(&installer)
        && sha512_of_file(&installer).is_ok_and(|measured| measured == file.sha512)
    {
        return Ok(Fetched::Ready {
            path: installer,
            sha512: file.sha512.clone(),
        });
    }
    clear(pending);

    let temp = pending.join(format!("temp-{}", name.as_str()));
    let mut into = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map(Hashing::new)
        .map_err(|_| UpdateFailure::Disk)?;
    let mut pace = Pace::new(clock, file.size, progress);
    let request = UpdateRequest::installer(tag, &name, file.size, WAIT);
    let ended = fetch(
        http,
        request,
        &mut into,
        &mut |bytes| pace.seen(bytes),
        cancel,
    );
    let measured = into.finish();

    let outcome = match ended {
        Ok(BodyEnd::Complete(length)) if length == file.size && measured == file.sha512 => {
            fs::rename(&temp, &installer)
                .map(|()| Fetched::Ready {
                    path: installer,
                    sha512: measured,
                })
                .map_err(|_| UpdateFailure::Disk)
        }
        Ok(BodyEnd::Complete(_) | BodyEnd::TooLarge(_) | BodyEnd::NotRead) => {
            Err(UpdateFailure::Mismatch)
        }
        Ok(BodyEnd::Cancelled(_)) | Err(Stop::Cancelled) => Ok(Fetched::Cancelled),
        Err(Stop::Failed(failure)) => Err(failure),
    };
    if !matches!(outcome, Ok(Fetched::Ready { .. })) {
        let _ = fs::remove_file(&temp);
    }
    outcome
}

fn fetch(
    http: &dyn UpdateHttp,
    first: UpdateRequest,
    into: &mut Hashing,
    progress: &mut dyn FnMut(u64),
    cancel: &CancelToken,
) -> Result<BodyEnd, Stop> {
    let (status, body) = follow(first, |request| {
        match http.download(request, into, progress, cancel) {
            Ok(answer) => Ok(Hop {
                status: answer.status,
                location: answer.location,
                answer: answer.body,
            }),
            Err(DownloadError::Cancelled) => Err(Stop::Cancelled),
            Err(DownloadError::Network(error)) => Err(Stop::Failed(error.into())),
            Err(DownloadError::Write(_)) => Err(Stop::Failed(UpdateFailure::Disk)),
        }
    })?;
    if (200..=299).contains(&status) {
        Ok(body)
    } else {
        Err(Stop::Failed(UpdateFailure::Status(status)))
    }
}

fn is_plain_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

// A file this or the Electron updater left; a folder or a link is not this app's to remove, and a file that will not go is left.
fn clear(pending: &Path) {
    let Ok(entries) = fs::read_dir(pending) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

struct Hashing {
    file: File,
    hash: Sha512,
}

impl Hashing {
    fn new(file: File) -> Self {
        Self {
            file,
            hash: Sha512::new(),
        }
    }

    fn finish(self) -> String {
        STANDARD.encode(self.hash.finalize())
    }
}

impl Write for Hashing {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.file.write(buffer)?;
        self.hash.update(&buffer[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

// electron-updater's pacing: a percent at most once a second, counted from the start and then from the last one told, and none for the last byte.
struct Pace<'a> {
    clock: &'a dyn Clock,
    next: OffsetDateTime,
    size: u64,
    report: &'a mut dyn FnMut(u8),
}

impl<'a> Pace<'a> {
    fn new(clock: &'a dyn Clock, size: u64, report: &'a mut dyn FnMut(u8)) -> Self {
        Self {
            clock,
            next: clock.now() + REPORT_EVERY,
            size,
            report,
        }
    }

    fn seen(&mut self, bytes: u64) {
        if self.size == 0 || bytes == self.size {
            return;
        }
        let now = self.clock.now();
        if now < self.next {
            return;
        }
        self.next = now + REPORT_EVERY;
        let percent = (u128::from(bytes) * 100 / u128::from(self.size)).min(100);
        (self.report)(u8::try_from(percent).unwrap_or(100));
    }
}
