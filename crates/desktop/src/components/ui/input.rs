// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Text fields: `Input` (single line) and `Textarea` (multi-line), built on the unstyled gpui-kit
//! engine (`base`). The engine does the editing; the frame, focus border, password eye and clear ✕ live here.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{Button, ButtonSize, ButtonVariant};
use gpui_kit::assets::IconName;
use gpui_kit::base::input::InputBase;
use gpui_kit::base::{Input as BaseInput, Textarea as BaseTextarea};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Edges, Entity, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, StyleRefinement, Styled, Window, div, rems,
};

// The state and its events come from the engine: the app imports them from `components`.
pub use gpui_kit::base::input::{InputEvent, InputState, TextareaState};

/// Single-line text field bound to an `InputState`.
#[derive(IntoElement)]
pub struct Input {
    state: Entity<InputState>,
    style: StyleRefinement,
    prefix: Option<AnyElement>,
    cleanable: bool,
    mask_toggle: bool,
    disabled: bool,
}

impl Input {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            prefix: None,
            cleanable: false,
            mask_toggle: false,
            disabled: false,
        }
    }

    /// Something to the left of the text, for example a magnifier.
    pub fn prefix(mut self, prefix: impl IntoElement) -> Self {
        self.prefix = Some(prefix.into_any_element());
        self
    }

    /// Shows a ✕ to clear the field when it has text.
    pub fn cleanable(mut self, cleanable: bool) -> Self {
        self.cleanable = cleanable;
        self
    }

    /// Shows the eye to show or hide the text (passwords).
    pub fn mask_toggle(mut self) -> Self {
        self.mask_toggle = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Input {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = colors();
        let (state, disabled) = (self.state, self.disabled);

        state.update(cx, |s, cx| s.set_disabled(disabled, cx));

        let (focused, masked, filled) = {
            let s = state.read(cx);

            (
                s.has_selection_focus(window, cx),
                s.presentation().is_masked(),
                !s.value().is_empty(),
            )
        };

        let clean = self.cleanable && filled && !disabled;
        let suffix = self.mask_toggle || clean;

        let mut frame = InputBase::new(("input", state.entity_id()))
            .focused(focused && !disabled)
            .disabled(disabled)
            .styles(|styles| {
                styles
                    .focused(|s| s.border_color(c.ring))
                    .disabled(|s| s.opacity(0.5))
            })
            .flex()
            .items_center()
            .gap(px(6.))
            .w_full()
            .h(px(32.))
            .pl(px(10.))
            .pr(if suffix { px(4.) } else { px(10.) })
            .line_height(rems(1.25))
            .bg(c.input_background)
            .rounded(px(6.))
            .border_1()
            .border_color(c.input)
            .when(disabled, |this| {
                this.capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
            })
            .children(self.prefix)
            .child(div().flex_1().min_w_0().child(BaseInput::new(&state)))
            .when(suffix, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .when(self.mask_toggle, |this| {
                            let state = state.clone();

                            this.child(
                                Button::new("toggle-mask")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::IconSmall)
                                    .icon(if masked {
                                        IconName::Eye
                                    } else {
                                        IconName::EyeOff
                                    })
                                    .focusable(false)
                                    .on_click(move |_, window, cx| {
                                        state.update(cx, |s, cx| s.toggle_masked(window, cx))
                                    }),
                            )
                        })
                        .when(clean, |this| {
                            let state = state.clone();

                            this.child(
                                Button::new("clear-input")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::IconSmall)
                                    .icon(IconName::X)
                                    .focusable(false)
                                    .on_click(move |_, window, cx| {
                                        state.update(cx, |s, cx| {
                                            s.clean(window, cx);
                                            s.focus(window, cx);
                                        })
                                    }),
                            )
                        }),
                )
            });

        // Whatever is set with `Styled` goes last, so it wins over the defaults.
        gpui_kit::Refineable::refine(frame.style(), &self.style);

        frame
    }
}

/// Multi-line text area bound to a `TextareaState`.
#[derive(IntoElement)]
pub struct Textarea {
    state: Entity<TextareaState>,
    style: StyleRefinement,
}

impl Textarea {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Textarea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Textarea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = colors();
        let state = self.state;

        state.update(cx, |s, _| {
            s.set_editor_paddings(Edges {
                top: px(8.),
                right: px(10.),
                bottom: px(8.),
                left: px(10.),
            });
        });

        let focused = state.read(cx).has_selection_focus(window, cx);

        let mut frame = InputBase::new(("textarea", state.entity_id()))
            .focused(focused)
            .styles(|styles| styles.focused(|s| s.border_color(c.ring)))
            .flex()
            .flex_col()
            .w_full()
            .line_height(rems(1.25))
            .bg(c.background)
            .rounded(px(6.))
            .border_1()
            .border_color(c.border)
            .child(div().relative().flex_1().child(BaseTextarea::new(&state)));

        gpui_kit::Refineable::refine(frame.style(), &self.style);

        frame
    }
}

/// State of a password field (with the text hidden).
pub fn password(window: &mut Window, cx: &mut App, placeholder: &str) -> Entity<InputState> {
    let placeholder = placeholder.to_string();

    cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .placeholder(placeholder)
    })
}

/// State of a text field with its initial value.
pub fn text(
    window: &mut Window,
    cx: &mut App,
    placeholder: &str,
    value: &str,
) -> Entity<InputState> {
    let (placeholder, value) = (placeholder.to_string(), value.to_string());

    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}
