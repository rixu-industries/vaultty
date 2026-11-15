// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Form to create or edit a host.

use crate::app::state::{AppState, Tab};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::WindowExt;
use crate::components::{Input, InputEvent, InputState, Textarea, TextareaState};
use crate::components::{choice, field, footer, note, text};
use crate::features::hosts::group_options;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use uuid::Uuid;
use vaults::HostInput;

/// Create (`id: None`) or edit a host. Access methods are managed separately, on their own page.
pub struct HostForm {
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    name: Entity<InputState>,
    address: Entity<InputState>,
    port: Entity<InputState>,
    notes: Entity<TextareaState>,
    group: Option<Uuid>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

pub fn open_host_form(
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    group: Option<Uuid>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| HostForm::new(state, vault, id, group, window, cx));
    let title = if id.is_some() {
        "Edit host"
    } else {
        "New host"
    };

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title(title).w(px(460.)).child(view.clone())
    });
}

impl HostForm {
    fn new(
        state: Entity<AppState>,
        vault: Uuid,
        id: Option<Uuid>,
        group: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let existing = id.and_then(|id| {
            state
                .read(cx)
                .views
                .get(&vault)
                .and_then(|v| v.hosts.iter().find(|h| h.id == id))
                .cloned()
        });

        let e = existing.as_ref();
        let name = text(window, cx, "web-01", e.map_or("", |h| h.name.as_str()));
        let address = text(
            window,
            cx,
            "10.0.0.1 or server.example.com",
            e.map_or("", |h| h.address.as_str()),
        );
        let port = text(
            window,
            cx,
            "",
            &e.map_or("22".to_string(), |h| h.port.to_string()),
        );
        let notes_value = e.map(|h| h.notes.clone()).unwrap_or_default();
        let notes = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx).default_value(notes_value);
            state.set_auto_grow(3, 6, cx);
            state
        });

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

        let subs = vec![
            cx.subscribe_in(&name, window, enter),
            cx.subscribe_in(&address, window, enter),
            cx.subscribe_in(&port, window, enter),
        ];

        Self {
            group: existing.as_ref().map_or(group, |h| h.group_id),
            state,
            vault,
            id,
            name,
            address,
            port,
            notes,
            error: None,
            _subs: subs,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let port = self.port.read(cx).value().trim().to_string();
        let parsed = port
            .parse::<u16>()
            .ok()
            .filter(|_| port.chars().all(|c| c.is_ascii_digit()));

        let Some(port) = parsed else {
            self.error = Some("The port must be a number between 1 and 65535.".into());
            cx.notify();
            return;
        };

        let input = HostInput {
            name: self.name.read(cx).value().trim().to_string(),
            address: self.address.read(cx).value().trim().to_string(),
            port,
            group_id: self.group,
            notes: self.notes.read(cx).value().trim().to_string(),
        };

        if input.name.is_empty() || input.address.is_empty() || port == 0 {
            self.error = Some("The name, address or port is missing.".into());
            cx.notify();
            return;
        }

        // `vaults` keeps the access methods and the server's host key when editing.
        let (vault, id) = (self.vault, self.id);
        let saved = self.state.update(cx, |s, cx| {
            s.change(vault, |v| v.save_host(vault, id, input), cx)
        });

        if let Some(saved) = saved {
            window.close_dialog(cx);

            if id.is_none() {
                self.state.update(cx, |s, cx| {
                    s.open_pinned(Tab::Host { vault, id: saved }, cx)
                });
            }
        }
    }
}

impl Render for HostForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let options = self
            .state
            .read(cx)
            .views
            .get(&self.vault)
            .map(group_options)
            .unwrap_or_default();
        let this = cx.entity();

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(field("Name", Input::new(&self.name)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .items_start()
                    .gap(px(8.))
                    .child(field("Address", Input::new(&self.address)).flex_1())
                    .child(field("Port", Input::new(&self.port)).w(px(96.))),
            )
            .child(field(
                "Group",
                choice("group", options, Some(self.group), {
                    let this = this.clone();
                    move |group, _, cx| {
                        this.update(cx, |this, cx| {
                            this.group = group;
                            cx.notify();
                        })
                    }
                }),
            ))
            .child(field("Notes", Textarea::new(&self.notes)))
            .when_some(self.error.clone(), |this, e| {
                this.child(note(e, c.destructive))
            })
            .child(footer("Save", false, move |_, window, cx| {
                this.update(cx, |this, cx| this.save(window, cx))
            }))
    }
}
