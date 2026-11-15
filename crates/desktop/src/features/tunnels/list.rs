// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! List of tunnels, in the sidebar.

use crate::app::state::{AppState, Tab};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{ContextMenuExt, PopupMenuItem, confirm, tree_row};
use crate::features::tunnels::form::open_tunnel_form;
use crate::features::tunnels::{list, status_color};
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use uuid::Uuid;

/// Tunnels of the vault, in the sidebar, with a dot showing how they are doing.
pub fn tunnel_list(state: &Entity<AppState>, vault: Uuid, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);

    let Some(view) = s.views.get(&vault) else {
        return div().into_any_element();
    };

    let tunnels = list(view, &s.search);
    let selected = match s.active_tab() {
        Some(Tab::Tunnel { id, .. }) => Some(id),
        _ => None,
    };

    if tunnels.is_empty() {
        return div()
            .px(px(12.))
            .py(px(8.))
            .text_size(px(12.))
            .text_color(c.muted_foreground)
            .child(if s.search.is_empty() {
                "There are no tunnels."
            } else {
                "No tunnels match."
            })
            .into_any_element();
    }

    let mut list = div().flex().flex_col();

    for tunnel in tunnels {
        let id = tunnel.id;
        let enabled = tunnel.enabled;
        let status = s.sessions.tunnel_status(id);
        let open = state.clone();
        let menu = state.clone();

        // Centered in a box the size of an icon, so it lines up with the other lists.
        let dot = div()
            .size(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(7.))
                    .rounded_full()
                    .bg(status_color(enabled, status.as_ref())),
            );

        list = list.child(
            tree_row(
                SharedString::from(format!("tunnel-{id}")),
                0,
                dot.into_any_element(),
                tunnel.name.clone(),
                Some(format!(":{}", tunnel.port)),
                selected == Some(id),
            )
            // A click opens a preview; a double click pins it.
            .on_click(move |e: &ClickEvent, _, cx| {
                open.update(cx, |s, cx| {
                    if e.click_count() >= 2 {
                        s.open_pinned(Tab::Tunnel { vault, id }, cx)
                    } else {
                        s.preview(Tab::Tunnel { vault, id }, cx)
                    }
                })
            })
            .context_menu(move |m, _, _| {
                let (toggle, edit, delete) = (menu.clone(), menu.clone(), menu.clone());

                m.min_w(px(200.))
                    .item(
                        PopupMenuItem::new(if enabled { "Disable" } else { "Enable" }).on_click(
                            move |_, _, cx| {
                                toggle.update(cx, |s, cx| {
                                    s.change(
                                        vault,
                                        |v| v.set_tunnel_enabled(vault, id, !enabled),
                                        cx,
                                    )
                                });
                            },
                        ),
                    )
                    .item(PopupMenuItem::new("Edit").icon(IconName::Pencil).on_click(
                        move |_, window, cx| {
                            open_tunnel_form(edit.clone(), vault, Some(id), window, cx)
                        },
                    ))
                    .separator()
                    .item(PopupMenuItem::new("Delete").icon(IconName::Trash).on_click(
                        move |_, window, cx| delete_tunnel(delete.clone(), vault, id, window, cx),
                    ))
            }),
        );
    }

    list.into_any_element()
}

pub(super) fn delete_tunnel(
    state: Entity<AppState>,
    vault: Uuid,
    id: Uuid,
    window: &mut Window,
    cx: &mut App,
) {
    let name = state
        .read(cx)
        .views
        .get(&vault)
        .and_then(|v| v.tunnels.iter().find(|t| t.id == id))
        .map(|t| t.name.clone())
        .unwrap_or_default();

    confirm(
        format!("Delete {name}"),
        "The tunnel stops and is removed from the vault.",
        "Delete",
        window,
        cx,
        move |_, cx| {
            state.update(cx, |s, cx| {
                if s.change(vault, |v| v.delete(vault, id), cx).is_some() {
                    s.close_item(id, cx);
                }
            })
        },
    );
}
