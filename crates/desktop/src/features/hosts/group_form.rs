// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Form to create or rename a host group.

use crate::app::state::AppState;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::WindowExt;
use crate::components::{Input, InputEvent, InputState};
use crate::components::{field, footer, note, text};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use uuid::Uuid;

/// Create (`id: None`) or rename a host group.
pub struct GroupForm {
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    name: Entity<InputState>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

pub fn open_group_form(
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| GroupForm::new(state, vault, id, window, cx));
    let title = if id.is_some() {
        "Rename group"
    } else {
        "New group"
    };

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title(title).w(px(380.)).child(view.clone())
    });
}

impl GroupForm {
    fn new(
        state: Entity<AppState>,
        vault: Uuid,
        id: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = id
            .and_then(|id| {
                state
                    .read(cx)
                    .views
                    .get(&vault)
                    .and_then(|v| v.groups.iter().find(|g| g.id == id))
                    .map(|g| g.name.clone())
            })
            .unwrap_or_default();
        let name = text(window, cx, "Production", &current);

        name.update(cx, |i, cx| i.focus(window, cx));

        let subs = vec![
            cx.subscribe_in(&name, window, |this, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    this.save(window, cx);
                }
            }),
        ];

        Self {
            state,
            vault,
            id,
            name,
            error: None,
            _subs: subs,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();

        if name.is_empty() {
            self.error = Some("The name is missing.".into());
            cx.notify();
            return;
        }

        // `vaults` keeps the parent group when renaming.
        let (vault, id) = (self.vault, self.id);

        if self
            .state
            .update(cx, |s, cx| {
                s.change(vault, |v| v.save_group(vault, id, &name), cx)
            })
            .is_some()
        {
            window.close_dialog(cx);
        }
    }
}

impl Render for GroupForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let this = cx.entity();

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(field("Name", Input::new(&self.name)))
            .when_some(self.error.clone(), |this, e| {
                this.child(note(e, c.destructive))
            })
            .child(footer("Save", false, move |_, window, cx| {
                this.update(cx, |this, cx| this.save(window, cx))
            }))
    }
}
