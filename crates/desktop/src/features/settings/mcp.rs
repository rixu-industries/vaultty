// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! "MCP server" category of the settings. The server and its settings live in the `mcp` crate; this view
//! shows them, saves every change to `settings.json` and owns the running server. The view lasts as long
//! as the app, so the server shuts down when the app closes.
//!
//! A vault's permissions are the defaults for its hosts; per-host exceptions are stored in the host itself
//! (encrypted in the vault), so they can only be seen and changed while the vault is unlocked.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::state::AppState;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{
    Button, ButtonSize, ButtonVariant, Checkbox, DropdownMenu, Input, InputEvent, InputState,
    PopupMenuItem, Switch, WindowExt, choice, confirm, field, footer, note, open_dialog,
    section_title, text,
};
use crate::features::settings::{header, setting};
use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Window, div,
};
use mcp::{Exec, ExecRequest, McpPerms, VaultAccess};
use uuid::Uuid;

/// The checkboxes in the permissions dialog. Execution is separate, with its own selector.
const CHECKS: [(&str, &str); 2] = [
    ("Read", "View hosts, groups and public keys."),
    ("Write", "Create, edit and delete hosts and groups."),
];

const EXEC: [(Exec, &str); 3] = [
    (Exec::No, "No"),
    (Exec::Ask, "Ask every time"),
    (Exec::Direct, "Direct, without asking"),
];

/// Summary for the table: "Read · Write · Execute (ask)".
fn summary(p: McpPerms) -> String {
    let mut names = Vec::new();

    if p.read {
        names.push("Read");
    }

    if p.write {
        names.push("Write");
    }

    match p.exec {
        Exec::No => {}
        Exec::Ask => names.push("Execute (ask)"),
        Exec::Direct => names.push("Execute directly"),
    }

    if names.is_empty() {
        "No permissions".into()
    } else {
        names.join(" · ")
    }
}

/// Ports below 1024 require privileges on Linux and macOS.
fn port_of(text: &str) -> Option<u16> {
    text.trim().parse().ok().filter(|p| *p >= 1024)
}

pub struct McpSettings {
    state: Entity<AppState>,
    /// What is saved in `settings.json`. Host and port are copied from the fields when the server is turned on.
    settings: mcp::Settings,
    /// `Some` while the server is running.
    server: Option<mcp::Server>,
    /// Read from the system credential store the first time it is needed.
    token: Option<String>,
    /// Error when starting, saving or reading the token.
    error: Option<String>,
    /// Commands on hosts set to "Ask every time" request confirmation through here.
    ask: mcp::Ask,
    host: Entity<InputState>,
    port: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl McpSettings {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = mcp::load();
        let host = text(window, cx, "localhost", &settings.host);
        let port = text(window, cx, "47610", &settings.port.to_string());

        // Field error messages are recalculated as the user types.
        let on_change =
            |_: &mut Self, _: Entity<InputState>, e: &InputEvent, cx: &mut Context<Self>| {
                if let InputEvent::Change = e {
                    cx.notify();
                }
            };

        let subs = vec![
            cx.subscribe(&host, on_change),
            cx.subscribe(&port, on_change),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];

        // Requests arrive from tokio; the dialog is opened here, on the UI thread.
        let (requests, mut received) = mpsc::unbounded::<(ExecRequest, oneshot::Sender<bool>)>();

        cx.spawn_in(window, async move |this, cx| {
            while let Some((request, answer)) = received.next().await {
                let _ = this.update_in(cx, |_, window, cx| ask_exec(request, answer, window, cx));
            }
        })
        .detach();

        let ask: mcp::Ask = Arc::new(move |request| {
            let (answer, reply) = oneshot::channel();
            let _ = requests.unbounded_send((request, answer));

            // If the dialog is closed without answering, the sender is dropped and counts as no.
            Box::pin(async move { reply.await.unwrap_or(false) })
        });

        let mut this = Self {
            state,
            settings,
            server: None,
            token: None,
            error: None,
            ask,
            host,
            port,
            _subs: subs,
        };

        // If it was left on, it starts again with the app.
        if this.settings.enabled {
            this.start(cx);
        }

        this
    }

