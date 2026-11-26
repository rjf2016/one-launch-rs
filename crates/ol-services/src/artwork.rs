// Game artwork: which games are looked up and when, what a lookup keeps, and the cache under `<data folder>\artwork\<game id>\`. The lookups themselves, the only requests the app sends to the internet, are in `fetch`. A download is held in memory until its lookup has ended and is written only when it is still wanted; a miss is recorded, so the next start does not ask again.

pub mod fetch;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::{ImageFormat, ImageReader, Limits};
use ol_formats::battlenet::products::known_product;
use ol_formats::epic::steam_ids;
use ol_model::{ArtworkKind, ArtworkSource, ArtworkStatus, Game, LauncherId, SettingKey};
use ol_store::{ArtworkRow, Store, StoreError};
use serde_json::json;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub use fetch::{Fetched, KeySource, Wanted, fetch};

use crate::library::{ARTWORK_FOLDER, artwork_file};
use crate::log::Log;

pub const CONCURRENCY: usize = 2;
// A lookup that found nothing is not tried again by itself before this has passed.
pub const RETRY_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const MAX_MANUAL_BYTES: u64 = 20 * 1024 * 1024;
const LARGEST_SIDE: u32 = 8192;
const MOST_DECODED_BYTES: u64 = 256 * 1024 * 1024;
const ATTEMPT_MOST: usize = 500;
pub const KEY_REFUSED: &str =
    "That does not look like a SteamGridDB API key. Paste the key itself.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ext {
    Jpg,
    Png,
    Webp,
}

impl Ext {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }
}

// A picture that was checked, held in memory.
#[derive(Clone, PartialEq, Eq)]
pub struct Download {
    pub bytes: Vec<u8>,
    pub ext: Ext,
}

// What the bytes start as. Anything but a JPEG, a PNG or a WebP is not kept, whatever its name or its server says.
pub fn sniff_image(bytes: &[u8]) -> Option<Ext> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes[..3] == [0xff, 0xd8, 0xff] {
        return Some(Ext::Jpg);
    }
    if bytes[..8] == [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] {
        return Some(Ext::Png);
    }
    (&bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP").then_some(Ext::Webp)
}

// Whether the bytes are a whole picture of the kind they start as, no larger than the Library draws. A file from the internet is decoded once here, before it is kept.
pub fn decodes(bytes: &[u8], ext: Ext) -> bool {
    let format = match ext {
        Ext::Jpg => ImageFormat::Jpeg,
        Ext::Png => ImageFormat::Png,
        Ext::Webp => ImageFormat::WebP,
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(LARGEST_SIDE);
    limits.max_image_height = Some(LARGEST_SIDE);
    limits.max_alloc = Some(MOST_DECODED_BYTES);
    reader.limits(limits);
    reader.decode().is_ok()
}

// The shape the settings screen and the backend hold a key to: one run of 20 to 64 letters and digits.
pub fn looks_like_a_key(text: &str) -> bool {
    (20..=64).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[derive(Debug, Error)]
pub enum ArtworkError {
    #[error("Game {0} does not exist")]
    GameNotFound(String),
    #[error("That image is larger than {0} MB")]
    TooLarge(u64),
    #[error("That file is not a JPEG, PNG, or WebP image")]
    BadImage,
    #[error("That file could not be read ({0})")]
    Unreadable(io::ErrorKind),
    #[error("The image could not be stored ({0})")]
    NotStored(io::ErrorKind),
    #[error("This run of One Launch does not reach the internet, so it fetches no artwork.")]
    NoNetwork,
    #[error("{KEY_REFUSED}")]
    NotAKey,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl ArtworkError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::GameNotFound(_) => "NOT_FOUND",
            Self::TooLarge(_) => "TOO_LARGE",
            Self::BadImage => "BAD_IMAGE",
            Self::Unreadable(_) | Self::NotStored(_) => "INTERNAL",
            Self::NoNetwork => "UNAVAILABLE",
            Self::NotAKey => "BAD_REQUEST",
            Self::Store(error) => error.code(),
        }
    }
}

pub fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join(ARTWORK_FOLDER)
}

