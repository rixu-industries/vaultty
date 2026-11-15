// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Vault data model and encryption. No I/O: the caller stores `VaultHeader` and `EncryptedRecord`.

mod model;
mod vault;

pub use model::{
    AccessKind, AccessMethod, Exec, Group, Host, Key, McpPerms, Record, Tunnel, TunnelKind,
};
pub use vault::{
    EncryptedRecord, FORMAT_VERSION, KdfParams, UnlockedVault, Vault, VaultHeader, WrappedKey,
};

/// Errors when opening or decrypting the vault.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// Wrong master password or tampered header.
    #[error("wrong master password or tampered vault header")]
    WrongPassword,
    /// Vault data is corrupted or tampered with.
    #[error("vault data is corrupted or was tampered with")]
    Corrupted,
    /// The vault was written by a newer version of the app.
    #[error("vault was written by a newer version of the app")]
    UnsupportedVersion,
}
