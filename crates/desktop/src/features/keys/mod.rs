// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Keychain: the sidebar list, a key's detail page and its forms. The pure selectors over the vault view live here.

pub mod detail;
pub mod generate;
pub mod key_form;
pub mod list;

use uuid::Uuid;
use vaults::{HostView, KeyView, VaultView};

/// Keys sorted by name and filtered by `query`.
pub fn list(view: &VaultView, query: &str) -> Vec<KeyView> {
    let query = query.to_lowercase();
    let mut keys: Vec<KeyView> = view
        .keys
        .iter()
        .filter(|k| query.is_empty() || k.name.to_lowercase().contains(&query))
        .cloned()
        .collect();

    keys.sort_by_key(|k| k.name.to_lowercase());
    keys
}

/// Hosts that use the key in some access method.
pub fn users(view: &VaultView, key: Uuid) -> Vec<HostView> {
    view.hosts
        .iter()
        .filter(|h| h.methods.iter().any(|m| m.key_id == Some(key)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vaults::MethodView;

    fn key(name: &str) -> KeyView {
        KeyView {
            id: Uuid::new_v4(),
            name: name.into(),
            has_passphrase: false,
            algorithm: "ssh-ed25519".into(),
            fingerprint: String::new(),
            public: None,
        }
    }

    #[test]
    fn list_sorted_filtered_and_users() {
        let (a, b) = (key("beta"), key("Alpha"));
        let used = a.id;
        let method = MethodView {
            id: Uuid::new_v4(),
            password: false,
            username: "root".into(),
            label: String::new(),
            key_id: Some(used),
            title: "root".into(),
            detail: String::new(),
            broken: false,
        };
        let host = HostView {
            id: Uuid::new_v4(),
            name: "web".into(),
            address: "x".into(),
            port: 22,
            host_key: None,
            group_id: None,
            notes: String::new(),
            os: None,
            mcp: None,
            methods: vec![method],
        };
        let view = VaultView {
            hosts: vec![host],
            groups: vec![],
            keys: vec![a, b],
            tunnels: vec![],
            warnings: vec![],
        };

        let names: Vec<_> = list(&view, "").iter().map(|k| k.name.clone()).collect();
        assert_eq!(names, ["Alpha", "beta"]);
        assert_eq!(list(&view, "ALP").len(), 1);
        assert_eq!(users(&view, used).len(), 1);
        assert!(users(&view, Uuid::new_v4()).is_empty());
    }
}
