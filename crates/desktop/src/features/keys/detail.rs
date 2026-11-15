// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! A key's detail page: its fingerprint, public key and the hosts that use it.

use crate::app::state::AppState;
use crate::app::tabs::Tab;
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::{Button, ButtonVariant, os_icon, property, section_title};
use crate::features::keys::key_form::open_key_form;
use crate::features::keys::list::delete_key;
use crate::features::keys::users;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div,
};
use std::time::Duration;
use uuid::Uuid;

/// A key and the hosts that use it.
pub struct KeyDetail {
    state: Entity<AppState>,
    vault: Uuid,
    id: Uuid,
    /// The "Copied" notice goes back to "Copy public key" after a moment.
    copied: bool,
    _observe: Subscription,
}

impl KeyDetail {
    pub fn new(state: Entity<AppState>, vault: Uuid, id: Uuid, cx: &mut Context<Self>) -> Self {
        Self {
            _observe: cx.observe(&state, |_, _, cx| cx.notify()),
            state,
            vault,
            id,
            copied: false,
        }
    }

    fn copy_public(&mut self, public: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(public));
        self.copied = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for KeyDetail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let (vault, id) = (self.vault, self.id);
        let s = self.state.read(cx);

        let Some((key, hosts)) = s.views.get(&vault).and_then(|v| {
            v.keys
                .iter()
                .find(|k| k.id == id)
                .map(|k| (k.clone(), users(v, id)))
        }) else {
            return crate::layout::not_found("The key").into_any_element();
        };

        let this = cx.entity();
        let st = self.state.clone();
        let public = key.public.clone();

        let mut used = div().flex().flex_col();

        if hosts.is_empty() {
            used = used.child(
                div()
                    .text_color(c.muted_foreground)
                    .child("No host uses this key."),
            );
        }

        for host in &hosts {
            let state = st.clone();
            let h = host.id;

            used = used.child(
                div()
                    .flex()
                    .items_center()
                    .id(SharedString::from(format!("user-{h}")))
                    .cursor_pointer()
                    .mx(px(-8.))
                    .gap(px(8.))
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(c.list_hover))
                    .child(os_icon(
                        host.os.as_deref(),
                        IconName::Server,
                        13.,
                        c.muted_foreground,
                    ))
                    .child(host.name.clone())
                    .child(
                        div()
                            .font_family(mono_font())
                            .text_size(px(12.))
                            .text_color(c.muted_foreground)
                            .child(host.address.clone()),
                    )
                    .on_click(move |_, _, cx| {
                        state.update(cx, |s, cx| s.preview(Tab::Host { vault, id: h }, cx))
                    }),
            );
        }

        div()
            .flex()
            .flex_col()
            .id("key-detail")
            .size_full()
            .overflow_y_scrollbar()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .max_w(px(720.))
                    .px(px(32.))
                    .py(px(24.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .items_start()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .child(key.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .font_family(mono_font())
                                            .text_size(px(12.))
                                            .text_color(c.muted_foreground)
                                            .mt(px(4.))
                                            .truncate()
                                            .child(key.fingerprint.clone()),
                                    ),
                            )
                            .child(
                                Button::new("copy-public")
                                    .label(if self.copied {
                                        "Copied"
                                    } else {
                                        "Copy public key"
                                    })
                                    .disabled(public.is_none())
                                    .on_click(move |_, _, cx| {
                                        if let Some(public) = public.clone() {
                                            this.update(cx, |this, cx| {
                                                this.copy_public(public, cx)
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("edit-key")
                                    .icon(IconName::Pencil)
                                    .label("Edit")
                                    .on_click({
                                        let state = st.clone();
                                        move |_, window, cx| {
                                            open_key_form(
                                                state.clone(),
                                                vault,
                                                Some(id),
                                                window,
                                                cx,
                                            )
                                        }
                                    }),
                            ),
                    )
                    .child(section_title("Key"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(property("Algorithm", div().child(key.algorithm.clone())))
                            .child(property(
                                "Fingerprint",
                                div().truncate().child(key.fingerprint.clone()),
                            ))
                            .child(property(
                                "Passphrase",
                                div().child(if key.has_passphrase {
                                    "Saved in the vault"
                                } else {
                                    "No"
                                }),
                            )),
                    )
                    .child(section_title("Used by"))
                    .child(used)
                    .child(section_title("Danger zone"))
                    .map(|this| {
                        if hosts.is_empty() {
                            let state = st.clone();

                            this.child(
                                div().flex().items_center().ml(px(-8.)).child(
                                    Button::new("delete-key")
                                        .variant(ButtonVariant::Destructive)
                                        .icon(IconName::Trash)
                                        .label("Delete key")
                                        .on_click(move |_, window, cx| {
                                            delete_key(state.clone(), vault, id, window, cx)
                                        }),
                                ),
                            )
                        } else {
                            this.child(div().text_color(c.muted_foreground).child(
                                "To delete the key, first remove it from the hosts that use it.",
                            ))
                        }
                    }),
            )
            .into_any_element()
    }
}
