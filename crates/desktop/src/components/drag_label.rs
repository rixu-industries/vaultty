// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! What is shown under the mouse while dragging a list item.

use crate::app::theme::colors;
use crate::app::theme::px;
use gpui_kit::{
    Context, CursorStyle, IntoElement, ParentElement, Pixels, Point, Render, Styled, Window,
    canvas, div,
};

pub struct DragLabel {
    text: String,
    /// Set by `on_drag`: where the element was grabbed, relative to its corner.
    cursor_offset: Point<Pixels>,
}

impl DragLabel {
    pub fn new(text: String, cursor_offset: Point<Pixels>) -> Self {
        Self {
            text,
            cursor_offset,
        }
    }
}

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let c = colors();

        // GPUI paints the view at `mouse - cursor_offset`: this zero-size point lands right on the
        // mouse and centers the bubble over it without having to measure it.
        div()
            .relative()
            .left(self.cursor_offset.x)
            .top(self.cursor_offset.y)
            .size_0()
            .flex()
            .items_center()
            .justify_center()
            // GPUI leaves the cursor of the source element; the drag view is painted last,
            // so this one wins.
            .child(canvas(
                |_, _, _| {},
                |_, _, window, _| window.set_window_cursor_style(CursorStyle::ClosedHand),
            ))
            .child(
                div()
                    .flex_none()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c.border)
                    .bg(c.card)
                    .text_color(c.foreground)
                    .text_size(px(13.))
                    .whitespace_nowrap()
                    .child(self.text.clone()),
            )
    }
}
