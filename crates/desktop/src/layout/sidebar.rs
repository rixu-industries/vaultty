// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Sidebar of a vault: the section name with its create button, the search box and the list.
//! The section is picked with the buttons in the status bar (like Zed's panels).

use crate::app::state::{AppState, Route, Section};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, Input, InputEvent, InputState, PopupMenuItem,
    glyph, icon,
};
use crate::features::hosts::group_form::open_group_form;
use crate::features::hosts::host_form::open_host_form;
use crate::features::hosts::tree::host_tree;
use crate::features::keys::generate::open_generate;
use crate::features::keys::key_form::open_key_form;
use crate::features::keys::list::key_list;
use crate::features::tunnels::form::open_tunnel_form;
use crate::features::tunnels::list::tunnel_list;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div,
};

pub struct Sidebar {
    state: Entity<AppState>,
    search: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl Sidebar {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search…"));

        // The search text survives route changes: it lives in the state.
        let subs = vec![
            cx.subscribe(&search, |this, input, e: &InputEvent, cx| {
                if let InputEvent::Change = e {
                    let text = input.read(cx).value().to_string();
                    this.state.update(cx, |s, cx| {
                        s.search = text;
                        cx.notify();
                    });
                }
            }),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];

        Self {
            state,
            search,
            _subs: subs,
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let s = self.state.read(cx);

        let Route::Vault(vault, current) = s.route else {
            return div().into_any_element();
        };

        let title = match current {
            Section::Hosts => "Hosts",
            Section::Keys => "Keychain",
            Section::Tunnels => "Tunnels",
        };

        let st = self.state.clone();
        let new_menu = Button::new("new")
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::IconSmall)
            .tooltip("New…")
            .child(glyph(IconName::Plus, 14.))
            .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu, _, _| {
                let (a, b) = (st.clone(), st.clone());
                let menu = menu.min_w(px(180.));

                match current {
                    Section::Keys => menu
                        .item(
                            PopupMenuItem::new("Import key")
                                .icon(IconName::Folder)
                                .on_click(move |_, window, cx| {
                                    open_key_form(a.clone(), vault, None, window, cx)
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Generate key")
                                .icon(IconName::Key)
                                .on_click(move |_, window, cx| {
                                    open_generate(b.clone(), vault, window, cx)
                                }),
                        ),
                    Section::Tunnels => menu.item(
                        PopupMenuItem::new("New tunnel")
                            .icon(IconName::Network)
                            .on_click(move |_, window, cx| {
                                open_tunnel_form(a.clone(), vault, None, window, cx)
                            }),
                    ),
                    Section::Hosts => menu
                        .item(
                            PopupMenuItem::new("New host")
                                .icon(IconName::Server)
                                .on_click(move |_, window, cx| {
                                    open_host_form(a.clone(), vault, None, None, window, cx)
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New group")
                                .icon(IconName::Folder)
                                .on_click(move |_, window, cx| {
                                    open_group_form(b.clone(), vault, None, window, cx)
                                }),
                        ),
                }
            });

        let list = match current {
            Section::Keys => key_list(&self.state, vault, cx),
            Section::Tunnels => tunnel_list(&self.state, vault, cx),
            // While a host is being dragged, all groups are shown as drop targets.
            Section::Hosts => host_tree(&self.state, vault, cx.has_active_drag(), cx),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .flex_shrink_0()
            .bg(c.sidebar)
            .text_color(c.sidebar_foreground)
            .border_r_1()
            .border_color(c.sidebar_border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(34.))
                    .flex_shrink_0()
                    .pl(px(12.))
                    .pr(px(6.))
                    .text_size(px(11.))
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(c.panel_header_foreground)
                    .child(title.to_uppercase())
                    .child(div().flex_1())
                    .child(new_menu),
            )
            .child(
                div().mx(px(8.)).mb(px(6.)).child(
                    Input::new(&self.search)
                        .prefix(icon(IconName::Search, 13., c.muted_icon))
                        .cleanable(true),
                ),
            )
            .child(
                div()
                    .id("sidebar-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .pb(px(8.))
                    .child(list),
            )
            .into_any_element()
    }
}
