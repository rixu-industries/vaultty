// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Dialogs: open one with the app background and a footer with Cancel and the main action. The host (layers,
//! focus, Escape) is gpui-kit's; `base` doesn't stack dialogs.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{Button, ButtonVariant};
use gpui_kit::component::dialog::{Dialog as KitDialog, DialogEntrance};
use gpui_kit::{
    App, ClickEvent, IntoElement, ParentElement, Pixels, SharedString, Styled, Window, div,
};

/// Side margin of the gpui-kit dialog; the divider makes up for it to reach the edges.
const SIDE: f32 = 16.;

/// The gpui-kit dialog with the app's look. Only exposes what is used; the rest is added here
/// when needed.
pub struct Dialog(KitDialog, bool);

impl Dialog {
    /// Header title, at the height of the close cross (gpui-kit's, 8px from the edge).
    pub fn title(self, title: impl Into<SharedString>) -> Self {
        Self(
            self.0.title(
                div()
                    .flex()
                    .items_center()
                    .h(px(28.))
                    .mt(px(8.))
                    // Leaves room for the cross on the right.
                    .pr(px(28.))
                    .text_size(px(13.))
                    .child(div().truncate().child(title.into())),
            ),
            self.1,
        )
    }

    pub fn w(self, width: impl Into<Pixels>) -> Self {
        Self(self.0.w(width), self.1)
    }

    /// The first child carries the header divider on top, full width. It goes here and not under the title
    /// so it matches the body's edge: whatever scrolls is cut right at the line (see
    /// `vaults/settings.rs`, which cancels the padding with negative margins).
    pub fn child(self, child: impl IntoElement) -> Self {
        let child = if self.1 {
            child.into_any_element()
        } else {
            div()
                .mx(px(-SIDE))
                .px(px(SIDE))
                .pt(px(SIDE))
                .border_t_1()
                .border_color(colors().border)
                .child(child)
                .into_any_element()
        };

        Self(self.0.child(child), true)
    }

    pub fn on_close(self, on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self(self.0.on_close(on_close), self.1)
    }
}

/// Opens a dialog with the app background (gpui-kit's is darker).
pub fn open_dialog(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        // No top padding: the header sets its own, so its divider reaches the edge.
        let dialog = dialog
            .bg(colors().popover)
            .pt(px(0.))
            // Fades in with a small slide down; the stock one comes down from the window edge.
            .entrance(DialogEntrance::FadeSlide);

        build(Dialog(dialog, false), window, cx).0
    });
}

/// Dialog footer: Cancel and the main action.
pub fn footer(
    ok: impl Into<SharedString>,
    busy: bool,
    on_ok: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let ok = ok.into();

    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .pt(px(12.))
        .child(
            Button::new("cancel")
                .label("Cancel")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            Button::new("ok")
                .variant(ButtonVariant::Primary)
                .label(ok)
                .disabled(busy)
                .on_click(on_ok),
        )
}

// Closing the top dialog (`window.close_dialog(cx)`) comes from gpui-kit; the app imports it from here.
pub use gpui_kit::component::WindowExt;
