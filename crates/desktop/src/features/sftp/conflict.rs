// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Conflict dialog: the destination file already exists.

use crate::app::state::AppState;
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::Checkbox;
use crate::components::WindowExt;
use crate::components::{Button, ButtonVariant};
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
};
use sftp::{Answer, Ask, Choice};
use uuid::Uuid;

/// The destination file already exists: overwrite, skip, or cancel the entire transfer.
struct Conflict {
    state: Entity<AppState>,
    tab: Uuid,
    ask: Ask,
    all: bool,
    answered: bool,
}

impl Conflict {
    fn answer(&mut self, answer: Option<Answer>, window: &mut Window, cx: &mut Context<Self>) {
        self.answered = true;
        self.state
            .read(cx)
            .sessions
            .answer(self.tab, self.ask.id, answer);
        window.close_dialog(cx);
    }
}

impl Render for Conflict {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let this = cx.entity();
        let all = self.all;
        let answer = |id: &'static str, label: &'static str, choice: Option<Choice>| {
            let this = this.clone();

            Button::new(id).label(label).on_click(move |_, window, cx| {
                this.update(cx, |this, cx| {
                    let all = this.all;
                    this.answer(choice.map(|choice| Answer { choice, all }), window, cx)
                })
            })
        };

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .font_family(mono_font())
                    .text_size(px(12.))
                    .text_color(c.muted_foreground)
                    .child(self.ask.item.dst.clone()),
            )
            .child(
                Checkbox::new("apply-all")
                    .checked(all)
                    .label("Apply to all in this transfer")
                    .on_click({
                        let this = this.clone();
                        move |checked, _, cx| {
                            this.update(cx, |this, cx| {
                                this.all = *checked;
                                cx.notify();
                            })
                        }
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(answer("conflict-cancel", "Cancel", None))
                    // Skip comes before Overwrite: pressing Enter out of habit must not overwrite anything.
                    .child(answer("conflict-skip", "Skip", Some(Choice::Skip)))
                    .child({
                        let this = this.clone();
                        Button::new("conflict-overwrite")
                            .variant(ButtonVariant::Danger)
                            .label("Overwrite")
                            .on_click(move |_, window, cx| {
                                this.update(cx, |this, cx| {
                                    let all = this.all;
                                    this.answer(
                                        Some(Answer {
                                            choice: Choice::Overwrite,
                                            all,
                                        }),
                                        window,
                                        cx,
                                    )
                                })
                            })
                    }),
            )
    }
}

pub(super) fn open_conflict(
    state: Entity<AppState>,
    tab: Uuid,
    ask: Ask,
    window: &mut Window,
    cx: &mut App,
) {
    let name = sftp::files::file_name(&ask.item.dst);
    let view = cx.new(|_| Conflict {
        state,
        tab,
        ask,
        all: false,
        answered: false,
    });

    crate::components::open_dialog(window, cx, move |dialog, _, _| {
        let view = view.clone();

        dialog
            .title(format!("\"{name}\" already exists"))
            .w(px(440.))
            .child(view.clone())
            // Closing the dialog without answering cancels the entire transfer.
            .on_close(move |_, window, cx| {
                view.update(cx, |this, cx| {
                    if !this.answered {
                        this.answer(None, window, cx);
                    }
                })
            })
    });
}
