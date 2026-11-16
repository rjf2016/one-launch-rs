use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::prelude::*;
use gpui::{
    App, BoxShadow, Context, Div, EventEmitter, FontWeight, Keystroke, ObjectFit, Pixels,
    RenderImage, ScrollStrategy, SharedString, Stateful, Subscription, Svg,
    UniformListScrollHandle, Window, anchored, deferred, div, hsla, img, linear_color_stop,
    linear_gradient, point, px, svg, uniform_list,
};
use ol_api::library::{LibraryGame, caption, monogram, visible};
use ol_api::{ApiError, Backend, Library};
use ol_model::{ArtworkSource, LauncherId, LibrarySort};
use ol_ui_kit::TextField;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::posters::{self, Lookup, Posters, Shape};
use crate::shell::sidebar_width;

const PAGE_PADDING: f32 = 32.;
const SCROLLBAR: f32 = 12.;
const CAPTION: f32 = 62.;
const BORDER: f32 = 1.;
const TITLE_BAR: f32 = 36.;
const HEADER: f32 = 152.;
const CONTROL: f32 = 34.;
const MENU_GAP: f32 = 6.;
const CLOCK: Duration = Duration::from_secs(60);

pub struct LibraryView {
    backend: Backend,
    search: TextField,
    scroll: UniformListScrollHandle,
    posters: Posters,
    load: Load,
    games: Vec<LibraryGame>,
    cards: Vec<Card>,
    shown: Vec<usize>,
    query: SharedString,
    filter: Option<LauncherId>,
    sort: LibrarySort,
    menu: Option<OpenMenu>,
    over_trigger: Option<Menu>,
    sweep: Option<Sweep>,
    sweep_report: Option<SweepReport>,
    _subscriptions: [Subscription; 2],
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

impl LibraryView {
    pub fn new(backend: Backend, window: &mut Window, cx: &mut Context<Self>) -> Self {
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

        cx.spawn({
            let backend = backend.clone();
            async move |this, cx| {
                let library = backend.library().await;
                this.update(cx, |this, cx| this.loaded(library, cx)).ok();
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
            search,
            scroll: UniformListScrollHandle::new(),
            posters: Posters::default(),
            load: Load::Pending,
            games: Vec::new(),
            cards: Vec::new(),
            shown: Vec::new(),
            query: SharedString::default(),
            filter: None,
            sort: LibrarySort::default(),
            menu: None,
            over_trigger: None,
            sweep: None,
            sweep_report: None,
            _subscriptions: [search_changed, keys],
        }
    }

    fn loaded(&mut self, library: Result<Library, ApiError>, cx: &mut Context<Self>) {
        match library {
            Ok(library) => {
                let now = SystemTime::now();
                self.cards = library
                    .games
                    .iter()
                    .map(|entry| Card::of(entry, now))
                    .collect();
                self.games = library.games;
                self.sort = library.sort;
                self.load = Load::Ready;
                self.search.set_disabled(self.games.is_empty(), cx);
                self.show(cx);
            }
            Err(error) => {
                self.load = Load::Failed(error.message.into());
                cx.notify();
            }
        }
        cx.emit(LibraryLoaded);
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

    fn show(&mut self, cx: &mut Context<Self>) {
        self.shown = visible(&self.games, &self.query, self.filter, self.sort);
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
                cx.spawn(async move |this, cx| {
                    let image = cx
                        .background_executor()
                        .spawn(async move { posters::decode(&file, shape, scale).map(Arc::new) })
                        .await;
                    this.update(cx, |this, cx| {
                        this.posters.insert(game, image);
                        cx.notify();
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
                    .opacity(0.)
                    .group_hover("card", |style| style.opacity(1.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(40.))
                            .rounded_full()
                            .bg(color(theme::ACCENT))
                            .child(icon(assets::PLAY, 20., theme::TEXT_STRONG)),
                    ),
            )
            .child(
                div()
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
            .child(button("rescan", assets::RESCAN, "Rescan"));
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
            button("clear-all", assets::CLEAR, "Clear search and filters").on_click(
                cx.listener(|this, _, window, cx| this.clear_search_and_filter(window, cx)),
            ),
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
        let pane = (f32::from(viewport.height) - TITLE_BAR - HEADER).max(0.);
        let grid = Grid::for_pane(
            viewport.width - sidebar_width(viewport.width),
            viewport.width,
            pane,
            self.shown.len(),
        );
        let rows = self.shown.len().div_ceil(grid.columns);
        let scale = window.scale_factor();

        self.posters.tick();
        let on_screen = (pane / f32::from(grid.row_height())).ceil() as usize + 1;
        for image in self.posters.evict((on_screen * grid.columns * 3).max(96)) {
            cx.drop_image(image, Some(window));
        }

        let body = match &self.load {
            Load::Pending => div().flex_1(),
            Load::Failed(message) => div()
                .flex_1()
                .px(px(PAGE_PADDING))
                .text_color(color(theme::DANGER))
                .child(format!("Could not load the library: {message}")),
            Load::Ready if self.games.is_empty() => div().flex_1().child(empty_state(
                assets::GAMEPAD,
                "No games found",
                "Installed Steam, Epic, Battle.net, and Riot games appear here after a scan.",
                primary_button("scan-now", assets::RESCAN, "Scan now"),
            )),
            Load::Ready if rows == 0 => div().flex_1().child(self.render_no_match(cx)),
            Load::Ready => div()
                .group("pane")
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    uniform_list(
                        "library-grid",
                        rows,
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            this.render_rows(range, grid, scale, cx)
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .size_full(),
                )
                .children(self.render_thumb(grid.height_of(self.shown.len()), pane)),
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
    action: Stateful<Div>,
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

fn launcher_dot(launcher: LauncherId) -> Div {
    let hex = match launcher {
        LauncherId::Steam => theme::STEAM,
        LauncherId::Epic => theme::EPIC,
        LauncherId::Battlenet => theme::BATTLENET,
        LauncherId::Riot => theme::RIOT,
    };
    div().flex_none().size(px(6.)).rounded_full().bg(color(hex))
}

fn icon(path: &'static str, size: f32, hex: u32) -> Svg {
    svg()
        .path(path)
        .flex_none()
        .size(px(size))
        .text_color(color(hex))
}

fn button(id: &'static str, glyph: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(CONTROL))
        .px(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(color(theme::SURFACE_2))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(color(theme::SURFACE_3))
                .border_color(color(theme::BORDER_STRONG))
        })
        .child(icon(glyph, 16., theme::TEXT))
        .child(label)
}

fn primary_button(id: &'static str, glyph: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .bg(color(theme::ACCENT))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(theme::TEXT_STRONG))
        .cursor_pointer()
        .hover(|style| style.bg(color(theme::ACCENT_HOVER)))
        .child(icon(glyph, 16., theme::TEXT_STRONG))
        .child(label)
}
