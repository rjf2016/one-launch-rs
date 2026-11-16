use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Div, Entity, EventEmitter, FontWeight,
    Keystroke, ObjectFit, Pixels, RenderImage, ScrollStrategy, SharedString, Stateful,
    Subscription, Task, UniformListScrollHandle, Window, anchored, deferred, div, hsla, img,
    linear_color_stop, linear_gradient, point, px, uniform_list,
};
use ol_api::library::{LibraryGame, caption, monogram, relative, visible};
use ol_api::{ApiError, Backend, Library};
use ol_model::{ArtworkSource, Game, LauncherId, LibraryScanResult, LibrarySort, RecentLaunch};
use ol_ui_kit::TextField;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::host::{JobSettled, LaunchHost};
use crate::posters::{self, Lookup, Posters, Shape};
use crate::shell::sidebar_width;
use crate::widgets::{Button, Tone, icon, launcher_dot, spinner};

const PAGE_PADDING: f32 = 32.;
const SCROLLBAR: f32 = 12.;
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
const FLASH_STAYS: Duration = Duration::from_secs(3);
const ERROR_FLASH_STAYS: Duration = Duration::from_secs(6);

pub struct LibraryView {
    backend: Backend,
    host: Entity<LaunchHost>,
    search: TextField,
    scroll: UniformListScrollHandle,
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
    sweep: Option<Sweep>,
    sweep_report: Option<SweepReport>,
    _subscriptions: [Subscription; 4],
}

enum Load {
    Pending,
    Failed(SharedString),
    Ready,
}

// A line beside Rescan that goes by itself.
struct Flash {
    text: SharedString,
    failed: bool,
    _timer: Task<()>,
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

#[derive(Clone, Copy)]
struct Grid {
    columns: usize,
    card: Pixels,
    gap: Pixels,
    strip: f32,
}

impl Grid {
    // As the page of the Electron app lays it out: the scrollbar takes its strip, from the header too, only when the rows do not fit.
    fn for_pane(pane: Pixels, viewport: Pixels, height: f32, games: usize) -> Self {
        let (least, gap) = if viewport >= px(1536.) {
            (190., 20.)
        } else {
            (160., 16.)
        };
        let fit = |strip: f32| {
            let room = (f32::from(pane) - 2. * PAGE_PADDING - strip).max(least);
            let columns = (((room + gap) / (least + gap)) as usize).max(1);
            let card = (room - gap * (columns - 1) as f32) / columns as f32;
            Self {
                columns,
                card: px(card),
                gap: px(gap),
                strip,
            }
        };
        let wide = fit(0.);
        if wide.height_of(games) <= height {
            wide
        } else {
            fit(SCROLLBAR)
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
            sweep: None,
            sweep_report: None,
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
        let stays = if failed {
            ERROR_FLASH_STAYS
        } else {
            FLASH_STAYS
        };
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(stays).await;
            this.update(cx, |this, cx| {
                this.flash = None;
                cx.notify();
            })
            .ok();
        });
        self.flash = Some(Flash {
            text: text.into(),
            failed,
            _timer: timer,
        });
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

    pub fn scroll_to_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.scroll.scroll_to_item_strict(row, ScrollStrategy::Top);
        cx.notify();
        // The list only moves when it is drawn. What is placed by how far it has moved, the Recent strip and the thumb, is drawn once more after that.
        let this = cx.weak_entity();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |_, cx| {
                this.update(cx, |_, cx| cx.notify()).ok();
            });
        });
    }

    fn on_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // A dialog over the Library has the keyboard to itself.
        if self.host.read(cx).is_modal() {
            return false;
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
        cx.notify();
    }

    fn clear_search_and_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = None;
        self.query = SharedString::default();
        self.search.clear(window, cx);
        self.show(cx);
    }

    fn toggle_menu(&mut self, which: Menu, cx: &mut Context<Self>) {
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
            let mut line = div().flex().gap(grid.gap).px(px(PAGE_PADDING)).pb(grid.gap);
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
                    // The card's menu is not built yet; a press here is not a press on Play.
                    .on_click(|_, _, cx| cx.stop_propagation())
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
            .child(art)
            .child(caption)
    }

    fn render_header(&self, strip: f32, window: &Window, cx: &mut Context<Self>) -> Div {
        let heading = div()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color(theme::TEXT_STRONG))
                            .child("Library"),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .truncate()
                            .text_color(color(theme::MUTED))
                            .child(
                                "Installed games across your launchers. Pick a game, pick an account, play.",
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .max_w(px(480.))
                    .children(self.flash.as_ref().map(|flash| {
                        div()
                            .min_w_0()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(if flash.failed {
                                theme::DANGER
                            } else {
                                theme::SUCCESS
                            }))
                            .child(flash.text.clone())
                    }))
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
            .pr(px(PAGE_PADDING + strip))
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
        strip: f32,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let now = SystemTime::now();
        let scrolled = self.scroll.0.borrow().base_handle.offset().y;
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
                    .w(px(240.))
                    .py(px(6.))
                    .pl(px(6.))
                    .pr(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(theme::BORDER))
                    .bg(color(theme::SURFACE))
                    .cursor_pointer()
                    .hover(|style| style.border_color(color(theme::BORDER_STRONG)))
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
        div()
            .absolute()
            .top(scrolled)
            .left_0()
            .right_0()
            .pl(px(PAGE_PADDING))
            .pr(px(PAGE_PADDING + strip))
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
            .child(
                div()
                    .id("recent-strip")
                    .flex()
                    .gap(px(8.))
                    .overflow_x_scroll()
                    .restrict_scroll_to_axis()
                    .children(cards),
            )
    }

    fn render_thumb(&self, content: f32, view: f32) -> Option<Div> {
        if content <= view || view <= 0. {
            return None;
        }
        let scrolled = -f32::from(self.scroll.0.borrow().base_handle.offset().y);
        let length = (view * view / content).max(32.);
        let top = (view - length) * (scrolled / (content - view)).clamp(0., 1.);
        Some(
            div()
                .absolute()
                .top(px(top + 3.))
                .right(px(3.))
                .w(px(6.))
                .h(px(length - 6.))
                .rounded_full()
                .bg(color(theme::BORDER_STRONG))
                .opacity(0.)
                .group_hover("pane", |style| style.opacity(1.)),
        )
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
        let banner = self.host.read(cx).banner_height();
        let pane = (f32::from(viewport.height) - TITLE_BAR - banner - HEADER).max(0.);
        let recent = self.recent_shown();
        let above = if recent.is_empty() { 0. } else { RECENT_STRIP };
        let grid = Grid::for_pane(
            viewport.width - sidebar_width(viewport.width),
            viewport.width,
            pane - above,
            self.shown.len(),
        );
        let rows = self.shown.len().div_ceil(grid.columns);
        let scale = window.scale_factor();

        self.posters.tick();
        let on_screen = (pane / f32::from(grid.row_height())).ceil() as usize + 1;
        let evicted = self.posters.evict((on_screen * grid.columns * 3).max(96));
        for image in evicted.into_iter().chain(self.stale.drain(..)) {
            cx.drop_image(image, Some(window));
        }

        let strip = (above > 0. && rows > 0 && matches!(self.load, Load::Ready))
            .then(|| self.render_recent(&recent, grid.strip, scale, cx));
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
                    .size_full()
                    .pt(px(above)),
                )
                .children(strip)
                .children(self.render_thumb(grid.height_of(self.shown.len()) + above, pane)),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_header(grid.strip, window, cx))
            .child(body)
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
