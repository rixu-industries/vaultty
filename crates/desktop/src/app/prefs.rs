// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! App preferences (theme, text size, terminal font, startup and tray) and the
//! themes available. They live in a `Global` because the views that read them do not know each other: the terminal, the
//! settings and the window. Each change is saved to `settings.json` and applied right away.

use gpui_kit::{App, Global, WindowAppearance};
use themes::{Appearance, AppearanceSettings, Library};

use crate::app::{shell, theme};

pub struct Prefs {
    pub library: Library,
    pub appearance: AppearanceSettings,
    pub terminal: terminal::Settings,
    pub general: system::Settings,
    /// The system one, which takes effect in `System` mode.
    pub system: Appearance,
}

impl Global for Prefs {}

fn appearance_of(window: WindowAppearance) -> Appearance {
    match window {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}

impl Prefs {
    /// Reads everything and applies the theme. It runs before the window opens: the `render` calls already read the colors.
    pub fn init(cx: &mut App) {
        let prefs = Prefs {
            library: Library::load(),
            appearance: AppearanceSettings::load(),
            terminal: terminal::Settings::load(),
            general: system::Settings::load(),
            system: appearance_of(cx.window_appearance()),
        };

        cx.set_global(prefs);
        Self::apply(cx);
    }

    pub fn get(cx: &App) -> &Prefs {
        cx.global::<Prefs>()
    }

    fn apply(cx: &mut App) {
        let prefs = Self::get(cx);
        let theme = prefs.library.pick(&prefs.appearance, prefs.system).clone();
        let size = prefs.appearance.ui_font_size;

        theme::apply(&theme, size, cx);
    }

    /// The system switched from light to dark (or the other way around).
    pub fn set_system(window: WindowAppearance, cx: &mut App) {
        let system = appearance_of(window);

        if Self::get(cx).system != system {
            cx.global_mut::<Prefs>().system = system;
            Self::apply(cx);
        }
    }

    pub fn set_appearance(appearance: AppearanceSettings, cx: &mut App) -> Result<(), String> {
        appearance.save()?;
        cx.global_mut::<Prefs>().appearance = appearance;
        Self::apply(cx);
        Ok(())
    }

    pub fn set_terminal(settings: terminal::Settings, cx: &mut App) -> Result<(), String> {
        settings.save()?;
        cx.global_mut::<Prefs>().terminal = settings;
        cx.refresh_windows();
        Ok(())
    }

    pub fn set_general(settings: system::Settings, cx: &mut App) -> Result<(), String> {
        settings.save()?;
        cx.global_mut::<Prefs>().general = settings;
        shell::sync_tray(cx);
        Ok(())
    }

    /// Reloads the themes folder (after installing or uninstalling). If the active theme is gone,
    /// `pick` falls back to the built-in one.
    pub fn reload_themes(cx: &mut App) {
        cx.global_mut::<Prefs>().library = Library::load();
        Self::apply(cx);
    }
}
