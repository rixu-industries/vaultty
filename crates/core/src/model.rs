// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

// Record IDs live in the `EncryptedRecord` envelope (authenticated by AAD), not in the data.

/// Host (server) stored in the vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Host {
    pub name: String,
    pub address: String,
    pub port: u16,
    #[zeroize(skip)]
    pub group_id: Option<Uuid>,
    pub access_methods: Vec<AccessMethod>,
    pub notes: String,
    /// Fingerprint of the server key (`SHA256:…`), stored when it is first accepted.
    /// Kept in the vault so other devices do not have to confirm it again.
    #[serde(default)]
    pub host_key: Option<String>,
    /// Operating system detected on connect (`ubuntu`, `debian`, `macos`, `windows`…), used for the icon.
    #[serde(default)]
    pub os: Option<String>,
    /// MCP permissions specific to the host. `None`: inherits the ones the vault has in the device settings.
    /// They are stored here, encrypted, so that nobody without the password can grant more permissions by editing a file.
    #[serde(default)]
    #[zeroize(skip)]
    pub mcp: Option<McpPerms>,
}

impl Host {
    /// Whether any access method of the host uses the key `key_id`.
    pub fn uses_key(&self, key_id: Uuid) -> bool {
        self.access_methods
            .iter()
            .any(|m| matches!(m.kind, AccessKind::Key { key_id: k, .. } if k == key_id))
    }
}

/// What the MCP server can do with a vault or a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct McpPerms {
    /// View hosts, groups and public keys.
    pub read: bool,
    /// Create, edit and delete hosts and groups.
    pub write: bool,
    pub exec: Exec,
}

/// Whether the MCP can run commands, and whether it must ask first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exec {
    #[default]
    No,
    /// The app asks for confirmation before each command.
    Ask,
    Direct,
}

/// Access method of a host: password or key, with its label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Zeroize)]
pub struct AccessMethod {
    #[zeroize(skip)]
    pub id: Uuid,
    pub label: String,
    pub kind: AccessKind,
}

/// Tag values must be kept in sync with `KNOWN_ACCESS_KINDS`.
#[derive(Clone, PartialEq, Serialize, Deserialize, Zeroize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccessKind {
    /// Username and password.
    Password { username: String, password: String },
    /// Username that signs in with a key from the keychain.
    Key {
        username: String,
        #[zeroize(skip)]
        key_id: Uuid,
    },
}

pub(crate) const KNOWN_ACCESS_KINDS: &[&str] = &["password", "key"];

impl fmt::Debug for AccessKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { username, .. } => f
                .debug_struct("Password")
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            Self::Key { username, key_id } => f
                .debug_struct("Key")
                .field("username", username)
                .field("key_id", key_id)
                .finish(),
        }
    }
}

/// Keychain entry, shared by all the hosts that use it.
#[derive(Clone, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Key {
    pub name: String,
    pub private_key: String,
    pub passphrase: Option<String>,
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Key")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Group of hosts; it can have a parent group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub parent_id: Option<Uuid>,
}

/// Tunnel: a local proxy whose connections leave through a host in the vault. It does not know which protocol
/// reaches the host: the access method decides that (SSH today).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tunnel {
    pub name: String,
    pub kind: TunnelKind,
    /// Local address it listens on. `127.0.0.1` so that only this device can use it.
    pub bind: String,
    pub port: u16,
    pub host_id: Uuid,
    pub method_id: Uuid,
    /// Started when the vault is opened.
    pub enabled: bool,
}

/// Which proxy the tunnel starts. Tag values must be kept in sync with
/// `KNOWN_TUNNEL_KINDS`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TunnelKind {
    Socks5,
    /// SOCKS4 and 4a (with domain name).
    Socks4,
    /// HTTP proxy: `CONNECT` (HTTPS and any TCP) and plain HTTP requests.
    Http,
    /// Fixed port forwarding, like `ssh -L`: everything that arrives goes to `host:port`.
    Forward {
        host: String,
        port: u16,
    },
}

pub(crate) const KNOWN_TUNNEL_KINDS: &[&str] = &["socks5", "socks4", "http", "forward"];

/// Decrypted record: host, key, group, tunnel or tombstone.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    /// A host.
    Host(Host),
    /// A key from the keychain.
    Key(Key),
    /// A group.
    Group(Group),
    /// A tunnel.
    Tunnel(Tunnel),
    /// Deleted record (tombstone).
    Tombstone,
}

/// Returns true if setting `new_parent` as the parent of `group` would create a cycle.
pub fn would_cycle(groups: &HashMap<Uuid, Group>, group: Uuid, new_parent: Option<Uuid>) -> bool {
    let mut cur = new_parent;

    // Bounded walk so that an already corrupt tree cannot loop forever.
    for _ in 0..=groups.len() {
        match cur {
            None => return false,
            Some(id) if id == group => return true,
            Some(id) => cur = groups.get(&id).and_then(|g| g.parent_id),
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // Checks that would_cycle detects cycles and does not flag false positives.
    #[test]
    fn detects_cycles() {
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let g = |p| Group {
            name: String::new(),
            parent_id: p,
        };
        let groups = HashMap::from([(a, g(None)), (b, g(Some(a))), (c, g(Some(b)))]);

        assert!(would_cycle(&groups, a, Some(c)));
        assert!(would_cycle(&groups, a, Some(a)));
        assert!(!would_cycle(&groups, c, Some(a)));
        assert!(!would_cycle(&groups, b, None));
    }

    // Checks that Debug does not show passwords or private keys.
    #[test]
    fn debug_redacts_secrets() {
        let k = Key {
            name: "k".into(),
            private_key: "SECRETKEY".into(),
            passphrase: Some("SECRETPP".into()),
        };
        let p = AccessKind::Password {
            username: "root".into(),
            password: "SECRETPW".into(),
        };
        let out = format!("{k:?}{p:?}");

        assert!(!out.contains("SECRET"), "{out}");
    }
}