    fn token(&mut self) -> Result<String, String> {
        if let Some(token) = &self.token {
            return Ok(token.clone());
        }

        let token = mcp::token()?;
        self.token = Some(token.clone());

        Ok(token)
    }

    /// Starts with what is in `settings`. If it fails, it stays off and the error is shown.
    fn start(&mut self, cx: &mut Context<Self>) {
        let sessions = self.state.read(cx).sessions.clone();

        let started = self.token().and_then(|token| {
            mcp::start(&self.settings, token, sessions.clone(), self.ask.clone())
        });

        match started {
            Ok((server, run)) => {
                sessions.spawn(run);
                self.server = Some(server);
                self.error = None;
            }
            Err(e) => {
                self.server = None;
                self.error = Some(e);
            }
        }

        cx.notify();
    }

    fn set_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        self.server = None;

        if on {
            self.settings.host = self.host.read(cx).value().trim().to_string();
            self.settings.port = port_of(&self.port.read(cx).value()).unwrap_or(self.settings.port);
            self.start(cx);
        } else {
            self.error = None;
        }

        self.settings.enabled = self.server.is_some();
        self.save(cx);
    }

    /// Saves to disk and passes the vaults to the server if it is running.
    fn save(&mut self, cx: &mut Context<Self>) {
        if let Some(server) = &self.server {
            server.set_vaults(self.settings.vaults.clone());
        }

        if let Err(e) = mcp::save(&self.settings) {
            self.error = Some(e);
        }

        cx.notify();
    }

    /// The old token stops being valid: if the server is running, it restarts with the new one.
    fn regenerate_token(&mut self, cx: &mut Context<Self>) {
        match mcp::new_token() {
            Ok(token) => self.token = Some(token),
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        }

        if self.server.take().is_some() {
            self.start(cx);
        }

        cx.notify();
    }

    fn copy_token(&mut self, cx: &mut Context<Self>) {
        match self.token() {
            Ok(token) => cx.write_to_clipboard(ClipboardItem::new_string(token)),
            Err(e) => self.error = Some(e),
        }

        cx.notify();
    }

    /// By default, a new vault gets read-only access.
    fn add(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        self.settings.vaults.push(VaultAccess {
            id: vault,
            perms: McpPerms {
                read: true,
                ..McpPerms::default()
            },
        });
        self.save(cx);
    }

    fn remove(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        self.settings.vaults.retain(|v| v.id != vault);
        self.save(cx);
    }

    fn set_perms(&mut self, vault: Uuid, perms: McpPerms, cx: &mut Context<Self>) {
        if let Some(entry) = self.settings.vaults.iter_mut().find(|v| v.id == vault) {
            entry.perms = perms;
        }

        self.save(cx);
    }

    /// `None` removes the exception: the host goes back to inheriting the vault's permissions.
    fn set_host_perms(
        &mut self,
        vault: Uuid,
        host: Uuid,
        perms: Option<McpPerms>,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |s, cx| {
            let saved = s.vaults.edit_host(vault, host, |h| h.mcp = perms);

            if s.report(saved, cx).is_some() {
                s.refresh_view(vault, cx);
            }
        });
    }

    fn perms_of(&self, vault: Uuid) -> McpPerms {
        self.settings
            .vaults
            .iter()
            .find(|v| v.id == vault)
            .map(|v| v.perms)
            .unwrap_or_default()
    }

    /// Table of the vaults with access: a header; for each vault, its row and the rows of its hosts
    /// that have their own permissions; and "Add vault".
    fn vaults_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors();
        let this = cx.entity();
        let state = self.state.read(cx);
        let list = state.list.clone();

        // A vault that was removed from the app stops being shown.
        let rows: Vec<_> = self
            .settings
            .vaults
            .iter()
            .filter_map(|a| Some((list.iter().find(|v| v.id == a.id)?.clone(), a.perms)))
            .collect();

        let header = |label: &'static str| {
            div()
                .text_size(px(11.))
                .text_color(c.muted_foreground)
                .child(label.to_uppercase())
        };

        let mut table = div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(c.border)
            .rounded(px(6.))
            .bg(c.card)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(28.))
                    .px(px(12.))
                    .bg(c.secondary)
                    .child(div().flex_1().child(header("Vault")))
                    .child(div().w(px(220.)).child(header("Permissions")))
                    .child(div().w(px(26.))),
            );

        if rows.is_empty() {
            table = table.child(
                div()
                    .px(px(12.))
                    .py(px(20.))
                    .text_center()
                    .text_color(c.muted_foreground)
                    .child("MCP has no access to any vault."),
            );
        }

        for (vault, perms) in rows {
            let id = vault.id;
            let (edit, except, remove) = (this.clone(), this.clone(), this.clone());
            let name = vault.name.clone();
            let unlocked = vault.unlocked;

            table = table.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(44.))
                    .px(px(12.))
                    .border_t_1()
                    .border_color(c.border)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().child(vault.name.clone()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c.muted_foreground)
                                    .truncate()
                                    .child(vault.path.clone()),
                            ),
                    )
                    .child(div().w(px(220.)).truncate().child(summary(perms)))
                    .child(
                        Button::new(SharedString::from(format!("mcp-vault-{id}")))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon(IconName::Ellipsis)
                            .dropdown_menu_with_anchor(
                                gpui_kit::Anchor::TopRight,
                                move |menu, _, _| {
                                    let (edit, except, remove, name) = (
                                        edit.clone(),
                                        except.clone(),
                                        remove.clone(),
                                        name.clone(),
                                    );

                                    menu.min_w(px(220.))
                                        .item(
                                            PopupMenuItem::new("Edit permissions…")
                                                .icon(IconName::Pencil)
                                                .on_click(move |_, window, cx| {
                                                    let perms = edit.read(cx).perms_of(id);
                                                    let title = format!("Permissions for {name}");

                                                    open_perms(
                                                        edit.clone(),
                                                        id,
                                                        None,
                                                        Vec::new(),
                                                        perms,
                                                        title,
                                                        window,
                                                        cx,
                                                    )
                                                }),
                                        )
                                        .item(
                                            PopupMenuItem::new("Add host exception…")
                                                .icon(IconName::Plus)
                                                // Host permissions live inside the vault.
                                                .disabled(!unlocked)
                                                .on_click(move |_, window, cx| {
                                                    add_exception(except.clone(), id, window, cx)
                                                }),
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new("Remove access")
                                                .icon(IconName::Trash)
                                                .on_click(move |_, _, cx| {
                                                    remove.update(cx, |t, cx| t.remove(id, cx))
                                                }),
                                        )
                                },
                            ),
                    ),
            );

            // Hosts with their own permissions, under their vault.
            let exceptions: Vec<_> = state
                .views
                .get(&id)
                .map(|view| {
                    view.hosts
                        .iter()
                        .filter_map(|h| Some((h.id, h.name.clone(), h.mcp?)))
                        .collect()
                })
                .unwrap_or_default();

            if !unlocked {
                table = table.child(
                    div()
                        .pl(px(28.))
                        .pr(px(12.))
                        .py(px(6.))
                        .text_size(px(12.))
                        .text_color(c.muted_foreground)
                        .child("Unlock the vault to view or change its per-host exceptions."),
                );
            }

            for (host, host_name, host_perms) in exceptions {
                let (edit, clear) = (this.clone(), this.clone());

                table = table.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(34.))
                        .pl(px(28.))
                        .pr(px(12.))
                        .text_size(px(13.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(format!("↳ {host_name}")),
                        )
                        .child(div().w(px(220.)).truncate().child(summary(host_perms)))
                        .child(
                            Button::new(SharedString::from(format!("mcp-host-{host}")))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Icon)
                                .icon(IconName::Ellipsis)
                                .dropdown_menu_with_anchor(
                                    gpui_kit::Anchor::TopRight,
                                    move |menu, _, _| {
                                        let (edit, clear, host_name) =
                                            (edit.clone(), clear.clone(), host_name.clone());

                                        menu.min_w(px(200.))
                                            .item(
                                                PopupMenuItem::new("Edit permissions…")
                                                    .icon(IconName::Pencil)
                                                    .on_click(move |_, window, cx| {
                                                        open_perms(
                                                            edit.clone(),
                                                            id,
                                                            Some(host),
                                                            Vec::new(),
                                                            host_perms,
                                                            format!("Permissions for {host_name}"),
                                                            window,
                                                            cx,
                                                        )
                                                    }),
                                            )
                                            .separator()
                                            .item(
                                                PopupMenuItem::new("Remove exception")
                                                    .icon(IconName::Trash)
                                                    .on_click(move |_, _, cx| {
                                                        clear.update(cx, |t, cx| {
                                                            t.set_host_perms(id, host, None, cx)
                                                        })
                                                    }),
                                            )
                                    },
                                ),
                        ),
                );
            }
        }

        let state = self.state.clone();
        let add = Button::new("mcp-add-vault")
            .variant(ButtonVariant::Outline)
            .size(ButtonSize::Small)
            .icon(IconName::Plus)
            .label("Add vault")
            .dropdown_menu(move |menu, _, cx| {
                let free: Vec<_> = state
                    .read(cx)
                    .list
                    .iter()
                    .filter(|v| {
                        !v.missing && !this.read(cx).settings.vaults.iter().any(|a| a.id == v.id)
                    })
                    .map(|v| (v.id, v.name.clone()))
                    .collect();

                let menu = menu.min_w(px(220.));

                if free.is_empty() {
                    return menu.item(PopupMenuItem::new("No vaults left to add").disabled(true));
                }

                free.into_iter().fold(menu, |menu, (id, name)| {
                    let this = this.clone();

                    menu.item(
                        PopupMenuItem::new(name)
                            .icon(IconName::Vault)
                            .on_click(move |_, _, cx| this.update(cx, |t, cx| t.add(id, cx))),
                    )
                })
            });

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(table)
            .child(div().flex().child(add))
            .into_any_element()
    }

    /// The token, masked, with its copy and regenerate buttons.
    fn token_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let (copy, regenerate) = (cx.entity(), cx.entity());

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .px(px(10.))
                            .py(px(6.))
                            .border_1()
                            .border_color(c.border)
                            .rounded(px(6.))
                            .bg(c.card)
                            .text_color(c.muted_foreground)
                            .child("••••••••••••••••••••••••••••••••"),
                    )
                    .child(
                        Button::new("mcp-copy-token")
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Small)
                            .icon(IconName::Copy)
                            .label("Copy")
                            .on_click(move |_, _, cx| copy.update(cx, |t, cx| t.copy_token(cx))),
                    )
                    .child(
                        Button::new("mcp-new-token")
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Small)
                            .icon(IconName::RefreshCw)
                            .label("Regenerate")
                            .on_click(move |_, window, cx| {
                                let regenerate = regenerate.clone();

                                confirm(
                                    "Regenerate token",
                                    "Clients using the current token will no longer be able to connect until you give them the new one.",
                                    "Regenerate",
                                    window,
                                    cx,
                                    move |_, cx| {
                                        regenerate.update(cx, |t, cx| t.regenerate_token(cx))
                                    },
                                )
                            }),
                    ),
            )
            .child(note(
                "The client sends it in the \"Authorization: Bearer <token>\" header. It is stored in the system credential store.",
                c.muted_foreground,
            ))
    }
}

