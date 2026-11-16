// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Anonymous instance telemetry: one ping that says "an installation of this version, on this
//! operating system, is running". It counts installations and nothing else: no event about what
//! the user does, and nothing from the vaults, ever goes in. See `PRIVACY.md`.

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// How often a running app pings again: one that is never closed still counts as active each day.
pub const EVERY: Duration = Duration::from_secs(24 * 60 * 60);

const ENDPOINT: &str = "https://analytics.bravinta.com/api/send";
const WEBSITE: &str = "5e716f19-c65a-40e8-9cbf-be0f8fc126fa";
const KEY: &str = "telemetry";

/// The Windows installer leaves this file in the configuration folder when the user clears its
/// checkbox: it cannot edit `settings.json` itself.
const MARKER: &str = "telemetry-off";

/// Settings are read, changed and written back: the ping and the switch must not interleave.
static CHANGING: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    enabled: bool,
    /// Random, made on the first ping and dropped when telemetry is turned off. It tells
    /// installations apart and is derived from nothing on the machine.
    install_id: Option<Uuid>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            install_id: None,
        }
    }
}

/// Whether the user allows the ping.
pub fn enabled() -> bool {
    let Some(dir) = settings::dir() else {
        return false;
    };
    let _changing = CHANGING.lock().unwrap_or_else(|e| e.into_inner());

    enabled_in(&dir)
}

/// Turns the ping on or off. Turning it off forgets the installation id.
pub fn set_enabled(on: bool) -> Result<(), String> {
    let dir = settings::dir().ok_or("System configuration folder not found")?;
    let _changing = CHANGING.lock().unwrap_or_else(|e| e.into_inner());

    set_enabled_in(&dir, on)
}

/// Sends the ping if the user allows it. Blocks while the request runs; a failure is not reported
/// nor retried, the next ping is the retry.
pub fn ping() {
    // A build from source during development is not an installation to count.
    if cfg!(debug_assertions) {
        return;
    }

    let Some(dir) = settings::dir() else {
        return;
    };

    // The lock covers the settings only, never the request.
    let id = {
        let _changing = CHANGING.lock().unwrap_or_else(|e| e.into_inner());
        install_id_in(&dir)
    };

    if let Some(id) = id {
        let _ = send(&body(id));
    }
}

fn file(dir: &Path) -> std::path::PathBuf {
    dir.join("settings.json")
}

/// Applies the installer's choice, once, and reads the setting.
fn enabled_in(dir: &Path) -> bool {
    let marker = dir.join(MARKER);

    if marker.exists() {
        // The marker stays if the save fails, so the choice is not lost; it still counts as off.
        if set_enabled_in(dir, false).is_ok() {
            let _ = fs::remove_file(&marker);
        }

        return false;
    }

    settings::load_from::<Settings>(&file(dir), KEY).enabled
}

fn set_enabled_in(dir: &Path, on: bool) -> Result<(), String> {
    let mut settings: Settings = settings::load_from(&file(dir), KEY);
    settings.enabled = on;

    if !on {
        settings.install_id = None;
    }

    settings::save_to(&file(dir), KEY, &settings)
}

/// The id to ping with, made and saved the first time. `None` when telemetry is off, or when the
/// id cannot be saved: a new one on every ping would count the same installation again and again.
fn install_id_in(dir: &Path) -> Option<Uuid> {
    if !enabled_in(dir) {
        return None;
    }

    let mut settings: Settings = settings::load_from(&file(dir), KEY);

    if let Some(id) = settings.install_id {
        return Some(id);
    }

    let id = Uuid::new_v4();
    settings.install_id = Some(id);
    settings::save_to(&file(dir), KEY, &settings).ok()?;

    Some(id)
}

/// Everything that leaves the machine. Only constants and the installation id: there is no field
/// a caller could put text in.
fn body(id: Uuid) -> Value {
    json!({
        "type": "event",
        "payload": {
            "website": WEBSITE,
            "id": id,
            "hostname": "app.vaultty",
            "title": "Vaultty",
            // Umami groups by page: the version, system and architecture show up as one.
            "url": format!(
                "/v{}/{}/{}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        }
    })
}

/// Umami drops what does not look like a browser and reads the operating system from here.
fn user_agent() -> String {
    let system = if cfg!(target_os = "windows") {
        "Windows NT 10.0; Win64; x64"
    } else if cfg!(target_os = "macos") {
        "Macintosh; Intel Mac OS X 10_15_7"
    } else {
        "X11; Linux x86_64"
    };

    format!(
        "Mozilla/5.0 ({system}) Vaultty/{}",
        env!("CARGO_PKG_VERSION")
    )
}

fn send(body: &Value) -> reqwest::Result<()> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(user_agent())
        // The endpoint is fixed: the id is not handed to wherever a redirect points.
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .build()?
        .post(ENDPOINT)
        .json(body)
        .send()?
        .error_for_status()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Checks that the body carries the id and the fixed fields, and nothing else.
    #[test]
    fn body_has_only_the_id_and_fixed_fields() {
        let id = Uuid::new_v4();
        let body = body(id);
        let payload = body["payload"].as_object().unwrap();

        assert_eq!(body["type"], "event");
        assert_eq!(payload["website"], WEBSITE);
        assert_eq!(payload["id"], id.to_string());
        assert_eq!(
            payload["url"],
            format!(
                "/v{}/{}/{}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        );

        let mut keys: Vec<_> = payload.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["hostname", "id", "title", "url", "website"]);
    }

    // Checks that telemetry is on with no settings and that the id is made once and kept.
    #[test]
    fn id_is_created_once_and_kept() {
        let tmp = tempfile::tempdir().unwrap();

        assert!(enabled_in(tmp.path()));

        let first = install_id_in(tmp.path()).unwrap();
        assert_eq!(install_id_in(tmp.path()), Some(first));
    }

    // Checks that turning it off stops the ping, forgets the id and keeps the other settings.
    #[test]
    fn turning_off_forgets_the_id() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(file(tmp.path()), r#"{"mcp":{"enabled":true}}"#).unwrap();
        let first = install_id_in(tmp.path()).unwrap();

        set_enabled_in(tmp.path(), false).unwrap();

        assert!(!enabled_in(tmp.path()));
        assert_eq!(install_id_in(tmp.path()), None);

        let all: Value = serde_json::from_slice(&fs::read(file(tmp.path())).unwrap()).unwrap();
        assert_eq!(all["telemetry"]["install_id"], Value::Null);
        assert_eq!(all["mcp"]["enabled"], true);

        set_enabled_in(tmp.path(), true).unwrap();
        assert_ne!(install_id_in(tmp.path()), Some(first));
    }

    // Checks that the installer's marker turns telemetry off before any id exists and is used once.
    #[test]
    fn installer_marker_turns_it_off_once() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(MARKER), "").unwrap();

        assert_eq!(install_id_in(tmp.path()), None);
        assert!(!tmp.path().join(MARKER).exists());
        assert!(!enabled_in(tmp.path()));

        set_enabled_in(tmp.path(), true).unwrap();
        assert!(install_id_in(tmp.path()).is_some());
    }

    // Checks that a marker is honoured even when the settings file cannot be written.
    #[test]
    fn marker_counts_even_if_settings_are_damaged() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(file(tmp.path()), "{broken").unwrap();
        fs::write(tmp.path().join(MARKER), "").unwrap();

        assert_eq!(install_id_in(tmp.path()), None);
        assert!(tmp.path().join(MARKER).exists());
    }
}
