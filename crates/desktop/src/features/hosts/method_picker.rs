// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Access method picker for connecting to a host that has several.

use crate::app::state::AppState;
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::WindowExt;
use crate::components::method_badge;
use gpui_kit::{
    App, Entity, InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Window, div,
};
use sessions::TabKind;

/// When connecting to a host with several access methods, asks which one to use.
pub fn open_method_picker(state: Entity<AppState>, window: &mut Window, cx: &mut App) {
    let Some(picking) = state.read(cx).picking else {
        return;
    };

    let Some(host) = state
        .read(cx)
        .views
        .get(&picking.vault)
        .and_then(|v| v.hosts.iter().find(|h| h.id == picking.host))
        .cloned()
    else {
        return;
    };

    let title = format!(
        "{} {}",
        if picking.kind == TabKind::Sftp {
            "SFTP to"
        } else {
            "Connect to"
        },
        host.name
    );

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        let c = colors();
        let mut list = div().flex().flex_col().py(px(4.));

        for (n, method) in host.methods.iter().enumerate() {
            let state = state.clone();
            let m = method.id;

            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .id(SharedString::from(format!("pick-{m}")))
                    .cursor_pointer()
                    .gap(px(12.))
                    .px(px(8.))
                    .h(px(32.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(c.list_hover))
                    .child(method_badge(method.password))
                    .child(method.title.clone())
                    .child(
                        div()
                            .truncate()
                            .text_color(c.muted_foreground)
                            .child(method.detail.clone()),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .font_family(mono_font())
                            .text_size(px(11.))
                            .text_color(c.muted_foreground)
                            .child((n + 1).to_string()),
                    )
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        state.update(cx, |s, cx| {
                            s.open_tab(picking.vault, picking.host, m, picking.kind, cx)
                        });
                    }),
            );
        }

        let state = state.clone();

        dialog
            .title(title.clone())
            .w(px(460.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c.muted_foreground)
                    .child("Choose an access method"),
            )
            .child(list)
            .on_close(move |_, _, cx| state.update(cx, |s, _| s.picking = None))
    });
}
