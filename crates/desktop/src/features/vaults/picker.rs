// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! List of vaults, like Zed's recent projects list.

use crate::app::state::{AppState, Route, Section, pick};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{Button, ButtonSize, ButtonVariant, confirm, icon, note};
use crate::components::{ContextMenuExt, PopupMenuItem};
use crate::features::vaults::create::open_create;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div,
};
use vaults::VaultItem;

pub struct VaultPicker {
    state: Entity<AppState>,
    error: Option<String>,
    _observe: Subscription,
}

impl VaultPicker {
    pub fn new(state: Entity<AppState>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            _observe: cx.observe(&state, |_, _, cx| cx.notify()),
            state,
            error: None,
        }
    }

    fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();

        pick(
            "Choose the vault folder",
            false,
            window,
            cx,
            move |dir, _, cx| {
                this.update(cx, |this, cx| {
                    match this.state.update(cx, |s, cx| s.add_existing(dir, cx)) {
                        Ok(vault) => this
                            .state
                            .update(cx, |s, cx| s.go(Route::Vault(vault, Section::Hosts), cx)),
                        Err(e) => this.error = Some(e),
                    }

                    cx.notify();
                });
            },
        );
    }

    fn relocate(&mut self, vault: VaultItem, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let prompt = format!("New location of {}", vault.name);

        pick(&prompt, false, window, cx, move |dir, _, cx| {
            this.update(cx, |this, cx| {
                this.error = this
                    .state
                    .update(cx, |s, cx| s.relocate(vault.id, dir, cx))
                    .err();
                cx.notify();
            });
        });
    }

    fn forget(&mut self, vault: VaultItem, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.clone();

        confirm(
            format!("Remove {} from the list", vault.name),
            "The vault disappears from this list, but its folder and data are not deleted. You can add it back with \"Open folder…\".",
            "Remove",
            window,
            cx,
            move |_, cx| state.update(cx, |s, cx| s.forget(vault.id, cx)),
        );
    }

    /// A list row: clicking it opens the vault; the ✕ removes it from the list. Right-click gives:
    /// open, lock (if unlocked) and remove. If its folder is missing, it does not open: changing its
    /// location is offered instead.
    fn vault_row(
        &self,
        vault: &VaultItem,
        opening: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = colors();
        let id = vault.id;
        let missing = vault.missing;
        let this = cx.entity();

        let (status_icon, status_color) = if missing {
            (IconName::TriangleAlert, c.destructive)
        } else if vault.unlocked {
            (IconName::Vault, c.muted_foreground)
        } else {
            (IconName::Lock, c.muted_foreground)
        };

        let status = if missing {
            div()
                .text_size(px(11.))
                .text_color(c.destructive)
                .child("Not found")
                .into_any_element()
        } else if vault.unlocked {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(11.))
                .text_color(c.success)
                .child(div().size(px(6.)).rounded_full().bg(c.success))
                .child("Unlocked")
                .into_any_element()
        } else {
            div()
                .text_size(px(11.))
                .text_color(c.muted_foreground)
                .child(if opening { "Unlocking…" } else { "Locked" })
                .into_any_element()
        };

        let menu_vault = vault.clone();
        let menu_this = this.clone();
        let menu_state = self.state.clone();

        div()
            .flex()
            .items_center()
            .id(SharedString::from(format!("vault-{id}")))
            .gap(px(12.))
            .px(px(12.))
            .h(px(52.))
            .when(!missing, |this| this.hover(|s| s.bg(c.list_hover)))
            .when(!missing, |row| {
                let state = self.state.clone();
                row.cursor_pointer().on_click(move |_, _, cx| {
                    state.update(cx, |s, cx| s.go(Route::Vault(id, Section::Hosts), cx))
                })
            })
            .child(
                div()
                    .size(px(32.))
                    .rounded(px(6.))
                    .bg(c.secondary)
                    .border_1()
                    .border_color(c.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(status_icon, 14., status_color)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(vault.name.clone())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(c.muted_foreground)
                            .truncate()
                            .child(vault.path.clone()),
                    ),
            )
            .child(status)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(missing, |row| {
                        let vault = vault.clone();
                        let this = this.clone();

                        row.child(
                            Button::new(SharedString::from(format!("relocate-{id}")))
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Small)
                                .icon(IconName::Folder)
                                .label("Change location…")
                                .on_click(move |_, window, cx| {
                                    this.update(cx, |this, cx| {
                                        this.relocate(vault.clone(), window, cx)
                                    })
                                }),
                        )
                    })
                    .child({
                        let vault = vault.clone();
                        let this = this.clone();

                        Button::new(SharedString::from(format!("forget-{id}")))
                            .variant(ButtonVariant::Destructive)
                            .size(ButtonSize::Icon)
                            .icon(IconName::X)
                            .tooltip("Remove from the list (does not delete the folder)")
                            .on_click(move |_, window, cx| {
                                this.update(cx, |this, cx| this.forget(vault.clone(), window, cx))
                            })
                    }),
            )
            .context_menu(move |menu, _, _| {
                let vault = menu_vault.clone();
                let this = menu_this.clone();
                let mut menu = menu.min_w(px(200.));

                menu = if vault.missing {
                    let (vault, this) = (vault.clone(), this.clone());
                    menu.item(
                        PopupMenuItem::new("Change location…")
                            .icon(IconName::Folder)
                            .on_click(move |_, window, cx| {
                                this.update(cx, |this, cx| this.relocate(vault.clone(), window, cx))
                            }),
                    )
                } else {
                    let state = menu_state.clone();
                    menu.item(PopupMenuItem::new("Open").icon(IconName::Folder).on_click(
                        move |_, _, cx| {
                            state.update(cx, |s, cx| s.go(Route::Vault(id, Section::Hosts), cx))
                        },
                    ))
                };

                if vault.unlocked {
                    let state = menu_state.clone();
                    menu = menu.item(
                        PopupMenuItem::new("Lock")
                            .icon(IconName::Lock)
                            .on_click(move |_, _, cx| state.update(cx, |s, cx| s.lock(id, cx))),
                    );
                }

                let (vault, this) = (vault.clone(), this.clone());

                menu.separator().item(
                    PopupMenuItem::new("Remove from the list")
                        .icon(IconName::X)
                        .on_click(move |_, window, cx| {
                            this.update(cx, |this, cx| this.forget(vault.clone(), window, cx))
                        }),
                )
            })
    }
}

