// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! An open vault: its records decrypted in memory and the operations to change them.
//! Each change is encrypted and written to disk immediately; memory is only updated if the write succeeds.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use ssh_key::rand_core::OsRng;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey};
use store::Store;
use uuid::Uuid;
use vault::{Group, Host, Key, Record, Tunnel, UnlockedVault, Vault};

pub struct OpenVault {
    pub id: Uuid,
    store: Store,
    key: UnlockedVault,
    pub hosts: BTreeMap<Uuid, Host>,
    pub keys: BTreeMap<Uuid, Key>,
    pub groups: BTreeMap<Uuid, Group>,
    pub tunnels: BTreeMap<Uuid, Tunnel>,
    /// Last known revision of each record, including deleted ones.
    revisions: HashMap<Uuid, u64>,
    /// Warnings on open: records from newer versions, damaged or in conflict.
    pub warnings: Vec<String>,
}

impl OpenVault {
    /// Creates a new, empty vault in `dir`. Takes time (Argon2): call off the UI thread.
    pub fn create(dir: &Path, password: &str) -> Result<Self, String> {
        let (key, header) = Vault::create(password);
        let store = Store::create(dir, &header).map_err(store_error)?;
        Ok(Self::empty(header.vault_id, store, key))
    }

    /// Unlocks the vault in `dir` and decrypts its records. Takes time (Argon2): call off the UI thread.
    pub fn open(dir: &Path, password: &str) -> Result<Self, String> {
        let (store, header) = Store::open(dir).map_err(store_error)?;
        let key = Vault::unlock(password, &header).map_err(core_error)?;
        let mut vault = Self::empty(header.vault_id, store, key);
        vault.reload()?;
        Ok(vault)
    }

    /// Reads the id of the vault in `dir` without a password, to add it to the vault list.
    pub fn peek_id(dir: &Path) -> Result<Uuid, String> {
        Store::open(dir)
            .map(|(_, header)| header.vault_id)
            .map_err(store_error)
    }

    /// Copy of the vault's folder, to work with it without holding the vaults lock.
    pub fn store(&self) -> Store {
        self.store.clone()
    }

