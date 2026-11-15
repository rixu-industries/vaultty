// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Section title in small uppercase text with a line underneath, like Zed's.

use crate::app::theme::colors;
use crate::app::theme::px;
use gpui_kit::{IntoElement, ParentElement, SharedString, Styled, div};

pub fn section_title(text: impl Into<SharedString>) -> impl IntoElement {
    let c = colors();

    div()
        .mt(px(28.))
        .mb(px(8.))
        .pb(px(6.))
        .border_b_1()
        .border_color(c.border)
        .text_size(px(11.))
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .text_color(c.muted_foreground)
        .child(text.into().to_uppercase())
}
