// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Stores a vault as a folder of JSON files: `vault.json` (header) and `records/<id>.json` (one encrypted record
//! per file). One file per record makes it possible to sync with Drive or Git without two devices clashing.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;
use vault::{EncryptedRecord, VaultHeader};

pub const HEADER_FILE: &str = "vault.json";
const RECORDS_DIR: &str = "records";

/// Storage errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Read or write failure on disk.
    #[error("disk error: {0}")]
    Io(#[from] std::io::Error),
    /// The folder does not contain a vault (`vault.json` is missing or damaged).
    #[error("the folder is not a valid vault")]
    NotAVault,
    /// Tried to create a vault in a folder that is not empty.
    #[error("the folder already exists and is not empty")]
    AlreadyExists,
    /// The disk has an equal or newer revision: another device changed it. It must be reloaded.
    #[error("record {0} was changed on another device")]
    Conflict(Uuid),
}

/// Result of reading all the records.
#[derive(Debug, Default)]
pub struct Loaded {
    /// One record per id, the one with the highest revision.
    pub records: Vec<EncryptedRecord>,
    /// IDs edited at the same time on two devices (same revision, different content). One of the two was loaded.
    pub conflicts: Vec<Uuid>,
    /// Files that could not be read (half-synced, Git conflict…). They are left untouched.
    pub unreadable: Vec<PathBuf>,
}

/// Vault stored in a folder.
#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Creates the vault in `dir`. Fails if the folder already has anything, so another vault is never overwritten.
    pub fn create(dir: &Path, header: &VaultHeader) -> Result<Self, Error> {
        if dir.exists() && fs::read_dir(dir)?.next().is_some() {
            return Err(Error::AlreadyExists);
        }

        fs::create_dir_all(dir.join(RECORDS_DIR))?;
        let store = Self {
            dir: dir.to_path_buf(),
        };

        store.set_header(header)?;
        Ok(store)
    }

    /// Opens an existing vault and returns its header, which is needed to unlock it.
    pub fn open(dir: &Path) -> Result<(Self, VaultHeader), Error> {
        let store = Self {
            dir: dir.to_path_buf(),
        };
        let header = store.header()?;
        fs::create_dir_all(dir.join(RECORDS_DIR))?;
        Ok((store, header))
    }

    /// Reads the current header, which may have changed through sync.
    pub fn header(&self) -> Result<VaultHeader, Error> {
        let bytes = fs::read(self.dir.join(HEADER_FILE)).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotAVault,
            _ => Error::Io(e),
        })?;
        serde_json::from_slice(&bytes).map_err(|_| Error::NotAVault)
    }

    /// Replaces the header (when changing the master password).
    pub fn set_header(&self, header: &VaultHeader) -> Result<(), Error> {
        write_atomic(&self.dir.join(HEADER_FILE), header)
    }

    /// Reads all the records. Drive conflict copies (`<id> (1).json`) are resolved by revision.
    pub fn load_all(&self) -> Result<Loaded, Error> {
        let mut best: HashMap<Uuid, (EncryptedRecord, bool)> = HashMap::new();
        let mut loaded = Loaded::default();

        for entry in fs::read_dir(self.dir.join(RECORDS_DIR))? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            // `.tmp` files are half-written files from write_atomic.
            if name.starts_with('.') || !name.ends_with(".json") {
                continue;
            }

            let Ok(rec) = fs::read(&path)
                .map_err(drop)
                .and_then(|b| serde_json::from_slice::<EncryptedRecord>(&b).map_err(drop))
            else {
                loaded.unreadable.push(path);
                continue;
            };

            // The id is read from the content, not the name, so copies renamed by Drive still count.
            let canonical = name == file_name(rec.id);

            match best.get(&rec.id) {
                Some((cur, _)) if cur.revision > rec.revision => {}
                Some((cur, cur_canonical)) if cur.revision == rec.revision => {
                    if *cur != rec && !loaded.conflicts.contains(&rec.id) {
                        loaded.conflicts.push(rec.id);
                    }

                    // On a tie the original file wins, so the result does not depend on the disk order.
                    if canonical && !cur_canonical {
                        best.insert(rec.id, (rec, canonical));
                    }
                }
                _ => {
                    best.insert(rec.id, (rec, canonical));
                }
            }
        }

        loaded.records = best.into_values().map(|(rec, _)| rec).collect();
        Ok(loaded)
    }

    /// Saves a record. Its revision must be greater than the one on disk; otherwise `Conflict`.
    pub fn put(&self, rec: &EncryptedRecord) -> Result<(), Error> {
        let path = self.record_path(rec.id);

        if let Ok(bytes) = fs::read(&path)
            && let Ok(on_disk) = serde_json::from_slice::<EncryptedRecord>(&bytes)
            && on_disk.revision >= rec.revision
        {
            return Err(Error::Conflict(rec.id));
        }

        // shortcut: old Drive conflict copies stay on disk; clean them up when they become a nuisance.
        write_atomic(&path, rec)
    }

    fn record_path(&self, id: Uuid) -> PathBuf {
        self.dir.join(RECORDS_DIR).join(file_name(id))
    }
}

