// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Label with the access method type: key in green, password in orange.

use crate::app::theme::colors;
use crate::app::theme::px;
use gpui_kit::{IntoElement, ParentElement, Styled, div};

pub fn method_badge(password: bool) -> impl IntoElement {
    let c = colors();
    let (text, color) = if password {
        ("PASSWORD", c.warning)
    } else {
        ("KEY", c.success)
    };

    div()
        .px(px(6.))
        .rounded(px(4.))
        .border_1()
        .border_color(color)
        .text_color(color)
        .text_size(px(11.))
        .line_height(px(16.))
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .child(text)
}
