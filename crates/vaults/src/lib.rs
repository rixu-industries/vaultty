// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! App vaults: the ones it knows about (`vaults.json`), the open ones, and everything that can be
//! done with their hosts, groups and keys. It knows nothing about the UI: the desktop one and a
//! future mobile one both use this same crate.
//!
//! Views (`VaultView` and friends) carry no secrets; only `method` and `key` return them, for
//! editing. Operations that derive the key with Argon2 (`create`, `unlock`, `auto_unlock`,
//! `change_password`) are slow: call them off the UI thread.
//!
//! Closing a vault's connections when locking it or deleting a host is up to the caller
//! (`sessions`): this crate does not know they exist.

mod registry;
mod remembered;
mod session;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use uuid::Uuid;
pub use vault::{AccessKind, AccessMethod, Exec, McpPerms, TunnelKind};
use vault::{Group, Host, Key, Record, Tunnel};

use registry::KnownVault;
pub use session::KeyInfo;
use session::{OpenVault, check_public_key};

const LOCKED: &str = "The vault is locked.";

/// Vaults the app knows about (`vaults.json`) and unlocked vaults, with their decrypted records.
pub struct Vaults {
    known: Mutex<Vec<KnownVault>>,
    open: Mutex<HashMap<Uuid, OpenVault>>,
    /// Vaults whose folder was gone the last time `check_missing` looked.
    missing: Mutex<HashSet<Uuid>>,
    /// Where the list is saved. `None`: the system has no configuration folder.
    file: Option<PathBuf>,
}

// ---- Views (no secrets) ----