fn file_name(id: Uuid) -> String {
    format!("{id}.json")
}

/// Writes to a temporary file and renames it: a power cut never leaves a half-written file.
fn write_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    let mut file = fs::File::create(&tmp)?;

    file.write_all(&serde_json::to_vec_pretty(value).expect("vault types always serialize"))?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault::{Record, Vault};

    fn record(id: Uuid, revision: u64, byte: u8) -> EncryptedRecord {
        EncryptedRecord {
            id,
            revision,
            format_version: 1,
            nonce: [byte; 24],
            ciphertext: vec![byte; 8],
        }
    }

    fn new_store(tmp: &tempfile::TempDir) -> Store {
        let (_, header) = Vault::create("pw");

        Store::create(&tmp.path().join("v"), &header).unwrap()
    }

    // Checks the full cycle: create, encrypt, save, reopen, unlock and decrypt.
    #[test]
    fn end_to_end_with_real_vault() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("MyVault");
        let (vault, header) = Vault::create("pw");
        let store = Store::create(&dir, &header).unwrap();
        let id = Uuid::new_v4();

        store.put(&vault.seal(id, 1, &Record::Tombstone)).unwrap();

        let (store, header) = Store::open(&dir).unwrap();
        let vault = Vault::unlock("pw", &header).unwrap();
        let loaded = store.load_all().unwrap();

        assert_eq!(loaded.records.len(), 1);
        assert_eq!(vault.open(&loaded.records[0]).unwrap(), Record::Tombstone);
    }

    // Checks that a vault is not created on top of a folder with content, nor that a non-vault folder is opened.
    #[test]
    fn refuses_non_empty_dir_and_non_vault() {
        let tmp = tempfile::tempdir().unwrap();

        fs::write(tmp.path().join("something.txt"), "x").unwrap();
        let (_, header) = Vault::create("pw");

        assert!(matches!(
            Store::create(tmp.path(), &header),
            Err(Error::AlreadyExists)
        ));
        assert!(matches!(Store::open(tmp.path()), Err(Error::NotAVault)));
    }

    // Checks that a revision equal to or older than the one on disk cannot be saved.
    #[test]
    fn put_rejects_stale_revisions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = new_store(&tmp);
        let id = Uuid::new_v4();

        store.put(&record(id, 2, 1)).unwrap();
        assert!(matches!(
            store.put(&record(id, 2, 9)),
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            store.put(&record(id, 1, 9)),
            Err(Error::Conflict(_))
        ));
        store.put(&record(id, 3, 1)).unwrap();
        assert_eq!(store.load_all().unwrap().records, vec![record(id, 3, 1)]);
    }

    // Checks how the conflict copies created by Drive are resolved.
    #[test]
    fn resolves_drive_conflict_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let store = new_store(&tmp);
        let dir = tmp.path().join("v").join(RECORDS_DIR);
        let copy = |id: Uuid, rec: &EncryptedRecord| {
            fs::write(
                dir.join(format!("{id} (1).json")),
                serde_json::to_vec(rec).unwrap(),
            )
            .unwrap()
        };

        // The copy has a higher revision: the copy wins.
        let newer = Uuid::new_v4();

        store.put(&record(newer, 1, 1)).unwrap();
        copy(newer, &record(newer, 2, 2));

        // Identical copy: not a conflict.
        let same = Uuid::new_v4();

        store.put(&record(same, 1, 1)).unwrap();
        copy(same, &record(same, 1, 1));

        // Same revision, different content: conflict, and the original wins.
        let tie = Uuid::new_v4();

        store.put(&record(tie, 4, 1)).unwrap();
        copy(tie, &record(tie, 4, 2));

        let loaded = store.load_all().unwrap();
        let get = |id| loaded.records.iter().find(|r| r.id == id).unwrap().clone();

        assert_eq!(loaded.records.len(), 3);
        assert_eq!(get(newer), record(newer, 2, 2));
        assert_eq!(get(same), record(same, 1, 1));
        assert_eq!(get(tie), record(tie, 4, 1));
        assert_eq!(loaded.conflicts, vec![tie]);
    }

    // Checks that a broken file is reported without blocking the rest, and that temporary files are ignored.
    #[test]
    fn reports_unreadable_and_skips_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = new_store(&tmp);
        let dir = tmp.path().join("v").join(RECORDS_DIR);

        store.put(&record(Uuid::new_v4(), 1, 1)).unwrap();
        fs::write(dir.join("broken.json"), "<<<<<<< HEAD").unwrap();
        fs::write(dir.join(".something.json.tmp"), "garbage").unwrap();

        let loaded = store.load_all().unwrap();

        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.unreadable, vec![dir.join("broken.json")]);
    }

    // Checks that changing the header is saved and read back.
    #[test]
    fn set_header_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("v");
        let (vault, header) = Vault::create("old");
        let store = Store::create(&dir, &header).unwrap();

        store.set_header(&vault.change_password("new")).unwrap();
        let (_, header) = Store::open(&dir).unwrap();

        assert!(Vault::unlock("new", &header).is_ok());
    }
}
