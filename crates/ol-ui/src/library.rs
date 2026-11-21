use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, DispatchPhase, Div, Entity, EventEmitter,
    FontWeight, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit,
    Pixels, Point, RenderImage, ScrollEvent, ScrollHandle, ScrollStrategy, SharedString, Size,
    Stateful, Subscription, UniformListScrollHandle, Window, anchored, canvas, deferred, div, hsla,
    img, linear_color_stop, linear_gradient, point, px, uniform_list,
};
use ol_api::library::{LibraryGame, caption, monogram, relative, visible};
use ol_api::{ApiError, Backend, Library};
use ol_model::{ArtworkSource, Game, LauncherId, LibraryScanResult, LibrarySort, RecentLaunch};
use ol_ui_kit::TextField;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::follow::{Follow, follow};
use crate::host::{JobSettled, LaunchHost};
use crate::posters::{self, Lookup, Posters, Shape};
use crate::shell::sidebar_width;
use crate::widgets::{
    Button, Flash, FlashTone, Tone, icon, launcher_dot, menu_shadow, page_title, spinner, tip,
};

// The owner's pick of 2026-11-21, from a prototype of the page: the padding at its sides, the least width of a card and the room between two.
const PAGE_PADDING: f32 = 24.;
const CARD_MIN: f32 = 163.;
const CARD_GAP: f32 = 6.;
const SCROLLBAR: f32 = 12.;
// The page's scrollbar keeps its strip at the right whether it shows or not, so one that comes or goes moves nothing.
const PAGE_END: f32 = PAGE_PADDING + SCROLLBAR;
const CAPTION: f32 = 62.;
const BORDER: f32 = 1.;
const TITLE_BAR: f32 = 36.;
const HEADER: f32 = 152.;
const CONTROL: f32 = 34.;
const MENU_GAP: f32 = 6.;
const CLOCK: Duration = Duration::from_secs(60);
// As many launches as the Recent strip asks for.
const RECENT: u32 = 8;
// The strip with its heading and the gap under it.
const RECENT_STRIP: f32 = 110.;
const RECENT_CARD: f32 = 240.;
const RECENT_GAP: f32 = 8.;
const RECENT_FADE: f32 = 40.;
const THUMB_LEAST: f32 = 32.;
const THUMB_INSET: f32 = 3.;

pub struct LibraryView {
    backend: Backend,
    host: Entity<LaunchHost>,
    search: TextField,
    scroll: UniformListScrollHandle,
    recent_scroll: ScrollHandle,
    grab: Option<Grab>,
    posters: Posters,
    // Counts the lists of games shown: a poster decoded for an earlier list is not one of this list's.
    list: u64,
    stale: Vec<Arc<RenderImage>>,
    load: Load,
    games: Vec<LibraryGame>,
    cards: Vec<Card>,
    recent: Vec<RecentLaunch>,
    scanning: bool,
    // The scan every start runs has ended: until then an empty Library says nothing about what is installed.
    scanned: bool,
    no_launchers: bool,
    flash: Option<Flash>,
    shown: Vec<usize>,
    query: SharedString,
    filter: Option<LauncherId>,
    sort: LibrarySort,
    menu: Option<OpenMenu>,
    over_trigger: Option<Menu>,
    card_menu: Option<CardMenu>,
    sweep: Option<Sweep>,
    sweep_report: Option<SweepReport>,
    frames: Option<Vec<LibraryFrame>>,
    sidebar: Pixels,
    on_screen: bool,
    _subscriptions: [Subscription; 4],
}

// What a frame was drawn for and how its grid was laid out, for a capture to tell a layout that changes its mind from one drawn late.
#[derive(Clone, Copy, Debug)]
pub struct LibraryFrame {
    pub at: Instant,
    pub viewport: Size<Pixels>,
    pub pane: f32,
    pub columns: usize,
    pub card: f32,
    pub gap: f32,
}

impl LibraryFrame {
    pub fn same_grid(&self, other: &Self) -> bool {
        self.columns == other.columns && self.card == other.card && self.gap == other.gap
    }
}

enum Load {
    Pending,
    Failed(SharedString),
    Ready,
}

// Sent once, when the games or the reason there are none are about to be drawn.
pub struct LibraryLoaded;

impl EventEmitter<LibraryLoaded> for LibraryView {}

struct Card {
    name: SharedString,
    monogram: SharedString,
    caption: SharedString,
    launcher: LauncherId,
    needs_update: bool,
    art: Option<(Arc<Path>, Shape)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Filter,
    Sort,
}

struct OpenMenu {
    which: Menu,
    highlight: usize,
}

// The menu of a card, at the place it was asked for; a scroll or a resize of the window closes it. Its one item, Play as, has the keyboard from the start: Refresh artwork and Choose portrait image come with the artwork fetching.
struct CardMenu {
    game: String,
    at: Point<Pixels>,
    viewport: Size<Pixels>,
}

fn artwork_line(game: &Game) -> String {
    match (&game.artwork, &game.artwork_error) {
        (Some(art), _) => format!(
            "Artwork: {}",
            match art.source {
                ArtworkSource::SteamCdn => "Steam",
                ArtworkSource::SteamGridDb => "SteamGridDB",
                ArtworkSource::Manual => "your image",
                ArtworkSource::Launcher => "the game's own icon",
            }
        ),
        (None, Some(error)) => error.clone(),
        (None, None) => "No artwork yet".to_owned(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy)]
struct Grab {
    axis: Axis,
    pointer: f32,
    scrolled: f32,
    per_pixel: f32,
}

#[derive(Clone, Copy)]
struct Track {
    view: f32,
    content: f32,
}

impl Track {
    fn thumb(self) -> Option<f32> {
        (self.view > 0. && self.content > self.view + 0.5).then(|| {
            (self.view * self.view / self.content).clamp(THUMB_LEAST.min(self.view), self.view)
        })
    }

    fn place(self, length: f32, offset: Pixels, max: Pixels) -> f32 {
        let max = f32::from(max);
        if max <= 0. {
            return 0.;
        }
        (self.view - length) * (-f32::from(offset) / max).clamp(0., 1.)
    }

    fn per_pixel(self, length: f32) -> f32 {
        let travel = self.view - length;
        if travel <= 0. {
            0.
        } else {
            (self.content - self.view) / travel
        }
    }
}

struct Sweep {
    frames_left: usize,
    step: Pixels,
    stamps: Vec<Instant>,
}

#[derive(Clone, Copy, Debug)]
pub struct SweepReport {
    pub frames: usize,
    pub median_ms: f32,
    pub p95_ms: f32,
    pub max_ms: f32,
    pub late: usize,
}

impl SweepReport {
    fn of(stamps: &[Instant]) -> Self {
        let mut gaps: Vec<f32> = stamps
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_secs_f32() * 1000.)
            .collect();
        gaps.sort_by(f32::total_cmp);
        let at = |share: f32| {
            let index = ((gaps.len() as f32 * share) as usize).min(gaps.len().saturating_sub(1));
            gaps.get(index).copied().unwrap_or_default()
        };
        let median_ms = at(0.5);
        Self {
            frames: gaps.len(),
            median_ms,
            p95_ms: at(0.95),
            max_ms: at(1.),
            late: gaps.iter().filter(|gap| **gap > median_ms * 1.5).count(),
        }
    }
}

