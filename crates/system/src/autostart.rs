// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Launch at login: the registry on Windows, a LaunchAgent on macOS and `~/.config/autostart` on
//! Linux. The entry starts the app with `--autostart` so it knows the user did not open it.

use auto_launch::AutoLaunchBuilder;

const ARG: &str = "--autostart";

/// The app was opened automatically at login.
pub fn launched_at_login() -> bool {
    std::env::args().any(|a| a == ARG)
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find the app: {e}"))?;

    // On macOS, the entry points to the `.app`, not to the binary inside it.
    #[cfg(target_os = "macos")]
    let exe = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(|p| p.to_path_buf())
        .unwrap_or(exe);

    let launcher = AutoLaunchBuilder::new()
        .set_app_name("Vaultty")
        .set_app_path(&exe.to_string_lossy())
        .set_args(&[ARG])
        .build()
        .map_err(|e| format!("Could not set up launch at login: {e}"))?;

    let result = match (enabled, launcher.is_enabled().unwrap_or(false)) {
        (true, _) => launcher.enable(),
        (false, true) => launcher.disable(),
        (false, false) => Ok(()),
    };

    result.map_err(|e| format!("Could not change launch at login: {e}"))
}