impl Render for McpSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let this = cx.entity();
        let running = self.server.is_some();

        let host = self.host.read(cx).value().trim().to_string();
        let port = port_of(&self.port.read(cx).value());

        let invalid = if host.is_empty() {
            Some("The host is missing.")
        } else if port.is_none() {
            Some("The port must be between 1024 and 65535.")
        } else {
            None
        };

        let switch = Switch::new("mcp-enabled", "Enable MCP server")
            .checked(running)
            // Turning it on with invalid data makes no sense; turning it off is always allowed.
            .disabled(!running && invalid.is_some())
            .on_click(move |on, _, cx| this.update(cx, |t, cx| t.set_enabled(*on, cx)));

        div()
            .flex()
            .flex_col()
            .child(header(
                "MCP server",
                "Lets an MCP client (an AI assistant, for example) read and change your vaults and run commands on their hosts. It only sees the vaults you add below, with the permissions you choose.",
            ))
            .child(setting(
                "Enable server",
                "Off by default. If you leave it on, it starts with the app.",
                switch,
            ))
            .when(running, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .mt(px(8.))
                        .text_size(px(12.))
                        .text_color(c.success)
                        .child(div().size(px(6.)).rounded_full().bg(c.success))
                        .child(format!(
                            "Listening on http://{}:{}/mcp",
                            self.settings.host, self.settings.port
                        )),
                )
            })
            .when_some(self.error.clone(), |this, e| {
                this.child(div().mt(px(8.)).child(note(e, c.destructive)))
            })
            .child(section_title("Connection"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .max_w(px(384.))
                    .gap(px(16.))
                    .child(
                        field("Allowed host", Input::new(&self.host).disabled(running)).child(
                            note(
                                "Listens on this address and only accepts requests addressed to it.",
                                c.muted_foreground,
                            ),
                        ),
                    )
                    .when(!host.is_empty() && !mcp::is_loopback(&host), |this| {
                        this.child(note(
                            "It listens beyond this computer, and the token travels unencrypted: anyone on the network who captures it can use the server.",
                            c.warning,
                        ))
                    })
                    .child(field("Port", Input::new(&self.port).disabled(running)))
                    .when(running, |this| {
                        this.child(note(
                            "Turn the server off to change the host or port.",
                            c.muted_foreground,
                        ))
                    })
                    .when_some(invalid, |this, e| this.child(note(e, c.destructive))),
            )
            .child(section_title("Access token"))
            .child(self.token_section(cx))
            .child(section_title("Vaults with access"))
            .child(self.vaults_table(cx))
    }
}

