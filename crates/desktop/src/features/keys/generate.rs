// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Form to generate a new key for the keychain.

use crate::app::state::{AppState, Tab};
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

/// Generate a new key for the keychain. Saving it opens its detail page to copy the public key.
pub struct GenerateKey {
    state: Entity<AppState>,
    vault: Uuid,
    name: Entity<InputState>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

pub fn open_generate(state: Entity<AppState>, vault: Uuid, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| GenerateKey::new(state, vault, window, cx));

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title("Generate key").w(px(440.)).child(view.clone())
    });
}

impl GenerateKey {
    fn new(
        state: Entity<AppState>,
        vault: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = text(window, cx, "Laptop", "");
        name.update(cx, |i, cx| i.focus(window, cx));

        let subs = vec![
            cx.subscribe_in(&name, window, |this, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    this.generate(window, cx);
                }
            }),
        ];

        Self {
            state,
            vault,
            name,
            error: None,
            _subs: subs,
        }
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();

        if name.is_empty() {
            self.error = Some("The name is missing.".into());
            cx.notify();
            return;
        }

        let vault = self.vault;

        if let Some(id) = self.state.update(cx, |s, cx| {
            s.change(vault, |v| v.generate_key(vault, &name), cx)
        }) {
            window.close_dialog(cx);
            self.state
                .update(cx, |s, cx| s.open_pinned(Tab::Key { vault, id }, cx));
        }
    }
}

impl Render for GenerateKey {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let this = cx.entity();

        div().flex().flex_col()
            .gap(px(12.))
            .child(field("Name", Input::new(&self.name)).child(note(
                "Ed25519 key. Afterwards, copy the public key to the server's authorized_keys file.",
                c.muted_foreground,
            )))
            .when_some(self.error.clone(), |this, e| this.child(note(e, c.destructive)))
            .child(footer("Generate", false, move |_, window, cx| {
                this.update(cx, |this, cx| this.generate(window, cx))
            }))
    }
}
