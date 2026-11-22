use gpui::prelude::*;
use gpui::{
    AnyElement, App, Div, ElementId, Entity, Focusable as _, MouseButton, SharedString, Stateful,
    Subscription, Window, div, px,
};
use gpui_kit::base::input::{Input, InputEditorStyle, InputEvent, InputState};

use crate::theme::{self, alpha, color};

#[derive(Clone)]
pub struct TextField {
    state: Entity<InputState>,
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
            state.set_editor_style(InputEditorStyle {
                foreground: color(theme::TEXT).into(),
                muted_foreground: color(theme::FAINT).into(),
                selection: alpha(theme::ACCENT, 0.4),
                caret: color(theme::TEXT).into(),
                ..InputEditorStyle::default()
            });
            state
        });
        Self { state }
    }

    pub fn clearing_on_escape(self, cx: &mut App) -> Self {
        self.state
            .update(cx, |state, _| state.set_clean_on_escape(true));
        self
    }

    pub fn text(&self, cx: &App) -> SharedString {
        self.state.read(cx).value()
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

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn focus_and_select(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
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
        let disabled = self.is_disabled(cx);
        let focused = !disabled && self.state.read(cx).focus_handle(cx).is_focused(window);
        let field = self.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(34.))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(if focused {
                theme::ACCENT
            } else {
                theme::BORDER
            }))
            .bg(color(theme::SURFACE))
            .line_height(px(20.))
            .when(disabled, |frame| frame.opacity(0.6))
            .when(!focused && !disabled, |frame| {
                frame.hover(|style| style.border_color(color(theme::BORDER_STRONG)))
            })
            .when(!disabled, |frame| {
                frame.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    field.focus(window, cx);
                })
            })
            .children(leading)
            .child(Input::new(&self.state))
            .children(trailing)
    }
}