/// Asks whether to run a command requested by the MCP. Cancelling or closing the dialog counts as no.
fn ask_exec(
    request: ExecRequest,
    answer: oneshot::Sender<bool>,
    window: &mut Window,
    cx: &mut App,
) {
    // It can arrive while the app is in the background: bring the window to the front.
    window.activate_window();

    let answer = Rc::new(RefCell::new(Some(answer)));

    confirm(
        "An MCP client wants to run a command",
        format!(
            "On {} (vault {}):\n\n{}",
            request.host, request.vault, request.command
        ),
        "Run",
        window,
        cx,
        move |_, _| {
            if let Some(answer) = answer.borrow_mut().take() {
                let _ = answer.send(true);
            }
        },
    );
}

/// Picks a host in the vault that has no permissions of its own yet and opens the dialog to set them.
fn add_exception(owner: Entity<McpSettings>, vault: Uuid, window: &mut Window, cx: &mut App) {
    let t = owner.read(cx);
    let perms = t.perms_of(vault);

    let hosts: Vec<_> = t
        .state
        .read(cx)
        .views
        .get(&vault)
        .map(|view| {
            view.hosts
                .iter()
                .filter(|h| h.mcp.is_none())
                .map(|h| (h.id, h.name.clone()))
                .collect()
        })
        .unwrap_or_default();

    // Without a free host, the dialog would have nobody to give them to.
    let Some(&(first, _)) = hosts.first() else {
        let state = t.state.clone();
        state.update(cx, |s, cx| {
            s.set_error(
                Some("All hosts in this vault already have their own permissions.".into()),
                cx,
            )
        });
        return;
    };

    open_perms(
        owner,
        vault,
        Some(first),
        hosts,
        perms,
        "Host exception".into(),
        window,
        cx,
    );
}

