use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, Div, ElementId, Entity, FocusHandle, Focusable as _,
    MouseButton, Pixels, SharedString, Stateful, Subscription, Window, div, px,
};
use gpui_kit::base::input::{Input, InputEditorStyle, InputEvent, InputState};

use crate::theme::{self, alpha, color};

#[derive(Clone)]
pub struct TextField {
    state: Entity<InputState>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldEvent {
    Changed,
    Enter,
    Blurred,
}

pub(crate) fn editor_style() -> InputEditorStyle {
    InputEditorStyle {
        foreground: color(theme::TEXT).into(),
        muted_foreground: color(theme::FAINT).into(),
        selection: alpha(theme::ACCENT, 0.4),
        caret: color(theme::TEXT).into(),
        ..InputEditorStyle::default()
    }
}

// The length a browser's `maxlength` counts: UTF-16 code units.
pub(crate) fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

impl TextField {
    pub fn new(
        placeholder: impl Into<SharedString>,
        masked: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let state = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .placeholder(placeholder)
                .masked(masked);
            state.set_editor_style(editor_style());
            state
        });
        Self { state }
    }

    pub fn clearing_on_escape(self, cx: &mut App) -> Self {
        self.state
            .update(cx, |state, _| state.set_clean_on_escape(true));
        self
    }

    pub fn limited_to(self, most: usize, cx: &mut App) -> Self {
        self.state.update(cx, |state, cx| {
            state.set_validator(move |text, _| utf16_len(text) <= most, cx);
        });
        self
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

    pub fn set_masked(&self, masked: bool, window: &mut Window, cx: &mut App) {
        self.state
            .update(cx, |state, cx| state.set_masked(masked, window, cx));
    }

    pub fn on_change(
        &self,
        cx: &mut App,
        on_change: impl Fn(SharedString, &mut App) + 'static,
    ) -> Subscription {
        cx.subscribe(&self.state, move |state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = state.read(cx).value();
                on_change(text, cx);
            }
        })
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
                    InputEvent::PressEnter { .. } => FieldEvent::Enter,
                    InputEvent::Blur => FieldEvent::Blurred,
                    InputEvent::Focus => return,
                };
                on_event(this, event, window, cx);
            },
        )
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn focus_and_select(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
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

    pub fn clear(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| state.clean(window, cx));
    }

    pub fn is_disabled(&self, cx: &App) -> bool {
        self.state.read(cx).presentation().is_disabled()
    }

    pub fn set_disabled(&self, disabled: bool, cx: &mut App) {
        self.state
            .update(cx, |state, cx| state.set_disabled(disabled, cx));
    }

    pub fn frame(
        &self,
        id: impl Into<ElementId>,
        leading: Option<AnyElement>,
        trailing: Option<AnyElement>,
        window: &Window,
        cx: &App,
    ) -> Stateful<Div> {
        self.framed(id, 34., theme::SURFACE, true, window, cx)
            .gap(px(8.))
            .children(leading)
            .child(Input::new(&self.state))
            .children(trailing)
    }

    pub fn form_frame(
        &self,
        id: impl Into<ElementId>,
        trailing: Option<AnyElement>,
        window: &Window,
        cx: &App,
    ) -> Stateful<Div> {
        self.framed(id, 38., theme::SURFACE_2, false, window, cx)
            .gap(px(8.))
            .child(div().flex_1().min_w_0().child(Input::new(&self.state)))
            .children(trailing)
    }

    pub fn bare(&self) -> Input {
        Input::new(&self.state)
    }

    fn framed(
        &self,
        id: impl Into<ElementId>,
        height: f32,
        fill: u32,
        hover: bool,
        window: &Window,
        cx: &App,
    ) -> Stateful<Div> {
        let disabled = self.is_disabled(cx);
        let focused = !disabled && self.state.read(cx).focus_handle(cx).is_focused(window);
        let field = self.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .h(px(height))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(if focused {
                theme::ACCENT
            } else {
                theme::BORDER
            }))
            .bg(color(fill))
            .line_height(px(20.))
            .when(disabled, |frame| frame.opacity(0.6))
            .when(hover && !focused && !disabled, |frame| {
                frame.hover(|style| style.border_color(color(theme::BORDER_STRONG)))
            })
            .when(!disabled, |frame| {
                frame.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    field.focus(window, cx);
                })
            })
    }
}
