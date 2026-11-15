use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    App, Context, Div, FontWeight, ObjectFit, Pixels, RenderImage, ScrollStrategy, SharedString,
    Stateful, Subscription, Svg, UniformListScrollHandle, Window, div, img, point, px, svg,
    uniform_list,
};
use ol_ui_kit::TextField;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::posters::{self, Lookup, Posters};
use crate::shell::sidebar_width;

const GAMES: usize = 500;
const PAGE_PADDING: f32 = 32.;
const SCROLLBAR: f32 = 12.;
const CAPTION: f32 = 62.;
const TITLE_BAR: f32 = 36.;
const HEADER: f32 = 150.;

pub struct LibraryView {
    search: TextField,
    secret: TextField,
    scroll: UniformListScrollHandle,
    posters: Posters,
    query: SharedString,
    shown: Vec<usize>,
    sweep: Option<Sweep>,
    sweep_report: Option<SweepReport>,
    _search_changed: Subscription,
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
}

impl Grid {
    fn for_pane(pane: Pixels, viewport: Pixels) -> Self {
        let (least, gap) = if viewport >= px(1536.) {
            (190., 20.)
        } else {
            (160., 16.)
        };
        let room = (f32::from(pane) - 2. * PAGE_PADDING - SCROLLBAR).max(least);
        let columns = (((room + gap) / (least + gap)) as usize).max(1);
        let card = (room - gap * (columns - 1) as f32) / columns as f32;
        Self {
            columns,
            card: px(card),
            gap: px(gap),
        }
    }

    fn row_height(self) -> Pixels {
        self.card * 1.5 + px(CAPTION) + self.gap
    }
}

impl LibraryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = TextField::new("Search games (Ctrl+F)", false, window, cx);
        let secret = TextField::new("Masked field (proof)", true, window, cx);
        let this = cx.weak_entity();
        let search_changed = search.on_change(cx, move |text, cx| {
            this.update(cx, |this, cx| this.filter(text, cx)).ok();
        });
        Self {
            search,
            secret,
            scroll: UniformListScrollHandle::new(),
            posters: Posters::default(),
            query: SharedString::default(),
            shown: (0..GAMES).collect(),
            sweep: None,
            sweep_report: None,
            _search_changed: search_changed,
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

    pub fn focus_secret(&self, window: &mut Window, cx: &mut App) {
        self.secret.focus(window, cx);
    }

    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.scroll.scroll_to_item_strict(row, ScrollStrategy::Top);
        cx.notify();
    }

    fn filter(&mut self, query: SharedString, cx: &mut Context<Self>) {
        let wanted = query.trim().to_lowercase();
        self.shown = (0..GAMES)
            .filter(|game| title(*game).to_lowercase().contains(&wanted))
            .collect();
        self.query = query;
        self.scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn poster(&mut self, game: usize, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        match self.posters.lookup(game) {
            Lookup::Ready(image) => Some(image),
            Lookup::Pending => None,
            Lookup::Missing => {
                cx.spawn(async move |this, cx| {
                    let image = cx
                        .background_executor()
                        .spawn(async move { Arc::new(posters::placeholder(game)) })
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

    fn render_rows(&mut self, rows: Range<usize>, grid: Grid, cx: &mut Context<Self>) -> Vec<Div> {
        rows.map(|row| {
            let first = row * grid.columns;
            let last = (first + grid.columns).min(self.shown.len());
            let mut line = div().flex().gap(grid.gap).px(px(PAGE_PADDING)).pb(grid.gap);
            for slot in first..last {
                let game = self.shown[slot];
                line = line.child(self.render_card(game, grid, cx));
            }
            line
        })
        .collect()
    }

    fn render_card(&mut self, game: usize, grid: Grid, cx: &mut Context<Self>) -> Stateful<Div> {
        let (launcher, dot) = theme::LAUNCHERS[game % theme::LAUNCHERS.len()];
        let poster = self.poster(game, cx);
        let art = div()
            .relative()
            .w_full()
            .h(grid.card * 1.5)
            .rounded_t(px(11.))
            .bg(color(theme::SURFACE_2))
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
                        .child(monogram(game)),
                )
            })
            .children(poster.map(|image| {
                img(image)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .rounded_t(px(11.))
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(12.))
                    .flex()
                    .justify_center()
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
                    .child(title(game)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(color(theme::MUTED))
                    .child(div().flex_none().size(px(6.)).rounded_full().bg(color(dot)))
                    .child(div().truncate().child(launcher)),
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

    fn render_header(&self, window: &Window, cx: &App) -> Div {
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
            .child(button("rescan", assets::RESCAN, theme::TEXT, "Rescan"));
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
                        window,
                        cx,
                    )
                    .flex_grow(1.)
                    .flex_basis(px(256.))
                    .min_w(px(256.))
                    .max_w(px(448.)),
            )
            .child(button("filter", assets::FILTER, theme::FAINT, "Filter"))
            .child(button("sort", assets::SORT, theme::FAINT, "Sort"))
            .child(
                self.secret
                    .frame(
                        "secret",
                        Some(icon(assets::KEY, 16., theme::FAINT).into_any_element()),
                        window,
                        cx,
                    )
                    .ml_auto()
                    .flex_none()
                    .w(px(220.)),
            );
        div()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(PAGE_PADDING))
            .pt(px(28.))
            .pb(px(20.))
            .child(heading)
            .child(toolbar)
    }

    fn render_thumb(&self) -> Option<Div> {
        let state = self.scroll.0.borrow();
        let sizes = state.last_item_size?;
        let (view, content) = (
            f32::from(sizes.item.height),
            f32::from(sizes.contents.height),
        );
        if content <= view || view <= 0. {
            return None;
        }
        let scrolled = -f32::from(state.base_handle.offset().y);
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

impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let grid = Grid::for_pane(
            viewport.width - sidebar_width(viewport.width),
            viewport.width,
        );
        let rows = self.shown.len().div_ceil(grid.columns);

        self.posters.tick();
        let pane = (f32::from(viewport.height) - TITLE_BAR - HEADER).max(0.);
        let on_screen = (pane / f32::from(grid.row_height())).ceil() as usize + 1;
        for image in self.posters.evict((on_screen * grid.columns * 3).max(96)) {
            cx.drop_image(image, Some(window));
        }

        let body = if rows == 0 {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(96.))
                .gap(px(4.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color(theme::TEXT_STRONG))
                        .child(format!("No games match \u{201c}{}\u{201d}", self.query)),
                )
                .child(div().text_color(color(theme::MUTED)).child(
                    "Try a shorter name, or rescan if the game was installed after the last scan.",
                ))
        } else {
            div()
                .group("pane")
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    uniform_list(
                        "library-grid",
                        rows,
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            this.render_rows(range, grid, cx)
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .size_full(),
                )
                .children(self.render_thumb())
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_header(window, cx))
            .child(body)
    }
}

fn title(game: usize) -> String {
    format!("Game {:03}", game + 1)
}

fn monogram(game: usize) -> String {
    title(game)
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .collect()
}

fn icon(path: &'static str, size: f32, hex: u32) -> Svg {
    svg()
        .path(path)
        .flex_none()
        .size(px(size))
        .text_color(color(hex))
}

fn button(
    id: &'static str,
    glyph: &'static str,
    glyph_hex: u32,
    label: &'static str,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(34.))
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
        .child(icon(glyph, 16., glyph_hex))
        .child(label)
}
