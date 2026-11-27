use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Div, ElementId, Entity, FocusHandle, Focusable as _, MouseButton, Pixels,
    SharedString, Stateful, Subscription, Window, div, px,
};
use gpui_kit::base::input::{InputEvent, Textarea, TextareaState};

use crate::text_field::{FieldEvent, editor_style, holding};
use crate::theme::{self, color};

#[derive(Clone)]
pub struct TextArea {
    state: Entity<TextareaState>,
}

impl TextArea {
    pub fn new(
        placeholder: impl Into<SharedString>,
        rows: usize,
        most_rows: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let state = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .placeholder(placeholder)
                .auto_grow(rows, most_rows);
            state.set_editor_style(editor_style());
            state
        });
        Self { state }
    }

    pub fn text(&self, cx: &App) -> SharedString {
        self.state.read(cx).value()
    }

    pub fn set_text(&self, text: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        let text: SharedString = text.into();
        self.state
            .update(cx, |state, cx| state.set_value(text, window, cx));
    }

    pub fn set_placeholder(
        &self,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let placeholder: SharedString = placeholder.into();
        self.state.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx)
        });
    }

    pub fn on_event_in<T: 'static>(
        &self,
        window: &Window,
        cx: &mut Context<T>,
        on_event: impl Fn(&mut T, FieldEvent, &mut Window, &mut Context<T>) + 'static,
    ) -> Subscription {
        cx.subscribe_in(
            &self.state,
            window,
            move |this, _, event: &InputEvent, window, cx| {
                let event = match event {
                    InputEvent::Change => FieldEvent::Changed,
                    InputEvent::Blur => FieldEvent::Blurred,
                    InputEvent::PressEnter { .. } | InputEvent::Focus => return,
                };
                on_event(this, event, window, cx);
            },
        )
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn bounds(&self, cx: &App) -> Bounds<Pixels> {
        self.state.read(cx).input_bounds()
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.state.read(cx).focus_handle(cx).is_focused(window)
    }

    pub fn is_disabled(&self, cx: &App) -> bool {
        self.state.read(cx).presentation().is_disabled()
    }

    pub fn set_disabled(&self, disabled: bool, cx: &mut App) {
        self.state
            .update(cx, |state, cx| state.set_disabled(disabled, cx));
    }

    pub fn frame(&self, id: impl Into<ElementId>, window: &Window, cx: &App) -> Stateful<Div> {
        let disabled = self.is_disabled(cx);
        let focused = !disabled && self.is_focused(window, cx);
        let field = self.clone();
        let frame = div()
            .id(id)
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(if focused {
                theme::ACCENT
            } else {
                theme::BORDER
            }))
            .bg(color(theme::SURFACE_2))
            .line_height(px(22.75))
            .when(disabled, |frame| frame.opacity(0.6))
            .when(!disabled, |frame| {
                frame.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    field.focus(window, cx);
                })
            })
            .child(div().w_full().child(Textarea::new(&self.state)));
        holding(frame, self.focus_handle(cx))
    }
}
