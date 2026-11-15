// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Form to create or edit a tunnel.

use crate::app::state::{AppState, Tab};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::WindowExt;
use crate::components::{Input, InputEvent, InputState};
use crate::components::{choice, field, footer, note, text};
use crate::features::tunnels::kind_name;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use uuid::Uuid;
use vaults::{TunnelInput, TunnelKind};

/// The types, in dropdown order. The port forward's destination is set separately, in the form.
const KINDS: [TunnelKind; 4] = [
    TunnelKind::Socks5,
    TunnelKind::Socks4,
    TunnelKind::Http,
    TunnelKind::Forward {
        host: String::new(),
        port: 0,
    },
];

/// Create (`id: None`) or edit a tunnel.
pub struct TunnelForm {
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    name: Entity<InputState>,
    /// Position in `KINDS`.
    kind: usize,
    bind: Entity<InputState>,
    port: Entity<InputState>,
    host: Option<Uuid>,
    method: Option<Uuid>,
    /// Destination of the port forward.
    to_host: Entity<InputState>,
    to_port: Entity<InputState>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

pub fn open_tunnel_form(
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| TunnelForm::new(state, vault, id, window, cx));
    let title = if id.is_some() {
        "Edit tunnel"
    } else {
        "New tunnel"
    };

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title(title).w(px(480.)).child(view.clone())
    });
}

impl TunnelForm {
    fn new(
        state: Entity<AppState>,
        vault: Uuid,
        id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let existing = id.and_then(|id| {
            state
                .read(cx)
                .views
                .get(&vault)
                .and_then(|v| v.tunnels.iter().find(|t| t.id == id))
                .cloned()
        });

        let e = existing.as_ref();
        let (to_host, to_port) = match e.map(|t| &t.kind) {
            Some(TunnelKind::Forward { host, port }) => (host.clone(), port.to_string()),
            _ => (String::new(), String::new()),
        };

        let name = text(window, cx, "work-proxy", e.map_or("", |t| t.name.as_str()));
        let bind = text(window, cx, "", e.map_or("127.0.0.1", |t| t.bind.as_str()));
        let port = text(
            window,
            cx,
            "",
            &e.map_or("1080".to_string(), |t| t.port.to_string()),
        );
        let to_host = text(window, cx, "localhost or db.internal", &to_host);
        let to_port = text(window, cx, "5432", &to_port);

        name.update(cx, |i, cx| i.focus(window, cx));

        let enter = |this: &mut Self,
                     _: &Entity<InputState>,
                     e: &InputEvent,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            if let InputEvent::PressEnter { .. } = e {
                this.save(window, cx);
            }
        };

        let subs = [&name, &bind, &port, &to_host, &to_port]
            .into_iter()
            .map(|input| cx.subscribe_in(input, window, enter))
            .collect();

        let kind = e.map_or(0, |t| {
            KINDS
                .iter()
                .position(|k| std::mem::discriminant(k) == std::mem::discriminant(&t.kind))
                .unwrap_or(0)
        });

        Self {
            host: e.map(|t| t.host_id),
            method: e.map(|t| t.method_id),
            state,
            vault,
            id,
            name,
            kind,
            bind,
            port,
            to_host,
            to_port,
            error: None,
            _subs: subs,
        }
    }

    fn fail(&mut self, error: &str, cx: &mut Context<Self>) {
        self.error = Some(error.into());
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let read = |input: &Entity<InputState>, cx: &App| input.read(cx).value().trim().to_string();

        let Some(port) = parse_port(&read(&self.port, cx)) else {
            return self.fail("The port must be a number between 1 and 65535.", cx);
        };

        let (Some(host_id), Some(method_id)) = (self.host, self.method) else {
            return self.fail("Choose the host and access method to go through.", cx);
        };

        let kind = match &KINDS[self.kind] {
            TunnelKind::Forward { .. } => {
                let Some(to_port) = parse_port(&read(&self.to_port, cx)) else {
                    return self.fail(
                        "The destination port must be a number between 1 and 65535.",
                        cx,
                    );
                };

                TunnelKind::Forward {
                    host: read(&self.to_host, cx),
                    port: to_port,
                }
            }
            kind => kind.clone(),
        };

        let input = TunnelInput {
            name: read(&self.name, cx),
            kind,
            bind: read(&self.bind, cx),
            port,
            host_id,
            method_id,
        };

        let (vault, id) = (self.vault, self.id);
        let saved = self.state.update(cx, |s, cx| {
            s.change(vault, |v| v.save_tunnel(vault, id, input), cx)
        });

        // Errors from `vaults` (duplicate port, malformed address…) are shown here, not below.
        let Some(saved) = saved else {
            let error = self.state.read(cx).error.clone().unwrap_or_default();
            return self.fail(&error, cx);
        };

        window.close_dialog(cx);

        if id.is_none() {
            self.state.update(cx, |s, cx| {
                s.open_pinned(Tab::Tunnel { vault, id: saved }, cx)
            });
        }
    }
}

