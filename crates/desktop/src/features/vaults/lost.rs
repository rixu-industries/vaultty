// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Lost-vault notice: the folder of an open vault is no longer there.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::WindowExt;
use crate::components::{Button, ButtonVariant};
use gpui_kit::{App, ParentElement, Styled, Window, div};

/// Warns that the folder of an open vault was lost: it has been locked and its terminals closed.
/// It does not detect the new location: that is set by hand from the vault list.
pub fn open_lost_alert(names: Vec<String>, window: &mut Window, cx: &mut App) {
    let message = format!(
        "{} was lost: its folder was moved, renamed or deleted. The vault has been locked and its terminals closed. You can set its new location from the vault list.",
        names.join(", ")
    );

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title("Vault not found").w(px(400.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .text_color(colors().muted_foreground)
                        .child(message.clone()),
                )
                .child(
                    div().flex().items_center().justify_end().child(
                        Button::new("ok")
                            .variant(ButtonVariant::Primary)
                            .label("Got it")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                ),
        )
    });
}