impl Render for VaultPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();

        let s = self.state.read(cx);
        let vaults = s.list.clone();
        let unlocking = s.unlocking.clone();

        let list = if vaults.is_empty() {
            div()
                .border_1()
                .border_dashed()
                .border_color(c.border)
                .rounded(px(6.))
                .px(px(16.))
                .py(px(32.))
                .text_center()
                .text_color(c.muted_foreground)
                .child("There are no vaults yet. Create a new one or open the folder of an existing one.")
                .into_any_element()
        } else {
            let mut list = div()
                .flex()
                .flex_col()
                .border_1()
                .border_color(c.border)
                .rounded(px(6.))
                .bg(c.card)
                .overflow_hidden();

            for (i, vault) in vaults.iter().enumerate() {
                list = list.child(
                    div()
                        .when(i > 0, |this| this.border_t_1().border_color(c.border))
                        .child(self.vault_row(vault, unlocking.contains(&vault.id), cx)),
                );
            }

            list.into_any_element()
        };

        let this = cx.entity();

        div().flex_1().flex().items_center().justify_center().child(
            div()
                .flex()
                .flex_col()
                .w(px(520.))
                .child(div().text_size(px(20.)).child("Vaults"))
                .child(
                    div()
                        .text_color(c.muted_foreground)
                        .mb(px(16.))
                        .child("Each vault is a folder with its own master password."),
                )
                .child(list)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mt(px(12.))
                        .child({
                            let this = this.clone();
                            Button::new("open-folder")
                                .icon(IconName::Folder)
                                .label("Open folder…")
                                .on_click(move |_, window, cx| {
                                    this.update(cx, |this, cx| this.open_folder(window, cx))
                                })
                        })
                        .child({
                            let state = self.state.clone();
                            Button::new("create-vault")
                                .variant(ButtonVariant::Ghost)
                                .icon(IconName::Plus)
                                .label("Create new vault")
                                .on_click(move |_, window, cx| {
                                    open_create(state.clone(), window, cx)
                                })
                        }),
                )
                .when_some(self.error.clone(), |this, error| {
                    this.child(div().mt(px(12.)).child(note(error, c.destructive)))
                }),
        )
    }
}
