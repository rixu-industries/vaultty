// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Status bar, like Zed's: on the left the buttons for each panel (hosts, keys, tunnels),
//! in the middle the vault and the latest notice or error, and on the right the counts and the lock.

use crate::app::menu::{ShowHosts, ShowKeychain, ShowTunnels, shortcut};
use crate::app::state::{AppState, Section};
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{Button, ButtonSize, ButtonVariant, glyph, icon};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Action, AnyElement, App, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div,
};
use uuid::Uuid;

/// "1 host", "2 hosts": singular only when the count is 1.
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn status_bar(state: &Entity<AppState>, vault: Uuid, window: &Window, cx: &App) -> AnyElement {
    let c = colors();
    let s = state.read(cx);

    let (Some(info), Some(view)) = (s.item(vault), s.views.get(&vault)) else {
        return div().into_any_element();
    };

    // The shown section's button, marked while the panel is open. Clicking it collapses the panel.
    let panel = |section: Section, name: IconName, label: &str, action: Box<dyn Action>| {
        let active = s.section() == Some(section) && s.layout.sidebar_open;
        let keys = shortcut(window.highest_precedence_binding_for_action(&*action));
        let tip = if keys.is_empty() {
            label.to_string()
        } else {
            format!("{label}  {keys}")
        };
        let state = state.clone();

        Button::new(label.to_string())
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::IconSmall)
            .tooltip(tip)
            .text_color(c.statusbar_foreground)
            .when(active, |this| {
                this.bg(c.statusbar_item_active)
                    .text_color(c.statusbar_item_active_foreground)
            })
            .child(glyph(name, 13.))
            .on_click(move |_, _, cx| state.update(cx, |s, cx| s.show_section(section, true, cx)))
    };

    let message = if let Some(error) = s.error.clone() {
        // The error from the last action takes priority over everything else; a click dismisses it.
        let state = state.clone();

        div()
            .flex()
            .items_center()
            .id("status-error")
            .cursor_pointer()
            .gap(px(6.))
            .min_w_0()
            .text_color(c.destructive)
            .child(icon(IconName::TriangleAlert, 11., c.destructive))
            .child(div().truncate().child(error))
            .on_click(move |_, _, cx| state.update(cx, |s, cx| s.set_error(None, cx)))
            .into_any_element()
    } else if let Some(warning) = view.warnings.first().cloned() {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .text_color(c.warning)
            .child(icon(IconName::TriangleAlert, 11., c.warning))
            .child(div().truncate().child(warning))
            .into_any_element()
    } else {
        div()
            .truncate()
            .min_w_0()
            .child(info.path.clone())
            .into_any_element()
    };

    let sessions = s.session_list.iter().filter(|i| i.vault == vault).count();
    let lock = state.clone();

    div()
        .flex()
        .items_center()
        .h(px(26.))
        .flex_shrink_0()
        .gap(px(10.))
        .px(px(6.))
        .bg(c.statusbar_background)
        .border_t_1()
        .border_color(c.statusbar_border)
        .text_size(px(11.))
        .text_color(c.statusbar_foreground)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .child(panel(
                    Section::Hosts,
                    IconName::Server,
                    "Hosts",
                    Box::new(ShowHosts),
                ))
                .child(panel(
                    Section::Keys,
                    IconName::Key,
                    "Keychain",
                    Box::new(ShowKeychain),
                ))
                .child(panel(
                    Section::Tunnels,
                    IconName::Network,
                    "Tunnels",
                    Box::new(ShowTunnels),
                )),
        )
        .child(div().w(px(1.)).h(px(12.)).bg(c.statusbar_border))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .whitespace_nowrap()
                .child(div().size(px(6.)).rounded_full().bg(c.success))
                .child(info.name.clone()),
        )
        .child(message)
        .child(div().ml_auto().whitespace_nowrap().child(format!(
            "{} · {} · {}",
            plural(view.hosts.len(), "host", "hosts"),
            plural(view.keys.len(), "key", "keys"),
            plural(sessions, "session", "sessions"),
        )))
        .child(
            Button::new("lock-vault")
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::IconSmall)
                .tooltip("Lock vault")
                .text_color(c.statusbar_foreground)
                .child(glyph(IconName::Lock, 12.))
                .on_click(move |_, _, cx| lock.update(cx, |s, cx| s.lock(vault, cx))),
        )
        .into_any_element()
}
