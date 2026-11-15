// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Global app settings: categories on the left and the selected one on the right. Each category is
//! its own view and all of them stay mounted, so what was typed is not lost when switching between them.
//!
//! Each category keeps its key in `settings.json` in the crate that owns it (`system`, `themes`, `terminal`,
//! `mcp`).

pub mod appearance;
pub mod general;
pub mod mcp;
pub mod terminal;

use crate::app::state::AppState;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{icon, note};
use crate::features::settings::appearance::AppearanceSettings;
use crate::features::settings::general::GeneralSettings;
use crate::features::settings::mcp::McpSettings;
use crate::features::settings::terminal::TerminalSettings;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyView, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    General,
    Appearance,
    Terminal,
    Mcp,
}

const CATEGORIES: [(Category, &str, IconName); 4] = [
    (Category::General, "General", IconName::Settings),
    (Category::Appearance, "Appearance", IconName::Palette),
    (Category::Terminal, "Terminal", IconName::SquareTerminal),
    (Category::Mcp, "MCP server", IconName::Bot),
];

/// Header of a category: its name and what it is for.
pub fn header(title: &str, description: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .mb(px(16.))
        .child(
            div()
                .text_size(px(20.))
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .text_color(colors().muted_foreground)
                .child(SharedString::from(description.to_string())),
        )
}

/// A setting in a row: name and explanation on the left, the control on the right.
pub fn setting(
    name: impl Into<SharedString>,
    help: impl Into<SharedString>,
    control: impl IntoElement,
) -> impl IntoElement {
    let c = colors();

    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(12.))
        .py(px(10.))
        .mb(px(8.))
        .border_1()
        .border_color(c.border)
        .rounded(px(6.))
        .bg(c.card)
        .text_color(c.card_foreground)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(name.into())
                .child(note(help, c.muted_foreground)),
        )
        .child(control)
}

pub struct SettingsView {
    category: Category,
    general: Entity<GeneralSettings>,
    appearance: Entity<AppearanceSettings>,
    terminal: Entity<TerminalSettings>,
    mcp: Entity<McpSettings>,
}

impl SettingsView {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            category: Category::General,
            general: cx.new(|_| GeneralSettings::default()),
            appearance: cx.new(|_| AppearanceSettings::default()),
            terminal: cx.new(|_| TerminalSettings::default()),
            mcp: cx.new(|cx| McpSettings::new(state, window, cx)),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();

        let body: AnyView = match self.category {
            Category::General => self.general.clone().into(),
            Category::Appearance => self.appearance.clone().into(),
            Category::Terminal => self.terminal.clone().into(),
            Category::Mcp => self.mcp.clone().into(),
        };

        let items = CATEGORIES.iter().map(|&(category, label, glyph)| {
            let selected = category == self.category;
            let this = cx.entity();

            div()
                .id(label)
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(26.))
                .px(px(10.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(selected, |d| {
                    d.bg(c.list_active).text_color(c.list_active_foreground)
                })
                .when(!selected, |d| {
                    d.text_color(c.list_foreground)
                        .hover(|s| s.bg(c.list_hover))
                })
                .child(icon(glyph, 13., c.muted_foreground))
                .child(label)
                .on_click(move |_, _, cx| {
                    this.update(cx, |t, cx| {
                        t.category = category;
                        cx.notify();
                    })
                })
        });

        div()
            .flex()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(220.))
                    .flex_shrink_0()
                    .gap(px(2.))
                    .p(px(8.))
                    .bg(c.sidebar)
                    .border_r_1()
                    .border_color(c.border)
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(6.))
                            .text_size(px(11.))
                            .text_color(c.muted_foreground)
                            .child("SETTINGS"),
                    )
                    .children(items),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scrollbar()
                    .child(div().max_w(px(680.)).px(px(32.)).py(px(24.)).child(body)),
            )
    }
}
