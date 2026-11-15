// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! A host's detail page, with its access methods and actions (connect, SFTP, edit).

use crate::app::state::AppState;
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::{
    Button, ButtonSize, ButtonVariant, confirm, method_badge, property, section_title,
};
use crate::components::{ContextMenuExt, PopupMenuItem};
use crate::features::hosts::group_name;
use crate::features::hosts::host_form::open_host_form;
use crate::features::hosts::method_form::open_method_form;
use crate::features::hosts::tree::delete_host;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, Entity, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    Window, div,
};
use sessions::TabKind;
use uuid::Uuid;

/// A host with its access methods.
pub fn host_detail(state: &Entity<AppState>, vault: Uuid, id: Uuid, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);

    let Some((view, host)) = s
        .views
        .get(&vault)
        .and_then(|v| v.hosts.iter().find(|h| h.id == id).map(|h| (v, h.clone())))
    else {
        return crate::layout::not_found("The host").into_any_element();
    };

    let no_methods = host.methods.is_empty();
    let st = state.clone();

    let header = div()
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
                        .child(host.name.clone()),
                )
                .child(
                    div()
                        .font_family(mono_font())
                        .text_size(px(12.))
                        .text_color(c.muted_foreground)
                        .mt(px(4.))
                        .child(format!("{}:{}", host.address, host.port)),
                ),
        )
        .child(
            Button::new("edit")
                .icon(IconName::Pencil)
                .label("Edit")
                .on_click({
                    let state = st.clone();
                    move |_, window, cx| {
                        open_host_form(state.clone(), vault, Some(id), None, window, cx)
                    }
                }),
        )
        .child(
            Button::new("sftp")
                .icon(IconName::FolderOpen)
                .label("SFTP")
                .disabled(no_methods)
                .on_click({
                    let state = st.clone();
                    move |_, _, cx| {
                        state.update(cx, |s, cx| s.connect(vault, id, TabKind::Sftp, cx))
                    }
                }),
        )
        .child(
            Button::new("connect")
                .variant(ButtonVariant::Primary)
                .label("Connect")
                .disabled(no_methods)
                .on_click({
                    let state = st.clone();
                    move |_, _, cx| {
                        state.update(cx, |s, cx| s.connect(vault, id, TabKind::Terminal, cx))
                    }
                }),
        );

    let address = host.address.clone();

    let connection = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(property(
            "Address",
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().truncate().child(host.address.clone()))
                .child(
                    Button::new("copy-address")
                        .variant(ButtonVariant::Ghost)
                        .size(crate::components::ButtonSize::IconSmall)
                        .icon(IconName::Copy)
                        .tooltip("Copy")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                address.clone(),
                            ))
                        }),
                ),
        ))
        .child(property("Port", div().child(host.port.to_string())))
        .child(property("Group", div().child(group_name(view, &host))));

    let methods = if no_methods {
        div()
            .text_color(c.muted_foreground)
            .child("This host has no access methods. Add one to be able to connect.")
            .into_any_element()
    } else {
        let mut list = div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(c.border)
            .rounded(px(6.))
            .overflow_hidden();

        for (i, method) in host.methods.iter().enumerate() {
            let m = method.id;
            let broken = method.broken;
            let (connect, sftp, edit, remove) = (st.clone(), st.clone(), st.clone(), st.clone());

            let item = div()
                .flex()
                .items_center()
                .id(SharedString::from(format!("method-{m}")))
                .gap(px(12.))
                .pl(px(12.))
                .pr(px(6.))
                .h(px(40.))
                .when(i > 0, |this| this.border_t_1().border_color(c.border))
                .child(method_badge(method.password))
                .child(method.title.clone())
                .child(
                    div()
                        .truncate()
                        .text_color(if broken {
                            c.destructive
                        } else {
                            c.muted_foreground
                        })
                        .child(method.detail.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .ml_auto()
                        .gap(px(2.))
                        .child(
                            Button::new(SharedString::from(format!("connect-{m}")))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Small)
                                .label("Connect")
                                .disabled(broken)
                                .on_click({
                                    let state = connect.clone();
                                    move |_, _, cx| {
                                        state.update(cx, |s, cx| {
                                            s.open_tab(vault, id, m, TabKind::Terminal, cx)
                                        })
                                    }
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("sftp-{m}")))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Small)
                                .label("SFTP")
                                .disabled(broken)
                                .on_click({
                                    let state = sftp.clone();
                                    move |_, _, cx| {
                                        state.update(cx, |s, cx| {
                                            s.open_tab(vault, id, m, TabKind::Sftp, cx)
                                        })
                                    }
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("edit-{m}")))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Icon)
                                .icon(IconName::Pencil)
                                .tooltip("Edit")
                                .on_click({
                                    let state = edit.clone();
                                    move |_, window, cx| {
                                        open_method_form(
                                            state.clone(),
                                            vault,
                                            id,
                                            Some(m),
                                            window,
                                            cx,
                                        )
                                    }
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("remove-{m}")))
                                .variant(ButtonVariant::Destructive)
                                .size(ButtonSize::Icon)
                                .icon(IconName::Trash)
                                .tooltip("Remove")
                                .on_click({
                                    let state = remove.clone();
                                    move |_, window, cx| {
                                        remove_method(state.clone(), vault, id, m, window, cx)
                                    }
                                }),
                        ),
                );

            // Right-click on a method: the same as its buttons.
            let menu_state = st.clone();
            let item = item.context_menu(move |menu, _, _| {
                let (a, b, e, r) = (
                    menu_state.clone(),
                    menu_state.clone(),
                    menu_state.clone(),
                    menu_state.clone(),
                );
                let mut menu = menu.min_w(px(200.));

                if !broken {
                    menu = menu
                        .item(
                            PopupMenuItem::new("Connect")
                                .icon(IconName::Terminal)
                                .on_click(move |_, _, cx| {
                                    a.update(cx, |s, cx| {
                                        s.open_tab(vault, id, m, TabKind::Terminal, cx)
                                    })
                                }),
                        )
                        .item(
                            PopupMenuItem::new("SFTP")
                                .icon(IconName::FolderOpen)
                                .on_click(move |_, _, cx| {
                                    b.update(cx, |s, cx| {
                                        s.open_tab(vault, id, m, TabKind::Sftp, cx)
                                    })
                                }),
                        );
                }

                menu.item(PopupMenuItem::new("Edit").icon(IconName::Pencil).on_click(
                    move |_, window, cx| {
                        open_method_form(e.clone(), vault, id, Some(m), window, cx)
                    },
                ))
                .separator()
                .item(
                    PopupMenuItem::new("Remove").icon(IconName::Trash).on_click(
                        move |_, window, cx| remove_method(r.clone(), vault, id, m, window, cx),
                    ),
                )
            });

            list = list.child(item);
        }

        list.into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .id("host-detail")
        .size_full()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .flex_col()
                .max_w(px(720.))
                .px(px(32.))
                .py(px(24.))
                .child(header)
                .child(section_title("Connection"))
                .child(connection)
                .child(section_title("Access methods"))
                .child(methods)
                .child(
                    div().flex().items_center().mt(px(8.)).ml(px(-8.)).child(
                        Button::new("add-method")
                            .variant(ButtonVariant::Ghost)
                            .icon(IconName::Plus)
                            .label("Add access method")
                            .on_click({
                                let state = st.clone();
                                move |_, window, cx| {
                                    open_method_form(state.clone(), vault, id, None, window, cx)
                                }
                            }),
                    ),
                )
                .when(!host.notes.is_empty(), |this| {
                    this.child(section_title("Notes")).child(
                        div()
                            .text_color(c.muted_foreground)
                            .child(host.notes.clone()),
                    )
                })
                .child(section_title("Danger zone"))
                .child(
                    div().flex().items_center().ml(px(-8.)).child(
                        Button::new("delete-host")
                            .variant(ButtonVariant::Destructive)
                            .icon(IconName::Trash)
                            .label("Delete host")
                            .on_click({
                                let state = st.clone();
                                move |_, window, cx| {
                                    delete_host(state.clone(), vault, id, window, cx)
                                }
                            }),
                    ),
                ),
        )
        .into_any_element()
}

fn remove_method(
    state: Entity<AppState>,
    vault: Uuid,
    host: Uuid,
    method: Uuid,
    window: &mut Window,
    cx: &mut App,
) {
    confirm(
        "Remove access method",
        "This access method is removed from the host. If it uses a key, the key stays in the keychain.",
        "Remove",
        window,
        cx,
        move |_, cx| {
            state.update(cx, |s, cx| {
                s.change(vault, |v| v.remove_method(vault, host, method), cx);
            })
        },
    );
}
