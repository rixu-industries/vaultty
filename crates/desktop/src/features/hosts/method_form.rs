// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Form to create or edit a host's access method.

use crate::app::state::AppState;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::WindowExt;
use crate::components::{Input, InputEvent, InputState};
use crate::components::{choice, field, footer, note, text};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use uuid::Uuid;
use vaults::{AccessKind, AccessMethod};

/// Create (`method: None`) or edit a host's access method. When editing, `vaults` is asked for the
/// method with its secret.
pub struct MethodForm {
    state: Entity<AppState>,
    vault: Uuid,
    host: Uuid,
    method: Option<Uuid>,
    /// `true`: username and password; `false`: key from the keychain.
    password_kind: bool,
    username: Entity<InputState>,
    label: Entity<InputState>,
    password: Entity<InputState>,
    key: Option<Uuid>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

pub fn open_method_form(
    state: Entity<AppState>,
    vault: Uuid,
    host: Uuid,
    method: Option<Uuid>,
    window: &mut Window,
    cx: &mut App,
) {
    let current = match method {
        Some(m) => match state.read(cx).vaults.method(vault, host, m) {
            Ok(current) => Some(current),
            Err(e) => {
                state.update(cx, |s, cx| s.set_error(Some(e), cx));
                return;
            }
        },
        None => None,
    };

    let view = cx.new(|cx| MethodForm::new(state, vault, host, method, current, window, cx));
    let title = if method.is_some() {
        "Edit access method"
    } else {
        "New access method"
    };

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title(title).w(px(460.)).child(view.clone())
    });
}

impl MethodForm {
    fn new(
        state: Entity<AppState>,
        vault: Uuid,
        host: Uuid,
        method: Option<Uuid>,
        current: Option<AccessMethod>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let first_key = keys_of(&state, vault, cx).first().map(|(id, _)| *id);

        let (password_kind, username, secret, key) = match current.as_ref().map(|m| &m.kind) {
            Some(AccessKind::Password { username, password }) => {
                (true, username.clone(), password.clone(), first_key)
            }
            Some(AccessKind::Key { username, key_id }) => {
                (false, username.clone(), String::new(), Some(*key_id))
            }
            None => (true, String::new(), String::new(), first_key),
        };

        let label_value = current
            .as_ref()
            .map(|m| m.label.clone())
            .unwrap_or_default();
        let username = text(window, cx, "root", &username);
        let label = text(window, cx, "Emergency", &label_value);
        // When editing, the field starts with the saved password.
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .default_value(secret)
        });

        username.update(cx, |i, cx| i.focus(window, cx));

        let enter = |this: &mut Self,
                     _: &Entity<InputState>,
                     e: &InputEvent,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            if let InputEvent::PressEnter { .. } = e {
                this.save(window, cx);
            }
        };

        let subs = vec![
            cx.subscribe_in(&username, window, enter),
            cx.subscribe_in(&label, window, enter),
            cx.subscribe_in(&password, window, enter),
        ];

        Self {
            state,
            vault,
            host,
            method,
            password_kind,
            username,
            label,
            password,
            key,
            error: None,
            _subs: subs,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let user = self.username.read(cx).value().trim().to_string();

        if user.is_empty() {
            self.error = Some("The username is missing.".into());
            cx.notify();
            return;
        }

        let kind = if self.password_kind {
            let password = self.password.read(cx).value().to_string();

            if password.is_empty() {
                self.error = Some("The password is missing.".into());
                cx.notify();
                return;
            }

            AccessKind::Password {
                username: user,
                password,
            }
        } else {
            let Some(key_id) = self.key else {
                self.error = Some("Choose a key from the keychain.".into());
                cx.notify();
                return;
            };

            AccessKind::Key {
                username: user,
                key_id,
            }
        };

        let method = AccessMethod {
            id: self.method.unwrap_or_else(Uuid::new_v4),
            label: self.label.read(cx).value().trim().to_string(),
            kind,
        };

        let (vault, host) = (self.vault, self.host);

        if self
            .state
            .update(cx, |s, cx| {
                s.change(vault, |v| v.save_method(vault, host, method), cx)
            })
            .is_some()
        {
            window.close_dialog(cx);
        }
    }
}

/// The vault's keychain keys, by name.
fn keys_of(state: &Entity<AppState>, vault: Uuid, cx: &App) -> Vec<(Uuid, String)> {
    let mut keys: Vec<(Uuid, String)> = state
        .read(cx)
        .views
        .get(&vault)
        .map(|v| v.keys.iter().map(|k| (k.id, k.name.clone())).collect())
        .unwrap_or_default();

    keys.sort_by(|a, b| a.1.cmp(&b.1));
    keys
}

impl Render for MethodForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let keys = keys_of(&self.state, self.vault, cx);
        let empty = keys.is_empty();
        let this = cx.entity();

        let kinds = vec![
            (true, "Username and password".to_string()),
            (false, "SSH key from the keychain".to_string()),
        ];

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(field(
                "Type",
                choice("kind", kinds, Some(self.password_kind), {
                    let this = this.clone();
                    move |kind, _, cx| {
                        this.update(cx, |this, cx| {
                            this.password_kind = kind;
                            cx.notify();
                        })
                    }
                }),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .items_start()
                    .gap(px(8.))
                    .child(field("Username", Input::new(&self.username)).flex_1())
                    .child(field("Label (optional)", Input::new(&self.label)).flex_1()),
            )
            .map(|form| {
                if self.password_kind {
                    form.child(field("Password", Input::new(&self.password).mask_toggle()))
                } else {
                    let this = this.clone();

                    form.child(
                        field(
                            "Key",
                            choice("key", keys, self.key, move |key, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.key = Some(key);
                                    cx.notify();
                                })
                            }),
                        )
                        .when(empty, |this| {
                            this.child(note(
                                "The keychain is empty: import a key first.",
                                c.muted_foreground,
                            ))
                        }),
                    )
                }
            })
            .when_some(self.error.clone(), |this, e| {
                this.child(note(e, c.destructive))
            })
            .child(footer("Save", false, move |_, window, cx| {
                this.update(cx, |this, cx| this.save(window, cx))
            }))
    }
}