// The Steam id a game is known by without asking anyone: its own, or the one a launcher's table has for it.
fn steam_app_id_for(game: &Game) -> Option<String> {
    match game.launcher {
        LauncherId::Steam => Some(game.launcher_game_id.clone()),
        LauncherId::Battlenet => known_product(&game.launcher_game_id)
            .and_then(|product| product.steam_app_id)
            .map(str::to_owned),
        LauncherId::Epic => steam_ids::steam_app_id(&game.launcher_game_id).map(str::to_owned),
        LauncherId::Riot => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Kept {
    rel_path: String,
    source: ArtworkSource,
}

type Kinds = Vec<(ArtworkKind, Kept)>;

fn of(kinds: &Kinds, kind: ArtworkKind) -> Option<&Kept> {
    kinds
        .iter()
        .find_map(|(held, kept)| (*held == kind).then_some(kept))
}

fn put(kinds: &mut Kinds, kind: ArtworkKind, kept: Kept) {
    match kinds.iter_mut().find(|(held, _)| *held == kind) {
        Some(slot) => slot.1 = kept,
        None => kinds.push((kind, kept)),
    }
}

fn from_row(row: Option<&ArtworkRow>) -> Kinds {
    let Some(row) = row else {
        return Kinds::new();
    };
    let Some(source) = ArtworkSource::parse(&row.source) else {
        return Kinds::new();
    };
    [
        (ArtworkKind::Grid, &row.grid_path),
        (ArtworkKind::Hero, &row.hero_path),
        (ArtworkKind::Logo, &row.logo_path),
    ]
    .into_iter()
    .filter_map(|(kind, path)| {
        let rel_path = path.clone().filter(|path| !path.is_empty())?;
        Some((kind, Kept { rel_path, source }))
    })
    .collect()
}

fn on_disk(dir: &Path, rel_path: &str) -> bool {
    artwork_file(dir, rel_path).is_some_and(|file| file.exists())
}

// One lookup, from the moment it leaves the queue.
pub struct Job {
    pub wanted: Wanted,
    pub force: bool,
    pub replaces_manual: bool,
    picks_at_start: u64,
    have: Kinds,
    // A launcher's icon an earlier version stored in place of the cover. It stays until a cover is found.
    stand_in: Option<Kept>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Ask {
    force: bool,
    replaces_manual: bool,
}

impl Ask {
    const fn with(self, other: Self) -> Self {
        Self {
            force: self.force || other.force,
            replaces_manual: self.replaces_manual || other.replaces_manual,
        }
    }
}

struct Active {
    id: String,
    ask: Ask,
    picks_at_start: u64,
}

// Which games wait for a lookup and which are being looked up, as the Electron app's service keeps them.
pub struct Queue {
    concurrency: usize,
    // In the order asked.
    pending: Vec<(String, Ask)>,
    active: Vec<Active>,
    // Forced requests the lookup running for the same game does not cover, with whether each may replace the user's picture: they run when it ends.
    again: BTreeMap<String, bool>,
    // How often the user chose a picture for a game, so a lookup a choice overtook can tell.
    manual_picks: HashMap<String, u64>,
    completed: usize,
    failed: usize,
    stopped: bool,
}

impl Default for Queue {
    fn default() -> Self {
        Self::with_concurrency(CONCURRENCY)
    }
}

impl Queue {
    pub fn with_concurrency(concurrency: usize) -> Self {
        Self {
            concurrency,
            pending: Vec::new(),
            active: Vec::new(),
            again: BTreeMap::new(),
            manual_picks: HashMap::new(),
            completed: 0,
            failed: 0,
            stopped: false,
        }
    }

    // How many of `ids` the call added to what will run. None of them replaces a picture the user chose.
    pub fn enqueue(&mut self, ids: &[String], force: bool) -> usize {
        self.ask(
            ids,
            Ask {
                force,
                replaces_manual: false,
            },
        )
    }

    fn ask(&mut self, ids: &[String], ask: Ask) -> usize {
        if self.stopped {
            return 0;
        }
        let mut queued = 0;
        for id in ids {
            if let Some(running) = self.active.iter().find(|active| active.id == *id) {
                // A lookup a picture of the user's own overtook writes nothing, so it covers no new download.
                let covered =
                    !ask.force || (running.ask.force && running.picks_at_start == self.picks(id));
                if !covered {
                    match self.again.get_mut(id) {
                        Some(replaces_manual) => *replaces_manual |= ask.replaces_manual,
                        None => {
                            self.again.insert(id.clone(), ask.replaces_manual);
                            queued += 1;
                        }
                    }
                }
                continue;
            }
            match self.pending.iter_mut().find(|(pending, _)| pending == id) {
                Some(waiting) => waiting.1 = waiting.1.with(ask),
                None => {
                    self.pending.push((id.clone(), ask));
                    queued += 1;
                }
            }
        }
        queued
    }

    // The next game to look up, when fewer run than may, with whether it is to download again what is cached.
    pub fn take_next(&mut self) -> Option<(String, bool)> {
        if self.active.len() >= self.concurrency || self.pending.is_empty() {
            return None;
        }
        let (id, ask) = self.pending.remove(0);
        self.active.push(Active {
            id: id.clone(),
            ask,
            picks_at_start: self.picks(&id),
        });
        Some((id, ask.force))
    }

    fn replaces_manual(&self, id: &str) -> bool {
        self.active
            .iter()
            .any(|active| active.id == id && active.ask.replaces_manual)
    }

    pub fn finish(&mut self, id: &str, outcome: Outcome) {
        self.active.retain(|active| active.id != id);
        match outcome {
            Outcome::Done => self.completed += 1,
            Outcome::Failed => self.failed += 1,
        }
        if let Some(replaces_manual) = self.again.remove(id)
            && !self.stopped
        {
            let ask = Ask {
                force: true,
                replaces_manual,
            };
            match self.pending.iter_mut().find(|(pending, _)| pending == id) {
                Some(waiting) => waiting.1 = waiting.1.with(ask),
                None => self.pending.push((id.to_owned(), ask)),
            }
        }
    }

    pub fn is_busy(&self, id: &str) -> bool {
        self.active.iter().any(|active| active.id == id)
            || self.pending.iter().any(|(held, _)| held == id)
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_empty() && self.pending.is_empty()
    }

    fn picks(&self, id: &str) -> u64 {
        self.manual_picks.get(id).copied().unwrap_or_default()
    }

    fn picked(&mut self, id: &str) {
        *self.manual_picks.entry(id.to_owned()).or_default() += 1;
    }

    // For a quit: nothing more is started, and a lookup still on its way ends without writing.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.pending.clear();
    }

    pub const fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub fn status(&self, has_api_key: bool, revision: u64, scanned: bool) -> ArtworkStatus {
        ArtworkStatus {
            queued: self.pending.len(),
            running: self.active.len(),
            completed: self.completed,
            failed: self.failed,
            has_api_key,
            revision,
            scanned,
        }
    }
}

// What became of a game that left the queue.
pub enum Prepared {
    // Nothing to ask anyone: the game is gone, its picture is the user's own, or nothing is missing.
    Nothing,
    Lookup(Box<Job>),
}

// The queue and the cache folder, used from the thread the database lives on.
pub struct Artwork {
    dir: PathBuf,
    pub queue: Queue,
    log: Log,
}

impl Artwork {
    pub fn new(data_dir: &Path, log: &Log) -> Self {
        Self {
            dir: folder(data_dir),
            queue: Queue::default(),
            log: log.child("artwork"),
        }
    }

    // Removes the cached pictures and the rows of games that are no longer installed, so the cache holds what the Library shows. A game installed again is looked up again.
    pub fn prune(&self, store: &Store) -> Result<usize, StoreError> {
        let keep: BTreeSet<String> = store
            .installed_games()?
            .into_iter()
            .map(|game| game.id)
            .collect();
        let stays = |id: &str| keep.contains(id) || self.queue.is_busy(id);
        let mut gone = BTreeSet::new();
        // A link is no folder of this cache: it is left alone, and so is whatever it points at.
        let folders = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok());
        for name in folders {
            if stays(&name) {
                continue;
            }
            let _ = fs::remove_dir_all(self.dir.join(&name));
            gone.insert(name);
        }
        // A row can outlive its folder.
        for id in store.artwork_game_ids()? {
            if !stays(&id) {
                gone.insert(id);
            }
        }
        for id in &gone {
            store.delete_artwork(id)?;
            store.clear_artwork_attempt(id)?;
        }
        if !gone.is_empty() {
            self.log
                .info("artwork pruned", json!({ "removed": gone.len() }));
        }
        Ok(gone.len())
    }

    // Every installed game without a cover on disk. A launcher's icon standing in counts as none. A game whose last lookup found nothing is passed over until the retry time has gone by, unless the user asked.
    fn missing(
        &self,
        store: &Store,
        ignore_attempts: bool,
        now: OffsetDateTime,
    ) -> Result<Vec<String>, StoreError> {
        let mut ids = Vec::new();
        for game in store.installed_games()? {
            let covered = game.artwork.as_ref().is_some_and(|art| {
                art.source != ArtworkSource::Launcher
                    && art
                        .grid
                        .as_deref()
                        .is_some_and(|grid| on_disk(&self.dir, grid))
            });
            if covered {
                continue;
            }
            if !ignore_attempts
                && let Some(attempted) = store.artwork_attempt(&game.id)?
                && OffsetDateTime::parse(&attempted, &Rfc3339)
                    .is_ok_and(|attempted| now - attempted < RETRY_AFTER)
            {
                continue;
            }
            ids.push(game.id);
        }
        Ok(ids)
    }

    // After a scan, and when the setting is turned on: what is missing is looked up, unless looking up by itself is off.
    pub fn enqueue_after_scan(
        &mut self,
        store: &Store,
        now: OffsetDateTime,
    ) -> Result<usize, StoreError> {
        if !store.settings()?.toggle(SettingKey::ArtworkAutoFetch) {
            return Ok(0);
        }
        let ids = self.missing(store, false, now)?;
        Ok(self.queue.enqueue(&ids, false))
    }

    // One game, or every installed game without a `game_id`. `force` downloads again what is cached: for one game it replaces a picture the user chose, for all it leaves those alone.
    pub fn refresh(
        &mut self,
        store: &Store,
        game_id: Option<&str>,
        force: bool,
        now: OffsetDateTime,
    ) -> Result<usize, ArtworkError> {
        let ids = match game_id {
            Some(game_id) => {
                store
                    .game(game_id)?
                    .ok_or_else(|| ArtworkError::GameNotFound(game_id.to_owned()))?;
                vec![game_id.to_owned()]
            }
            None if !force => self.missing(store, true, now)?,
            None => store
                .installed_games()?
                .into_iter()
                .filter(|game| {
                    game.artwork
                        .as_ref()
                        .is_none_or(|art| art.source != ArtworkSource::Manual)
                })
                .map(|game| game.id)
                .collect(),
        };
        let ask = Ask {
            force,
            replaces_manual: force && game_id.is_some(),
        };
        Ok(self.queue.ask(&ids, ask))
    }

    // What a game that left the queue needs looked up, from the database and the disk as they are now.
    pub fn prepare(
        &self,
        store: &Store,
        game_id: &str,
        force: bool,
    ) -> Result<Prepared, StoreError> {
        let Some(game) = store.game(game_id)?.filter(|game| game.installed) else {
            return Ok(Prepared::Nothing);
        };
        let row = store.artwork_row(game_id)?;
        let replaces_manual = self.queue.replaces_manual(game_id);
        if !replaces_manual
            && row
                .as_ref()
                .is_some_and(|row| row.source == ArtworkSource::Manual.as_str())
        {
            return Ok(Prepared::Nothing);
        }
        let mut have = if force {
            Kinds::new()
        } else {
            from_row(row.as_ref())
        };
        // A cached file that was deleted is as good as never fetched.
        have.retain(|(_, kept)| on_disk(&self.dir, &kept.rel_path));
        let stand_in = of(&have, ArtworkKind::Grid)
            .filter(|grid| grid.source == ArtworkSource::Launcher)
            .cloned();
        if stand_in.is_some() {
            have.retain(|(kind, _)| *kind != ArtworkKind::Grid);
        }
        let missing: Vec<ArtworkKind> = ArtworkKind::ALL
            .into_iter()
            .filter(|kind| of(&have, *kind).is_none())
            .collect();
        if missing.is_empty() {
            return Ok(Prepared::Nothing);
        }
        Ok(Prepared::Lookup(Box::new(Job {
            wanted: Wanted {
                game_id: game.id.clone(),
                name: game.name.clone(),
                launcher: game.launcher,
                app_id: steam_app_id_for(&game),
                missing,
            },
            force,
            replaces_manual,
            picks_at_start: self.queue.picks(game_id),
            have,
            stand_in,
        })))
    }

    // The end of a lookup: what it found is written and recorded, or its miss is. Answers whether the Library changed. Nothing is written for a lookup a quit or a picture of the user's own overtook.
    pub fn finish(
        &mut self,
        store: &Store,
        job: &Job,
        fetched: Fetched,
        now: OffsetDateTime,
    ) -> (Outcome, bool) {
        let game_id = job.wanted.game_id.as_str();
        if self.queue.is_stopped() {
            return (Outcome::Done, false);
        }
        if let Some(reason) = &fetched.crashed {
            self.log.error(
                "artwork job crashed",
                json!({ "gameId": game_id, "error": reason }),
            );
            return (Outcome::Failed, false);
        }
        if self.queue.picks(game_id) != job.picks_at_start {
            self.log.info(
                "artwork fetch dropped: a manual image was set meanwhile",
                json!({ "gameId": game_id }),
            );
            return (Outcome::Done, false);
        }
        match self.keep(store, job, fetched, now) {
            Ok(outcome) => (outcome, true),
            Err(error) => {
                self.log.error(
                    "artwork job crashed",
                    json!({ "gameId": game_id, "error": error.to_string() }),
                );
                (Outcome::Failed, false)
            }
        }
    }

    fn keep(
        &self,
        store: &Store,
        job: &Job,
        fetched: Fetched,
        now: OffsetDateTime,
    ) -> Result<Outcome, ArtworkError> {
        let (game_id, name) = (job.wanted.game_id.as_str(), job.wanted.name.as_str());
        let stamp = ol_store::iso_timestamp(now);
        let error = if fetched.problems.is_empty() {
            "No artwork found".to_owned()
        } else {
            fetched.problems.join("; ")
        };
        if fetched.found.is_empty() {
            // A stand-in on disk stays as it is; only the reason and the time of this miss are recorded.
            store.record_artwork_attempt(
                game_id,
                &stamp,
                fetch::cut_utf16(&error, ATTEMPT_MOST),
            )?;
            self.log.info(
                "artwork not found",
                json!({ "gameId": game_id, "name": name, "error": error, "standIn": job.stand_in.is_some() }),
            );
            return Ok(Outcome::Failed);
        }
        let mut stored = Kinds::new();
        let mut kinds = Vec::new();
        for (kind, image, source) in &fetched.found {
            let rel_path = self.store_file(game_id, *kind, image.ext, &image.bytes)?;
            put(
                &mut stored,
                *kind,
                Kept {
                    rel_path,
                    source: *source,
                },
            );
            kinds.push(kind.as_str());
        }
        // Still no cover: the icon keeps standing in, and the miss stays on record.
        let keeps_stand_in = of(&stored, ArtworkKind::Grid).is_none() && job.stand_in.is_some();
        if let Some(stand_in) = job.stand_in.clone().filter(|_| keeps_stand_in) {
            put(&mut stored, ArtworkKind::Grid, stand_in);
        }
        let source = of(&stored, ArtworkKind::Grid)
            .or_else(|| of(&job.have, ArtworkKind::Grid))
            .map_or(fetched.found[0].2, |grid| grid.source);
        let mut found = if job.force {
            Kinds::new()
        } else {
            job.have.clone()
        };
        for (kind, kept) in stored {
            put(&mut found, kind, kept);
        }
        let attempt_error = keeps_stand_in.then_some(error.as_str());
        self.commit(
            store,
            game_id,
            &found,
            source,
            job.force,
            attempt_error,
            &stamp,
        )?;
        self.log.info(
            "artwork cached",
            json!({ "gameId": game_id, "name": name, "kinds": kinds, "source": source.as_str() }),
        );
        Ok(Outcome::Done)
    }

    #[expect(clippy::too_many_arguments, reason = "one row, written in one place")]
    fn commit(
        &self,
        store: &Store,
        game_id: &str,
        found: &Kinds,
        source: ArtworkSource,
        replace_all: bool,
        attempt_error: Option<&str>,
        stamp: &str,
    ) -> Result<(), StoreError> {
        let mut merged = if replace_all {
            Kinds::new()
        } else {
            from_row(store.artwork_row(game_id)?.as_ref())
        };
        for (kind, kept) in found {
            put(&mut merged, *kind, kept.clone());
        }
        let path = |kind| of(&merged, kind).map(|kept| kept.rel_path.clone());
        store.put_artwork(
            game_id,
            &ArtworkRow {
                source: source.as_str().to_owned(),
                grid_path: path(ArtworkKind::Grid),
                hero_path: path(ArtworkKind::Hero),
                logo_path: path(ArtworkKind::Logo),
                fetched_at: Some(stamp.to_owned()),
            },
        )?;
        match attempt_error {
            None => store.clear_artwork_attempt(game_id),
            Some(error) => {
                store.record_artwork_attempt(game_id, stamp, fetch::cut_utf16(error, ATTEMPT_MOST))
            }
        }
    }

    // The user's own picture for a game: checked like a download, copied into the cache and recorded as theirs. Asks nobody.
    pub fn set_manual(
        &mut self,
        store: &Store,
        game_id: &str,
        kind: ArtworkKind,
        picked: &Path,
        now: OffsetDateTime,
    ) -> Result<Game, ArtworkError> {
        let missing = || ArtworkError::GameNotFound(game_id.to_owned());
        store.game(game_id)?.ok_or_else(missing)?;
        let unreadable = |error: io::Error| ArtworkError::Unreadable(error.kind());
        let megabytes = MAX_MANUAL_BYTES / 1024 / 1024;
        let file = fs::File::open(picked).map_err(unreadable)?;
        if file.metadata().map_err(unreadable)?.len() > MAX_MANUAL_BYTES {
            return Err(ArtworkError::TooLarge(megabytes));
        }
        let mut bytes = Vec::new();
        file.take(MAX_MANUAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(unreadable)?;
        if bytes.len() as u64 > MAX_MANUAL_BYTES {
            return Err(ArtworkError::TooLarge(megabytes));
        }
        let ext = sniff_image(&bytes)
            .filter(|ext| decodes(&bytes, *ext))
            .ok_or(ArtworkError::BadImage)?;
        self.queue.picked(game_id);
        let rel_path = self.store_file(game_id, kind, ext, &bytes)?;
        let found = vec![(
            kind,
            Kept {
                rel_path,
                source: ArtworkSource::Manual,
            },
        )];
        let stamp = ol_store::iso_timestamp(now);
        self.commit(
            store,
            game_id,
            &found,
            ArtworkSource::Manual,
            false,
            None,
            &stamp,
        )?;
        self.log.info(
            "manual artwork set",
            json!({ "gameId": game_id, "kind": kind.as_str() }),
        );
        store.game(game_id)?.ok_or_else(missing)
    }

    // Written beside its place and moved over it, and then every other file of the same kind goes, so a kind has one file whatever its type was before. Answers the path the row stores.
    fn store_file(
        &self,
        game_id: &str,
        kind: ArtworkKind,
        ext: Ext,
        bytes: &[u8],
    ) -> Result<String, ArtworkError> {
        let name = format!("{}.{}", kind.as_str(), ext.as_str());
        let rel_path = format!("{game_id}/{name}");
        // Only a game id of the app's own shape names a folder of the cache.
        let target = artwork_file(&self.dir, &rel_path)
            .ok_or(ArtworkError::NotStored(io::ErrorKind::InvalidInput))?;
        let not_stored = |error: io::Error| ArtworkError::NotStored(error.kind());
        let folder = self.dir.join(game_id);
        fs::create_dir_all(&folder).map_err(not_stored)?;
        let temporary = folder.join(format!("{name}.{}.tmp", std::process::id()));
        let written = fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, &target));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written.map_err(not_stored)?;
        let prefix = format!("{}.", kind.as_str());
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let other = entry.file_name().to_string_lossy().into_owned();
            if other != name && other.starts_with(&prefix) && !other.ends_with(".tmp") {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(rel_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn two_lookups_run_at_once_and_the_rest_wait_in_the_order_asked() {
        let mut queue = Queue::default();
        assert_eq!(queue.enqueue(&ids(&["a", "b", "c", "a"]), false), 3);
        assert_eq!(queue.take_next(), Some(("a".to_owned(), false)));
        assert_eq!(queue.take_next(), Some(("b".to_owned(), false)));
        assert_eq!(queue.take_next(), None);
        let status = queue.status(false, 0, false);
        assert_eq!((status.queued, status.running), (1, 2));
        assert!(queue.is_busy("a") && queue.is_busy("c") && !queue.is_busy("d"));
        queue.finish("a", Outcome::Done);
        assert_eq!(queue.take_next(), Some(("c".to_owned(), false)));
        queue.finish("b", Outcome::Failed);
        queue.finish("c", Outcome::Done);
        let status = queue.status(true, 7, true);
        assert_eq!(
            (
                status.queued,
                status.running,
                status.completed,
                status.failed
            ),
            (0, 0, 2, 1)
        );
        assert!(status.has_api_key && status.scanned && status.revision == 7);
        assert!(queue.is_idle());
    }

    #[test]
    fn a_forced_request_for_a_game_being_looked_up_runs_after_it() {
        let mut queue = Queue::default();
        queue.enqueue(&ids(&["a"]), false);
        queue.take_next();
        assert_eq!(queue.enqueue(&ids(&["a"]), false), 0);
        assert_eq!(queue.enqueue(&ids(&["a"]), true), 1);
        assert_eq!(queue.enqueue(&ids(&["a"]), true), 0);
        queue.finish("a", Outcome::Done);
        assert_eq!(queue.take_next(), Some(("a".to_owned(), true)));
        assert_eq!(queue.enqueue(&ids(&["a"]), true), 0);
        queue.finish("a", Outcome::Done);
        assert_eq!(queue.take_next(), None);
    }

    #[test]
    fn a_forced_request_after_a_pick_during_a_forced_lookup_runs_after_it() {
        let mut queue = Queue::default();
        queue.enqueue(&ids(&["a"]), true);
        assert_eq!(queue.take_next(), Some(("a".to_owned(), true)));
        queue.picked("a");
        assert_eq!(queue.enqueue(&ids(&["a"]), true), 1);
        assert_eq!(queue.enqueue(&ids(&["a"]), true), 0);
        queue.finish("a", Outcome::Done);
        assert_eq!(queue.take_next(), Some(("a".to_owned(), true)));
        queue.finish("a", Outcome::Done);
        assert_eq!(queue.take_next(), None);
    }

    #[test]
    fn a_waiting_game_asked_for_again_keeps_its_place_and_takes_the_force() {
        let mut queue = Queue::with_concurrency(1);
        assert_eq!(queue.enqueue(&ids(&["a", "b", "c"]), false), 3);
        assert_eq!(queue.enqueue(&ids(&["b"]), true), 0);
        assert_eq!(queue.take_next(), Some(("a".to_owned(), false)));
        queue.finish("a", Outcome::Done);
        assert_eq!(queue.take_next(), Some(("b".to_owned(), true)));
    }

    #[test]
    fn a_stopped_queue_takes_nothing_and_starts_nothing() {
        let mut queue = Queue::default();
        queue.enqueue(&ids(&["a", "b", "c"]), false);
        queue.take_next();
        queue.enqueue(&ids(&["a"]), true);
        queue.stop();
        assert_eq!(queue.enqueue(&ids(&["d"]), false), 0);
        queue.finish("a", Outcome::Done);
        assert_eq!(queue.take_next(), None);
        assert!(queue.is_idle() && queue.is_stopped());
    }

    #[test]
    fn a_key_is_one_run_of_20_to_64_letters_and_digits() {
        assert!(looks_like_a_key("placeholderkey0000000000"));
        assert!(looks_like_a_key(&"a".repeat(20)));
        assert!(looks_like_a_key(&"Z9".repeat(32)));
        for text in [
            String::new(),
            "a".repeat(19),
            "a".repeat(65),
            format!("{} ", "a".repeat(20)),
            format!("{}\r\nX-Other: 1", "a".repeat(20)),
            "https://www.steamgriddb.com/profile".to_owned(),
            format!("Bearer {}", "a".repeat(20)),
            "\u{e9}".repeat(20),
        ] {
            assert!(!looks_like_a_key(&text), "{text:?}");
        }
    }
}