fn row_room(pane: Pixels) -> f32 {
    (f32::from(pane) - PAGE_PADDING - PAGE_END).max(0.)
}

#[derive(Clone, Copy)]
struct Grid {
    columns: usize,
    card: Pixels,
    gap: Pixels,
}

impl Grid {
    // Not quite as the Electron app, on the owner's word: there the least width of a card changed at a window 1536 px wide and the scrollbar took its room only while the page scrolled, so a drag of the window's edge took columns away and gave them back. Here the width of the pane alone decides the columns, and the cards share out the row.
    fn for_pane(pane: Pixels) -> Self {
        let room = row_room(pane).max(CARD_MIN);
        let columns = (((room + CARD_GAP) / (CARD_MIN + CARD_GAP)) as usize).max(1);
        Self {
            columns,
            card: px((room - CARD_GAP * (columns - 1) as f32) / columns as f32),
            gap: px(CARD_GAP),
        }
    }

    fn height_of(self, games: usize) -> f32 {
        games.div_ceil(self.columns) as f32 * f32::from(self.row_height())
    }

    // The poster is two to three inside the card's border.
    fn poster_width(self) -> Pixels {
        self.card - px(2. * BORDER)
    }

    fn row_height(self) -> Pixels {
        self.poster_width() * 1.5 + px(CAPTION + 2. * BORDER) + self.gap
    }
}

// The games with the stored sort, and the last launches.
type Loaded = Result<(Library, Vec<RecentLaunch>), ApiError>;

async fn read(backend: &Backend) -> Loaded {
    let library = backend.library().await?;
    // The strip is an extra: without it the games still show.
    let recent = backend.recent_launches(RECENT).await.unwrap_or_default();
    Ok((library, recent))
}

// Whether a Play may skip the dialog: nothing to say about an update first, and no job to show instead.
fn plays_at_once(game: &Game, host: &LaunchHost) -> bool {
    let update_first = game.needs_update && game.launcher != LauncherId::Steam;
    !update_first && !host.job_running()
}

