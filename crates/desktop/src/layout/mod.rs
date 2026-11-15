// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Frame of an open vault, like a project's in Zed: a collapsible, resizable side panel
//! on the left, tabs and content on the right, and the status bar below (with the buttons for
//! each panel).

pub mod sidebar;
pub mod status_bar;
pub mod tab_bar;
pub mod title_bar;

use crate::app::menu::{
    GenerateKey, NewGroup, NewHost, NewKey, NewTunnel, OpenSettings, ToggleSidebar, shortcut,
};
use crate::app::state::{AppState, SIDEBAR_WIDTH, Section};
use crate::app::theme::{colors, px, unscaled};
use crate::components::{Button, ButtonVariant, glyph, icon};
use crate::layout::sidebar::Sidebar;
use crate::layout::status_bar::status_bar;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Action, AnyElement, App, AppContext, ClickEvent, Context, CursorStyle, Entity,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use uuid::Uuid;

/// Content for something that does not exist (for example, a deleted host's details).
pub fn not_found(what: &str) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(colors().muted_foreground)
        .child(format!("{what} does not exist."))
}

/// Sidebar, tabs and content, with the status bar below.
pub fn vault(
    state: &Entity<AppState>,
    vault: Uuid,
    sidebar: Entity<Sidebar>,
    content: AnyElement,
    window: &mut Window,
    cx: &App,
) -> impl IntoElement {
    let s = state.read(cx);
    let (open, width) = (s.layout.sidebar_open, s.layout.sidebar_width);
    let (resize, save) = (state.clone(), state.clone());

    let panel = div()
        .relative()
        .flex_shrink_0()
        .h_full()
        .w(px(width))
        .child(sidebar)
        .child(resize_handle(state));

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        // Dragging is tracked here, not on the edge: the mouse leaves its 6px strip right away.
        .on_drag_move::<ResizeSidebar>(move |e, window, cx| {
            cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);

            let width = unscaled(e.event.position.x - e.bounds.left());
            let max = unscaled(e.bounds.size.width) - 360.;
            resize.update(cx, |s, cx| s.resize_sidebar(width, max, cx));
        })
        .on_drop(move |_: &ResizeSidebar, _, cx| save.update(cx, |s, cx| s.save_layout(cx)))
        .child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .when(open, |this| this.child(panel))
                // The panes, each with its own tab bar (`Workspace::content`).
                .child(div().flex_1().min_w_0().min_h_0().child(content)),
        )
        .child(status_bar(state, vault, window, cx))
}

/// What gets dragged to change the panel width.
#[derive(Clone, Copy)]
struct ResizeSidebar;

/// Nothing is shown under the mouse while the edge is dragged: the panel is what changes.
struct Resizing;

impl Render for Resizing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Right edge of the panel, like Zed's: drag it to change the width, and double-click to
/// restore the default. It takes up 6px straddling the edge and is highlighted on hover.
fn resize_handle(state: &Entity<AppState>) -> impl IntoElement {
    let c = colors();
    let state = state.clone();

    div()
        .id("sidebar-resize")
        .absolute()
        .top_0()
        .bottom_0()
        .right(px(-3.))
        .w(px(6.))
        .cursor_col_resize()
        .group("sidebar-resize")
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(2.))
                .w(px(2.))
                .group_hover("sidebar-resize", |s| s.bg(c.sidebar_resize)),
        )
        .on_drag(ResizeSidebar, |_, _, _, cx| cx.new(|_| Resizing))
        .on_click(move |e: &ClickEvent, _, cx| {
            if e.click_count() >= 2 {
                state.update(cx, |s, cx| {
                    s.resize_sidebar(SIDEBAR_WIDTH.1, f32::MAX, cx);
                    s.save_layout(cx);
                });
            }
        })
}

/// An action for the empty state: text, icon and the menu action (which provides the shortcut).
type Shortcut = (&'static str, IconName, Box<dyn Action>);

/// With no active tab: what can be done in the section, with its shortcuts, like the empty
/// screen in Zed. That way, anyone arriving for the first time knows where to start.
pub fn empty(state: &Entity<AppState>, vault: Uuid, window: &mut Window, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);
    let section = s.section().unwrap_or(Section::Hosts);

    let Some(view) = s.views.get(&vault) else {
        return div().into_any_element();
    };

    let (glyph_name, hint, actions): (IconName, &str, Vec<Shortcut>) = match section {
        Section::Hosts => (
            IconName::Server,
            if view.hosts.is_empty() {
                "This vault has no hosts yet."
            } else {
                "Click a host to see it. Double-click to connect."
            },
            vec![
                ("New host", IconName::Plus, Box::new(NewHost)),
                ("New group", IconName::Folder, Box::new(NewGroup)),
            ],
        ),
        Section::Keys => (
            IconName::Key,
            if view.keys.is_empty() {
                "The keychain is empty."
            } else {
                "Click a key to see it and the hosts that use it."
            },
            vec![
                ("Import key", IconName::Folder, Box::new(NewKey)),
                ("Generate key", IconName::Plus, Box::new(GenerateKey)),
            ],
        ),
        Section::Tunnels => (
            IconName::Network,
            if view.tunnels.is_empty() {
                "There are no tunnels yet."
            } else {
                "Click a tunnel to see its status."
            },
            vec![("New tunnel", IconName::Plus, Box::new(NewTunnel))],
        ),
    };

    let common: Vec<Shortcut> = vec![
        ("Toggle panel", IconName::PanelLeft, Box::new(ToggleSidebar)),
        ("Settings", IconName::Settings, Box::new(OpenSettings)),
    ];

    let row = |ix: usize, (label, name, action): Shortcut| {
        let keys = shortcut(window.highest_precedence_binding_for_action(&*action));

        Button::new(("empty-action", ix))
            .variant(ButtonVariant::Ghost)
            .w_full()
            .justify_start()
            .h(px(30.))
            .child(glyph(name, 14.))
            .child(
                div()
                    .flex_1()
                    .text_color(c.foreground)
                    .child(label.to_string()),
            )
            .when(!keys.is_empty(), |this| {
                this.child(
                    div()
                        .px(px(5.))
                        .rounded(px(4.))
                        .bg(c.muted)
                        .text_size(px(11.))
                        .text_color(c.muted_foreground)
                        .child(SharedString::from(keys)),
                )
            })
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
    };

    let n = actions.len();
    let rows = actions.into_iter().enumerate().map(|(i, a)| row(i, a));
    let common = common.into_iter().enumerate().map(|(i, a)| row(n + i, a));

    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(300.))
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .mb(px(20.))
                        .child(icon(glyph_name, 32., c.muted_icon))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(c.muted_foreground)
                                .text_center()
                                .child(hint),
                        ),
                )
                .children(rows)
                .child(div().my(px(6.)).h(px(1.)).bg(c.border_subtle))
                .children(common),
        )
        .into_any_element()
}
