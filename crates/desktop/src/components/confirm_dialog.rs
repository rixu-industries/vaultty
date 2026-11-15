// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Confirmation dialog for actions that cannot be undone.

use crate::app::theme::colors;
use crate::components::Button;
use crate::components::ButtonVariant;
use crate::components::open_dialog;

use crate::app::theme::px;
use gpui_kit::component::WindowExt;
use gpui_kit::{App, ParentElement, SharedString, Styled, Window, div};

/// `confirm` is the text of the red button.
pub fn confirm(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    confirm: &str,
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    let (title, message, confirm) = (
        title.into(),
        message.into(),
        SharedString::from(confirm.to_string()),
    );
    let on_confirm = std::rc::Rc::new(on_confirm);

    open_dialog(window, cx, move |dialog, _, _| {
        let on_confirm = on_confirm.clone();

        dialog.title(title.clone()).w(px(400.)).child(
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
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm")
                                .variant(ButtonVariant::Danger)
                                .label(confirm.clone())
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    on_confirm(window, cx);
                                }),
                        ),
                ),
        )
    });
}
