// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Machine-wide settings in `settings.json`, next to `vaults.json`. Each crate owns one key
//! (`mcp`, `appearance`, `terminal`…) and only touches its own: the others are kept as they are.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The app's configuration folder (`…/vaultty`). It holds `settings.json`, `vaults.json` and `themes/`.
pub fn dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("vaultty"))
}

fn file() -> Option<PathBuf> {
    Some(dir()?.join("settings.json"))
}

/// Reads a key. If there is no file, the key is missing or cannot be parsed, the built-in default.
pub fn load<T: DeserializeOwned + Default>(key: &str) -> T {
    file().map(|f| load_from(&f, key)).unwrap_or_default()
}

/// Saves a key without touching the others.
pub fn save<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    let file = file().ok_or("System configuration folder not found")?;
    save_to(&file, key, value)
}

pub fn load_from<T: DeserializeOwned + Default>(file: &Path, key: &str) -> T {
    fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|mut all| serde_json::from_value(all.get_mut(key)?.take()).ok())
        .unwrap_or_default()
}

pub fn save_to<T: Serialize>(file: &Path, key: &str, value: &T) -> Result<(), String> {
    // If the file exists but cannot be parsed, it is not overwritten: the other settings would be lost.
    let mut all: Map<String, Value> = match fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| format!("{} is damaged: fix or delete it.", file.display()))?,
        Err(_) => Map::new(),
    };

    let value = serde_json::to_value(value).map_err(|e| format!("Invalid settings: {e}"))?;
    all.insert(key.into(), value);

    let json = serde_json::to_vec_pretty(&all).expect("a JSON object always serializes");

    fs::create_dir_all(file.parent().expect("has a parent folder"))
        .and_then(|_| fs::write(file, json))
        .map_err(|e| format!("Could not save the settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Demo {
        n: u32,
    }

    #[test]
    fn saves_only_its_key_and_keeps_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.json");
        fs::write(&file, r#"{"mcp":{"enabled":true}}"#).unwrap();

        save_to(&file, "demo", &Demo { n: 3 }).unwrap();

        let all: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(all["mcp"]["enabled"], true);
        assert_eq!(load_from::<Demo>(&file, "demo"), Demo { n: 3 });
    }

    #[test]
    fn does_not_overwrite_a_damaged_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.json");
        fs::write(&file, "{broken").unwrap();

        assert!(save_to(&file, "demo", &Demo::default()).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "{broken");
        assert_eq!(load_from::<Demo>(&file, "demo"), Demo::default());
    }
}
