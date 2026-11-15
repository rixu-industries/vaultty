// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Centered empty state: icon, a text and an action.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::icon;
use gpui_kit::assets::IconName;
use gpui_kit::{AnyElement, IntoElement, ParentElement, SharedString, Styled, div};

pub fn empty_state(
    name: IconName,
    text: impl Into<SharedString>,
    action: Option<AnyElement>,
) -> impl IntoElement {
    let c = colors();

    div()
        .flex()
        .flex_col()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .text_color(c.muted_foreground)
        .child(icon(name, 28., c.muted_icon))
        .child(div().max_w(px(320.)).text_center().child(text.into()))
        .children(action)
}
