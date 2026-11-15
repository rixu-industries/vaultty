// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! List of keychain keys, in the sidebar.

use crate::app::state::AppState;
use crate::app::tabs::Tab;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{ContextMenuExt, PopupMenuItem};
use crate::components::{confirm, icon, tree_row};
use crate::features::keys::key_form::open_key_form;
use crate::features::keys::{list, users};
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use uuid::Uuid;

/// Keychain keys, in the sidebar.
pub fn key_list(state: &Entity<AppState>, vault: Uuid, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);

    let Some(view) = s.views.get(&vault) else {
        return div().into_any_element();
    };

    let keys = list(view, &s.search);
    let selected = match s.active_tab() {
        Some(Tab::Key { id, .. }) => Some(id),
        _ => None,
    };

    if keys.is_empty() {
        return div()
            .px(px(12.))
            .py(px(8.))
            .text_size(px(12.))
            .text_color(c.muted_foreground)
            .child(if s.search.is_empty() {
                "The keychain is empty."
            } else {
                "No keys match."
            })
            .into_any_element();
    }

    let mut list = div().flex().flex_col();

    for key in keys {
        let id = key.id;
        let deletable = users(view, id).is_empty();
        let open = state.clone();
        let menu = state.clone();

        list = list.child(
            tree_row(
                SharedString::from(format!("key-{id}")),
                0,
                icon(IconName::Key, 14., c.muted_foreground).into_any_element(),
                key.name.clone(),
                None,
                selected == Some(id),
            )
            // A single click opens the preview; double-click pins it.
            .on_click(move |e: &ClickEvent, _, cx| {
                open.update(cx, |s, cx| {
                    if e.click_count() >= 2 {
                        s.open_pinned(Tab::Key { vault, id }, cx)
                    } else {
                        s.preview(Tab::Key { vault, id }, cx)
                    }
                })
            })
            .context_menu(move |m, _, _| {
                let (edit, delete) = (menu.clone(), menu.clone());
                let m = m.min_w(px(200.)).item(
                    PopupMenuItem::new("Edit").icon(IconName::Pencil).on_click(
                        move |_, window, cx| {
                            open_key_form(edit.clone(), vault, Some(id), window, cx)
                        },
                    ),
                );

                // As on its page: a key used by some host cannot be deleted.
                if deletable {
                    m.separator()
                        .item(PopupMenuItem::new("Delete").icon(IconName::Trash).on_click(
                            move |_, window, cx| delete_key(delete.clone(), vault, id, window, cx),
                        ))
                } else {
                    m
                }
            }),
        );
    }

    list.into_any_element()
}

pub(super) fn delete_key(
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
        .and_then(|v| v.keys.iter().find(|k| k.id == id))
        .map(|k| k.name.clone())
        .unwrap_or_default();

    confirm(
        format!("Delete {name}"),
        "The private key is deleted from the vault. If you have no other copy, you won't be able to recover it.",
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
