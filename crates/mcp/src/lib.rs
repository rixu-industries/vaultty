// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Vaultty's MCP server: lets an MCP client (an AI assistant, for example) read and change the
//! vaults unlocked in the app and run commands on their hosts. It only reaches the vaults the user
//! adds in the settings, with their permissions; each host can have its own permissions, stored encrypted in
//! the vault (`Host::mcp`).
//!
//! It never returns passwords or private keys, whatever permissions it has.

mod jobs;
mod server;
mod settings;
mod tools;

pub use server::{Server, start};
pub use settings::{Settings, VaultAccess, load, new_token, save, token};
pub use tools::{Ask, ExecRequest};
pub use vaults::{Exec, McpPerms};
