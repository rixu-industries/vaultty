// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! MCP server settings: the `mcp` key of `settings.json` (crate `settings`). The token lives in the
//! system store.

use keyring::Entry;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vaults::McpPerms;

const KEY: &str = "mcp";

/// Default port: below the system's dynamic range (49152+), so it is not handed to
/// another program, and away from the development ones (3000, 5173, 8000, 8080…).
const DEFAULT_PORT: u16 = 47610;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Address it listens on and the only `Host` accepted in requests.
    pub host: String,
    pub port: u16,
    /// Vaults the MCP can reach. The entries here are the default permissions for their hosts.
    pub vaults: Vec<VaultAccess>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "localhost".into(),
            port: DEFAULT_PORT,
            vaults: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultAccess {
    pub id: Uuid,
    pub perms: McpPerms,
}

/// If there is no file or it is damaged, the built-in defaults: the server stays off.
pub fn load() -> Settings {
    settings::load(KEY)
}

pub fn save(value: &Settings) -> Result<(), String> {
    settings::save(KEY, value)
}

fn entry() -> Result<Entry, String> {
    Entry::new("vaultty", "mcp-token").map_err(keyring_error)
}

/// The token the client sends in `Authorization: Bearer …`. If there is none yet, one is created.
pub fn token() -> Result<String, String> {
    match entry()?.get_password() {
        Ok(token) => Ok(token),
        Err(keyring::Error::NoEntry) => new_token(),
        Err(e) => Err(keyring_error(e)),
    }
}

/// Creates a new token; the previous one stops being valid.
pub fn new_token() -> Result<String, String> {
    // Two UUID v4 values come from the system's random generator: 244 bits of randomness.
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

    entry()?.set_password(&token).map_err(keyring_error)?;
    Ok(token)
}

fn keyring_error(e: keyring::Error) -> String {
    format!("Could not use the system password store: {e}")
}
