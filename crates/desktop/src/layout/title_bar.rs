// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Title bar, like Zed's: on the left the vault switcher (with what is done with the
//! vault: its settings, locking it), and on the right the app settings. Drag it to
//! move the window; on Windows and Linux it holds the app menu and the window buttons (the
//! gpui-kit adds them). On macOS it leaves room for the traffic lights, and the menu goes in the system bar.

use crate::app::theme::px;
use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Action, Anchor, AnyElement, App, Entity, InteractiveElement, IntoElement, OwnedMenuItem,
    ParentElement, Styled, Window, div,
};

use crate::app::menu::{OpenSettings, OpenVaultSettings, shortcut};
use crate::app::state::{AppState, Route, Section};
use crate::app::theme::colors;
use crate::components::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, PopupMenuItem, glyph, icon,
};

pub fn render(state: &Entity<AppState>, window: &mut Window, cx: &mut App) -> AnyElement {
    let c = colors();

    TitleBar::new()
        .h(px(34.))
        .pb(px(-1.))
        .bg(c.titlebar_background)
        .border_b_1()
        .border_color(c.titlebar_border)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .when(cfg!(not(target_os = "macos")), |this| {
                    this.child(app_menu(window, cx))
                })
                .child(div().occlude().child(vault_switcher(state, cx))),
        )
        .child(div().flex_1())
        .when_some(state.read(cx).update.as_ref(), |this, update| {
            let s = state.read(cx);
            let (updating, error) = (s.updating, s.update_error.clone());
            let state = state.clone();

            let label = if updating {
                "Updating…".to_string()
            } else if error.is_some() {
                "Retry update".to_string()
            } else {
                format!("Update to {}", update.version)
            };

            // Without `occlude`, Windows reads the press as a bar drag and the click never arrives.
            this.child(
                div().occlude().child(
                    Button::new("update")
                        .label(label)
                        .when_some(error, |b, e| b.tooltip(e))
                        .icon(IconName::Download)
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Small)
                        .disabled(updating)
                        .mr_1()
                        .on_click(move |_, _, cx| state.update(cx, |s, cx| s.install_update(cx))),
                ),
            )
        })
        .child(settings_menu(state))
        .into_any_element()
}

/// The app menu (`app::menu`) on Windows and Linux: the button shows the menus (File, Edit…) in the
/// bar, like Zed, and each opens when clicked. They hide again once something is picked.
fn app_menu(window: &mut Window, cx: &mut App) -> impl IntoElement {
    let open = window.use_keyed_state("app-menu-open", cx, |_, _| false);
    let shown = *open.read(cx);
    let toggle = open.clone();

    let menus = if shown {
        cx.get_menus().unwrap_or_default()
    } else {
        Vec::new()
    };

    let buttons = menus.into_iter().enumerate().map(move |(ix, group)| {
        let open = open.clone();

        Button::new(("app-menu", ix))
            .label(group.name)
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Small)
            .dropdown_menu(move |mut menu, window, _| {
                menu = menu.min_w(px(220.));

                for item in group.items.clone() {
                    menu = match item {
                        OwnedMenuItem::Action {
                            name,
                            action,
                            disabled,
                            ..
                        } => {
                            let keys =
                                shortcut(window.highest_precedence_binding_for_action(&*action));
                            let open = open.clone();

                            let row = PopupMenuItem::element(move |_, _| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(16.))
                                    .child(div().flex_1().child(name.clone()))
                                    .child(
                                        div()
                                            .text_color(colors().muted_foreground)
                                            .child(keys.clone()),
                                    )
                            })
                            .disabled(disabled)
                            .on_click(move |_, window, cx| {
                                open.update(cx, |o, _| *o = false);
                                window.dispatch_action(action.boxed_clone(), cx);
                            });

                            menu.item(row)
                        }
                        OwnedMenuItem::Separator => menu.separator(),
                        _ => menu,
                    };
                }

                menu
            })
    });

    div()
        .occlude()
        .flex()
        .items_center()
        .gap(px(2.))
        .child(
            Button::new("app-menu")
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::IconSmall)
                .tooltip("Menu")
                .child(glyph(IconName::Menu, 14.))
                .on_click(move |_, _, cx| toggle.update(cx, |o, _| *o = !*o)),
        )
        .children(buttons)
}