impl LibraryView {
    pub fn new(
        backend: Backend,
        host: Entity<LaunchHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            TextField::new("Search games (Ctrl+F)", false, window, cx).clearing_on_escape(cx);
        search.set_disabled(true, cx);
        let this = cx.weak_entity();
        let search_changed = search.on_change(cx, {
            let this = this.clone();
            move |text, cx| {
                this.update(cx, |this, cx| this.set_query(text, cx)).ok();
            }
        });
        // Before the bindings of whatever has the keyboard: a text field has its own for Ctrl+F.
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let taken = this
                .update(cx, |this, cx| {
                    this.on_keystroke(&event.keystroke, window, cx)
                })
                .unwrap_or(false);
            if taken {
                cx.stop_propagation();
            }
        });

        let host_changed = cx.observe(&host, |_, _, cx| cx.notify());
        let job_settled = cx.subscribe(&host, |this, _, _: &JobSettled, cx| this.reload(cx));
        cx.spawn({
            let backend = backend.clone();
            async move |this, cx| {
                let library = read(&backend).await;
                this.update(cx, |this, cx| {
                    this.loaded(library, cx);
                    cx.emit(LibraryLoaded);
                    this.rescan(false, cx);
                })
                .ok();
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK).await;
                if this.update(cx, |this, cx| this.reword(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            backend,
            host,
            search,
            scroll: UniformListScrollHandle::new(),
            recent_scroll: ScrollHandle::new(),
            grab: None,
            posters: Posters::default(),
            list: 0,
            stale: Vec::new(),
            load: Load::Pending,
            games: Vec::new(),
            cards: Vec::new(),
            recent: Vec::new(),
            scanning: false,
            scanned: false,
            no_launchers: false,
            flash: None,
            shown: Vec::new(),
            query: SharedString::default(),
            filter: None,
            sort: LibrarySort::default(),
            menu: None,
            over_trigger: None,
            card_menu: None,
            sweep: None,
            sweep_report: None,
            frames: None,
            sidebar: sidebar_width(false),
            on_screen: true,
            _subscriptions: [search_changed, keys, host_changed, job_settled],
        }
    }

    fn loaded(&mut self, library: Loaded, cx: &mut Context<Self>) {
        let (library, recent) = match library {
            Ok(read) => read,
            Err(error) if matches!(self.load, Load::Ready) => {
                return self.say(
                    format!("Could not load the library: {}", error.message),
                    true,
                    cx,
                );
            }
            Err(error) => {
                self.load = Load::Failed(error.message.into());
                return cx.notify();
            }
        };
        let now = SystemTime::now();
        let first = !matches!(self.load, Load::Ready);
        let same_list = !first
            && self.games.len() == library.games.len()
            && self
                .games
                .iter()
                .zip(&library.games)
                .all(|(old, new)| old.game.id == new.game.id && old.grid == new.grid);
        self.cards = library
            .games
            .iter()
            .map(|entry| Card::of(entry, now))
            .collect();
        self.games = library.games;
        if let Some(menu) = &self.card_menu
            && !self.games.iter().any(|entry| entry.game.id == menu.game)
        {
            self.card_menu = None;
        }
        self.recent = recent;
        self.load = Load::Ready;
        self.search.set_disabled(self.games.is_empty(), cx);
        if same_list {
            // The same games in the same places: the posters stay, and so does the place scrolled to.
            self.arrange();
            return cx.notify();
        }
        // Later reads keep the order chosen in this run, stored or not.
        if first {
            self.sort = library.sort;
        }
        self.list += 1;
        self.stale.extend(self.posters.clear());
        self.show(cx);
    }

    // Reads the Library again after something changed it: a scan, or a launch.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let library = read(&backend).await;
            this.update(cx, |this, cx| this.loaded(library, cx)).ok();
        })
        .detach();
    }

    // Asks the backend to read every launcher again. The scan a start runs says nothing; one the user asked for says what it found.
    fn rescan(&mut self, announce: bool, cx: &mut Context<Self>) {
        if announce {
            if self.scanning {
                return;
            }
            self.scanning = true;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let scan = backend.rescan().await;
            let library = read(&backend).await;
            this.update(cx, |this, cx| {
                if announce {
                    this.scanning = false;
                }
                this.scanned = true;
                if let Ok(found) = &scan {
                    this.no_launchers =
                        !found.is_empty() && found.iter().all(|launcher| !launcher.installed);
                }
                if announce {
                    let (text, failed) = this.scan_summary(scan);
                    this.say(text, failed, cx);
                }
                this.loaded(library, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn scan_summary(&self, scan: Result<Vec<LibraryScanResult>, ApiError>) -> (String, bool) {
        let found = match scan {
            Ok(found) => found,
            Err(error) => return (error.message, true),
        };
        let failures: Vec<String> = found
            .iter()
            .filter_map(|launcher| {
                let error = launcher.error.as_ref()?;
                Some(format!("{}: {error}", launcher.launcher.display_name()))
            })
            .collect();
        if !failures.is_empty() {
            return (failures.join(" \u{b7} "), true);
        }
        if found.is_empty() {
            return (
                "No launcher is available in this build yet, so nothing was scanned.".to_owned(),
                true,
            );
        }
        if self.no_launchers {
            return (
                "No launchers found. Install Steam, Epic Games, Battle.net, or Riot Games, then rescan."
                    .to_owned(),
                true,
            );
        }
        let count = |of: fn(&LibraryScanResult) -> usize| found.iter().map(of).sum::<usize>();
        let (added, removed) = (
            count(|launcher| launcher.added),
            count(|launcher| launcher.removed),
        );
        if added + removed == 0 {
            ("Library is up to date".to_owned(), false)
        } else {
            (format!("{added} added, {removed} removed"), false)
        }
    }

    fn say(&mut self, text: String, failed: bool, cx: &mut Context<Self>) {
        let tone = if failed {
            FlashTone::Error
        } else {
            FlashTone::Success
        };
        self.flash = Some(Flash::new(
            text,
            tone,
            None,
            |this: &mut Self| &mut this.flash,
            cx,
        ));
        cx.notify();
    }

    // A click on a card plays at once as the account the game was last launched with. The dialog comes first when there is no such account, when the launcher wants to update the game, and while a job runs.
    fn open_card(&mut self, game: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.games.get(game) else {
            return;
        };
        let game = entry.game.clone();
        let last = game.last_launch.clone();
        let known = last
            .as_ref()
            .is_some_and(|last| last.account_name.is_some());
        let instant = known && plays_at_once(&game, self.host.read(cx));
        let account = last.map(|last| last.account_id);
        self.host
            .update(cx, |host, cx| host.open(game, account, instant, cx));
    }

    fn open_card_menu(
        &mut self,
        game: usize,
        at: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.games.get(game) else {
            return;
        };
        self.menu = None;
        self.card_menu = Some(CardMenu {
            game: entry.game.id.clone(),
            at,
            viewport: window.viewport_size(),
        });
        cx.notify();
    }

    fn close_card_menu(&mut self, cx: &mut Context<Self>) {
        if self.card_menu.take().is_some() {
            cx.notify();
        }
    }

    // The account list, never a one-click Play.
    fn play_as(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.card_menu.take() else {
            return;
        };
        cx.notify();
        let Some(game) = self
            .games
            .iter()
            .find(|entry| entry.game.id == menu.game)
            .map(|entry| entry.game.clone())
        else {
            return;
        };
        let account = game
            .last_launch
            .as_ref()
            .map(|last| last.account_id.clone());
        self.host
            .update(cx, |host, cx| host.open(game, account, false, cx));
    }

    fn open_recent(&mut self, entry: usize, cx: &mut Context<Self>) {
        let Some(recent) = self.recent.get(entry) else {
            return;
        };
        let played = self
            .games
            .iter()
            .find(|game| game.game.id == recent.game_id);
        let Some(game) = played.map(|game| game.game.clone()) else {
            return;
        };
        let instant = plays_at_once(&game, self.host.read(cx));
        let account = Some(recent.account_id.clone());
        self.host
            .update(cx, |host, cx| host.open(game, account, instant, cx));
    }

    fn reword(&mut self, cx: &mut Context<Self>) {
        let now = SystemTime::now();
        let mut changed = false;
        for (card, entry) in self.cards.iter_mut().zip(&self.games) {
            let caption = caption(&entry.game, now);
            if *card.caption != *caption {
                card.caption = caption.into();
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    pub fn start_sweep(
        &mut self,
        frames: usize,
        step: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sweep_report = None;
        self.sweep = Some(Sweep {
            frames_left: frames,
            step: px(step),
            stamps: Vec::with_capacity(frames + 1),
        });
        self.await_frame(window, cx);
    }

    pub fn take_sweep_report(&mut self) -> Option<SweepReport> {
        self.sweep_report.take()
    }

    pub fn log_frames(&mut self) {
        self.frames = Some(Vec::new());
    }

    pub fn take_frames(&mut self) -> Vec<LibraryFrame> {
        self.frames.take().unwrap_or_default()
    }

    fn await_frame(&self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            this.update(cx, |this, cx| this.sweep_frame(window, cx))
                .ok();
        });
    }

    fn sweep_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sweep) = self.sweep.as_mut() else {
            return;
        };
        sweep.stamps.push(Instant::now());
        if sweep.frames_left == 0 {
            self.sweep_report = Some(SweepReport::of(&sweep.stamps));
            self.sweep = None;
            return;
        }
        sweep.frames_left -= 1;
        let step = sweep.step;
        {
            let state = self.scroll.0.borrow();
            let offset = state.base_handle.offset();
            state
                .base_handle
                .set_offset(point(offset.x, offset.y - step));
        }
        cx.notify();
        self.await_frame(window, cx);
    }

    pub fn focus_search(&self, window: &mut Window, cx: &mut App) {
        self.search.focus(window, cx);
    }

    pub fn set_sidebar(&mut self, width: Pixels, cx: &mut Context<Self>) {
        self.sidebar = width;
        cx.notify();
    }

    pub fn typing(&self, window: &Window, cx: &App) -> bool {
        self.search.is_focused(window, cx)
    }

    // As the Electron app's page, which is made afresh each time it is shown: no search, no filter, the top of the grid and of the Recent strip, the sort kept, and the games read again.
    pub fn arrive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.on_screen = true;
        self.flash = None;
        self.menu = None;
        self.card_menu = None;
        self.clear_search_and_filter(window, cx);
        if matches!(self.load, Load::Ready) {
            self.reload(cx);
        }
    }

    pub fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.on_screen = false;
        self.menu = None;
        self.card_menu = None;
        self.over_trigger = None;
        self.let_go(cx);
        if self.typing(window, cx) {
            window.blur(cx);
        }
    }

    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.scroll.scroll_to_item_strict(row, ScrollStrategy::Top);
        cx.notify();
    }

    fn on_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // A dialog over the Library has the keyboard to itself.
        if !self.on_screen || self.host.read(cx).is_modal() {
            return false;
        }
        if self.card_menu.is_some() {
            let held = keystroke.modifiers;
            if held.alt || held.platform {
                return false;
            }
            // Down, Up, Home and End all land on the one item, which has the keyboard already.
            match keystroke.key.as_str() {
                "down" | "up" | "home" | "end" => {}
                "enter" | "space" => self.play_as(cx),
                "escape" | "tab" => self.close_card_menu(cx),
                _ => return false,
            }
            return true;
        }
        if keystroke.key == "escape" && self.menu.is_some() {
            self.menu = None;
            cx.notify();
            return true;
        }
        let held = keystroke.modifiers;
        let only_control =
            held.control && !held.alt && !held.shift && !held.platform && !held.function;
        if keystroke.key != "f" || !only_control {
            return false;
        }
        if !self.search.is_disabled(cx) {
            self.search.focus_and_select(window, cx);
        }
        true
    }

    fn set_query(&mut self, query: SharedString, cx: &mut Context<Self>) {
        self.query = query;
        self.show(cx);
    }

    fn arrange(&mut self) {
        self.shown = visible(&self.games, &self.query, self.filter, self.sort);
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        self.arrange();
        self.scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
        self.recent_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn clear_search_and_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = None;
        self.query = SharedString::default();
        self.search.clear(window, cx);
        self.show(cx);
    }

    fn toggle_menu(&mut self, which: Menu, cx: &mut Context<Self>) {
        self.card_menu = None;
        let open = self.menu.as_ref().is_some_and(|menu| menu.which == which);
        self.menu = (!open).then(|| OpenMenu {
            which,
            highlight: match which {
                Menu::Filter => LauncherId::ALL
                    .iter()
                    .position(|launcher| Some(*launcher) == self.filter),
                Menu::Sort => LibrarySort::ALL.iter().position(|sort| *sort == self.sort),
            }
            .unwrap_or_default(),
        });
        cx.notify();
    }

    fn choose(&mut self, which: Menu, option: usize, cx: &mut Context<Self>) {
        self.menu = None;
        match which {
            Menu::Filter => {
                let launcher = LauncherId::ALL.get(option).copied();
                self.filter = launcher.filter(|launcher| Some(*launcher) != self.filter);
            }
            Menu::Sort => {
                if let Some(sort) = LibrarySort::ALL.get(option).copied()
                    && sort != self.sort
                {
                    self.sort = sort;
                    let backend = self.backend.clone();
                    // A choice that could not be stored still holds for this run, as in the Electron app.
                    cx.spawn(async move |_, _| {
                        let _ = backend.set_library_sort(sort).await;
                    })
                    .detach();
                }
            }
        }
        self.show(cx);
    }

    fn poster(
        &mut self,
        game: usize,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let (file, shape) = self.cards[game].art.clone()?;
        match self.posters.lookup(game) {
            Lookup::Ready(image) => Some(image),
            Lookup::Pending | Lookup::Unreadable => None,
            Lookup::Missing => {
                let list = self.list;
                cx.spawn(async move |this, cx| {
                    let image = cx
                        .background_executor()
                        .spawn(async move { posters::decode(&file, shape, scale).map(Arc::new) })
                        .await;
                    this.update(cx, |this, cx| {
                        if this.list == list {
                            this.posters.insert(game, image);
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
                None
            }
        }
    }

    fn render_rows(
        &mut self,
        rows: Range<usize>,
        grid: Grid,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        rows.map(|row| {
            let first = row * grid.columns;
            let last = (first + grid.columns).min(self.shown.len());
            let mut line = div()
                .flex()
                .gap(grid.gap)
                .pl(px(PAGE_PADDING))
                .pr(px(PAGE_END))
                .pb(grid.gap);
            for slot in first..last {
                let game = self.shown[slot];
                line = line.child(self.render_card(game, grid, scale, cx));
            }
            line
        })
        .collect()
    }

    fn render_card(
        &mut self,
        game: usize,
        grid: Grid,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let poster = self.poster(game, scale, cx);
        // A one-click Play is being checked: its mark stays up and turns.
        let busy = self.host.read(cx).silent_game() == Some(self.games[game].game.id.as_str());
        let card = &self.cards[game];
        let icon_art = matches!(card.art, Some((_, Shape::Icon)));
        let (width, height) = (grid.poster_width(), grid.poster_width() * 1.5);
        let art = div()
            .relative()
            .w_full()
            .h(height)
            .rounded_t(px(11.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(color(theme::SURFACE_2), 0.),
                linear_color_stop(color(theme::SURFACE_3), 1.),
            ))
            .when(poster.is_none(), |art| {
                art.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(48.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(alpha(theme::TEXT_STRONG, 0.1))
                        .child(card.monogram.clone()),
                )
            })
            .when(poster.is_none() || icon_art, |art| {
                art.child(
                    div()
                        .absolute()
                        .left(px(12.))
                        .right(px(12.))
                        .bottom(px(12.))
                        .line_clamp(3)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color(theme::TEXT_STRONG))
                        .child(card.name.clone()),
                )
            })
            .children(poster.map(|image| {
                if icon_art {
                    img(image)
                        .absolute()
                        .top(height * 0.22)
                        .left(width * 0.25)
                        .size(width * 0.5)
                        .object_fit(ObjectFit::Contain)
                } else {
                    img(image)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .rounded_t(px(11.))
                }
            }))
            .when(card.needs_update, |art| {
                art.child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .left(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(alpha(theme::WARNING, 0.4))
                        .bg(alpha(theme::BG, 0.8))
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(theme::WARNING))
                        .child(icon(assets::UPDATE, 12., theme::WARNING))
                        .child("Update"),
                )
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .flex()
                    .justify_center()
                    .pt(px(40.))
                    .pb(px(12.))
                    .bg(linear_gradient(
                        0.,
                        linear_color_stop(alpha(theme::BG, 0.8), 0.),
                        linear_color_stop(alpha(theme::BG, 0.), 1.),
                    ))
                    .when(!busy, |mark| {
                        mark.opacity(0.)
                            .group_hover("card", |style| style.opacity(1.))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(40.))
                            .rounded_full()
                            .bg(color(theme::ACCENT))
                            .child(if busy {
                                spinner("checking", 20., theme::TEXT_STRONG)
                            } else {
                                icon(assets::PLAY, 20., theme::TEXT_STRONG).into_any_element()
                            }),
                    ),
            )
            .child(
                div()
                    .id("more")
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(color(theme::BORDER))
                    .bg(alpha(theme::BG, 0.8))
                    .opacity(0.)
                    .group_hover("card", |style| style.opacity(1.))
                    .cursor_pointer()
                    .hover(|style| style.text_color(color(theme::TEXT)))
                    // A press here is not a press on Play.
                    .on_click(cx.listener(move |this, click: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_card_menu(game, click.position(), window, cx);
                    }))
                    .child(icon(assets::MORE, 16., theme::MUTED)),
            );
        let caption = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(12.))
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(theme::TEXT_STRONG))
                    .child(card.name.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(color(theme::MUTED))
                    .child(launcher_dot(card.launcher))
                    .child(div().truncate().child(card.caption.clone())),
            );
        div()
            .id(("card", game))
            .group("card")
            .flex_none()
            .w(grid.card)
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .overflow_hidden()
            .cursor_pointer()
            .hover(|style| style.border_color(color(theme::BORDER_STRONG)))
            // The second click of a double-click is no second Play.
            .on_click(cx.listener(move |this, click: &ClickEvent, _, cx| {
                if click.click_count() == 1 {
                    this.open_card(game, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, down: &MouseDownEvent, window, cx| {
                    this.open_card_menu(game, down.position, window, cx);
                }),
            )
            .child(art)
            .child(caption)
    }

    fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let heading = div()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(16.))
            .child(page_title(
                "Library",
                Some("Installed games across your launchers. Pick a game, pick an account, play."),
            ))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .max_w(px(480.))
                    .children(self.flash.as_ref().map(Flash::render))
                    .child(
                        Button::new(
                            "rescan",
                            Tone::Secondary,
                            if self.scanning {
                                "Scanning\u{2026}"
                            } else {
                                "Rescan"
                            },
                        )
                        .glyph(assets::RESCAN)
                        .turning(self.scanning)
                        .disabled(self.scanning)
                        .on_click(cx.listener(|this, _, _, cx| this.rescan(true, cx))),
                    ),
            );
        let empty = self.games.is_empty();
        let clear = (!self.query.is_empty()).then(|| {
            div()
                .id("clear-search")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(24.))
                .mr(px(-4.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(color(theme::SURFACE_2)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.search.clear(window, cx);
                    this.search.focus(window, cx);
                }))
                .child(icon(assets::CLEAR, 14., theme::FAINT))
                .into_any_element()
        });
        let toolbar = div()
            .mt(px(20.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                self.search
                    .frame(
                        "search",
                        Some(icon(assets::SEARCH, 16., theme::FAINT).into_any_element()),
                        clear,
                        window,
                        cx,
                    )
                    .flex_grow(1.)
                    .flex_basis(px(256.))
                    .min_w(px(256.))
                    .max_w(px(448.)),
            )
            .child(self.render_dropdown(Menu::Filter, empty, cx))
            .child(self.render_dropdown(Menu::Sort, empty, cx));
        div()
            .flex()
            .flex_col()
            .flex_none()
            .pl(px(PAGE_PADDING))
            .pr(px(PAGE_END))
            .pt(px(28.))
            .pb(px(20.))
            .child(heading)
            .child(toolbar)
    }

    fn render_dropdown(&self, which: Menu, disabled: bool, cx: &mut Context<Self>) -> Div {
        let (id, glyph, label) = match which {
            Menu::Filter => ("filter", assets::FILTER, "Filter"),
            Menu::Sort => ("sort", assets::SORT, "Sort"),
        };
        let open = self.menu.as_ref().filter(|menu| menu.which == which);
        let narrowing = which == Menu::Filter && self.filter.is_some();
        let trigger = div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(CONTROL))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .font_weight(FontWeight::MEDIUM)
            .child(icon(
                glyph,
                14.,
                if narrowing {
                    theme::ACCENT
                } else {
                    theme::FAINT
                },
            ))
            .child(label)
            .map(|trigger| {
                if narrowing {
                    trigger
                        .border_color(color(theme::ACCENT))
                        .bg(alpha(theme::ACCENT, 0.16))
                        .text_color(color(theme::TEXT_STRONG))
                } else {
                    trigger
                        .border_color(color(if open.is_some() {
                            theme::BORDER_STRONG
                        } else {
                            theme::BORDER
                        }))
                        .bg(color(theme::SURFACE_2))
                }
            })
            .map(|trigger| {
                if disabled {
                    return trigger.opacity(0.6);
                }
                trigger
                    .cursor_pointer()
                    .when(!narrowing, |trigger| {
                        trigger.hover(|style| {
                            style
                                .bg(color(theme::SURFACE_3))
                                .border_color(color(theme::BORDER_STRONG))
                        })
                    })
                    .on_hover(cx.listener(move |this, over: &bool, _, _| {
                        if *over {
                            this.over_trigger = Some(which);
                        } else if this.over_trigger == Some(which) {
                            this.over_trigger = None;
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(which, cx)))
            });
        div()
            .relative()
            .flex_none()
            .child(trigger)
            .children(open.map(|open| {
                div().absolute().left_0().top(px(CONTROL + MENU_GAP)).child(
                    deferred(
                        anchored()
                            .snap_to_window_with_margin(px(8.))
                            .child(self.render_menu(which, open.highlight, cx)),
                    )
                    .with_priority(1),
                )
            }))
    }

    fn render_menu(&self, which: Menu, highlight: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let options: Vec<(Option<Div>, SharedString, bool)> = match which {
            Menu::Filter => LauncherId::ALL
                .into_iter()
                .map(|launcher| {
                    let count = self
                        .games
                        .iter()
                        .filter(|entry| entry.game.launcher == launcher)
                        .count();
                    (
                        Some(launcher_dot(launcher)),
                        format!("{} ({count})", launcher.display_name()).into(),
                        self.filter == Some(launcher),
                    )
                })
                .collect(),
            Menu::Sort => LibrarySort::ALL
                .into_iter()
                .map(|sort| (None, sort.label().into(), sort == self.sort))
                .collect(),
        };
        let menu = div()
            .id("menu")
            .occlude()
            .flex()
            .flex_col()
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE_2))
            .shadow(vec![
                BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.35)).blur_radius(px(24.)),
            ])
            .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                // A press on the button that opened the menu is that button's to answer: it closes it.
                if this.over_trigger != Some(which) {
                    this.menu = None;
                    cx.notify();
                }
            }));
        menu.children(
            options
                .into_iter()
                .enumerate()
                .map(|(option, (mark, label, chosen))| {
                    let lit = option == highlight;
                    div()
                        .id(("option", option))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .py(px(6.))
                        .pl(px(10.))
                        .pr(px(8.))
                        .rounded(px(6.))
                        .whitespace_nowrap()
                        .cursor_pointer()
                        .when(lit, |row| row.bg(color(theme::SURFACE_3)))
                        .when(lit || chosen, |row| {
                            row.text_color(color(theme::TEXT_STRONG))
                        })
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if let Some(menu) = this.menu.as_mut()
                                && menu.highlight != option
                            {
                                menu.highlight = option;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.choose(which, option, cx)))
                        .children(mark)
                        .child(div().flex_1().child(label))
                        .child(
                            icon(assets::CHECK, 14., theme::ACCENT)
                                .when(!chosen, |check| check.invisible()),
                        )
                }),
        )
    }

    fn render_no_match(&self, cx: &mut Context<Self>) -> Div {
        let searched = self.query.trim();
        let launcher = self.filter.map(LauncherId::display_name);
        let title = if searched.is_empty() {
            format!("No {} games installed", launcher.unwrap_or_default())
        } else {
            format!("No games match \u{201c}{searched}\u{201d}")
        };
        let description = match launcher {
            Some(launcher) => format!(
                "Only {launcher} games are shown. Clear the filter to search every launcher."
            ),
            None => "Try a shorter name, or rescan if the game was installed after the last scan."
                .to_owned(),
        };
        empty_state(
            assets::NO_MATCH,
            title,
            description,
            Button::new("clear-all", Tone::Secondary, "Clear search and filters")
                .glyph(assets::CLEAR)
                .on_click(
                    cx.listener(|this, _, window, cx| this.clear_search_and_filter(window, cx)),
                ),
        )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let waiting = self.scanning || !self.scanned;
        let title = if waiting {
            "Scanning your launchers\u{2026}"
        } else if self.no_launchers {
            "No launchers found"
        } else {
            "No games found"
        };
        let description = if self.no_launchers {
            "One Launch looks for Steam, Epic Games Launcher, Battle.net, and the Riot Client. Install one of them, then rescan."
        } else {
            "Installed Steam, Epic, Battle.net, and Riot games appear here after a scan."
        };
        empty_state(
            assets::GAMEPAD,
            title,
            description,
            Button::new("scan-now", Tone::Primary, "Scan now")
                .glyph(assets::RESCAN)
                .disabled(waiting)
                .on_click(cx.listener(|this, _, _, cx| this.rescan(true, cx))),
        )
    }

    // The last launches whose game is still in the Library, each with the place of its game. Only while nothing narrows the grid.
    fn recent_shown(&self) -> Vec<(usize, usize)> {
        if !self.query.is_empty() || self.filter.is_some() {
            return Vec::new();
        }
        self.recent
            .iter()
            .enumerate()
            .filter_map(|(entry, recent)| {
                let game = self
                    .games
                    .iter()
                    .position(|game| game.game.id == recent.game_id)?;
                Some((entry, game))
            })
            .collect()
    }

    // Scrolls away with the grid: it sits over the room the grid leaves at its top, moved by as much as the grid is scrolled.
    fn render_recent(
        &mut self,
        shown: &[(usize, usize)],
        track: Track,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> Follow {
        let now = SystemTime::now();
        let mut cards = Vec::with_capacity(shown.len());
        for (entry, game) in shown.iter().copied() {
            let poster = self.poster(game, scale, cx);
            let recent = &self.recent[entry];
            let when = relative(&recent.launched_at, now).unwrap_or_default();
            cards.push(
                div()
                    .id(("recent", entry))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(12.))
                    .w(px(RECENT_CARD))
                    .py(px(6.))
                    .pl(px(6.))
                    .pr(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(theme::BORDER))
                    .bg(color(theme::SURFACE))
                    .cursor_pointer()
                    .hover(|style| style.border_color(color(theme::BORDER_STRONG)))
                    .tooltip(tip(format!(
                        "Play {} as {}",
                        recent.game_name, recent.account_name
                    )))
                    .on_click(cx.listener(move |this, click: &ClickEvent, _, cx| {
                        if click.click_count() == 1 {
                            this.open_recent(entry, cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .w(px(32.))
                            .h(px(48.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color(theme::SURFACE_3))
                            .child(match poster {
                                Some(image) => img(image)
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .rounded(px(4.))
                                    .into_any_element(),
                                None => launcher_dot(recent.launcher).into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(color(theme::TEXT_STRONG))
                                    .child(recent.game_name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .text_color(color(theme::MUTED))
                                    .child(format!("{} \u{b7} {when}", recent.account_name)),
                            ),
                    ),
            );
        }
        // The offset is the one the wheel left, which may lie past either end until the strip is laid out.
        let beyond = (track.content - track.view).max(0.);
        let scrolled = (-f32::from(self.recent_scroll.offset().x)).clamp(0., beyond);
        let fade = |angle: f32| {
            div()
                .absolute()
                .top_0()
                .bottom(px(SCROLLBAR))
                .w(px(RECENT_FADE))
                .bg(linear_gradient(
                    angle,
                    linear_color_stop(color(theme::BG), 0.),
                    linear_color_stop(alpha(theme::BG, 0.), 1.),
                ))
        };
        let thumb = track.thumb().map(|length| {
            let per_pixel = track.per_pixel(length);
            let thumb = div()
                .absolute()
                .left_0()
                .bottom_0()
                .w(px(length))
                .h(px(SCROLLBAR))
                .p(px(THUMB_INSET))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, down: &MouseDownEvent, _, cx| {
                        this.grab_thumb(Axis::Horizontal, down.position, per_pixel, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(self.thumb_bar(Axis::Horizontal, "recent"));
            follow(
                &self.recent_scroll,
                move |handle| {
                    point(
                        px(track.place(length, handle.offset().x, handle.max_offset().x)),
                        px(0.),
                    )
                },
                thumb,
            )
        });
        let row = div()
            .group("recent")
            .relative()
            .pb(px(SCROLLBAR))
            .child(
                div()
                    .id("recent-strip")
                    .flex()
                    .gap(px(RECENT_GAP))
                    .pb(px(4.))
                    .overflow_x_scroll()
                    .restrict_scroll_to_axis()
                    .track_scroll(&self.recent_scroll)
                    .children(cards),
            )
            .when(scrolled > 1., |row| row.child(fade(90.).left_0()))
            .when(scrolled < beyond - 1., |row| {
                row.child(fade(270.).right_0())
            })
            .children(thumb);
        let strip = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .pl(px(PAGE_PADDING))
            .pr(px(PAGE_END))
            .child(
                div()
                    .mb(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(theme::MUTED))
                    .child(icon(assets::CLOCK, 14., theme::MUTED))
                    .child("RECENT"),
            )
            .child(row);
        follow(
            &self.scroll.0.borrow().base_handle,
            |handle| point(px(0.), handle.offset().y),
            strip,
        )
    }

    fn render_card_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let menu = self.card_menu.as_ref()?;
        let game = &self
            .games
            .iter()
            .find(|entry| entry.game.id == menu.game)?
            .game;
        let play_as = div()
            .id("play-as")
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .cursor_pointer()
            .text_color(color(theme::TEXT))
            .bg(color(theme::SURFACE_3))
            .on_click(cx.listener(|this, _, _, cx| this.play_as(cx)))
            .child(icon(assets::ACCOUNTS, 16., theme::MUTED))
            .child("Play as\u{2026}");
        let card = div()
            .id("card-menu")
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(208.))
            .max_w(px(320.))
            .overflow_hidden()
            .py(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE_2))
            .shadow(menu_shadow())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_card_menu(cx)))
            .child(
                div()
                    .border_b_1()
                    .border_color(alpha(theme::BORDER, 0.6))
                    .px(px(12.))
                    .py(px(6.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(theme::TEXT_STRONG))
                            .child(game.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(color(theme::FAINT))
                            .child(artwork_line(game)),
                    ),
            )
            .child(play_as);
        Some(
            deferred(
                anchored()
                    .position(menu.at)
                    .snap_to_window_with_margin(px(8.))
                    .child(card),
            )
            .with_priority(1),
        )
    }

    fn render_thumb(&self, track: Track, cx: &mut Context<Self>) -> Option<Follow> {
        let length = track.thumb()?;
        let per_pixel = track.per_pixel(length);
        let thumb = div()
            .absolute()
            .top_0()
            .right_0()
            .w(px(SCROLLBAR))
            .h(px(length))
            .p(px(THUMB_INSET))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, down: &MouseDownEvent, _, cx| {
                    this.grab_thumb(Axis::Vertical, down.position, per_pixel, cx);
                    cx.stop_propagation();
                }),
            )
            .child(self.thumb_bar(Axis::Vertical, "pane"));
        Some(follow(
            &self.scroll.0.borrow().base_handle,
            move |handle| {
                point(
                    px(0.),
                    px(track.place(length, handle.offset().y, handle.max_offset().y)),
                )
            },
            thumb,
        ))
    }

    fn thumb_bar(&self, axis: Axis, shown_over: &'static str) -> Div {
        let held = self.grab.is_some_and(|grab| grab.axis == axis);
        div()
            .size_full()
            .rounded_full()
            .bg(color(theme::BORDER_STRONG))
            .when(!held, |bar| {
                bar.opacity(0.)
                    .group_hover(shown_over, |style| style.opacity(1.))
            })
    }

    fn grab_thumb(
        &mut self,
        axis: Axis,
        pointer: Point<Pixels>,
        per_pixel: f32,
        cx: &mut Context<Self>,
    ) {
        let (pointer, offset) = match axis {
            Axis::Vertical => (pointer.y, self.scroll.0.borrow().base_handle.offset().y),
            Axis::Horizontal => (pointer.x, self.recent_scroll.offset().x),
        };
        self.grab = Some(Grab {
            axis,
            pointer: f32::from(pointer),
            scrolled: -f32::from(offset),
            per_pixel,
        });
        cx.notify();
    }

    // The element scrolled keeps the offset within its range when it is next laid out.
    fn drag_thumb(&mut self, pointer: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(grab) = self.grab else {
            return;
        };
        let along = match grab.axis {
            Axis::Vertical => pointer.y,
            Axis::Horizontal => pointer.x,
        };
        let scrolled = px(-(grab.scrolled + (f32::from(along) - grab.pointer) * grab.per_pixel));
        match grab.axis {
            Axis::Vertical => {
                let state = self.scroll.0.borrow();
                let offset = state.base_handle.offset();
                state.base_handle.set_offset(point(offset.x, scrolled));
            }
            Axis::Horizontal => {
                let offset = self.recent_scroll.offset();
                self.recent_scroll.set_offset(point(scrolled, offset.y));
            }
        }
        cx.notify();
    }

    // While a thumb is held the pointer is followed wherever it goes, in the window or out of it, until the button is let go. The listeners stand in every frame: a button let go before the frame after the press would find none, and the thumb would stay held.
    fn render_grab(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                let moved = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        moved
                            .update(cx, |this, cx| {
                                if event.dragging() {
                                    this.drag_thumb(event.position, cx);
                                } else {
                                    this.let_go(cx);
                                }
                            })
                            .ok();
                    }
                });
                let released = this.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        released.update(cx, |this, cx| this.let_go(cx)).ok();
                    }
                });
                // Any wheel closes the card's menu, as any scroll closes the Electron app's.
                let wheeled = this.clone();
                window.on_mouse_event(move |_: &ScrollEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        wheeled.update(cx, |this, cx| this.close_card_menu(cx)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn let_go(&mut self, cx: &mut Context<Self>) {
        if self.grab.take().is_some() {
            cx.notify();
        }
    }
}

impl Card {
    fn of(entry: &LibraryGame, now: SystemTime) -> Self {
        let game = &entry.game;
        let shape = match game.artwork.as_ref().map(|art| art.source) {
            Some(ArtworkSource::Launcher) => Shape::Icon,
            _ => Shape::Cover,
        };
        Self {
            name: game.name.clone().into(),
            monogram: monogram(&game.name).into(),
            caption: caption(game, now).into(),
            launcher: game.launcher,
            needs_update: game.needs_update,
            art: entry.grid.as_deref().map(|file| (Arc::from(file), shape)),
        }
    }
}

impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        // A resize of the window closes the card's menu, as in the Electron app.
        if self
            .card_menu
            .as_ref()
            .is_some_and(|menu| menu.viewport != viewport)
        {
            self.card_menu = None;
        }
        let banner = self.host.read(cx).banner_height();
        let pane = (f32::from(viewport.height) - TITLE_BAR - banner - HEADER).max(0.);
        let recent = self.recent_shown();
        let pane_width = viewport.width - self.sidebar;
        let launches = recent.len() as f32;
        let recent_track = Track {
            view: row_room(pane_width),
            content: launches * RECENT_CARD + (launches - 1.).max(0.) * RECENT_GAP,
        };
        // The Recent strip's scrollbar lies in the gap under the strip, so the strip is as high with it as without.
        let above = if recent.is_empty() { 0. } else { RECENT_STRIP };
        let grid = Grid::for_pane(pane_width);
        let rows = self.shown.len().div_ceil(grid.columns);
        let scale = window.scale_factor();
        if let Some(frames) = self.frames.as_mut() {
            frames.push(LibraryFrame {
                at: Instant::now(),
                viewport,
                pane: f32::from(pane_width),
                columns: grid.columns,
                card: f32::from(grid.card),
                gap: f32::from(grid.gap),
            });
        }

        self.posters.tick();
        let on_screen = (pane / f32::from(grid.row_height())).ceil() as usize + 1;
        let evicted = self.posters.evict((on_screen * grid.columns * 3).max(96));
        for image in evicted.into_iter().chain(self.stale.drain(..)) {
            cx.drop_image(image, Some(window));
        }

        let strip = (above > 0. && rows > 0 && matches!(self.load, Load::Ready))
            .then(|| self.render_recent(&recent, recent_track, scale, cx));
        let thumb = self.render_thumb(
            Track {
                view: pane,
                content: grid.height_of(self.shown.len()) + above,
            },
            cx,
        );
        let body = match &self.load {
            Load::Pending => div().flex_1(),
            Load::Failed(message) => div()
                .flex_1()
                .px(px(PAGE_PADDING))
                .text_color(color(theme::DANGER))
                .child(format!("Could not load the library: {message}")),
            Load::Ready if self.games.is_empty() => div().flex_1().child(self.render_empty(cx)),
            Load::Ready if rows == 0 => div().flex_1().child(self.render_no_match(cx)),
            Load::Ready => div()
                .group("pane")
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(
                    uniform_list(
                        "library-grid",
                        rows,
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            this.render_rows(range, grid, scale, cx)
                        }),
                    )
                    .track_scroll(&self.scroll)
                    // Shift and the wheel scroll the Recent strip sideways; without this the grid would take them as a vertical scroll too.
                    .map(|mut list| {
                        list.style().restrict_scroll_to_axis = Some(true);
                        list
                    })
                    .size_full()
                    .pt(px(above)),
                )
                .children(strip)
                .children(thumb),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_header(window, cx))
            .child(body)
            .child(self.render_grab(cx))
            .children(self.render_card_menu(cx))
    }
}

