// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! A vault's settings, on their own tab: name, location, master password and unlock.

use crate::app::state::AppState;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{Button, ButtonVariant, field, note, password, section_title};
use crate::components::{Input, InputEvent, InputState};
use crate::features::settings::header;
use crate::features::vaults::MIN_PASSWORD;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use uuid::Uuid;

pub struct VaultSettings {
    state: Entity<AppState>,
    vault: Uuid,
    current: Entity<InputState>,
    password: Entity<InputState>,
    confirm: Entity<InputState>,
    busy: bool,
    error: Option<String>,
    saved: bool,
    _subs: Vec<Subscription>,
}

impl VaultSettings {
    pub fn new(
        state: Entity<AppState>,
        vault: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = password(window, cx, "");
        let new = password(window, cx, "");
        let confirm = password(window, cx, "");

        let on_input = |this: &mut Self,
                        _: &Entity<InputState>,
                        event: &InputEvent,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            match event {
                InputEvent::Change => {
                    this.saved = false;
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => {}
            }
        };

        let subs = vec![
            cx.subscribe_in(&current, window, on_input),
            cx.subscribe_in(&new, window, on_input),
            cx.subscribe_in(&confirm, window, on_input),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];

        Self {
            state,
            vault,
            current,
            password: new,
            confirm,
            busy: false,
            error: None,
            saved: false,
            _subs: subs,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }

        let new = self.password.read(cx).value().to_string();

        self.saved = false;
        self.error = if new.chars().count() < MIN_PASSWORD {
            Some("The master password needs at least 8 characters.".into())
        } else if new != self.confirm.read(cx).value() {
            Some("The passwords do not match.".into())
        } else {
            None
        };

        if self.error.is_some() {
            cx.notify();
            return;
        }

        self.busy = true;
        cx.notify();

        let current = self.current.read(cx).value().to_string();
        let vault = self.vault;
        let task = self
            .state
            .update(cx, |s, cx| s.change_password(vault, current, new, cx));

        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;

            this.update_in(cx, |this, window, cx| {
                // The current one is always cleared; the new one is kept on failure so it is not retyped.
                this.current.update(cx, |i, cx| i.set_value("", window, cx));

                match result {
                    Ok(()) => {
                        this.password
                            .update(cx, |i, cx| i.set_value("", window, cx));
                        this.confirm.update(cx, |i, cx| i.set_value("", window, cx));
                        this.saved = true;
                    }
                    Err(e) => this.error = Some(e),
                }

                this.busy = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for VaultSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let Some(info) = self.state.read(cx).item(self.vault).cloned() else {
            return div().into_any_element();
        };

        let muted = |text: &'static str| {
            div()
                .max_w(px(560.))
                .text_size(px(12.))
                .text_color(colors().muted_foreground)
                .child(text)
        };

        let this = cx.entity();
        let state = self.state.clone();
        let vault = self.vault;

        let body = div().flex().flex_col()
            .child(header("Vault settings", &info.name))
            .child(section_title("General"))
            .child(crate::components::property("Name", div().child(info.name.clone())))
            .child(crate::components::property("Location", div().min_w_0().truncate().child(info.path.clone())))
            .child(
                div().flex().items_center().gap(px(12.)).mt(px(12.))
                    .child(
                        Button::new("reload-vault")
                            .icon(IconName::RefreshCw)
                            .label("Reload from disk")
                            .on_click({
                                let state = self.state.clone();
                                move |_, _, cx| state.update(cx, |s, cx| s.reload(vault, cx))
                            }),
                    )
                    .child(muted("Reads the vault folder again, for example after another device changed it.")),
            )
            .child(section_title("Master password"))
            .child(muted(
                "Change the password used to unlock this vault. Open connections are kept.",
            ))
            .child(
                div().flex().flex_col()
                    .mt(px(16.))
                    .max_w(px(384.))
                    .gap(px(16.))
                    .child(field("Current password", Input::new(&self.current).mask_toggle().disabled(self.busy)))
                    .child(
                        field("New password", Input::new(&self.password).mask_toggle().disabled(self.busy))
                            .child(note("At least 8 characters.", c.muted_foreground)),
                    )
                    .child(field(
                        "Confirm new password",
                        Input::new(&self.confirm).mask_toggle().disabled(self.busy),
                    ))
                    .when_some(self.error.clone(), |this, e| this.child(note(e, c.destructive)))
                    .when(self.saved, |this| this.child(note("Master password updated.", c.success)))
                    .child(
                        div().flex().items_center().child(
                            Button::new("change-password")
                                .variant(ButtonVariant::Primary)
                                .label(if self.busy { "Saving…" } else { "Change password" })
                                .disabled(self.busy)
                                .on_click(move |_, window, cx| this.update(cx, |this, cx| this.submit(window, cx))),
                        ),
                    ),
            )
            .child(div().mt(px(20.)).child(muted(
                "Earlier copies of the vault keep their previous password. This change does not revoke access for anyone who already has a copy of the header and its password.",
            )))
            .child(section_title("Automatic unlock"))
            .map(|this| {
                if info.remember_password {
                    this.child(muted(
                        "The password is stored in the system credential store and the vault unlocks by itself when the app opens. Anyone with access to your user session can open it.",
                    ))
                    .child(
                        div().flex().items_center().mt(px(12.)).child(
                            Button::new("forget-password")
                                .label("Forget password")
                                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.forget_password(vault, cx))),
                        ),
                    )
                } else {
                    this.child(muted(
                        "Disabled. To enable it, check \"Remember password on this computer\" when unlocking the vault.",
                    ))
                }
            });

        div()
            .id("vault-settings")
            .size_full()
            .overflow_y_scrollbar()
            .child(div().max_w(px(680.)).px(px(32.)).py(px(24.)).child(body))
            .into_any_element()
    }
}
