// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Dialog that asks for a file or folder name.

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

/// Called with the new name when the dialog is saved.
type OnSave = std::rc::Rc<dyn Fn(String, &mut App)>;

/// Asks for a file or folder name (create folder, rename).
struct NameDialog {
    name: Entity<InputState>,
    initial: String,
    error: Option<String>,
    on_save: OnSave,
    _subs: Vec<Subscription>,
}

impl NameDialog {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();

        if name.is_empty() || name == "." || name == ".." {
            self.error = Some("The name is missing.".into());
        } else if name.contains(['/', '\\']) {
            self.error = Some("The name cannot contain / or \\.".into());
        } else {
            window.close_dialog(cx);

            if name != self.initial {
                (self.on_save)(name, cx);
            }

            return;
        }

        cx.notify();
    }
}

impl Render for NameDialog {
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

pub fn open_name_dialog(
    title: &'static str,
    initial: String,
    window: &mut Window,
    cx: &mut App,
    on_save: impl Fn(String, &mut App) + 'static,
) {
    let view = cx.new(|cx| {
        let name = text(window, cx, "", &initial);
        name.update(cx, |i, cx| i.focus(window, cx));

        let subs = vec![cx.subscribe_in(
            &name,
            window,
            |this: &mut NameDialog, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    this.save(window, cx);
                }
            },
        )];

        NameDialog {
            name,
            initial,
            error: None,
            on_save: std::rc::Rc::new(on_save),
            _subs: subs,
        }
    });

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        dialog.title(title).w(px(380.)).child(view.clone())
    });
}
