// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::Error;
use crate::model::{Host, KNOWN_ACCESS_KINDS, KNOWN_TUNNEL_KINDS, Record, Tunnel};

/// Format version written by this app.
pub const FORMAT_VERSION: u32 = 1;
/// Padding size for the plaintext, so the length of the fields is not revealed.
const PAD: usize = 256;
// Header values are not trusted and reach Argon2 before being authenticated, so they are capped.
const MAX_M_KIB: u32 = 1 << 20;
const MAX_T: u32 = 64;
const MAX_P: u32 = 16;

/// Argon2 parameters and salt; they are stored in plaintext in the header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    #[serde(with = "b64")]
    pub salt: [u8; 16],
}

impl KdfParams {
    fn fresh() -> Self {
        Self {
            m_kib: 64 * 1024,
            t: 3,
            p: 1,
            salt: random(),
        }
    }
}

/// Vault key encrypted with the KEK: nonce and ciphertext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey {
    #[serde(with = "b64")]
    pub nonce: [u8; 24],
    #[serde(with = "b64")]
    pub ciphertext: Vec<u8>,
}

/// Stored in plaintext; it contains no secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultHeader {
    pub format_version: u32,
    pub vault_id: Uuid,
    pub kdf: KdfParams,
    pub wrapped_vault_key: WrappedKey,
}

/// Encrypted record, ready to be stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedRecord {
    pub id: Uuid,
    pub revision: u64,
    pub format_version: u32,
    #[serde(with = "b64")]
    pub nonce: [u8; 24],
    #[serde(with = "b64")]
    pub ciphertext: Vec<u8>,
}

/// Entry point for creating and unlocking a vault.
pub struct Vault;

impl Vault {
    /// Creates a new vault with a random key; returns the unlocked vault and its header.
    pub fn create(master_password: &str) -> (UnlockedVault, VaultHeader) {
        let vault = UnlockedVault {
            vault_id: Uuid::new_v4(),
            vault_key: Zeroizing::new(random()),
        };
        let header = vault.change_password(master_password);

        (vault, header)
    }

    /// Unlocks the vault with the master password; `WrongPassword` if it does not match.
    pub fn unlock(master_password: &str, header: &VaultHeader) -> Result<UnlockedVault, Error> {
        if header.format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion);
        }

        let kek = derive_kek(master_password, &header.kdf)?;
        let w = &header.wrapped_vault_key;
        let aad = header_aad(header.format_version, header.vault_id, &header.kdf);
        let key = Zeroizing::new(
            cipher(&kek)
                .decrypt(
                    XNonce::from_slice(&w.nonce),
                    Payload {
                        msg: &w.ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| Error::WrongPassword)?,
        );
        let vault_key: [u8; 32] = key.as_slice().try_into().map_err(|_| Error::Corrupted)?;

        Ok(UnlockedVault {
            vault_id: header.vault_id,
            vault_key: Zeroizing::new(vault_key),
        })
    }
}

/// Unlocked vault, with the key in memory.
pub struct UnlockedVault {
    vault_id: Uuid,
    vault_key: Zeroizing<[u8; 32]>,
}

impl UnlockedVault {
    /// Returns the vault id.
    pub fn vault_id(&self) -> Uuid {
        self.vault_id
    }

