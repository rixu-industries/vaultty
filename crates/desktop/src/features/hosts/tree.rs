// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Host tree in the sidebar, grouped by folder, with drag to change group and context menus.

use crate::app::state::AppState;
use crate::app::tabs::Tab;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{
    Button, ButtonSize, ButtonVariant, DragLabel, confirm, glyph, icon, os_icon, tree_row,
};
use crate::components::{ContextMenuExt, PopupMenuItem};
use crate::features::hosts::group_form::open_group_form;
use crate::features::hosts::host_form::open_host_form;
use crate::features::hosts::{host_move, tree};
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
};
use sessions::TabKind;
use uuid::Uuid;
use vaults::HostView;

/// What is dragged when moving a host to another group.
#[derive(Clone)]
pub struct DragHost {
    pub id: Uuid,
    pub group: Option<Uuid>,
    pub name: String,
}

/// Hosts grouped by folder, in the sidebar. Double-clicking a host connects. Right-clicking a
/// group or a host opens its menu.
pub fn host_tree(state: &Entity<AppState>, vault: Uuid, dragging: bool, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);

    let Some(view) = s.views.get(&vault) else {
        return div().into_any_element();
    };

    let query = s.search.clone();
    let selected = match s.active_tab() {
        Some(Tab::Host { id, .. }) => Some(id),
        _ => None,
    };
    let groups = tree(view, &query, dragging);

    if groups.is_empty() {
        return div()
            .px(px(12.))
            .py(px(8.))
            .text_size(px(12.))
            .text_color(c.muted_foreground)
            .child(if query.is_empty() {
                "There are no hosts yet."
            } else {
                "No hosts match."
            })
            .into_any_element();
    }

    let mut list = div().flex().flex_col();

    for group in groups {
        // While searching, all groups are opened to show the results.
        let open = !query.is_empty() || !s.collapsed.contains(&(vault, group.id));
        let group_id = group.id;
        let key = group_id
            .map(|g| g.to_string())
            .unwrap_or_else(|| "ungrouped".into());

        let header = tree_row(
            SharedString::from(format!("group-{key}")),
            0,
            icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                14.,
                c.muted_foreground,
            )
            .into_any_element(),
            group.name.clone(),
            Some(group.hosts.len().to_string()),
            false,
        )
        .on_click({
            let state = state.clone();
            move |_, _, cx| state.update(cx, |s, cx| s.toggle_group(vault, group_id, cx))
        })
        .drag_over::<DragHost>(move |style, drag, _, _| {
            if drag.group != group_id {
                style.bg(c.highlight).border_1().border_color(c.list_drop)
            } else {
                style
            }
        })
        .on_drop({
            let state = state.clone();
            move |drag: &DragHost, _, cx| {
                let id = drag.id;
                state.update(cx, |s, cx| {
                    let Some(input) = s.views.get(&vault).and_then(|v| host_move(v, id, group_id))
                    else {
                        return;
                    };
                    s.change(vault, |v| v.save_host(vault, Some(id), input), cx);
                });
            }
        });

        // "No group" is not a real group, so it has no menu.
        let header = match group_id {
            Some(id) => group_menu(state, vault, id, header).into_any_element(),
            None => header.into_any_element(),
        };

        list = list.child(header);

        if open {
            for host in group.hosts {
                list = list.child(host_item(state, vault, &host, selected == Some(host.id)));
            }
        }
    }

    list.into_any_element()
}

fn host_item(state: &Entity<AppState>, vault: Uuid, host: &HostView, selected: bool) -> AnyElement {
    let c = colors();
    let id = host.id;
    let drag = DragHost {
        id,
        group: host.group_id,
        name: host.name.clone(),
    };

    let item = tree_row(
        SharedString::from(format!("host-{id}")),
        1,
        os_icon(
            host.os.as_deref(),
            IconName::Server,
            14.,
            c.muted_foreground,
        ),
        host.name.clone(),
        None,
        selected,
    )
    // The address on the right; on hover, connect and open SFTP take its place, so they can be
    // found without knowing about double-click.
    .child(
        div()
            .ml_auto()
            .text_size(px(11.))
            .text_color(c.muted_foreground)
            .truncate()
            .group_hover("row", |s| s.invisible())
            .child(host.address.clone()),
    )
    .child(
        div()
            .absolute()
            .right(px(4.))
            .top_0()
            .bottom_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .invisible()
            .group_hover("row", |s| s.visible())
            .child(session_button(state, vault, id, TabKind::Sftp))
            .child(session_button(state, vault, id, TabKind::Terminal)),
    )
    .on_click({
        let state = state.clone();
        move |e: &ClickEvent, _, cx| {
            // Double-click connects; a single click opens the detail page.
            if e.click_count() >= 2 {
                state.update(cx, |s, cx| s.connect(vault, id, TabKind::Terminal, cx));
            } else {
                state.update(cx, |s, cx| s.preview(Tab::Host { vault, id }, cx));
            }
        }
    })
    .on_drag(drag, |drag, cursor_offset, _, cx| {
        cx.new(|_| DragLabel::new(drag.name.clone(), cursor_offset))
    });

    host_menu(state, vault, id, item).into_any_element()
}

