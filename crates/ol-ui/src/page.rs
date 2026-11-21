use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Bounds, Corners, DispatchPhase, Div, Hitbox, HitboxBehavior, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent, ScrollHandle, canvas, div, fill, point, px,
    size,
};
use ol_ui_kit::theme::{self, color};

const SIDE: f32 = 32.;
const STRIP: f32 = 12.;
const TRACK_MARGIN: f32 = 6.;
const THUMB_INSET: f32 = 3.;
const THUMB_LEAST: f32 = 32.;
const COLUMN: f32 = 1040.;
// A click on the track moves a page at a time, less a little so a line stays in sight, as Chromium does.
const PAGE_STEP: f32 = 0.875;
const REVEAL_ROOM: f32 = 36.;

#[derive(Clone, Copy)]
struct Grab {
    pointer: f32,
    scrolled: f32,
    per_pixel: f32,
}

#[derive(Clone, Default)]
pub struct PageScroll {
    handle: ScrollHandle,
    grab: Rc<Cell<Option<Grab>>>,
    over: Rc<Cell<bool>>,
}

impl PageScroll {
    pub fn to_top(&self) {
        clamped(&self.handle, 0.);
    }

    // As a browser brings the field that takes the keyboard into sight.
    pub fn reveal(&self, target: Bounds<Pixels>) {
        let view = self.handle.bounds();
        let room = px(REVEAL_ROOM);
        let scrolled = -f32::from(self.handle.offset().y);
        let below = f32::from(target.bottom() + room - view.bottom());
        let above = f32::from(view.top() - (target.top() - room));
        if below > 0. {
            clamped(&self.handle, scrolled + below);
        } else if above > 0. {
            clamped(&self.handle, scrolled - above);
        }
    }
}

fn column(content: impl IntoElement) -> Div {
    div()
        .pl(px(SIDE))
        .pr(px(SIDE + STRIP))
        .child(div().mx_auto().w_full().max_w(px(COLUMN)).child(content))
}

pub fn scroll_page(
    id: &'static str,
    scroll: &PageScroll,
    header: impl IntoElement,
    body: impl IntoElement,
) -> Div {
    div()
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .child(column(header).flex_none())
        .child(
            div()
                .id(id)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&scroll.handle)
                .child(column(body)),
        )
        .child(scrollbar(scroll))
}

struct Laid {
    page: Hitbox,
    strip: Hitbox,
    thumb: Option<Bounds<Pixels>>,
    travel: f32,
    view: f32,
    max: f32,
}

fn clamped(handle: &ScrollHandle, scrolled: f32) {
    let max = f32::from(handle.max_offset().y).max(0.);
    let offset = handle.offset();
    handle.set_offset(point(offset.x, px(-scrolled.clamp(0., max))));
}

fn scrollbar(scroll: &PageScroll) -> impl IntoElement + use<> {
    let (handle, grab, over) = (
        scroll.handle.clone(),
        scroll.grab.clone(),
        scroll.over.clone(),
    );
    let measured = handle.clone();
    canvas(
        move |bounds, window, _| {
            let view = f32::from(bounds.size.height);
            let max = f32::from(measured.max_offset().y).max(0.);
            let track_top = bounds.top() + px(TRACK_MARGIN);
            let track = (view - 2. * TRACK_MARGIN).max(0.);
            let strip = Bounds::new(
                point(bounds.right() - px(STRIP), bounds.top()),
                size(px(STRIP), bounds.size.height),
            );
            let (thumb, travel) = if max > 0.5 && track > 0. {
                let length = (track * view / (view + max)).clamp(THUMB_LEAST.min(track), track);
                let travel = track - length;
                let scrolled = (-f32::from(measured.offset().y)).clamp(0., max);
                let top = track_top + px(travel * scrolled / max);
                (
                    Some(Bounds::new(
                        point(strip.left(), top),
                        size(px(STRIP), px(length)),
                    )),
                    travel,
                )
            } else {
                (None, 0.)
            };
            Laid {
                page: window.insert_hitbox(bounds, HitboxBehavior::Normal),
                strip: window.insert_hitbox(strip, HitboxBehavior::Normal),
                thumb,
                travel,
                view,
                max,
            }
        },
        move |_, laid, window, _| {
            let Laid {
                page,
                strip,
                thumb,
                travel,
                view,
                max,
            } = laid;
            over.set(page.is_hovered(window));
            if let Some(thumb) = thumb
                && (over.get() || grab.get().is_some())
            {
                let bar = Bounds::new(
                    thumb.origin + point(px(THUMB_INSET), px(THUMB_INSET)),
                    thumb.size - size(px(2. * THUMB_INSET), px(2. * THUMB_INSET)),
                );
                let radius = bar.size.width.min(bar.size.height) / 2.;
                window.paint_quad(
                    fill(bar, color(theme::BORDER_STRONG)).corner_radii(Corners::all(radius)),
                );
            }

            let moved = (grab.clone(), over.clone(), handle.clone(), page.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                let (grab, over, handle, page) = &moved;
                if phase == DispatchPhase::Capture {
                    if let Some(held) = grab.get() {
                        if event.dragging() {
                            let along = f32::from(event.position.y) - held.pointer;
                            clamped(handle, held.scrolled + along * held.per_pixel);
                        } else {
                            grab.set(None);
                        }
                        window.refresh();
                    }
                    return;
                }
                if page.is_hovered(window) != over.get() {
                    window.refresh();
                }
            });

            let released = grab.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
                if phase == DispatchPhase::Capture
                    && event.button == MouseButton::Left
                    && released.take().is_some()
                {
                    window.refresh();
                }
            });

            let pressed = (grab.clone(), handle.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                let (grab, handle) = &pressed;
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !strip.is_hovered(window)
                {
                    return;
                }
                let Some(thumb) = thumb else {
                    return;
                };
                let scrolled = -f32::from(handle.offset().y);
                let y = event.position.y;
                if thumb.contains(&event.position) {
                    grab.set(Some(Grab {
                        pointer: f32::from(y),
                        scrolled,
                        per_pixel: if travel > 0. { max / travel } else { 0. },
                    }));
                } else if y < thumb.top() {
                    clamped(handle, scrolled - view * PAGE_STEP);
                } else {
                    clamped(handle, scrolled + view * PAGE_STEP);
                }
                cx.stop_propagation();
                window.refresh();
            });

            // The wheel over the header scrolls the body too, as over the Electron app's sticky header.
            let wheeled = handle;
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, _| {
                if phase != DispatchPhase::Bubble
                    || !page.is_hovered(window)
                    || wheeled.bounds().contains(&event.position)
                {
                    return;
                }
                let delta = event.delta.pixel_delta(window.line_height());
                if delta.y != px(0.) {
                    clamped(
                        &wheeled,
                        -f32::from(wheeled.offset().y) - f32::from(delta.y),
                    );
                    window.refresh();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