    /// Wraps the vault key again with a new password. Records remain valid.
    pub fn change_password(&self, new_password: &str) -> VaultHeader {
        let kdf = KdfParams::fresh();
        let kek = derive_kek(new_password, &kdf).expect("default KDF params are valid");
        let nonce: [u8; 24] = random();
        let aad = header_aad(FORMAT_VERSION, self.vault_id, &kdf);
        let ciphertext = cipher(&kek)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: self.vault_key.as_slice(),
                    aad: &aad,
                },
            )
            .expect("encryption with a valid key cannot fail");

        VaultHeader {
            format_version: FORMAT_VERSION,
            vault_id: self.vault_id,
            kdf,
            wrapped_vault_key: WrappedKey { nonce, ciphertext },
        }
    }

    /// Encrypts a record with its id and revision.
    pub fn seal(&self, id: Uuid, revision: u64, record: &Record) -> EncryptedRecord {
        let plaintext = match record {
            Record::Host(h) => envelope("host", h),
            Record::Key(k) => envelope("key", k),
            Record::Group(g) => envelope("group", g),
            Record::Tunnel(t) => envelope("tunnel", t),
            Record::Tombstone => envelope("tombstone", &()),
        };

        self.seal_bytes(id, revision, plaintext)
    }

    fn seal_bytes(
        &self,
        id: Uuid,
        revision: u64,
        mut plaintext: Zeroizing<Vec<u8>>,
    ) -> EncryptedRecord {
        // shortcut: resize may reallocate memory and leave an unzeroed copy on the heap; best-effort hygiene only.
        // JSON ignores trailing whitespace, so the space padding does not need to be stripped when reading.
        let padded_len = plaintext.len().div_ceil(PAD).max(1) * PAD;

        plaintext.resize(padded_len, b' ');
        let nonce: [u8; 24] = random();
        let aad = record_aad(self.vault_id, id, revision, FORMAT_VERSION);
        let ciphertext = cipher(&self.vault_key)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .expect("encryption with a valid key cannot fail");

        EncryptedRecord {
            id,
            revision,
            format_version: FORMAT_VERSION,
            nonce,
            ciphertext,
        }
    }

    /// `UnsupportedVersion` means it was written by a newer app: the record is left untouched.
    pub fn open(&self, rec: &EncryptedRecord) -> Result<Record, Error> {
        if rec.format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion);
        }

        let aad = record_aad(self.vault_id, rec.id, rec.revision, rec.format_version);
        let plaintext = Zeroizing::new(
            cipher(&self.vault_key)
                .decrypt(
                    XNonce::from_slice(&rec.nonce),
                    Payload {
                        msg: &rec.ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| Error::Corrupted)?,
        );

        // Two-step decoding: an unknown kind is treated as "newer app", not as corruption.
        #[derive(Deserialize)]
        struct Envelope<'a> {
            kind: String,
            #[serde(borrow)]
            data: &'a RawValue,
        }

        let env: Envelope = serde_json::from_slice(&plaintext).map_err(|_| Error::Corrupted)?;
        let data = env.data.get();

        match env.kind.as_str() {
            "host" => {
                check_access_kinds(data)?;
                Ok(Record::Host(parse::<Host>(data)?))
            }
            "key" => Ok(Record::Key(parse(data)?)),
            "group" => Ok(Record::Group(parse(data)?)),
            "tunnel" => {
                check_tunnel_kind(data)?;
                Ok(Record::Tunnel(parse::<Tunnel>(data)?))
            }
            "tombstone" => Ok(Record::Tombstone),
            _ => Err(Error::UnsupportedVersion),
        }
    }
}

fn check_access_kinds(host_json: &str) -> Result<(), Error> {
    // Reads only the `type` tags, so no secrets are copied outside the buffer that gets zeroed.
    #[derive(Deserialize)]
    struct Probe {
        access_methods: Vec<ProbeMethod>,
    }

    #[derive(Deserialize)]
    struct ProbeMethod {
        kind: ProbeKind,
    }

    #[derive(Deserialize)]
    struct ProbeKind {
        #[serde(rename = "type")]
        tag: String,
    }

    let probe: Probe = parse(host_json)?;

    match probe
        .access_methods
        .iter()
        .all(|m| KNOWN_ACCESS_KINDS.contains(&m.kind.tag.as_str()))
    {
        true => Ok(()),
        false => Err(Error::UnsupportedVersion),
    }
}

