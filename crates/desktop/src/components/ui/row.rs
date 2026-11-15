// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! List or tree row: icon, text and a detail on the right.

use crate::app::theme::colors;
use crate::app::theme::px;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{AnyElement, InteractiveElement, ParentElement, Styled, div};

/// `depth` is the indent level.
pub fn tree_row(
    id: impl Into<gpui_kit::ElementId>,
    depth: usize,
    leading: AnyElement,
    label: String,
    detail: Option<String>,
    selected: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors();

    div()
        .flex()
        .items_center()
        .id(id)
        .cursor_pointer()
        .group("row")
        .relative()
        .h(px(26.))
        .gap(px(6.))
        .pr(px(12.))
        .pl(px(8. + depth as f32 * 16.))
        .when(selected, |this| {
            this.bg(c.list_active).text_color(c.list_active_foreground)
        })
        .when(!selected, |this| {
            this.text_color(c.list_foreground)
                .hover(|s| s.bg(c.list_hover))
        })
        // Selection mark on the left, as in Zed's project panel.
        .when(selected, |this| {
            this.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(2.))
                    .bg(c.list_active_indicator),
            )
        })
        .child(leading)
        .child(div().truncate().child(label))
        .child(
            div()
                .ml_auto()
                .text_size(px(11.))
                .text_color(c.muted_foreground)
                .truncate()
                .children(detail),
        )
}
