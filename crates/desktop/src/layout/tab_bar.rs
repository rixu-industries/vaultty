// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! A pane's tab bar: the tabs of the current vault and the global ones (settings) it holds,
//! with drag to reorder them or move them to another pane. Detail tabs (host, key, tunnel) open as a preview (in
//! italics) and the next one replaces it; double-click pins it.

use crate::app::state::AppState;
use crate::app::tabs::{OpenTab, Tab};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{ContextMenuExt, DragLabel, PopupMenuItem, icon, os_icon};
use crate::features::sftp::name_dialog::open_name_dialog;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div,
};
use sessions::TabKind;
use uuid::Uuid;

/// What gets dragged when reordering tabs.
#[derive(Clone)]
pub struct DragTab {
    pub id: Uuid,
    pub title: String,
}

/// Icon and title of a tab.
fn label(s: &AppState, tab: &OpenTab) -> (AnyElement, String) {
    let c = colors();
    let muted = c.muted_foreground;
    let view = |vault: Uuid| s.views.get(&vault);

    match tab.tab {
        Tab::Session(id) => {
            let Some(info) = s.session(id) else {
                return (
                    icon(IconName::Terminal, 13., muted).into_any_element(),
                    "?".into(),
                );
            };

            let os = view(info.vault)
                .and_then(|v| v.hosts.iter().find(|h| h.id == info.host))
                .and_then(|h| h.os.clone());

            // SFTP keeps its folder icon, to tell it apart from the terminal of the same host.
            let leading = match info.kind {
                TabKind::Sftp => icon(IconName::FolderOpen, 13., muted).into_any_element(),
                TabKind::Terminal => os_icon(os.as_deref(), IconName::Terminal, 13., muted),
            };

            (leading, s.tab_title(info))
        }
        Tab::Host { vault, id } => {
            let host = view(vault).and_then(|v| v.hosts.iter().find(|h| h.id == id));

            (
                os_icon(
                    host.and_then(|h| h.os.as_deref()),
                    IconName::Server,
                    13.,
                    muted,
                ),
                host.map(|h| h.name.clone()).unwrap_or_else(|| "?".into()),
            )
        }
        Tab::Key { vault, id } => (
            icon(IconName::Key, 13., muted).into_any_element(),
            view(vault)
                .and_then(|v| v.keys.iter().find(|k| k.id == id))
                .map(|k| k.name.clone())
                .unwrap_or_else(|| "?".into()),
        ),
        Tab::Tunnel { vault, id } => (
            icon(IconName::Network, 13., muted).into_any_element(),
            view(vault)
                .and_then(|v| v.tunnels.iter().find(|t| t.id == id))
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "?".into()),
        ),
        Tab::VaultSettings(vault) => (
            icon(IconName::SlidersHorizontal, 13., muted).into_any_element(),
            match s.item(vault) {
                Some(v) => format!("{} settings", v.name),
                None => "Vault settings".into(),
            },
        ),
        Tab::KnownHosts(_) => (
            icon(IconName::Lock, 13., muted).into_any_element(),
            "Known Hosts".into(),
        ),
        Tab::Settings => (
            icon(IconName::Settings, 13., muted).into_any_element(),
            "Settings".into(),
        ),
        Tab::Licenses => (
            icon(IconName::ScrollText, 13., muted).into_any_element(),
            "Licenses".into(),
        ),
    }
}