/// An unknown tunnel type comes from a newer app, not from a damaged record.
fn check_tunnel_kind(tunnel_json: &str) -> Result<(), Error> {
    #[derive(Deserialize)]
    struct Probe {
        kind: ProbeKind,
    }

    #[derive(Deserialize)]
    struct ProbeKind {
        #[serde(rename = "type")]
        tag: String,
    }

    let probe: Probe = parse(tunnel_json)?;

    match KNOWN_TUNNEL_KINDS.contains(&probe.kind.tag.as_str()) {
        true => Ok(()),
        false => Err(Error::UnsupportedVersion),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, Error> {
    serde_json::from_str(json).map_err(|_| Error::Corrupted)
}

fn envelope<T: Serialize>(kind: &str, data: &T) -> Zeroizing<Vec<u8>> {
    #[derive(Serialize)]
    struct Envelope<'a, T> {
        kind: &'a str,
        data: &'a T,
    }

    Zeroizing::new(
        serde_json::to_vec(&Envelope { kind, data }).expect("model types always serialize"),
    )
}

fn derive_kek(password: &str, kdf: &KdfParams) -> Result<Zeroizing<[u8; 32]>, Error> {
    if kdf.m_kib > MAX_M_KIB || kdf.t > MAX_T || kdf.p > MAX_P {
        return Err(Error::Corrupted);
    }

    let params = Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|_| Error::Corrupted)?;
    let password = Zeroizing::new(password.nfc().collect::<String>());
    let mut master_key = Zeroizing::new([0u8; 32]);

    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &kdf.salt, master_key.as_mut_slice())
        .map_err(|_| Error::Corrupted)?;
    let mut kek = Zeroizing::new([0u8; 32]);

    Hkdf::<Sha256>::new(None, master_key.as_slice())
        .expand(b"vaultty wrap v1", kek.as_mut_slice())
        .expect("32 bytes is a valid HKDF output length");
    Ok(kek)
}

// Header AAD: covers the version, id and KDF parameters.
fn header_aad(format_version: u32, vault_id: Uuid, kdf: &KdfParams) -> Vec<u8> {
    let mut aad = Vec::with_capacity(48);

    aad.extend_from_slice(&format_version.to_le_bytes());
    aad.extend_from_slice(vault_id.as_bytes());
    aad.extend_from_slice(&kdf.m_kib.to_le_bytes());
    aad.extend_from_slice(&kdf.t.to_le_bytes());
    aad.extend_from_slice(&kdf.p.to_le_bytes());
    aad.extend_from_slice(&kdf.salt);
    aad
}

// Record AAD: binds the blob to its vault, id, revision and version; moving or reverting it fails.
fn record_aad(vault_id: Uuid, id: Uuid, revision: u64, format_version: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(44);

    aad.extend_from_slice(vault_id.as_bytes());
    aad.extend_from_slice(id.as_bytes());
    aad.extend_from_slice(&revision.to_le_bytes());
    aad.extend_from_slice(&format_version.to_le_bytes());
    aad
}

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

fn random<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];

    OsRng.fill_bytes(&mut buf);
    buf
}

