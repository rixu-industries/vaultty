// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Detail view of a tunnel: its status, route and actions.

use crate::app::state::AppState;
use crate::app::tabs::Tab;
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::{Button, ButtonVariant, Switch, property, section_title};
use crate::features::tunnels::form::open_tunnel_form;
use crate::features::tunnels::list::delete_tunnel;
use crate::features::tunnels::{kind_name, status_color};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div,
};
use sessions::TunnelStatus;
use uuid::Uuid;
use vaults::TunnelKind;

/// A tunnel: its status, where it exits and how to use it.
pub fn tunnel_detail(state: &Entity<AppState>, vault: Uuid, id: Uuid, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);

    let Some((tunnel, host)) = s.views.get(&vault).and_then(|v| {
        let tunnel = v.tunnels.iter().find(|t| t.id == id)?.clone();
        let host = v.hosts.iter().find(|h| h.id == tunnel.host_id).cloned();
        Some((tunnel, host))
    }) else {
        return crate::layout::not_found("The tunnel").into_any_element();
    };

    let status = s.sessions.tunnel_status(id);
    let st = state.clone();

    // An IPv6 address goes in brackets; when listening on all addresses, this computer is reached through localhost.
    let local = match tunnel.bind.as_str() {
        "0.0.0.0" | "::" => "127.0.0.1".to_string(),
        bind if bind.contains(':') => format!("[{bind}]"),
        bind => bind.to_string(),
    };
    let listen = if tunnel.bind.contains(':') {
        format!("[{}]:{}", tunnel.bind, tunnel.port)
    } else {
        format!("{}:{}", tunnel.bind, tunnel.port)
    };
    let endpoint = format!("{local}:{}", tunnel.port);

    let (status_text, status_text_color) = match (&status, tunnel.enabled) {
        (_, false) => ("Off".to_string(), c.muted_foreground),
        (Some(TunnelStatus::Running), _) => (format!("Listening on {listen}"), c.success),
        (Some(TunnelStatus::Failed { error }), _) => (error.clone(), c.destructive),
        (Some(TunnelStatus::Starting) | None, _) => ("Starting…".to_string(), c.warning),
    };

    let usage = match &tunnel.kind {
        TunnelKind::Socks5 => format!("curl --socks5-hostname {endpoint} https://example.com"),
        TunnelKind::Socks4 => format!("curl --socks4a {endpoint} https://example.com"),
        TunnelKind::Http => format!("curl --proxy http://{endpoint} https://example.com"),
        TunnelKind::Forward { host, port } => {
            format!("Connect to {endpoint} to reach {host}:{port} from the host.")
        }
    };

    let mono = |text: String| {
        div()
            .font_family(mono_font())
            .text_size(px(12.))
            .truncate()
            .child(text)
    };

    let header = div()
        .flex()
        .items_center()
        .items_start()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .child(tunnel.name.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .mt(px(4.))
                        .text_size(px(12.))
                        .text_color(status_text_color)
                        .child(
                            div()
                                .flex_shrink_0()
                                .size(px(6.))
                                .rounded_full()
                                .bg(status_color(tunnel.enabled, status.as_ref())),
                        )
                        .child(div().min_w_0().child(status_text)),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(28.))
                .text_color(c.muted_foreground)
                .child("Enabled")
                .child(
                    Switch::new("tunnel-enabled", "Enabled")
                        .checked(tunnel.enabled)
                        // A broken tunnel cannot be started: it can only be turned off.
                        .disabled(tunnel.broken && !tunnel.enabled)
                        .on_click({
                            let state = st.clone();
                            move |on, _, cx| {
                                let on = *on;
                                state.update(cx, |s, cx| {
                                    s.change(vault, |v| v.set_tunnel_enabled(vault, id, on), cx);
                                })
                            }
                        }),
                ),
        )
        .child(
            Button::new("edit-tunnel")
                .icon(IconName::Pencil)
                .label("Edit")
                .on_click({
                    let state = st.clone();
                    move |_, window, cx| {
                        open_tunnel_form(state.clone(), vault, Some(id), window, cx)
                    }
                }),
        );

    let through = match &host {
        Some(h) => {
            let method = h
                .methods
                .iter()
                .find(|m| m.id == tunnel.method_id)
                .map_or("method removed".to_string(), |m| m.title.clone());
            let (state, host_id) = (st.clone(), h.id);

            div()
                .id("tunnel-host")
                .cursor_pointer()
                .text_color(c.primary)
                .hover(|s| s.underline())
                .child(format!("{} · {method}", h.name))
                .on_click(move |_, _, cx| {
                    state.update(cx, |s, cx| s.preview(Tab::Host { vault, id: host_id }, cx))
                })
                .into_any_element()
        }
        None => div()
            .text_color(c.destructive)
            .child("The host no longer exists")
            .into_any_element(),
    };

    div()
        .flex()
        .flex_col()
        .id("tunnel-detail")
        .size_full()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .flex_col()
                .max_w(px(720.))
                .px(px(32.))
                .py(px(24.))
                .child(header)
                .child(section_title("Tunnel"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(property("Type", div().child(kind_name(&tunnel.kind))))
                        .child(property("Listen on", mono(listen.clone())))
                        .when_some(
                            match &tunnel.kind {
                                TunnelKind::Forward { host, port } => Some(format!("{host}:{port}")),
                                _ => None,
                            },
                            |this, to| this.child(property("Destination", mono(to))),
                        )
                        .child(property("Through", through)),
                )
                .child(section_title("Usage"))
                .child(
                    div()
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(4.))
                        .bg(c.card)
                        .border_1()
                        .border_color(c.border)
                        .child(mono(usage)),
                )
                .when(tunnel.bind != "127.0.0.1" && tunnel.bind != "::1", |this| {
                    this.child(
                        div()
                            .mt(px(8.))
                            .text_size(px(12.))
                            .text_color(c.warning)
                            .child("It listens beyond this computer: anyone who can reach it on the network can use the tunnel."),
                    )
                })
                .child(section_title("Danger zone"))
                .child(
                    div().flex().items_center().ml(px(-8.)).child(
                        Button::new("delete-tunnel")
                            .variant(ButtonVariant::Destructive)
                            .icon(IconName::Trash)
                            .label("Delete tunnel")
                            .on_click({
                                let state = st.clone();
                                move |_, window, cx| {
                                    delete_tunnel(state.clone(), vault, id, window, cx)
                                }
                            }),
                    ),
                ),
        )
        .into_any_element()
}