fn empty_state(
    glyph: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    action: AnyElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .min_h(px(420.))
        .px(px(PAGE_PADDING))
        .child(
            div()
                .mb(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .size(px(64.))
                .rounded(px(16.))
                .border_1()
                .border_color(color(theme::BORDER))
                .bg(color(theme::SURFACE))
                .child(icon(glyph, 28., theme::MUTED)),
        )
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(24.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(theme::TEXT_STRONG))
                .child(title.into()),
        )
        .child(
            div()
                .mt(px(4.))
                .max_w(px(384.))
                .text_center()
                .text_color(color(theme::MUTED))
                .child(description.into()),
        )
        .child(div().mt(px(20.)).child(action))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(artwork: Option<ArtworkSource>, error: Option<&str>) -> Game {
        Game {
            id: "g".to_owned(),
            launcher: LauncherId::Steam,
            launcher_game_id: "1".to_owned(),
            name: "Game".to_owned(),
            install_path: None,
            installed: true,
            last_seen_at: String::new(),
            needs_update: false,
            last_played_at: None,
            artwork: artwork.map(|source| ol_model::GameArtwork {
                source,
                grid: None,
                hero: None,
                logo: None,
                fetched_at: None,
            }),
            artwork_error: error.map(str::to_owned),
            last_launch: None,
        }
    }

    #[test]
    fn the_card_menu_names_where_the_artwork_came_from_or_why_there_is_none() {
        let named = [
            (ArtworkSource::SteamCdn, "Artwork: Steam"),
            (ArtworkSource::SteamGridDb, "Artwork: SteamGridDB"),
            (ArtworkSource::Manual, "Artwork: your image"),
            (ArtworkSource::Launcher, "Artwork: the game's own icon"),
        ];
        for (source, line) in named {
            assert_eq!(artwork_line(&game(Some(source), Some("missed"))), line);
        }
        assert_eq!(artwork_line(&game(None, Some("Not found"))), "Not found");
        assert_eq!(artwork_line(&game(None, None)), "No artwork yet");
    }

    const NARROWEST: usize = 960;
    const FOLDED: bool = false;
    const OPEN: bool = true;
    const BOTH: [bool; 2] = [FOLDED, OPEN];

    fn grid_at(window: usize, open: bool) -> (Grid, f32) {
        let pane = px(window as f32) - sidebar_width(open);
        (Grid::for_pane(pane), row_room(pane))
    }

    #[test]
    fn a_window_dragged_wider_never_takes_a_column_away() {
        for sidebar in BOTH {
            let (mut before, _) = grid_at(NARROWEST, sidebar);
            for window in NARROWEST + 1..=3840 {
                let (grid, _) = grid_at(window, sidebar);
                assert!(grid.columns >= before.columns, "{window} open {sidebar}");
                assert!(
                    grid.columns <= before.columns + 1,
                    "{window} open {sidebar}"
                );
                before = grid;
            }
        }
    }

    #[test]
    fn the_cards_fill_the_row_at_every_width_and_none_is_narrower_than_the_least() {
        for sidebar in BOTH {
            for window in NARROWEST..=3840 {
                let (grid, room) = grid_at(window, sidebar);
                let (card, columns) = (f32::from(grid.card), grid.columns as f32);
                let row = columns * card + (columns - 1.) * f32::from(grid.gap);
                assert!((row - room).abs() < 0.01, "{window} open {sidebar}");
                assert!(card >= CARD_MIN, "{window} open {sidebar}");
                assert!(
                    card < CARD_MIN + (CARD_MIN + CARD_GAP) / columns,
                    "{window} open {sidebar}"
                );
            }
        }
    }

    #[test]
    fn a_card_only_grows_while_the_window_is_dragged_wider_and_the_columns_stay() {
        for sidebar in BOTH {
            let (mut before, _) = grid_at(NARROWEST, sidebar);
            for window in NARROWEST + 1..=3840 {
                let (grid, _) = grid_at(window, sidebar);
                if grid.columns == before.columns {
                    assert!(grid.card > before.card, "{window} open {sidebar}");
                }
                before = grid;
            }
        }
    }

    #[test]
    fn the_narrowest_window_shows_five_in_a_row_and_the_window_as_it_first_opens_six() {
        assert_eq!(grid_at(NARROWEST, FOLDED).0.columns, 5);
        assert_eq!(grid_at(1280, FOLDED).0.columns, 6);
    }

    #[test]
    fn with_the_sidebar_open_the_narrowest_window_shows_four_and_the_first_one_five() {
        assert_eq!(grid_at(NARROWEST, OPEN).0.columns, 4);
        assert_eq!(grid_at(1280, OPEN).0.columns, 5);
    }

    #[test]
    fn opening_the_sidebar_takes_at_most_one_column_away_and_never_gives_one() {
        for window in NARROWEST..=3840 {
            let (folded, _) = grid_at(window, FOLDED);
            let (open, _) = grid_at(window, OPEN);
            assert!(open.columns <= folded.columns, "{window}");
            assert!(open.columns + 1 >= folded.columns, "{window}");
        }
    }
}
