// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Remembered master passwords, in the system credential store (Credential Manager on Windows,
//! Keychain on macOS, Secret Service on Linux). They never go to `vaults.json`.

use keyring::Entry;
use uuid::Uuid;

const SERVICE: &str = "vaultty";

fn entry(vault: Uuid) -> Result<Entry, String> {
    Entry::new(SERVICE, &vault.to_string()).map_err(error)
}

pub fn save(vault: Uuid, password: &str) -> Result<(), String> {
    entry(vault)?.set_password(password).map_err(error)
}

/// The remembered password, or `None` if there is none.
pub fn load(vault: Uuid) -> Result<Option<String>, String> {
    match entry(vault)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(error(e)),
    }
}

/// Deletes the remembered password. There being none is not an error.
pub fn forget(vault: Uuid) -> Result<(), String> {
    match entry(vault)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(error(e)),
    }
}

fn error(e: keyring::Error) -> String {
    format!("Could not use the system password store: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uses the real system store, so it does not run by default: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn roundtrip_in_system_store() {
        let vault = Uuid::new_v4();

        save(vault, "test password").unwrap();
        assert_eq!(load(vault).unwrap().as_deref(), Some("test password"));

        forget(vault).unwrap();
        assert_eq!(load(vault).unwrap(), None);
        forget(vault).unwrap();
    }
}