    fn empty(id: Uuid, store: Store, key: UnlockedVault) -> Self {
        Self {
            id,
            store,
            key,
            hosts: BTreeMap::new(),
            keys: BTreeMap::new(),
            groups: BTreeMap::new(),
            tunnels: BTreeMap::new(),
            revisions: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    /// Reads everything from disk again, for example after a conflict with another device.
    pub fn reload(&mut self) -> Result<(), String> {
        let loaded = self.store.load_all().map_err(store_error)?;
        let (mut newer, mut broken) = (0, 0);

        self.hosts.clear();
        self.keys.clear();
        self.groups.clear();
        self.tunnels.clear();
        self.revisions.clear();

        for rec in &loaded.records {
            self.revisions.insert(rec.id, rec.revision);

            match self.key.open(rec) {
                Ok(Record::Host(host)) => {
                    self.hosts.insert(rec.id, host);
                }
                Ok(Record::Key(key)) => {
                    self.keys.insert(rec.id, key);
                }
                Ok(Record::Group(group)) => {
                    self.groups.insert(rec.id, group);
                }
                Ok(Record::Tunnel(tunnel)) => {
                    self.tunnels.insert(rec.id, tunnel);
                }
                Ok(Record::Tombstone) => {}
                // They are left untouched on disk: a newer version of the app does understand them.
                Err(vault::Error::UnsupportedVersion) => newer += 1,
                Err(_) => broken += 1,
            }
        }

        self.warnings.clear();

        if newer > 0 {
            self.warnings.push(format!(
                "{newer} records are from a newer version of the app and are not shown."
            ));
        }

        if broken > 0 || !loaded.unreadable.is_empty() {
            let count = broken + loaded.unreadable.len();
            self.warnings
                .push(format!("{count} records are damaged and are not shown."));
        }

        if !loaded.conflicts.is_empty() {
            let count = loaded.conflicts.len();
            self.warnings.push(format!("{count} records were edited at the same time on two devices; one of the versions has been loaded."));
        }

        Ok(())
    }

    /// Saves a new or changed record. With `Record::Tombstone` it deletes it.
    pub fn save(&mut self, id: Uuid, record: Record) -> Result<(), String> {
        let revision = self.revisions.get(&id).copied().unwrap_or(0) + 1;
        let sealed = self.key.seal(id, revision, &record);
        self.store.put(&sealed).map_err(store_error)?;
        self.revisions.insert(id, revision);

        match record {
            Record::Host(host) => {
                self.hosts.insert(id, host);
            }
            Record::Key(key) => {
                self.keys.insert(id, key);
            }
            Record::Group(group) => {
                self.groups.insert(id, group);
            }
            Record::Tunnel(tunnel) => {
                self.tunnels.insert(id, tunnel);
            }
            Record::Tombstone => {
                self.hosts.remove(&id);
                self.keys.remove(&id);
                self.groups.remove(&id);
                self.tunnels.remove(&id);
            }
        }

        Ok(())
    }

    pub fn delete(&mut self, id: Uuid) -> Result<(), String> {
        self.save(id, Record::Tombstone)
    }
}

/// Public data of a private key, read with `ssh-key`.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyInfo {
    pub algorithm: String,
    pub fingerprint: String,
    /// Public key in `authorized_keys` format.
    pub public: String,
    /// The key is protected with a passphrase.
    pub encrypted: bool,
}

impl KeyInfo {
    /// Only understands the OpenSSH format (`-----BEGIN OPENSSH PRIVATE KEY-----`).
    pub fn parse(private_key: &str) -> Result<Self, String> {
        let key = PrivateKey::from_openssh(private_key.trim())
            .map_err(|_| "Not a private key in OpenSSH format.".to_string())?;

        Ok(Self {
            algorithm: key.algorithm().as_str().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
            public: key.public_key().to_openssh().unwrap_or_default(),
            encrypted: key.is_encrypted(),
        })
    }
}

/// Checks that `public` (`authorized_keys` format) is the pair of the private key.
/// The comment does not count: only the key data is compared.
pub fn check_public_key(private_key: &str, public: &str) -> Result<(), String> {
    let private = PrivateKey::from_openssh(private_key.trim())
        .map_err(|_| "Not a private key in OpenSSH format.".to_string())?;
    let public = PublicKey::from_openssh(public.trim())
        .map_err(|_| "The public key is not in authorized_keys format.".to_string())?;

    if public.key_data() != private.public_key().key_data() {
        return Err("The public key does not match the private one.".into());
    }

    Ok(())
}

/// Generates a new Ed25519 key in OpenSSH format. No passphrase: it is already encrypted inside the vault.
pub fn generate_key(comment: &str) -> Result<String, String> {
    let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(|e| e.to_string())?;
    key.set_comment(comment);

    let pem = key.to_openssh(LineEnding::LF).map_err(|e| e.to_string())?;
    Ok(pem.to_string())
}

/// Changes the master password of the vault `id` without touching its records. Takes time (Argon2
/// twice): call off the UI thread and without holding the vaults lock.
pub fn change_password(id: Uuid, store: &Store, current: &str, new: &str) -> Result<(), String> {
    if new.chars().count() < 8 {
        return Err("The master password needs at least 8 characters.".into());
    }

    // It is validated against the header on disk: another device may have changed it since it was opened.
    let header = store.header().map_err(store_error)?;

    if header.vault_id != id {
        return Err("The folder now contains another vault.".into());
    }

    let key = Vault::unlock(current, &header).map_err(core_error)?;
    let replacement = key.change_password(new);

    // shortcut: there is no lock between processes; add one if simultaneous local writers are allowed.
    if store.header().map_err(store_error)? != header {
        return Err("The password was changed on another device. Try again.".into());
    }

    store.set_header(&replacement).map_err(store_error)
}

fn store_error(e: store::Error) -> String {
    match e {
        store::Error::NotAVault => "The folder does not contain a vault.".into(),
        store::Error::AlreadyExists => {
            "The folder already has files. Choose an empty folder.".into()
        }
        store::Error::Conflict(_) => {
            "This record was changed on another device. Reload the vault before editing it.".into()
        }
        store::Error::Io(e) => format!("Disk error: {e}"),
    }
}

fn core_error(e: vault::Error) -> String {
    match e {
        vault::Error::WrongPassword => "Wrong password.".into(),
        vault::Error::UnsupportedVersion => "This vault is from a newer version of the app.".into(),
        vault::Error::Corrupted => "The vault is damaged.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test key generated only for these tests (ssh-keygen -t ed25519).
    const TEST_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACAr0ZAbZJFrco8LlU7WeIurVIheMogUlb4FkgQBfvIkcgAAAJCaxZLjmsWS
4wAAAAtzc2gtZWQyNTUxOQAAACAr0ZAbZJFrco8LlU7WeIurVIheMogUlb4FkgQBfvIkcg
AAAEBb/P8+u7buCACwLg2JZrGq2b/NIO8Cxf2uoT0nWVOSESvRkBtkkWtyjwuVTtZ4i6tU
iF4yiBSVvgWSBAF+8iRyAAAAC3Rlc3RAbWFyay02AQI=
-----END OPENSSH PRIVATE KEY-----";

    fn host(name: &str) -> Host {
        Host {
            name: name.into(),
            address: "10.0.0.1".into(),
            port: 22,
            group_id: None,
            access_methods: vec![],
            notes: String::new(),
            host_key: None,
            os: None,
            mcp: None,
        }
    }

    // Checks creating, saving, editing, deleting and reopening, with the changes on disk.
    #[test]
    fn changes_survive_reopening() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Vault");
        let mut vault = OpenVault::create(&dir, "pw").unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());

        vault.save(a, Record::Host(host("a"))).unwrap();
        vault.save(b, Record::Host(host("b"))).unwrap();
        vault.save(a, Record::Host(host("a2"))).unwrap();
        vault.delete(b).unwrap();

        let reopened = OpenVault::open(&dir, "pw").unwrap();
        assert_eq!(reopened.id, vault.id);
        assert_eq!(reopened.hosts.len(), 1);
        assert_eq!(reopened.hosts[&a].name, "a2");
        assert!(reopened.warnings.is_empty());
        assert_eq!(OpenVault::peek_id(&dir).unwrap(), vault.id);
    }

