// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Create a new vault: the folder it goes in, its name and master password.

use crate::app::state::{AppState, Route, Section, pick};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::WindowExt;
use crate::components::{Button, field, footer, note, password, text};
use crate::components::{Input, InputEvent, InputState};
use crate::features::vaults::MIN_PASSWORD;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use std::path::PathBuf;

/// Create a new vault: the folder it goes in, its name and master password.
pub struct CreateVault {
    state: Entity<AppState>,
    parent: Option<PathBuf>,
    name: Entity<InputState>,
    password: Entity<InputState>,
    confirm: Entity<InputState>,
    error: Option<String>,
    busy: bool,
    _subs: Vec<Subscription>,
}

pub fn open_create(state: Entity<AppState>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| CreateVault::new(state, window, cx));

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog
            .title("Create new vault")
            .w(px(480.))
            .child(view.clone())
    });
}

impl CreateVault {
    fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = text(window, cx, "Personal", "");
        let password = password(window, cx, "");
        let confirm = password_again(window, cx);

        name.update(cx, |input, cx| input.focus(window, cx));

        let subs = vec![
            cx.subscribe_in(&name, window, Self::on_input),
            cx.subscribe_in(&password, window, Self::on_input),
            cx.subscribe_in(&confirm, window, Self::on_input),
        ];

        Self {
            state,
            parent: None,
            name,
            password,
            confirm,
            error: None,
            busy: false,
            _subs: subs,
        }
    }

    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => self.submit(window, cx),
            InputEvent::Change => cx.notify(),
            _ => {}
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();
        let password = self.password.read(cx).value().to_string();

        let error = if self.parent.is_none() {
            Some("Choose where to save the vault.".to_string())
        } else if name.is_empty() {
            Some("The name is missing.".into())
        } else if password.chars().count() < MIN_PASSWORD {
            // Characters are counted, not bytes.
            Some(format!(
                "The master password needs at least {MIN_PASSWORD} characters."
            ))
        } else if password != self.confirm.read(cx).value() {
            Some("The passwords do not match.".into())
        } else {
            None
        };

        if error.is_some() || self.busy {
            self.error = error;
            cx.notify();
            return;
        }

        let dir = self.parent.clone().unwrap_or_default().join(name);
        self.busy = true;
        self.error = None;
        cx.notify();

        let task = self.state.update(cx, |s, cx| s.create(dir, password, cx));

        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;

            this.update_in(cx, |this, window, cx| match result {
                Ok(vault) => {
                    window.close_dialog(cx);
                    this.state
                        .update(cx, |s, cx| s.go(Route::Vault(vault, Section::Hosts), cx));
                }
                Err(e) => {
                    this.error = Some(e);
                    this.busy = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

fn password_again(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    password(window, cx, "")
}

impl Render for CreateVault {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let name = self.name.read(cx).value().to_string();
        let name = name.trim();

        let location = match &self.parent {
            Some(parent) if !name.is_empty() => parent.join(name).display().to_string(),
            Some(parent) => parent.display().to_string(),
            None => "No folder chosen".into(),
        };

        let this = cx.entity();

        div().flex().flex_col()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c.muted_foreground)
                    .child("A folder is created with the encrypted vault. You can put it in Drive or in your backup system."),
            )
            .child(
                field("Name", Input::new(&self.name))
                    .child(note("It is also the name of the folder.", c.muted_foreground)),
            )
            .child(field(
                "Location",
                div().flex().items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .bg(c.background)
                            .border_1()
                            .border_color(c.border)
                            .rounded(px(6.))
                            .text_color(c.muted_foreground)
                            .truncate()
                            .child(location),
                    )
                    .child(Button::new("pick").icon(IconName::Folder).label("Choose…").on_click(
                        move |_, window, cx| {
                            let this = this.clone();
                            pick("Where to save the vault",false, window, cx, move |dir, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.parent = Some(dir);
                                    cx.notify();
                                });
                            });
                        },
                    )),
            ))
            .child(
                div().flex().items_center()
                    .gap(px(8.))
                    .child(field("Master password", Input::new(&self.password).mask_toggle()).flex_1())
                    .child(field("Repeat it", Input::new(&self.confirm).mask_toggle()).flex_1()),
            )
            .child(note("If you forget the master password, there is no way to recover the vault.", c.warning))
            .when_some(self.error.clone(), |this, e| this.child(note(e, c.destructive)))
            .child({
                let this = cx.entity();
                footer(
                    if self.busy { "Creating…" } else { "Create vault" },
                    self.busy,
                    move |_, window, cx| this.update(cx, |this, cx| this.submit(window, cx)),
                )
            })
    }
}
