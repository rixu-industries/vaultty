// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Machine-wide settings in `settings.json`, next to `vaults.json`. Each crate owns one key
//! (`mcp`, `appearance`, `terminal`…) and only touches its own: the others are kept as they are.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    // The file is read, changed and written back: two threads saving at once would lose a key.
    static SAVING: Mutex<()> = Mutex::new(());

    let file = file().ok_or("System configuration folder not found")?;
    let _saving = SAVING.lock().unwrap_or_else(|e| e.into_inner());

    save_to(&file, key, value)
}

/// Writes to a temporary file and renames it: a power cut never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    let mut file = fs::File::create(&tmp)?;

    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
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
        .and_then(|_| write_atomic(file, &json))
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

    // Checks that the file is replaced as a whole and no temporary file is left behind.
    #[test]
    fn atomic_write_replaces_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.json");

        write_atomic(&file, b"first, and longer").unwrap();
        write_atomic(&file, b"second").unwrap();

        assert_eq!(fs::read(&file).unwrap(), b"second");
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
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
