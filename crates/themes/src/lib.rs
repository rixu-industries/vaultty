// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Themes for the app and the terminal. A theme is a JSON of colors with flat keys (`"sidebar.accent"`,
//! `"terminal.ansi.red"`…), like Zed's, and comes in a package with its `manifest.json`. The app ships
//! the `vaultty` package (dark and light), and community ones install into `<config>/themes/<id>/`.
//!
//! A theme does not have to include every key: what is missing comes from the built-in theme of the same
//! appearance. That way an old theme keeps working when the app adds new colors.
//!
//! The JSON schema is in `schema/theme-v1.json`.

mod color;
mod package;
mod style;
mod vscode;

use serde::{Deserialize, Serialize};

pub use color::Color;
pub use package::{Library, Manifest, Package, install, themes_dir, uninstall};
pub use style::{Terminal, Theme, Ui, keys};
pub use vscode::{Imported, import_vscode, import_vscode_into};

/// Format version the app understands. A package with another version is not loaded.
pub const SCHEMA_VERSION: u32 = 1;

pub const DEFAULT_DARK: &str = "Vaultty Dark";
pub const DEFAULT_LIGHT: &str = "Vaultty Light";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
}

/// Which appearance to use: the system's or a fixed one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    System,
    Light,
    Dark,
}

/// The `appearance` key of `settings.json`. A theme is saved for each appearance, like Zed: when
/// the system switches from light to dark, the app switches from one to the other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub mode: Mode,
    pub light: String,
    pub dark: String,
    /// Size of the interface text, in pixels.
    pub ui_font_size: f32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            mode: Mode::System,
            light: DEFAULT_LIGHT.into(),
            dark: DEFAULT_DARK.into(),
            ui_font_size: 13.,
        }
    }
}

impl AppearanceSettings {
    const KEY: &str = "appearance";

    pub fn load() -> Self {
        settings::load(Self::KEY)
    }

    pub fn save(&self) -> Result<(), String> {
        settings::save(Self::KEY, self)
    }

    /// The appearance that applies, given the system's.
    pub fn appearance(&self, system: Appearance) -> Appearance {
        match self.mode {
            Mode::System => system,
            Mode::Light => Appearance::Light,
            Mode::Dark => Appearance::Dark,
        }
    }

    /// The name of the theme chosen for an appearance.
    pub fn theme_for(&self, appearance: Appearance) -> &str {
        match appearance {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        }
    }
}
