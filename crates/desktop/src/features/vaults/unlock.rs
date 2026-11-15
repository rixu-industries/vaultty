// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Unlocking a vault with its master password.

use crate::app::state::{AppState, Route};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::Checkbox;
use crate::components::{Button, ButtonVariant, icon, note, password};
use crate::components::{Input, InputEvent, InputState};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
};
use uuid::Uuid;

/// Unlocks a vault with its master password. After unlocking, it stays on the route that was requested.
pub struct Unlock {
    state: Entity<AppState>,
    vault: Uuid,
    password: Entity<InputState>,
    remember: bool,
    error: Option<String>,
    busy: bool,
    _subs: Vec<Subscription>,
}

impl Unlock {
    pub fn new(
        state: Entity<AppState>,
        vault: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let password = password(window, cx, "Master password");
        let remember = state
            .read(cx)
            .item(vault)
            .is_some_and(|v| v.remember_password);
        let error = state.update(cx, |s, _| s.take_auto_error(vault));

        password.update(cx, |input, cx| input.focus(window, cx));

        let subs = vec![
            cx.subscribe_in(&password, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.submit(window, cx);
                }
            }),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];

        Self {
            state,
            vault,
            password,
            remember,
            error,
            busy: false,
            _subs: subs,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.password.read(cx).value().to_string();

        if self.busy || password.is_empty() {
            return;
        }

        self.busy = true;
        self.error = None;
        cx.notify();

        let (vault, remember) = (self.vault, self.remember);
        let task = self
            .state
            .update(cx, |s, cx| s.unlock(vault, password, remember, cx));

        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;

            this.update_in(cx, |this, window, cx| {
                this.error = result.err();
                this.busy = false;
                this.password
                    .update(cx, |input, cx| input.set_value("", window, cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for Unlock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let Some(info) = self.state.read(cx).item(self.vault).cloned() else {
            return div().into_any_element();
        };

        let this = cx.entity();
        let state = self.state.clone();

        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(340.))
                    .bg(c.card)
                    .border_1()
                    .border_color(c.border)
                    .rounded(px(8.))
                    .p(px(14.))
                    .shadow_2xl()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .mb(px(20.))
                            .child(
                                div()
                                    .size(px(36.))
                                    .rounded(px(6.))
                                    .bg(c.secondary)
                                    .border_1()
                                    .border_color(c.border)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(IconName::Lock, 16., c.muted_foreground)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .child(div().text_size(px(15.)).child(info.name.clone()))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(c.muted_foreground)
                                            .truncate()
                                            .child(info.path.clone()),
                                    ),
                            ),
                    )
                    .child(Input::new(&self.password).mask_toggle())
                    .child(
                        div().mt(px(12.)).child(
                            Checkbox::new("remember")
                                .checked(self.remember)
                                .label("Remember password on this computer")
                                .on_click({
                                    let this = this.clone();
                                    move |checked, _, cx| {
                                        this.update(cx, |this, cx| {
                                            this.remember = *checked;
                                            cx.notify();
                                        })
                                    }
                                }),
                        ),
                    )
                    .when_some(self.error.clone(), |this, e| {
                        this.child(div().mt(px(8.)).child(note(e, c.destructive)))
                    })
                    .child(
                        div().mt(px(12.)).child(
                            Button::new("unlock")
                                .variant(ButtonVariant::Primary)
                                .w_full()
                                .label(if self.busy { "Unlocking…" } else { "Unlock" })
                                .disabled(self.busy)
                                .on_click({
                                    let this = this.clone();
                                    move |_, window, cx| {
                                        this.update(cx, |this, cx| this.submit(window, cx))
                                    }
                                }),
                        ),
                    )
                    .child(
                        div().mt(px(8.)).child(
                            Button::new("other-vaults")
                                .variant(ButtonVariant::Ghost)
                                .w_full()
                                .icon(IconName::ArrowLeft)
                                .label("Other vaults")
                                .on_click(move |_, _, cx| {
                                    state.update(cx, |s, cx| s.go(Route::Picker, cx))
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }
}
