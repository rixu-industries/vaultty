// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Notice bar above a tab while there is no session.

use crate::app::state::AppState;
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::{Button, ButtonSize, ButtonVariant};
use gpui_kit::{AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, div};
use sessions::{Status, TabInfo};

/// Bar above the tab (terminal or SFTP) while there is no session: connecting, host key to confirm,
/// error, or closed.
pub fn notice(state: &Entity<AppState>, tab: &TabInfo, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);
    let id = tab.id;

    let host = s
        .views
        .get(&tab.vault)
        .and_then(|v| v.hosts.iter().find(|h| h.id == tab.host));
    let method = host.and_then(|h| h.methods.iter().find(|m| m.id == tab.method));
    let address = match (host, method) {
        (Some(h), Some(m)) => format!("{}@{}:{}", m.username, h.address, h.port),
        _ => String::new(),
    };

    let bar = || {
        div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .gap(px(12.))
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(c.border)
            .bg(c.card)
    };

    let retry = |label: &'static str| {
        let state = state.clone();
        Button::new(SharedString::from(format!("retry-{id}")))
            .size(ButtonSize::Small)
            .label(label)
            .on_click(move |_, _, cx| state.update(cx, |s, _| s.reconnect(id)))
    };

    match &tab.status {
        Status::Connecting => bar()
            .text_color(c.muted_foreground)
            .child(format!("Connecting to {address}…"))
            .into_any_element(),
        Status::UnknownHostKey { fingerprint } => {
            let (state, fingerprint) = (state.clone(), fingerprint.clone());

            bar()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(format!(
                            "First connection to {address}. Check that this fingerprint matches the server's:"
                        ))
                        .child(
                            div()
                                .font_family(mono_font())
                                .text_color(c.muted_foreground)
                                .truncate()
                                .child(fingerprint.clone()),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("trust-{id}")))
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Small)
                        .label("Trust and connect")
                        .on_click(move |_, _, cx| {
                            state.update(cx, |s, cx| s.trust_host_key(id, fingerprint.clone(), cx))
                        }),
                )
                .into_any_element()
        }
        Status::Failed { error } => bar()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(c.destructive)
                    .child(error.clone()),
            )
            .child(retry("Retry"))
            .into_any_element(),
        Status::Closed => bar()
            .text_color(c.muted_foreground)
            .child(div().flex_1().child("Session closed."))
            .child(retry("Reconnect"))
            .into_any_element(),
        Status::Connected => div().into_any_element(),
    }
}
