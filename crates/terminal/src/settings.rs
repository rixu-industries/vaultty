// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! The `terminal` key of `settings.json`: how the terminal looks. Colors go in the theme.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None`: the app's choice (Cascadia Mono if available; otherwise the system monospace font).
    pub font_family: Option<String>,
    pub font_size: f32,
    /// Programs on the server may copy text to this computer's clipboard (OSC 52). Off by default:
    /// a server could plant a command there for the user to paste somewhere else.
    pub remote_clipboard: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 13.,
            remote_clipboard: false,
        }
    }
}

impl Settings {
    const KEY: &str = "terminal";

    pub fn load() -> Self {
        settings::load(Self::KEY)
    }

    pub fn save(&self) -> Result<(), String> {
        settings::save(Self::KEY, self)
    }
}
