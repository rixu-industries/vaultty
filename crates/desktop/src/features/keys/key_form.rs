// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Form to import or edit a keychain key.

use crate::app::state::{AppState, pick};
use crate::app::tabs::Tab;
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::WindowExt;
use crate::components::{Button, ButtonVariant, field, footer, note, text};
use crate::components::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div,
};
use uuid::Uuid;
use vaults::{KeyInfo, KeyInput, read_key_file};

/// Import (`id: None`) or edit a keychain key. The key is pasted or loaded from a file.
pub struct KeyForm {
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    name: Entity<InputState>,
    private_key: Entity<TextareaState>,
    public_key: Entity<TextareaState>,
    passphrase: Entity<InputState>,
    /// The key info, or the error text if it could not be read.
    info: Option<Result<KeyInfo, String>>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

pub fn open_key_form(
    state: Entity<AppState>,
    vault: Uuid,
    id: Option<Uuid>,
    window: &mut Window,
    cx: &mut App,
) {
    // When editing, secrets are not in the view: they are requested from `vaults`.
    let current = match id {
        Some(id) => match state.read(cx).vaults.key(vault, id) {
            Ok(key) => key,
            Err(e) => {
                state.update(cx, |s, cx| s.set_error(Some(e), cx));
                return;
            }
        },
        None => KeyInput::default(),
    };

    let view = cx.new(|cx| KeyForm::new(state, vault, id, current, window, cx));
    let title = if id.is_some() {
        "Edit key"
    } else {
        "Import key"
    };

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title(title).w(px(520.)).child(view.clone())
    });
}

fn textarea(
    window: &mut Window,
    cx: &mut App,
    placeholder: &str,
    value: &str,
    rows: usize,
) -> Entity<TextareaState> {
    let (placeholder, value) = (placeholder.to_string(), value.to_string());

    cx.new(|cx| {
        let mut state = TextareaState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value);
        state.set_auto_grow(rows, rows, cx);
        state
    })
}

impl KeyForm {
    fn new(
        state: Entity<AppState>,
        vault: Uuid,
        id: Option<Uuid>,
        current: KeyInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = text(window, cx, "Laptop", &current.name);
        let private_key = textarea(
            window,
            cx,
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            &current.private_key,
            7,
        );
        let public_key = textarea(window, cx, "ssh-ed25519 AAAA…", &current.public_key, 2);
        let passphrase = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .default_value(current.passphrase.clone())
        });

        name.update(cx, |i, cx| i.focus(window, cx));

        // Validated as you type, to show the algorithm or the error right away.
        let subs = vec![
            cx.subscribe_in(&private_key, window, |this, _, e: &InputEvent, _, cx| {
                if let InputEvent::Change = e {
                    this.check(cx);
                }
            }),
        ];

        let mut form = Self {
            state,
            vault,
            id,
            name,
            private_key,
            public_key,
            passphrase,
            info: None,
            error: None,
            _subs: subs,
        };

        form.check(cx);
        form
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        let key = self.private_key.read(cx).value().to_string();
        self.info = (!key.trim().is_empty()).then(|| KeyInfo::parse(&key));
        cx.notify();
    }

    fn load_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();

        pick(
            "Choose the private key",
            true,
            window,
            cx,
            move |path, window, cx| {
                this.update(cx, |this, cx| {
                    match read_key_file(&path) {
                        Ok(file) => {
                            this.private_key
                                .update(cx, |i, cx| i.set_value(file.private_key, window, cx));

                            // If its `.pub` sits next to the private key (as `ssh-keygen` leaves it), it is loaded too.
                            if let Some(public) = file.public_key {
                                this.public_key
                                    .update(cx, |i, cx| i.set_value(public, window, cx));
                            }

                            if this.name.read(cx).value().is_empty() {
                                this.name
                                    .update(cx, |i, cx| i.set_value(file.name, window, cx));
                            }

                            this.check(cx);
                        }
                        Err(e) => this.error = Some(e),
                    }

                    cx.notify();
                });
            },
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();

        if name.is_empty() {
            self.error = Some("The name is missing.".into());
            cx.notify();
            return;
        }

        let input = KeyInput {
            name,
            private_key: self.private_key.read(cx).value().trim().to_string(),
            public_key: self.public_key.read(cx).value().to_string(),
            passphrase: self.passphrase.read(cx).value().to_string(),
        };

        // Direct, not through `change`: this way key errors show in the form and not in the bar.
        let (vault, id) = (self.vault, self.id);
        let saved = match self.state.read(cx).vaults.save_key(vault, id, input) {
            Ok(saved) => saved,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };

        self.state.update(cx, |s, cx| s.refresh_view(vault, cx));
        window.close_dialog(cx);

        if id.is_none() {
            self.state
                .update(cx, |s, cx| s.open_pinned(Tab::Key { vault, id: saved }, cx));
        }
    }
}

impl Render for KeyForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let this = cx.entity();

        let (status, color) = match &self.info {
            Some(Err(e)) => (e.clone(), c.destructive),
            Some(Ok(info)) if info.encrypted => (
                format!("{} · protected with passphrase", info.algorithm),
                c.success,
            ),
            Some(Ok(info)) => (
                format!("{} · {}", info.algorithm, info.fingerprint),
                c.success,
            ),
            None => (String::new(), c.muted_foreground),
        };

        div().flex().flex_col()
            .gap(px(12.))
            .child(field("Name", Input::new(&self.name)))
            .child(field(
                "Private key (OpenSSH format)",
                Textarea::new(&self.private_key).font_family(mono_font()).text_size(px(12.)),
            ))
            .child(
                div().flex().items_center()
                    .gap(px(8.))
                    .child(div().ml(px(-8.)).child(
                        Button::new("load-file").variant(ButtonVariant::Ghost).icon(IconName::Folder).label("Load from file…").on_click({
                            let this = this.clone();
                            move |_, window, cx| this.update(cx, |this, cx| this.load_file(window, cx))
                        }),
                    ))
                    .child(div().ml_auto().truncate().text_size(px(12.)).text_color(color).child(status)),
            )
            .child(
                field(
                    "Public key (optional)",
                    Textarea::new(&self.public_key).font_family(mono_font()).text_size(px(12.)),
                )
                    .child(note("If you paste it, it is checked against the private key.", c.muted_foreground)),
            )
            .child(
                field("Passphrase (optional)", Input::new(&self.passphrase).mask_toggle()).child(note(
                    "If the key has a passphrase and you save it here, you won't have to type it when connecting.",
                    c.muted_foreground,
                )),
            )
            .when_some(self.error.clone(), |this, e| this.child(note(e, c.destructive)))
            .child(footer("Save", false, move |_, window, cx| {
                this.update(cx, |this, cx| this.save(window, cx))
            }))
    }
}
