// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Vaults the app knows about: they are saved in `vaults.json` inside the system configuration
//! folder (`%APPDATA%\vaultty` on Windows). It contains no secrets, only names and paths.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownVault {
    /// The `vault_id` from the vault's header, the same on all devices.
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    /// A password is remembered in the system store (the password is not stored here).
    #[serde(default)]
    pub remember_password: bool,
}

fn file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("vaultty").join("vaults.json"))
}

/// Reads the list. If it does not exist or is damaged, it starts empty: losing it does not lose any vault.
pub fn load() -> Vec<KnownVault> {
    file()
        .and_then(|f| fs::read(f).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(vaults: &[KnownVault]) -> Result<(), String> {
    let file = file().ok_or("System configuration folder not found")?;
    let json = serde_json::to_vec_pretty(vaults).expect("the list always serializes");

    fs::create_dir_all(file.parent().expect("has a parent folder"))
        .and_then(|_| fs::write(&file, json))
        .map_err(|e| format!("Could not save the vault list: {e}"))
}

impl KnownVault {
    /// The folder no longer has its header (disk disconnected, folder moved or deleted). A
    /// permissions error does not count as missing: it is unknown, so opening is allowed.
    pub fn is_missing(&self) -> bool {
        !self
            .path
            .join(store::HEADER_FILE)
            .try_exists()
            .unwrap_or(true)
    }
}

/// Name shown for a vault: that of its folder.
pub fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_when_the_folder_or_its_header_is_gone() {
        let temp = tempfile::tempdir().unwrap();
        let known = |dir: &str| KnownVault {
            id: Uuid::new_v4(),
            name: dir.into(),
            path: temp.path().join(dir),
            remember_password: false,
        };
        fs::create_dir(temp.path().join("ok")).unwrap();
        fs::write(temp.path().join("ok").join(store::HEADER_FILE), "{}").unwrap();
        fs::create_dir(temp.path().join("no-header")).unwrap();

        assert!(!known("ok").is_missing());
        assert!(known("no-header").is_missing());
        assert!(known("deleted").is_missing());
    }
}