/// Connect or open SFTP from the host row.
fn session_button(state: &Entity<AppState>, vault: Uuid, id: Uuid, kind: TabKind) -> Button {
    let state = state.clone();
    let (name, tip) = match kind {
        TabKind::Terminal => (IconName::Terminal, "Connect"),
        TabKind::Sftp => (IconName::FolderOpen, "Open SFTP"),
    };

    Button::new(SharedString::from(format!("{tip}-{id}")))
        .variant(ButtonVariant::Ghost)
        .size(ButtonSize::IconSmall)
        .tooltip(tip)
        .child(glyph(name, 13.))
        .on_click(move |_, _, cx| state.update(cx, |s, cx| s.connect(vault, id, kind, cx)))
}

/// Context menu of a host in the tree.
fn host_menu(
    state: &Entity<AppState>,
    vault: Uuid,
    id: Uuid,
    item: gpui_kit::Stateful<gpui_kit::Div>,
) -> impl IntoElement {
    let state = state.clone();

    item.context_menu(move |menu, _, _| {
        let s = state.clone();
        let terminal = state.clone();
        let sftp = state.clone();
        let edit = state.clone();

        menu.min_w(px(200.))
            .item(
                PopupMenuItem::new("Connect")
                    .icon(IconName::Terminal)
                    .on_click(move |_, _, cx| {
                        terminal.update(cx, |s, cx| s.connect(vault, id, TabKind::Terminal, cx))
                    }),
            )
            .item(
                PopupMenuItem::new("SFTP")
                    .icon(IconName::FolderOpen)
                    .on_click(move |_, _, cx| {
                        sftp.update(cx, |s, cx| s.connect(vault, id, TabKind::Sftp, cx))
                    }),
            )
            .item(PopupMenuItem::new("Edit").icon(IconName::Pencil).on_click(
                move |_, window, cx| {
                    edit.update(cx, |s, cx| s.open_pinned(Tab::Host { vault, id }, cx));
                    open_host_form(edit.clone(), vault, Some(id), None, window, cx)
                },
            ))
            .separator()
            .item(
                PopupMenuItem::new("Delete")
                    .icon(IconName::Trash)
                    .on_click(move |_, window, cx| delete_host(s.clone(), vault, id, window, cx)),
            )
    })
}

/// Context menu of a group in the tree. Deleting a group moves its hosts to "No group".
fn group_menu(
    state: &Entity<AppState>,
    vault: Uuid,
    id: Uuid,
    item: gpui_kit::Stateful<gpui_kit::Div>,
) -> impl IntoElement {
    let state = state.clone();

    item.context_menu(move |menu, _, _| {
        let (new, rename, delete) = (state.clone(), state.clone(), state.clone());

        menu.min_w(px(200.))
            .item(
                PopupMenuItem::new("New host")
                    .icon(IconName::Plus)
                    .on_click(move |_, window, cx| {
                        open_host_form(new.clone(), vault, None, Some(id), window, cx)
                    }),
            )
            .item(
                PopupMenuItem::new("Rename")
                    .icon(IconName::Pencil)
                    .on_click(move |_, window, cx| {
                        open_group_form(rename.clone(), vault, Some(id), window, cx)
                    }),
            )
            .separator()
            .item(PopupMenuItem::new("Delete").icon(IconName::Trash).on_click(
                move |_, window, cx| {
                    let state = delete.clone();
                    let name = state
                        .read(cx)
                        .views
                        .get(&vault)
                        .and_then(|v| v.groups.iter().find(|g| g.id == id))
                        .map(|g| g.name.clone())
                        .unwrap_or_default();

                    confirm(
                        format!("Delete {name}"),
                        "The group's hosts move to \"No group\". No host is deleted.",
                        "Delete",
                        window,
                        cx,
                        move |_, cx| {
                            state.update(cx, |s, cx| {
                                s.change(vault, |v| v.delete(vault, id), cx);
                            })
                        },
                    );
                },
            ))
    })
}

/// Asks for confirmation, then deletes the host, with its sessions and its detail page.
pub fn delete_host(
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
        .and_then(|v| v.hosts.iter().find(|h| h.id == id))
        .map(|h| h.name.clone())
        .unwrap_or_else(|| "?".into());

    confirm(
        format!("Delete {name}"),
        "The host and its access methods are deleted. Keychain keys are not touched.",
        "Delete",
        window,
        cx,
        move |_, cx| {
            state.update(cx, |s, cx| {
                if s.change(vault, |v| v.delete(vault, id), cx).is_some() {
                    s.close_host(id, cx);
                }
            })
        },
    );
}