    // Checks that a wrong password gives a clear message.
    #[test]
    fn wrong_password_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Vault");
        OpenVault::create(&dir, "pw").unwrap();
        assert_eq!(
            OpenVault::open(&dir, "other").err().unwrap(),
            "Wrong password."
        );
    }

    #[test]
    fn password_change_preserves_records_and_checks_current_header() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Vault");
        let mut vault = OpenVault::create(&dir, "original").unwrap();
        let id = Uuid::new_v4();
        vault.save(id, Record::Host(host("web"))).unwrap();
        let change = |current, new| change_password(vault.id, &vault.store, current, new);
        let before = std::fs::read(dir.join("vault.json")).unwrap();
        let record = std::fs::read(dir.join(format!("records/{id}.json"))).unwrap();

        // Wrong current password or new one too short: the header does not change.
        assert!(change("wrong", "replacement").is_err());
        assert!(change("original", "short").is_err());
        assert_eq!(std::fs::read(dir.join("vault.json")).unwrap(), before);

        change("original", "replacement").unwrap();
        assert!(OpenVault::open(&dir, "original").is_err());
        assert_eq!(
            OpenVault::open(&dir, "replacement").unwrap().hosts[&id].name,
            "web"
        );
        assert_eq!(
            std::fs::read(dir.join(format!("records/{id}.json"))).unwrap(),
            record
        );

        // The current password is checked against the disk, not against the one in memory.
        assert!(change("original", "another password").is_err());

        // The open vault can still save with the key it already had.
        vault.save(id, Record::Host(host("still open"))).unwrap();
        assert_eq!(
            OpenVault::open(&dir, "replacement").unwrap().hosts[&id].name,
            "still open"
        );
    }

    #[test]
    fn password_change_refuses_replaced_vault_and_disk_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Vault");
        let vault = OpenVault::create(&dir, "original").unwrap();

        // The folder now contains another vault: its header is not touched.
        let (_, replacement) = Vault::create("original");
        vault.store.set_header(&replacement).unwrap();
        assert!(change_password(vault.id, &vault.store, "original", "replacement").is_err());
        assert_eq!(vault.store.header().unwrap(), replacement);

        // Without a header on disk it fails and does not create a new one.
        std::fs::remove_file(dir.join("vault.json")).unwrap();
        assert!(change_password(vault.id, &vault.store, "original", "replacement").is_err());
        assert!(!dir.join("vault.json").exists());
    }

    // Checks that if another device changed a record, saving fails instead of overwriting it.
    #[test]
    fn concurrent_edit_is_a_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Vault");
        let mut here = OpenVault::create(&dir, "pw").unwrap();
        let id = Uuid::new_v4();
        here.save(id, Record::Host(host("v1"))).unwrap();

        let mut there = OpenVault::open(&dir, "pw").unwrap();
        there.save(id, Record::Host(host("other device"))).unwrap();

        assert!(here.save(id, Record::Host(host("here"))).is_err());
        here.reload().unwrap();
        assert_eq!(here.hosts[&id].name, "other device");
        here.save(id, Record::Host(host("here"))).unwrap();
    }

    // Checks that the key's public half is accepted (even if the comment changes) and others are rejected.
    #[test]
    fn checks_public_key_pair() {
        let public = KeyInfo::parse(TEST_KEY).unwrap().public;
        let without_comment = public.rsplit_once(' ').unwrap().0;
        let other = KeyInfo::parse(&generate_key("other").unwrap())
            .unwrap()
            .public;

        assert!(check_public_key(TEST_KEY, &public).is_ok());
        assert!(check_public_key(TEST_KEY, &format!("{without_comment} other-name")).is_ok());
        assert!(check_public_key(TEST_KEY, &other).is_err());
        assert!(check_public_key(TEST_KEY, "not a key").is_err());
    }

    // Checks that the generated key is Ed25519, without passphrase and with the name as comment.
    #[test]
    fn generates_ed25519_keys() {
        let info = KeyInfo::parse(&generate_key("résumé").unwrap()).unwrap();

        assert_eq!(info.algorithm, "ssh-ed25519");
        assert!(!info.encrypted);
        assert!(info.public.ends_with(" résumé"), "{}", info.public);
    }

    // Checks that a key's algorithm and fingerprint are read, and that non-keys are rejected.
    #[test]
    fn parses_openssh_keys() {
        let info = KeyInfo::parse(TEST_KEY).unwrap();
        assert_eq!(info.algorithm, "ssh-ed25519");
        assert_eq!(
            info.fingerprint,
            "SHA256:YPepSdegZxVhhAWIIkDa4vsxjXU7wjsWaV1HejqgZRM"
        );
        assert!(info.public.starts_with("ssh-ed25519 "));
        assert!(!info.encrypted);
        assert!(KeyInfo::parse("hello").is_err());
    }
}