/// Dialog to choose what the MCP can do in a vault, or in a host when `host` or `hosts` is set.
struct PermsForm {
    owner: Entity<McpSettings>,
    vault: Uuid,
    host: Option<Uuid>,
    /// Hosts to choose from when adding an exception. Empty in the other cases.
    hosts: Vec<(Uuid, String)>,
    perms: McpPerms,
}

#[allow(clippy::too_many_arguments)]
fn open_perms(
    owner: Entity<McpSettings>,
    vault: Uuid,
    host: Option<Uuid>,
    hosts: Vec<(Uuid, String)>,
    perms: McpPerms,
    title: String,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|_| PermsForm {
        owner,
        vault,
        host,
        hosts,
        perms,
    });

    open_dialog(window, cx, move |dialog, _, _| {
        dialog.title(title.clone()).w(px(420.)).child(view.clone())
    });
}

impl Render for PermsForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let this = cx.entity();

        let mut body = div().flex().flex_col().gap(px(12.));

        if !self.hosts.is_empty() {
            let this = this.clone();

            body = body
                .child(field(
                    "Host",
                    choice("mcp-host", self.hosts.clone(), self.host, move |id, _, cx| {
                        this.update(cx, |t, cx| {
                            t.host = Some(id);
                            cx.notify();
                        })
                    }),
                ))
                .child(note(
                    "Their permissions replace the vault's. They are stored encrypted in the vault, so they travel with it to other machines.",
                    c.muted_foreground,
                ));
        }

        for (i, (name, hint)) in CHECKS.into_iter().enumerate() {
            let this = this.clone();
            let on = if i == 0 {
                self.perms.read
            } else {
                self.perms.write
            };

            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        Checkbox::new(("mcp-perm", i))
                            .checked(on)
                            .label(name)
                            .on_click(move |on, _, cx| {
                                this.update(cx, |t, cx| {
                                    if i == 0 {
                                        t.perms.read = *on;
                                    } else {
                                        t.perms.write = *on;
                                    }

                                    cx.notify();
                                })
                            }),
                    )
                    .child(div().pl(px(24.)).child(note(hint, c.muted_foreground))),
            );
        }

        let pick = this.clone();

        body = body.child(
            field(
                "Execution",
                choice(
                    "mcp-exec",
                    EXEC.iter().map(|(e, l)| (*e, l.to_string())).collect(),
                    Some(self.perms.exec),
                    move |exec, _, cx| {
                        pick.update(cx, |t, cx| {
                            t.perms.exec = exec;
                            cx.notify();
                        })
                    },
                ),
            )
            .child(note(
                "Run commands over SSH. \"Ask\" opens a prompt in the app for each command.",
                c.muted_foreground,
            )),
        );

        body.child(footer("Save", false, move |_, window, cx| {
            let (owner, vault, host, perms) = {
                let t = this.read(cx);
                (t.owner.clone(), t.vault, t.host, t.perms)
            };

            match host {
                Some(host) => {
                    owner.update(cx, |o, cx| o.set_host_perms(vault, host, Some(perms), cx))
                }
                None => owner.update(cx, |o, cx| o.set_perms(vault, perms, cx)),
            }

            window.close_dialog(cx);
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_port_only_without_privileges() {
        assert_eq!(port_of(" 47610 "), Some(47610));
        assert_eq!(port_of("80"), None);
        assert_eq!(port_of("70000"), None);
        assert_eq!(port_of("abc"), None);
    }

    #[test]
    fn permissions_summary() {
        let p = McpPerms {
            read: true,
            write: true,
            exec: Exec::Ask,
        };

        assert_eq!(summary(p), "Read · Write · Execute (ask)");
        assert_eq!(summary(McpPerms::default()), "No permissions");
    }
}