fn parse_port(text: &str) -> Option<u16> {
    text.parse::<u16>()
        .ok()
        .filter(|p| *p != 0 && text.chars().all(|c| c.is_ascii_digit()))
}

impl Render for TunnelForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let this = cx.entity();
        let s = self.state.read(cx);

        let mut hosts: Vec<_> = s
            .views
            .get(&self.vault)
            .map(|v| v.hosts.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|h| !h.methods.is_empty())
            .collect();
        hosts.sort_by_key(|h| h.name.to_lowercase());

        let methods: Vec<(Uuid, String)> = hosts
            .iter()
            .find(|h| Some(h.id) == self.host)
            .map(|h| {
                h.methods
                    .iter()
                    .map(|m| (m.id, format!("{} · {}", m.title, m.detail)))
                    .collect()
            })
            .unwrap_or_default();

        let host_options: Vec<(Uuid, String)> = hosts
            .iter()
            .map(|h| (h.id, format!("{} ({})", h.name, h.address)))
            .collect();

        let kinds: Vec<(usize, String)> = KINDS
            .iter()
            .enumerate()
            .map(|(i, k)| (i, kind_name(k).to_string()))
            .collect();

        let forward = matches!(KINDS[self.kind], TunnelKind::Forward { .. });

        let hint = match KINDS[self.kind] {
            TunnelKind::Socks5 => {
                "Set it as the SOCKS5 proxy of your browser or app; names are resolved on the host."
            }
            TunnelKind::Socks4 => "For old clients that only speak SOCKS4 or 4a.",
            TunnelKind::Http => "Set it as the HTTP and HTTPS proxy of your browser or app.",
            TunnelKind::Forward { .. } => {
                "Everything that reaches the local port goes to this destination, as seen from the host."
            }
        };

        // A method that is no longer on the chosen host (or none): the first one is picked.
        if !methods.iter().any(|(id, _)| Some(*id) == self.method) {
            self.method = methods.first().map(|(id, _)| *id);
        }

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(field("Name", Input::new(&self.name)))
            .child(field(
                "Type",
                choice("kind", kinds, Some(self.kind), {
                    let this = this.clone();
                    move |kind, _, cx| {
                        this.update(cx, |this, cx| {
                            this.kind = kind;
                            cx.notify();
                        })
                    }
                }),
            ))
            .child(note(hint, c.muted_foreground))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(field("Listen address", Input::new(&self.bind)).flex_1())
                    .child(field("Port", Input::new(&self.port)).w(px(96.))),
            )
            .when(forward, |this| {
                this.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(field("Destination", Input::new(&self.to_host)).flex_1())
                        .child(field("Port", Input::new(&self.to_port)).w(px(96.))),
                )
            })
            .child(field(
                "Through host",
                choice("host", host_options, self.host, {
                    let this = this.clone();
                    move |id, _, cx| {
                        this.update(cx, |this, cx| {
                            this.host = Some(id);
                            cx.notify();
                        })
                    }
                }),
            ))
            // With only one method there is nothing to choose.
            .when(methods.len() > 1, |el| {
                el.child(field(
                    "Access method",
                    choice("method", methods, self.method, {
                        let this = this.clone();
                        move |id, _, cx| {
                            this.update(cx, |this, cx| {
                                this.method = Some(id);
                                cx.notify();
                            })
                        }
                    }),
                ))
            })
            .when_some(self.error.clone(), |this, e| {
                this.child(note(e, c.destructive))
            })
            .child(footer("Save", false, move |_, window, cx| {
                this.update(cx, |this, cx| this.save(window, cx))
            }))
    }
}