/// Serializes bytes as base64 text instead of lists of numbers in the JSON.
mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<S: Serializer>(bytes: impl AsRef<[u8]>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: TryFrom<Vec<u8>>>(
        d: D,
    ) -> Result<T, D::Error> {
        let text = String::deserialize(d)?;
        let bytes = STANDARD.decode(text).map_err(de::Error::custom)?;

        T::try_from(bytes).map_err(|_| de::Error::custom("incorrect byte length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Checks that bytes are base64 in the JSON and round-trip unchanged.
    #[test]
    fn json_uses_base64_and_roundtrips() {
        let (vault, header) = Vault::create("pw");
        let rec = vault.seal(Uuid::new_v4(), 1, &Record::Tombstone);
        let json = serde_json::to_string(&rec).unwrap();

        assert!(!json.contains('['), "{json}");
        assert_eq!(serde_json::from_str::<EncryptedRecord>(&json).unwrap(), rec);
        let json = serde_json::to_string(&header).unwrap();

        assert_eq!(serde_json::from_str::<VaultHeader>(&json).unwrap(), header);
    }
    use crate::model::{AccessKind, AccessMethod, Group, Key, TunnelKind};

    fn host(name: &str) -> Host {
        Host {
            name: name.into(),
            address: "10.0.0.1".into(),
            port: 22,
            group_id: None,
            host_key: None,
            os: None,
            mcp: None,
            access_methods: vec![
                AccessMethod {
                    id: Uuid::new_v4(),
                    label: String::new(),
                    kind: AccessKind::Password {
                        username: "deploy".into(),
                        password: "hunter2".into(),
                    },
                },
                AccessMethod {
                    id: Uuid::new_v4(),
                    label: "root key".into(),
                    kind: AccessKind::Key {
                        username: "root".into(),
                        key_id: Uuid::new_v4(),
                    },
                },
            ],
            notes: String::new(),
        }
    }

    // Checks that only the correct password unlocks the vault.
    #[test]
    fn unlock_with_right_and_wrong_password() {
        let (_, header) = Vault::create("correct horse");

        assert!(Vault::unlock("correct horse", &header).is_ok());
        assert_eq!(
            Vault::unlock("wrong", &header).err(),
            Some(Error::WrongPassword)
        );
    }

    // Checks that changing the password does not invalidate existing records.
    #[test]
    fn change_password_keeps_records() {
        let (vault, old_header) = Vault::create("old");
        let original = Record::Host(host("web"));
        let rec = vault.seal(Uuid::new_v4(), 1, &original);
        let new_header = vault.change_password("new");

        assert_eq!(
            Vault::unlock("old", &new_header).err(),
            Some(Error::WrongPassword)
        );
        let reopened = Vault::unlock("new", &new_header).unwrap();

        assert_eq!(reopened.open(&rec).unwrap(), original);
        assert_eq!(old_header.vault_id, new_header.vault_id);
    }

    // Checks that every record kind survives encryption and decryption.
    #[test]
    fn roundtrips_every_record_kind() {
        let (vault, _) = Vault::create("pw");
        let records = [
            Record::Host(host("web")),
            Record::Key(Key {
                name: "laptop".into(),
                private_key: "-----BEGIN".into(),
                passphrase: Some("pp".into()),
            }),
            Record::Group(Group {
                name: "prod".into(),
                parent_id: Some(Uuid::new_v4()),
            }),
            Record::Tunnel(Tunnel {
                name: "proxy".into(),
                kind: TunnelKind::Forward {
                    host: "db.internal".into(),
                    port: 5432,
                },
                bind: "127.0.0.1".into(),
                port: 1080,
                host_id: Uuid::new_v4(),
                method_id: Uuid::new_v4(),
                enabled: true,
            }),
            Record::Tombstone,
        ];

        for r in records {
            let sealed = vault.seal(Uuid::new_v4(), 7, &r);

            assert_eq!(vault.open(&sealed).unwrap(), r);
        }
    }

    // Checks that moving, reverting or altering a record is detected.
    #[test]
    fn tampering_is_detected() {
        let (vault, _) = Vault::create("pw");
        let (other, _) = Vault::create("pw");
        let rec = vault.seal(Uuid::new_v4(), 3, &Record::Host(host("web")));

        let mut moved = rec.clone();
        moved.id = Uuid::new_v4();
        let mut rolled_back = rec.clone();
        rolled_back.revision = 2;
        let mut flipped = rec.clone();
        flipped.ciphertext[0] ^= 1;

        for bad in [&moved, &rolled_back, &flipped] {
            assert_eq!(vault.open(bad).err(), Some(Error::Corrupted));
        }
        assert_eq!(other.open(&rec).err(), Some(Error::Corrupted));
    }

    // Checks that altering the header or its parameters prevents unlocking.
    #[test]
    fn header_tampering_fails_unlock() {
        let (_, header) = Vault::create("pw");
        let mut weaker = header.clone();
        weaker.kdf.t = 1;
        let mut other_id = header.clone();
        other_id.vault_id = Uuid::new_v4();

        assert_eq!(
            Vault::unlock("pw", &weaker).err(),
            Some(Error::WrongPassword)
        );
        assert_eq!(
            Vault::unlock("pw", &other_id).err(),
            Some(Error::WrongPassword)
        );

        let mut huge = header.clone();
        huge.kdf.m_kib = u32::MAX;
        assert_eq!(Vault::unlock("pw", &huge).err(), Some(Error::Corrupted));
    }

    // Checks that newer formats give UnsupportedVersion, not Corrupted.
    #[test]
    fn newer_formats_are_unsupported_not_corrupted() {
        let (vault, header) = Vault::create("pw");
        let id = Uuid::new_v4();
        let json = |s: &str| Zeroizing::new(s.as_bytes().to_vec());

        let unknown_kind = vault.seal_bytes(id, 1, json(r#"{"kind":"snippet","data":{}}"#));

        assert_eq!(
            vault.open(&unknown_kind).err(),
            Some(Error::UnsupportedVersion)
        );

        let unknown_method = vault.seal_bytes(
            id,
            1,
            json(
                r#"{"kind":"host","data":{"name":"x","address":"x","port":22,"group_id":null,"notes":"",
                "access_methods":[{"id":"00000000-0000-0000-0000-000000000000","label":"","kind":{"type":"totp","seed":"s"}}]}}"#,
            ),
        );

        assert_eq!(
            vault.open(&unknown_method).err(),
            Some(Error::UnsupportedVersion)
        );

        let unknown_tunnel = vault.seal_bytes(
            id,
            1,
            json(
                r#"{"kind":"tunnel","data":{"name":"x","kind":{"type":"socks6"},"bind":"127.0.0.1","port":1,
                "host_id":"00000000-0000-0000-0000-000000000000","method_id":"00000000-0000-0000-0000-000000000000","enabled":true}}"#,
            ),
        );

        assert_eq!(
            vault.open(&unknown_tunnel).err(),
            Some(Error::UnsupportedVersion)
        );

        let garbage = vault.seal_bytes(id, 1, json(r#"{"kind":"host","data":{"name":1}}"#));

        assert_eq!(vault.open(&garbage).err(), Some(Error::Corrupted));

        let mut future = vault.seal(id, 1, &Record::Tombstone);
        future.format_version = FORMAT_VERSION + 1;

        assert_eq!(vault.open(&future).err(), Some(Error::UnsupportedVersion));

        let mut future_header = header;
        future_header.format_version = FORMAT_VERSION + 1;

        assert_eq!(
            Vault::unlock("pw", &future_header).err(),
            Some(Error::UnsupportedVersion)
        );
    }

    // Checks that the password is normalized to NFC before deriving the key.
    #[test]
    fn password_is_unicode_normalized() {
        let (_, header) = Vault::create("caf\u{e9}");

        assert!(Vault::unlock("cafe\u{301}", &header).is_ok());
    }

    // Checks that padding hides the length and that each encryption uses a fresh nonce.
    #[test]
    fn padding_hides_length_and_nonces_are_fresh() {
        let (vault, _) = Vault::create("pw");
        let id = Uuid::new_v4();
        let a = vault.seal(id, 1, &Record::Host(host("a")));
        let b = vault.seal(id, 1, &Record::Host(host("a much longer host name")));

        assert_eq!(a.ciphertext.len(), b.ciphertext.len());
        let a2 = vault.seal(id, 1, &Record::Host(host("a")));

        assert_ne!(a.nonce, a2.nonce);
    }
}
