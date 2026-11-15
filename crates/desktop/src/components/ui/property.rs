// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Name/value pair of a details card (address, port, fingerprint…).

use crate::app::theme::colors;
use crate::app::theme::px;
use gpui_kit::{IntoElement, ParentElement, Styled, div};

pub fn property(name: &str, value: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .min_h(px(22.))
        .child(
            div()
                .w(px(120.))
                .flex_shrink_0()
                .text_color(colors().muted_foreground)
                .child(name.to_string()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .flex_1()
                .min_w_0()
                .gap(px(6.))
                .child(value),
        )
}
