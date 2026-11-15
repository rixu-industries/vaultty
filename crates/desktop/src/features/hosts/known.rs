// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Host keys of the servers accepted in the current vault.

use crate::app::state::AppState;
use crate::app::theme::{colors, mono_font, px};
use crate::components::{Button, ButtonSize, ButtonVariant, confirm, empty_state};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, App, Entity, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    div,
};
use uuid::Uuid;

pub fn known_hosts(state: &Entity<AppState>, vault: Uuid, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);
    let Some(view) = s.views.get(&vault) else {
        return crate::layout::not_found("The vault").into_any_element();
    };
    let hosts = super::trusted_hosts(view);

    if hosts.is_empty() {
        return empty_state(
            IconName::Lock,
            "No known hosts yet. Connect to a host and accept its fingerprint to see it here.",
            None,
        )
        .into_any_element();
    }

    div()
        .id("known-hosts")
        .size_full()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .p(px(24.))
                .child(div().text_size(px(20.)).child("Known Hosts"))
                .child(
                    div()
                        .text_color(c.muted_foreground)
                        .child("Accepted SSH server fingerprints in this vault."),
                )
                .children(hosts.into_iter().map(|(host, fingerprint)| {
                    let fingerprint = SharedString::from(fingerprint.to_string());
                    let (state, host_id, name) = (state.clone(), host.id, host.name.clone());

                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .p(px(12.))
                        .border_1()
                        .border_color(c.border)
                        .rounded(px(6.))
                        .child(host.name.clone())
                        .child(
                            div()
                                .font_family(mono_font())
                                .text_size(px(12.))
                                .text_color(c.muted_foreground)
                                .child(format!("{} · Port {}", host.address, host.port)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .min_w_0()
                                        .font_family(mono_font())
                                        .text_size(px(12.))
                                        .child(fingerprint.clone()),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("copy-{}", host.id)))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::IconSmall)
                                        .icon(IconName::Copy)
                                        .tooltip("Copy fingerprint")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(
                                                gpui_kit::ClipboardItem::new_string(
                                                    fingerprint.to_string(),
                                                ),
                                            );
                                        }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("forget-{}", host.id)))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::IconSmall)
                                        .icon(IconName::Trash)
                                        .tooltip("Forget fingerprint")
                                        .on_click(move |_, window, cx| {
                                            let state = state.clone();

                                            confirm(
                                                "Forget fingerprint",
                                                format!(
                                                    "{name} will ask you to accept its fingerprint again on the next connection."
                                                ),
                                                "Forget",
                                                window,
                                                cx,
                                                move |_, cx| {
                                                    state.update(cx, |s, cx| {
                                                        s.change(
                                                            vault,
                                                            |v| v.forget_host_key(vault, host_id),
                                                            cx,
                                                        );
                                                    })
                                                },
                                            );
                                        }),
                                )
                        )
                })),
        )
        .into_any_element()
}
