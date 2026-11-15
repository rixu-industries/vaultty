// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Form field: label on top, the control, and below it a hint or an error.

use crate::app::theme::colors;
use crate::app::theme::px;
use gpui_kit::{Hsla, IntoElement, ParentElement, SharedString, Styled, div};

/// Label of a form field.
pub fn label(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .text_color(colors().muted_foreground)
        .child(text.into())
}

/// Form field: label on top and the control below.
pub fn field(name: impl Into<SharedString>, control: impl IntoElement) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(label(name))
        .child(control)
}

/// Small text under a field, in the given color.
pub fn note(text: impl Into<SharedString>, color: Hsla) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .text_color(color)
        .child(text.into())
}