/// Menu row with its shortcut on the right, like the ones in the app menu.
fn action_item(label: &'static str, name: IconName, action: Box<dyn Action>) -> PopupMenuItem {
    let run = action.boxed_clone();

    PopupMenuItem::element(move |window, _| {
        let keys = shortcut(window.highest_precedence_binding_for_action(&*action));

        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(div().flex_1().child(label))
            .child(div().text_color(colors().muted_foreground).child(keys))
    })
    .icon(name)
    .on_click(move |_, window, cx| window.dispatch_action(run.boxed_clone(), cx))
}

/// Vault dropdown, like Zed's project picker: switch vaults and do things with
/// the open one (its settings, locking it).
fn vault_switcher(state: &Entity<AppState>, cx: &App) -> impl IntoElement {
    let c = colors();
    let s = state.read(cx);
    let current = s.current();
    let open = current.filter(|v| s.views.contains_key(v));
    let label = current
        .and_then(|v| s.item(v))
        .map(|v| v.name.clone())
        .unwrap_or_else(|| "Vaults".into());
    let state = state.clone();

    Button::new("vault-switcher")
        .variant(ButtonVariant::Ghost)
        .size(ButtonSize::Small)
        .ml(px(4.))
        .text_color(c.foreground)
        .child(icon(IconName::Vault, 13., c.muted_foreground))
        .child(label)
        .child(icon(IconName::ChevronDown, 12., c.muted_foreground))
        .dropdown_menu(move |menu, _, cx| {
            let s = state.read(cx);
            let mut menu = menu.min_w(px(280.));

            for vault in s.list.clone() {
                let state = state.clone();
                let id = vault.id;
                let selected = Some(id) == current;

                let name = if vault.missing {
                    IconName::TriangleAlert
                } else if vault.unlocked {
                    IconName::Vault
                } else {
                    IconName::Lock
                };

                // Custom row: a green dot on the unlocked ones.
                let item = PopupMenuItem::element(move |_, _| {
                    let c = colors();

                    div()
                        .flex()
                        .items_center()
                        .w_full()
                        .gap(px(8.))
                        .h(px(24.))
                        .child(div().flex_1().truncate().child(vault.name.clone()))
                        .when(vault.unlocked, |this| {
                            this.child(div().size(px(6.)).rounded_full().bg(c.success))
                        })
                })
                .icon(name)
                // It can't be opened without its folder: change its location from the vault list.
                .disabled(vault.missing)
                .checked(selected)
                .on_click(move |_, _, cx| {
                    state.update(cx, |s, cx| {
                        let section = s.section().unwrap_or(Section::Hosts);
                        s.go(Route::Vault(id, section), cx)
                    });
                });

                menu = menu.item(item);
            }

            menu = menu.separator();

            if let Some(vault) = open {
                let lock = state.clone();

                menu = menu
                    .item(action_item(
                        "Vault settings",
                        IconName::SlidersHorizontal,
                        Box::new(OpenVaultSettings),
                    ))
                    .item(
                        PopupMenuItem::new("Lock vault")
                            .icon(IconName::Lock)
                            .on_click(move |_, _, cx| lock.update(cx, |s, cx| s.lock(vault, cx))),
                    )
                    .separator();
            }

            let state = state.clone();

            menu.item(
                PopupMenuItem::new("All vaults…")
                    .icon(IconName::LayoutList)
                    .on_click(move |_, _, cx| state.update(cx, |s, cx| s.go(Route::Picker, cx))),
            )
        })
}

/// Chevron that opens the app settings and, if a vault is open, the vault's settings.
fn settings_menu(state: &Entity<AppState>) -> impl IntoElement {
    let c = colors();
    let menu = state.clone();

    div().occlude().flex().items_center().mr(px(6.)).child(
        Button::new("user-menu")
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Icon)
            .tooltip("Settings")
            .child(icon(IconName::ChevronDown, 12., c.muted_foreground))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |m, _, cx| {
                let in_vault = {
                    let s = menu.read(cx);
                    s.current().is_some_and(|v| s.views.contains_key(&v))
                };

                let m = m.min_w(px(220.)).item(action_item(
                    "Settings",
                    IconName::Settings,
                    Box::new(OpenSettings),
                ));

                if in_vault {
                    m.item(action_item(
                        "Vault settings",
                        IconName::SlidersHorizontal,
                        Box::new(OpenVaultSettings),
                    ))
                } else {
                    m
                }
            }),
    )
}
