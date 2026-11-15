// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! "General" category: launching with the computer and what the close button does.

use crate::app::prefs::Prefs;
use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::{Switch, note};
use crate::features::settings::{header, setting};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Context, IntoElement, ParentElement, Render, Styled, Window, div};

#[derive(Default)]
pub struct GeneralSettings {
    error: Option<String>,
}

impl GeneralSettings {
    fn update(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut system::Settings)) {
        let mut settings = Prefs::get(cx).general.clone();
        edit(&mut settings);

        self.error = Prefs::set_general(settings, cx).err();
        cx.notify();
    }
}

impl Render for GeneralSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let settings = Prefs::get(cx).general.clone();
        let (on_login, on_hidden, on_tray) = (cx.entity(), cx.entity(), cx.entity());

        let (tray, close) = if cfg!(target_os = "macos") {
            ("the menu bar", "Close to menu bar")
        } else {
            ("the system tray", "Close to tray")
        };

        div()
            .flex()
            .flex_col()
            .child(header("General", "How the app starts and closes."))
            .when_some(self.error.clone(), |this, e| {
                this.child(div().mb(px(8.)).child(note(e, c.destructive)))
            })
            .child(setting(
                "Launch at login",
                "Open Vaultty when you sign in to your computer.",
                Switch::new("launch-at-login", "Launch at login")
                    .checked(settings.launch_at_login)
                    .on_click(move |on, _, cx| {
                        on_login.update(cx, |t, cx| t.update(cx, |s| s.launch_at_login = *on));
                    }),
            ))
            .child(setting(
                "Start in the background",
                format!("When it opens at login, stay in {tray} without showing the window."),
                Switch::new("start-hidden", "Start in the background")
                    .checked(settings.launch_at_login && settings.start_hidden)
                    .disabled(!settings.launch_at_login)
                    .on_click(move |on, _, cx| {
                        on_hidden.update(cx, |t, cx| t.update(cx, |s| s.start_hidden = *on));
                    }),
            ))
            .child(setting(
                close,
                format!("Closing the window keeps Vaultty running in {tray}. Quit it from there."),
                Switch::new("close-to-tray", close)
                    .checked(settings.close_to_tray)
                    .on_click(move |on, _, cx| {
                        on_tray.update(cx, |t, cx| t.update(cx, |s| s.close_to_tray = *on));
                    }),
            ))
    }
}
