// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! "Terminal" category: the font and its size. Colors come from the theme (Appearance).

use crate::app::prefs::Prefs;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{choice, note};
use crate::features::settings::{header, setting};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, Window, div};

const SIZES: [f32; 9] = [10., 11., 12., 13., 14., 15., 16., 18., 20.];

#[derive(Default)]
pub struct TerminalSettings {
    error: Option<String>,
    /// The system fonts; they are requested once, the first time it is painted.
    fonts: Vec<String>,
}

impl TerminalSettings {
    fn update(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut terminal::Settings)) {
        let mut settings = Prefs::get(cx).terminal.clone();
        edit(&mut settings);

        self.error = Prefs::set_terminal(settings, cx).err();
        cx.notify();
    }
}

impl Render for TerminalSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();

        if self.fonts.is_empty() {
            let mut fonts = window.text_system().all_font_names();
            fonts.sort();
            fonts.dedup();
            self.fonts = fonts;
        }

        let settings = Prefs::get(cx).terminal.clone();
        let (on_family, on_size) = (cx.entity(), cx.entity());

        // `None` is "the built-in one": so if Cascadia Mono is installed later, it is picked up automatically.
        let families = std::iter::once((None, "Default".to_string()))
            .chain(self.fonts.iter().map(|f| (Some(f.clone()), f.clone())))
            .collect();
        let sizes = SIZES.iter().map(|s| (*s, format!("{s} px"))).collect();

        div()
            .flex()
            .flex_col()
            .child(header(
                "Terminal",
                "How terminal tabs look. Colors come from the theme in Appearance.",
            ))
            .when_some(self.error.clone(), |this, e| {
                this.child(div().mb(px(8.)).child(note(e, c.destructive)))
            })
            .child(setting(
                "Font",
                "A monospaced font. Default uses Cascadia Mono if it is installed, or the system one.",
                div().w(px(220.)).child(choice(
                    "terminal-font",
                    families,
                    Some(settings.font_family.clone()),
                    move |family, _, cx| {
                        on_family.update(cx, |t, cx| t.update(cx, |s| s.font_family = family));
                    },
                )),
            ))
            .child(setting(
                "Font size",
                "Open terminals resize to the new size right away.",
                div().w(px(120.)).child(choice(
                    "terminal-font-size",
                    sizes,
                    Some(settings.font_size),
                    move |size, _, cx| {
                        on_size.update(cx, |t, cx| t.update(cx, |s| s.font_size = size));
                    },
                )),
            ))
    }
}