#[derive(Clone, Debug, PartialEq)]
pub struct VaultItem {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub unlocked: bool,
    pub remember_password: bool,
    /// Its folder is no longer where the app remembers it: the new location must be given.
    pub missing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MethodView {
    pub id: Uuid,
    pub password: bool,
    pub username: String,
    pub label: String,
    pub key_id: Option<Uuid>,
    /// The label, or the username if it has none.
    pub title: String,
    pub detail: String,
    /// Uses a key that is no longer in the keychain: it cannot be connected with.
    pub broken: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostView {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: u16,
    /// Server fingerprint accepted on connect; `None` if it has not been accepted yet.
    pub host_key: Option<String>,
    pub group_id: Option<Uuid>,
    pub notes: String,
    /// Operating system detected on connect (`ubuntu`, `macos`, `windows`…); `None` if not known yet.
    pub os: Option<String>,
    pub methods: Vec<MethodView>,
    /// Own MCP permissions; `None` if it inherits the vault's.
    pub mcp: Option<McpPerms>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupView {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyView {
    pub id: Uuid,
    pub name: String,
    pub has_passphrase: bool,
    pub algorithm: String,
    /// The fingerprint, or the reason the key could not be read.
    pub fingerprint: String,
    pub public: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TunnelView {
    pub id: Uuid,
    pub name: String,
    pub kind: TunnelKind,
    pub bind: String,
    pub port: u16,
    pub host_id: Uuid,
    pub method_id: Uuid,
    pub enabled: bool,
    /// Its host or access method no longer exists: it cannot be started.
    pub broken: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VaultView {
    pub hosts: Vec<HostView>,
    pub groups: Vec<GroupView>,
    pub keys: Vec<KeyView>,
    pub tunnels: Vec<TunnelView>,
    pub warnings: Vec<String>,
}

/// Host data for creating or editing a host. Access methods are handled separately.
#[derive(Clone, Debug, Default)]
pub struct HostInput {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub group_id: Option<Uuid>,
    pub notes: String,
}

/// Tunnel data for creating or editing a tunnel. Whether it is enabled is saved separately (`set_tunnel_enabled`).
#[derive(Clone, Debug)]
pub struct TunnelInput {
    pub name: String,
    pub kind: TunnelKind,
    pub bind: String,
    pub port: u16,
    pub host_id: Uuid,
    pub method_id: Uuid,
}

/// A key with its secrets, for the import or edit form.
#[derive(Clone, Default)]
pub struct KeyInput {
    pub name: String,
    pub private_key: String,
    /// Optional: only used to check that it pairs with the private key. Not saved.
    pub public_key: String,
    pub passphrase: String,
}

/// A key read from a file.
pub struct KeyFile {
    pub name: String,
    pub private_key: String,
    /// The `.pub` that `ssh-keygen` leaves next to the private key, if it exists.
    pub public_key: Option<String>,
}

fn method_view(method: &AccessMethod, keys: &BTreeMap<Uuid, Key>) -> MethodView {
    let (username, key_id) = match &method.kind {
        AccessKind::Password { username, .. } => (username.clone(), None),
        AccessKind::Key { username, key_id } => (username.clone(), Some(*key_id)),
    };

    MethodView {
        id: method.id,
        password: matches!(method.kind, AccessKind::Password { .. }),
        // The label, or the username if it has none.
        title: if method.label.is_empty() {
            username.clone()
        } else {
            method.label.clone()
        },
        detail: method_detail(method, keys),
        broken: matches!(method.kind, AccessKind::Key { key_id, .. } if !keys.contains_key(&key_id)),
        label: method.label.clone(),
        username,
        key_id,
    }
}

fn method_detail(method: &AccessMethod, keys: &BTreeMap<Uuid, Key>) -> String {
    let (username, kind) = match &method.kind {
        AccessKind::Password { username, .. } => (username, "password".to_string()),
        AccessKind::Key { username, key_id } => (
            username,
            match keys.get(key_id) {
                Some(key) => format!("key {}", key.name),
                None => "the key no longer exists".to_string(),
            },
        ),
    };

    // Without a label, the title is already the username: it is not repeated here.
    if method.label.is_empty() {
        kind
    } else {
        format!("{username} · {kind}")
    }
}

fn tunnel_view(id: Uuid, tunnel: &Tunnel, hosts: &BTreeMap<Uuid, Host>) -> TunnelView {
    let broken = !hosts
        .get(&tunnel.host_id)
        .is_some_and(|h| h.access_methods.iter().any(|m| m.id == tunnel.method_id));

    TunnelView {
        id,
        name: tunnel.name.clone(),
        kind: tunnel.kind.clone(),
        bind: tunnel.bind.clone(),
        port: tunnel.port,
        host_id: tunnel.host_id,
        method_id: tunnel.method_id,
        enabled: tunnel.enabled,
        broken,
    }
}

fn key_view(id: Uuid, key: &Key) -> KeyView {
    let (algorithm, fingerprint, public) = match KeyInfo::parse(&key.private_key) {
        Ok(info) => (info.algorithm, info.fingerprint, Some(info.public)),
        Err(e) => ("unknown".into(), e, None),
    };

    KeyView {
        id,
        name: key.name.clone(),
        has_passphrase: key.passphrase.is_some(),
        algorithm,
        fingerprint,
        public,
    }
}

impl Vaults {
    /// Reads the vault list from disk. All of them start locked.
    pub fn load() -> Self {
        Self::new(registry::file())
    }

    /// Like `load`, with the list in another file: tests must not touch the real one.
    pub fn at(file: PathBuf) -> Self {
        Self::new(Some(file))
    }

    fn new(file: Option<PathBuf>) -> Self {
        Self {
            known: Mutex::new(registry::load(file.as_deref())),
            open: Mutex::new(HashMap::new()),
            missing: Mutex::default(),
            file,
        }
    }

    /// Looks on disk for the vaults whose folder is gone, for `list`. It can take long (a network
    /// drive that does not answer): call it off the UI thread. Holds no lock while looking.
    pub fn check_missing(&self) {
        let known = self.known.lock().unwrap().clone();

        let missing = known
            .iter()
            .filter(|v| v.is_missing())
            .map(|v| v.id)
            .collect();

        *self.missing.lock().unwrap() = missing;
    }

    /// Reads from an open vault. The caller must return its own data, not references.
    fn with_open<R>(&self, vault: Uuid, read: impl FnOnce(&OpenVault) -> R) -> Result<R, String> {
        self.open
            .lock()
            .unwrap()
            .get(&vault)
            .map(read)
            .ok_or_else(|| LOCKED.into())
    }

    fn change(
        &self,
        vault: Uuid,
        action: impl FnOnce(&mut OpenVault) -> Result<(), String>,
    ) -> Result<(), String> {
        match self.open.lock().unwrap().get_mut(&vault) {
            Some(open) => action(open),
            None => Err(LOCKED.into()),
        }
    }

    /// Changes an existing host.
    pub fn edit_host(
        &self,
        vault: Uuid,
        id: Uuid,
        edit: impl FnOnce(&mut Host),
    ) -> Result<(), String> {
        // Read and saved under the same lock: a change made in between by another thread would be lost.
        self.change(vault, |open| {
            let mut host = open
                .hosts
                .get(&id)
                .cloned()
                .ok_or("The host no longer exists.")?;

            edit(&mut host);
            open.save(id, Record::Host(host))
        })
    }

    /// Copy of the host and of the key the method uses, with their secrets. Only to open the SSH
    /// session: it is not displayed or passed to anyone else. Copies are erased from memory when dropped.
    pub fn target(&self, vault: Uuid, host: Uuid, method: Uuid) -> Option<(Host, Option<Key>)> {
        self.with_open(vault, |v| {
            let host = v.hosts.get(&host)?.clone();

            let key = match host.access_methods.iter().find(|m| m.id == method)?.kind {
                AccessKind::Key { key_id, .. } => v.keys.get(&key_id).cloned(),
                AccessKind::Password { .. } => None,
            };

            Some((host, key))
        })
        .ok()?
    }

    /// Adds a vault to the list, if it was not there, and saves the list to disk.
    fn remember(&self, vault: KnownVault) -> Result<(), String> {
        let mut known = self.known.lock().unwrap();

        if !known.iter().any(|v| v.id == vault.id) {
            known.push(vault);
        }

        registry::save(self.file.as_deref(), &known)
    }

    /// The vault's folder and whether it has a remembered password.
    fn known(&self, vault: Uuid) -> Result<(PathBuf, bool), String> {
        self.known
            .lock()
            .unwrap()
            .iter()
            .find(|v| v.id == vault)
            .map(|v| (v.path.clone(), v.remember_password))
            .ok_or_else(|| "The vault is not in the list.".into())
    }

    /// Remembers or forgets a vault's password, in the system store and in the list.
    fn remember_password(&self, vault: Uuid, password: Option<&str>) -> Result<(), String> {
        match password {
            Some(password) => remembered::save(vault, password)?,
            None => remembered::forget(vault)?,
        }

        let mut known = self.known.lock().unwrap();

        if let Some(v) = known.iter_mut().find(|v| v.id == vault) {
            v.remember_password = password.is_some();
        }

        registry::save(self.file.as_deref(), &known)
    }

    // ---- Vaults ----

    /// Does not touch the disk: `missing` is what `check_missing` last found.
    pub fn list(&self) -> Vec<VaultItem> {
        let open = self.open.lock().unwrap();
        let missing = self.missing.lock().unwrap();

        self.known
            .lock()
            .unwrap()
            .iter()
            .map(|v| VaultItem {
                id: v.id,
                name: v.name.clone(),
                path: v.path.display().to_string(),
                unlocked: open.contains_key(&v.id),
                remember_password: v.remember_password,
                missing: missing.contains(&v.id),
            })
            .collect()
    }

    /// Points a vault in the list to another folder (it was moved, or the drive letter changed).
    /// The new folder must be the same vault: this is checked by the id in its header. If it was
    /// open, it is locked: its store pointed to the old path.
    pub fn relocate(&self, vault: Uuid, dir: PathBuf) -> Result<(), String> {
        if OpenVault::peek_id(&dir)? != vault {
            return Err("That folder belongs to another vault.".into());
        }

        self.open.lock().unwrap().remove(&vault);
        // Its header has just been read from the new folder.
        self.missing.lock().unwrap().remove(&vault);

        let mut known = self.known.lock().unwrap();
        let entry = known
            .iter_mut()
            .find(|v| v.id == vault)
            .ok_or("The vault is not in the list.")?;
        entry.name = registry::name_of(&dir);
        entry.path = dir;

        registry::save(self.file.as_deref(), &known)
    }

    /// Creates an empty vault in `dir`, adds it to the list and leaves it unlocked. Takes time (Argon2).
    pub fn create(&self, dir: PathBuf, password: &str) -> Result<Uuid, String> {
        let open = OpenVault::create(&dir, password)?;
        let id = open.id;
        let name = registry::name_of(&dir);

        self.open.lock().unwrap().insert(id, open);
        self.remember(KnownVault {
            id,
            name,
            path: dir,
            remember_password: false,
        })?;

        Ok(id)
    }

    /// Adds to the list a vault that already exists in `dir` (for example, synced from Drive).
    pub fn add(&self, dir: PathBuf) -> Result<Uuid, String> {
        let id = OpenVault::peek_id(&dir)?;
        let name = registry::name_of(&dir);

        self.remember(KnownVault {
            id,
            name,
            path: dir,
            remember_password: false,
        })?;

        Ok(id)
    }

    /// Removes a vault from the list. Does not delete any file.
    pub fn forget(&self, vault: Uuid) -> Result<(), String> {
        self.open.lock().unwrap().remove(&vault);

        // If the system store fails, the password stays there, but removing the vault must not fail.
        let _ = remembered::forget(vault);

        let mut known = self.known.lock().unwrap();
        known.retain(|v| v.id != vault);

        registry::save(self.file.as_deref(), &known)
    }

    /// Decrypts the vault at `path`. Holds no lock while running: Argon2 is slow.
    fn open_vault(vault: Uuid, path: &Path, password: &str) -> Result<OpenVault, String> {
        let open = OpenVault::open(path, password)?;

        // The folder may have been replaced by another vault since it was added to the list.
        if open.id != vault {
            return Err("The folder now contains another vault.".into());
        }

        Ok(open)
    }

    /// Unlocks with the typed password. `remember` saves it in the system store, or forgets it if
    /// false. Takes time (Argon2).
    pub fn unlock(&self, vault: Uuid, password: &str, remember: bool) -> Result<(), String> {
        let (path, remembered) = self.known(vault)?;
        let open = Self::open_vault(vault, &path, password)?;

        // Before leaving it open: if it cannot be remembered, it stays locked and the error says why.
        if remember {
            self.remember_password(vault, Some(password))?;
        } else if remembered {
            self.remember_password(vault, None)?;
        }

        self.open.lock().unwrap().insert(vault, open);

        Ok(())
    }

    /// Unlocks with the remembered password. Returns `false` if there is none and one must be
    /// typed. Takes time (Argon2).
    pub fn auto_unlock(&self, vault: Uuid) -> Result<bool, String> {
        let (path, remembered) = self.known(vault)?;

        if !remembered {
            return Ok(false);
        }

        // It may be missing even though the list says it is there, if it was deleted outside the app.
        let Some(password) = remembered::load(vault)? else {
            return Ok(false);
        };

        let open = Self::open_vault(vault, &path, &password)?;
        self.open.lock().unwrap().insert(vault, open);

        Ok(true)
    }

    /// Forgets the remembered password: the next unlock asks for it again.
    pub fn forget_password(&self, vault: Uuid) -> Result<(), String> {
        self.remember_password(vault, None)
    }

    /// Changes the master password without closing the SSH sessions. Takes time (Argon2 twice).
    pub fn change_password(&self, vault: Uuid, current: &str, new: &str) -> Result<(), String> {
        // Only the folder is copied: Argon2 runs without the lock so as not to slow down other vaults.
        let store = self.with_open(vault, |v| v.store())?;
        let (_, remembered) = self.known(vault)?;

        session::change_password(vault, &store, current, new)?;

        // The remembered password is replaced with the new one; otherwise the next start would fail with the old one.
        if remembered {
            self.remember_password(vault, Some(new)).map_err(|e| {
                format!(
                    "The password was changed, but the remembered one could not be updated. {e}"
                )
            })?;
        }

        Ok(())
    }

    /// Locks a vault: forgets its key and its records.
    pub fn lock(&self, vault: Uuid) {
        self.open.lock().unwrap().remove(&vault);
    }

    pub fn is_unlocked(&self, vault: Uuid) -> bool {
        self.open.lock().unwrap().contains_key(&vault)
    }

    /// Reads the vault from disk again (for example, after a conflict with another device).
    pub fn reload(&self, vault: Uuid) -> Result<(), String> {
        self.change(vault, |open| open.reload())
    }

    pub fn view(&self, vault: Uuid) -> Result<VaultView, String> {
        self.with_open(vault, |v| VaultView {
            hosts: v
                .hosts
                .iter()
                .map(|(id, h)| HostView {
                    id: *id,
                    name: h.name.clone(),
                    address: h.address.clone(),
                    port: h.port,
                    host_key: h.host_key.clone(),
                    group_id: h.group_id,
                    notes: h.notes.clone(),
                    os: h.os.clone(),
                    mcp: h.mcp,
                    methods: h
                        .access_methods
                        .iter()
                        .map(|m| method_view(m, &v.keys))
                        .collect(),
                })
                .collect(),
            groups: v
                .groups
                .iter()
                .map(|(id, g)| GroupView {
                    id: *id,
                    name: g.name.clone(),
                })
                .collect(),
            keys: v.keys.iter().map(|(id, k)| key_view(*id, k)).collect(),
            tunnels: v
                .tunnels
                .iter()
                .map(|(id, t)| tunnel_view(*id, t, &v.hosts))
                .collect(),
            warnings: v.warnings.clone(),
        })
    }

    // ---- Hosts and groups ----

    /// Creates (`id: None`) or edits a host. Returns its id.
    pub fn save_host(
        &self,
        vault: Uuid,
        id: Option<Uuid>,
        host: HostInput,
    ) -> Result<Uuid, String> {
        let name = host.name.trim().to_string();
        let address = host.address.trim().to_string();

        if name.is_empty() || address.is_empty() || host.port == 0 {
            return Err("The name, address or port is missing.".into());
        }

        let host_id = id.unwrap_or_else(Uuid::new_v4);

        self.change(vault, |open| {
            let existing = id.and_then(|id| open.hosts.get(&id));
            let same = existing.filter(|h| h.address == address && h.port == host.port);

            let new_host = Host {
                // When editing, the access methods it already had are kept.
                access_methods: existing
                    .map(|h| h.access_methods.clone())
                    .unwrap_or_default(),
                notes: host.notes.trim().to_string(),
                // The fingerprint and OS are kept if it is still the same server; if the address changes, they are looked up again.
                host_key: same.and_then(|h| h.host_key.clone()),
                os: same.and_then(|h| h.os.clone()),
                mcp: existing.and_then(|h| h.mcp),
                name,
                address,
                port: host.port,
                group_id: host.group_id,
            };

            open.save(host_id, Record::Host(new_host))
        })?;

        Ok(host_id)
    }

    /// Creates (`id: None`) or renames a group. Renaming keeps its parent group.
    pub fn save_group(&self, vault: Uuid, id: Option<Uuid>, name: &str) -> Result<Uuid, String> {
        if name.trim().is_empty() {
            return Err("The name is missing.".into());
        }

        let group_id = id.unwrap_or_else(Uuid::new_v4);

        self.change(vault, |open| {
            let group = Group {
                name: name.trim().to_string(),
                parent_id: id
                    .and_then(|id| open.groups.get(&id))
                    .and_then(|g| g.parent_id),
            };

            open.save(group_id, Record::Group(group))
        })?;

        Ok(group_id)
    }

    /// Deletes a host, key, group or tunnel. If it is a host, the caller closes the connections that used it.
    pub fn delete(&self, vault: Uuid, id: Uuid) -> Result<(), String> {
        self.change(vault, |open| open.delete(id))
    }

    /// The method with its secret, for editing it in a form.
    pub fn method(&self, vault: Uuid, host: Uuid, method: Uuid) -> Result<AccessMethod, String> {
        self.with_open(vault, |v| {
            let host = v.hosts.get(&host)?;
            host.access_methods.iter().find(|m| m.id == method).cloned()
        })?
        .ok_or_else(|| "The access method no longer exists.".into())
    }

    /// Adds an access method to a host, or replaces it if it already has one with the same id.
    pub fn save_method(&self, vault: Uuid, host: Uuid, method: AccessMethod) -> Result<(), String> {
        self.edit_host(vault, host, |h| {
            match h.access_methods.iter_mut().find(|m| m.id == method.id) {
                Some(existing) => *existing = method,
                None => h.access_methods.push(method),
            }
        })
    }

    pub fn remove_method(&self, vault: Uuid, host: Uuid, method: Uuid) -> Result<(), String> {
        self.edit_host(vault, host, |h| h.access_methods.retain(|m| m.id != method))
    }

    /// Saves the server fingerprint that the user accepted on first connection.
    pub fn trust_host_key(
        &self,
        vault: Uuid,
        host: Uuid,
        fingerprint: String,
    ) -> Result<(), String> {
        self.edit_host(vault, host, |h| h.host_key = Some(fingerprint))
    }

    /// Forgets the accepted fingerprint: the next connection asks for confirmation again.
    pub fn forget_host_key(&self, vault: Uuid, host: Uuid) -> Result<(), String> {
        self.edit_host(vault, host, |h| h.host_key = None)
    }

    // ---- Tunnels ----

    /// The vault's tunnels, without the rest of the view: to start or stop them.
    pub fn tunnels(&self, vault: Uuid) -> Result<Vec<TunnelView>, String> {
        self.with_open(vault, |v| {
            v.tunnels
                .iter()
                .map(|(id, t)| tunnel_view(*id, t, &v.hosts))
                .collect()
        })
    }

    /// Creates (`id: None`, already enabled) or edits a tunnel. When editing, whether it is enabled is kept.
    pub fn save_tunnel(
        &self,
        vault: Uuid,
        id: Option<Uuid>,
        tunnel: TunnelInput,
    ) -> Result<Uuid, String> {
        let name = tunnel.name.trim().to_string();
        let bind = tunnel.bind.trim().to_string();

        if name.is_empty() || tunnel.port == 0 {
            return Err("The name or port is missing.".into());
        }

        let Ok(ip) = bind.parse::<std::net::IpAddr>() else {
            return Err("The listen address must be an IP, like 127.0.0.1.".into());
        };

        let kind = match tunnel.kind {
            TunnelKind::Forward { host, port } => {
                let host = host.trim().to_string();

                if host.is_empty() || port == 0 {
                    return Err("The destination host or port is missing.".into());
                }

                TunnelKind::Forward { host, port }
            }
            kind => kind,
        };

        let tunnel_id = id.unwrap_or_else(Uuid::new_v4);

        self.change(vault, |v| {
            let enabled = id
                .and_then(|id| v.tunnels.get(&id))
                .is_none_or(|t| t.enabled);

            // Two tunnels of the same vault on the same port: the second one could never listen.
            let taken = v.tunnels.iter().any(|(other, t)| {
                Some(*other) != id
                    && t.port == tunnel.port
                    && t.bind
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|b| b == ip || b.is_unspecified() || ip.is_unspecified())
            });

            let target = v
                .hosts
                .get(&tunnel.host_id)
                .is_some_and(|h| h.access_methods.iter().any(|m| m.id == tunnel.method_id));

            if taken {
                return Err(format!("Another tunnel already uses port {}.", tunnel.port));
            }

            if !target {
                return Err("The host or access method no longer exists.".into());
            }

            let record = Tunnel {
                name,
                kind,
                bind,
                port: tunnel.port,
                host_id: tunnel.host_id,
                method_id: tunnel.method_id,
                enabled,
            };

            v.save(tunnel_id, Record::Tunnel(record))
        })?;

        Ok(tunnel_id)
    }

    /// Enables or disables a tunnel. Starting or stopping it is up to the caller (`sessions`).
    pub fn set_tunnel_enabled(&self, vault: Uuid, id: Uuid, enabled: bool) -> Result<(), String> {
        self.change(vault, |open| {
            let mut tunnel = open
                .tunnels
                .get(&id)
                .cloned()
                .ok_or("The tunnel no longer exists.")?;

            tunnel.enabled = enabled;
            open.save(id, Record::Tunnel(tunnel))
        })
    }

    // ---- Keychain ----

    /// The key with its secrets, for editing it in a form.
    pub fn key(&self, vault: Uuid, id: Uuid) -> Result<KeyInput, String> {
        let key = self
            .with_open(vault, |v| v.keys.get(&id).cloned())?
            .ok_or("The key no longer exists.")?;

        Ok(KeyInput {
            public_key: KeyInfo::parse(&key.private_key)
                .map(|info| info.public)
                .unwrap_or_default(),
            name: key.name.clone(),
            private_key: key.private_key.clone(),
            passphrase: key.passphrase.clone().unwrap_or_default(),
        })
    }

    /// Imports (`id: None`) or edits a key. Returns its id.
    pub fn save_key(&self, vault: Uuid, id: Option<Uuid>, key: KeyInput) -> Result<Uuid, String> {
        if key.name.trim().is_empty() {
            return Err("The name is missing.".into());
        }

        KeyInfo::parse(&key.private_key)?;

        if !key.public_key.trim().is_empty() {
            check_public_key(&key.private_key, &key.public_key)?;
        }

        let new_key = Key {
            name: key.name.trim().to_string(),
            private_key: key.private_key.trim().to_string(),
            passphrase: (!key.passphrase.is_empty()).then(|| key.passphrase.clone()),
        };

        let key_id = id.unwrap_or_else(Uuid::new_v4);
        self.change(vault, |open| open.save(key_id, Record::Key(new_key)))?;

        Ok(key_id)
    }

    /// Generates a new key with the given name and saves it in the keychain. Returns its id.
    pub fn generate_key(&self, vault: Uuid, name: &str) -> Result<Uuid, String> {
        let private_key =
            session::generate_key(name).map_err(|e| format!("Could not generate the key: {e}"))?;

        let id = Uuid::new_v4();

        let key = Key {
            name: name.to_string(),
            private_key,
            passphrase: None,
        };

        self.change(vault, |open| open.save(id, Record::Key(key)))?;

        Ok(id)
    }
}

/// Reads a private key from a file and, if present, its `.pub` next to it.
pub fn read_key_file(path: &Path) -> Result<KeyFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Disk error: {e}"))?;

    let private_key = String::from_utf8(bytes)
        .map_err(|_| "The file is not text: it does not look like an OpenSSH key.".to_string())?;

    let mut public_path = path.to_path_buf().into_os_string();
    public_path.push(".pub");

    Ok(KeyFile {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        private_key,
        public_key: std::fs::read_to_string(public_path)
            .ok()
            .map(|p| p.trim().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Checks that a vault is reported missing only after `check_missing` has looked at the disk.
    #[test]
    fn missing_folder_is_found_by_check_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Vault");
        let vaults = Vaults::at(tmp.path().join("vaults.json"));
        vaults.create(dir.clone(), "pw").unwrap();

        vaults.check_missing();
        assert!(!vaults.list()[0].missing);

        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!vaults.list()[0].missing);

        vaults.check_missing();
        assert!(vaults.list()[0].missing);
    }

    // `Vaults` is built by hand: `create` would write `vaults.json` to the real config folder.
    #[test]
    fn host_key_is_saved_and_forgotten() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Vault");
        let mut open = OpenVault::create(&dir, "pw").unwrap();
        let (vault, id) = (open.id, Uuid::new_v4());

        let host = Host {
            name: "web".into(),
            address: "10.0.0.1".into(),
            port: 22,
            group_id: None,
            access_methods: vec![],
            notes: String::new(),
            host_key: None,
            os: None,
            mcp: None,
        };
        open.save(id, Record::Host(host)).unwrap();

        let vaults = Vaults {
            known: Mutex::new(vec![]),
            open: Mutex::new(HashMap::from([(vault, open)])),
            missing: Mutex::default(),
            file: None,
        };
        let key = |v: &Vaults| {
            v.with_open(vault, |o| o.hosts[&id].host_key.clone())
                .unwrap()
        };

        vaults
            .trust_host_key(vault, id, "SHA256:web".into())
            .unwrap();
        assert_eq!(key(&vaults), Some("SHA256:web".into()));

        vaults.forget_host_key(vault, id).unwrap();
        assert_eq!(key(&vaults), None);

        // Forgetting a host that no longer exists returns an error, not a panic.
        assert!(vaults.forget_host_key(vault, Uuid::new_v4()).is_err());
    }
}