/// A pane's bar: its tabs and, highlighted, the one being shown.
pub fn tab_bar(state: &Entity<AppState>, pane: Uuid, cx: &App) -> impl IntoElement {
    let c = colors();
    let s = state.read(cx);
    let shown = s.shown(pane);

    let mut bar = div()
        .flex()
        .id(SharedString::from(format!("tab-bar-{pane}")))
        .h(px(34.))
        .flex_shrink_0()
        .bg(c.tab_background)
        .overflow_x_scroll();

    for open in s.pane_tabs(pane) {
        let id = open.id;
        let session = matches!(open.tab, Tab::Session(_));
        let active = shown == Some(id);
        let (leading, title) = label(s, open);
        let title = s.tabs.renames.get(&id).cloned().unwrap_or(title);

        let (show, close, drop, menu, over) = (
            state.clone(),
            state.clone(),
            state.clone(),
            state.clone(),
            state.clone(),
        );
        let drag = DragTab {
            id,
            title: title.clone(),
        };
        let current = title.clone();

        let item = div()
            .flex()
            .items_center()
            .id(SharedString::from(format!("tab-{id}")))
            .cursor_pointer()
            .group("tab")
            .relative()
            .h_full()
            .gap(px(8.))
            .pl(px(12.))
            .pr(px(28.))
            .min_w(px(if active { 64. } else { 40. }))
            .max_w(px(220.))
            .flex_shrink_0()
            .border_r_1()
            .border_color(c.tab_border)
            // The active tab uses the content background and has no line below, as in Zed: so the
            // content below is clearly its own.
            .when(active, |this| {
                this.bg(c.tab_active_background)
                    .text_color(c.tab_active_foreground)
            })
            .when(!active, |this| {
                this.border_b_1()
                    .text_color(c.tab_foreground)
                    .hover(|s| s.text_color(c.tab_hover_foreground))
            })
            .child(leading)
            .child(
                div()
                    .truncate()
                    .when(open.preview, |this| this.italic())
                    .child(title),
            )
            .on_click(move |e: &ClickEvent, _, cx| {
                show.update(cx, |s, cx| {
                    s.show_tab(id, cx);

                    if e.click_count() >= 2 {
                        s.pin(id, cx);
                    }
                })
            })
            .on_drag(drag, |drag, cursor_offset, _, cx| {
                cx.new(|_| DragLabel::new(drag.title.clone(), cursor_offset))
            })
            // Drop indicator line, as in Zed: `move_tab` puts it after the target if coming from the left and
            // before it if coming from the right. `border_color` tints all borders, so the others are
            // removed; the padding offsets the width so the text does not move.
            .drag_over::<DragTab>(move |style, drag: &DragTab, _, cx| {
                let tabs = &over.read(cx).tabs.list;
                let at = |t| tabs.iter().position(|o| o.id == t);
                let style = style.border_0().border_color(c.tab_drop);

                match (at(drag.id), at(id)) {
                    (Some(from), Some(to)) if from < to => style.border_r_2().pr(px(27.)),
                    (Some(from), Some(to)) if from > to => {
                        style.border_l_2().pl(px(10.)).pr(px(29.))
                    }
                    _ => style.border_r_1().border_color(c.tab_border),
                }
            })
            .on_drop(move |drag: &DragTab, _, cx| {
                drop.update(cx, |s, cx| s.move_tab(drag.id, id, cx))
            })
            // ✕ only on the active tab or while the mouse is over it, floating over the `pr(28)`.
            .child(
                div()
                    .id(SharedString::from(format!("close-{id}")))
                    .cursor_pointer()
                    .absolute()
                    .right(px(6.))
                    .top(px(8.))
                    .size(px(18.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(!active, |this| {
                        this.invisible().group_hover("tab", |s| s.visible())
                    })
                    .hover(|s| s.bg(c.tab_close_hover))
                    .child(icon(IconName::X, 11., c.muted_foreground))
                    .on_click(move |_, _, cx| {
                        // Closing it must not also activate the tab.
                        cx.stop_propagation();
                        close.update(cx, |s, cx| s.close_tab(id, cx))
                    }),
            )
            .context_menu(move |m, _, cx| {
                let (reconnect, pin, close, others) =
                    (menu.clone(), menu.clone(), menu.clone(), menu.clone());
                let (right, down) = (menu.clone(), menu.clone());
                let mut m = m.min_w(px(200.));

                // Same as dropping it on the right or bottom edge of its pane. Not possible when it is
                // the only tab: the pane would be left empty.
                if menu.read(cx).pane_tabs(pane).nth(1).is_some() {
                    m = m
                        .item(
                            PopupMenuItem::new("Split right")
                                .icon(IconName::Columns2)
                                .on_click(move |_, _, cx| {
                                    right.update(cx, |s, cx| {
                                        s.drop_tab(id, pane, Some((false, false)), cx)
                                    })
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Split down")
                                .icon(IconName::Rows2)
                                .on_click(move |_, _, cx| {
                                    down.update(cx, |s, cx| {
                                        s.drop_tab(id, pane, Some((true, false)), cx)
                                    })
                                }),
                        )
                        .separator();
                }

                let m = if session {
                    m.item(
                        PopupMenuItem::new("Reconnect")
                            .icon(IconName::RefreshCw)
                            .on_click(move |_, _, cx| reconnect.update(cx, |s, _| s.reconnect(id))),
                    )
                    .separator()
                } else {
                    m.item(
                        PopupMenuItem::new("Keep open")
                            .icon(IconName::Pin)
                            .on_click(move |_, _, cx| pin.update(cx, |s, cx| s.pin(id, cx))),
                    )
                    .separator()
                };

                let rename = menu.clone();
                let current = current.clone();

                m.item(
                    PopupMenuItem::new("Rename")
                        .icon(IconName::Pencil)
                        .on_click(move |_, window, cx| {
                            let rename = rename.clone();

                            open_name_dialog(
                                "Rename tab",
                                current.clone(),
                                window,
                                cx,
                                move |name, cx| {
                                    rename.update(cx, |s, cx| s.rename_tab(id, name, cx))
                                },
                            )
                        }),
                )
                .item(
                    PopupMenuItem::new("Close")
                        .icon(IconName::X)
                        .on_click(move |_, _, cx| close.update(cx, |s, cx| s.close_tab(id, cx))),
                )
                .item(
                    PopupMenuItem::new("Close others").on_click(move |_, _, cx| {
                        others.update(cx, |s, cx| {
                            let rest: Vec<Uuid> = s
                                .pane_tabs(pane)
                                .map(|t| t.id)
                                .filter(|t| *t != id)
                                .collect();

                            for tab in rest {
                                s.close_tab(tab, cx);
                            }
                        })
                    }),
                )
            });

        bar = bar.child(item);
    }

    // Filler with the bottom line, as in Zed. Dropping a tab here moves it to this pane.
    let drop = state.clone();

    bar.child(
        div()
            .flex_1()
            .min_w(px(24.))
            .border_b_1()
            .border_color(c.tab_border)
            .drag_over::<DragTab>(move |style, _, _, _| style.bg(c.tab_drop.opacity(0.25)))
            .on_drop(move |drag: &DragTab, _, cx| {
                drop.update(cx, |s, cx| s.drop_tab(drag.id, pane, None, cx))
            }),
    )
}
