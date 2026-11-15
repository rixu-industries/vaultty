// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! System integration: launching at login and the tray icon (on macOS, in the menu
//! bar). The `general` key of `settings.json` stores what the user wants.

mod autostart;
mod tray;

pub use autostart::{launched_at_login, set_launch_at_login};
pub use tray::{Tray, TrayEvent};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Opens when logging in to the computer.
    pub launch_at_login: bool,
    /// When opened with the computer, it stays in the tray without showing the window.
    pub start_hidden: bool,
    /// Closing the window hides it in the tray instead of quitting.
    pub close_to_tray: bool,
}

impl Settings {
    const KEY: &str = "general";

    pub fn load() -> Self {
        settings::load(Self::KEY)
    }

    /// Saves, and sets launch at login as `launch_at_login` says.
    pub fn save(&self) -> Result<(), String> {
        set_launch_at_login(self.launch_at_login)?;
        settings::save(Self::KEY, self)
    }
}
