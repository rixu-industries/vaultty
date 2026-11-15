// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Hosts: the sidebar tree, a host's detail page, its forms and the access method picker. The pure selectors over the vault view live here.

pub mod detail;
pub mod group_form;
pub mod host_form;
pub mod known;
pub mod method_form;
pub mod method_picker;
pub mod tree;

use uuid::Uuid;
use vaults::{HostInput, HostView, VaultView};

/// A group in the sidebar tree. `id: None` is "No group".
#[derive(Clone, Debug, PartialEq)]
pub struct TreeGroup {
    pub id: Option<Uuid>,
    pub name: String,
    pub hosts: Vec<HostView>,
}

fn by_name(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// Hosts with an accepted host key, sorted by name.
fn trusted_hosts(view: &VaultView) -> Vec<(&HostView, &str)> {
    let mut hosts: Vec<_> = view
        .hosts
        .iter()
        .filter_map(|host| host.host_key.as_deref().map(|key| (host, key)))
        .collect();

    hosts.sort_by(|(a, _), (b, _)| by_name(&a.name, &b.name));
    hosts
}

/// Groups sorted by name with their hosts, filtered by `query` (name or address). Empty
/// groups are shown (just created), except "No group" and except while searching. While
/// dragging, all targets are shown, including the empty ones.
pub fn tree(view: &VaultView, query: &str, dragging: bool) -> Vec<TreeGroup> {
    let search = query.to_lowercase();
    let mut groups: Vec<TreeGroup> = view
        .groups
        .iter()
        .map(|g| TreeGroup {
            id: Some(g.id),
            name: g.name.clone(),
            hosts: Vec::new(),
        })
        .collect();

    groups.sort_by(|a, b| by_name(&a.name, &b.name));
    groups.push(TreeGroup {
        id: None,
        name: "No group".into(),
        hosts: Vec::new(),
    });

    for host in &view.hosts {
        let matches = search.is_empty()
            || host.name.to_lowercase().contains(&search)
            || host.address.to_lowercase().contains(&search);

        // A group that no longer exists counts as "No group".
        let group = host
            .group_id
            .filter(|g| view.groups.iter().any(|x| x.id == *g));

        if matches && let Some(target) = groups.iter_mut().find(|g| g.id == group) {
            target.hosts.push(host.clone());
        }
    }

    for group in &mut groups {
        group.hosts.sort_by(|a, b| by_name(&a.name, &b.name));
    }

    groups.retain(|g| dragging || !g.hosts.is_empty() || (g.id.is_some() && search.is_empty()));
    groups
}

/// The host's current data for moving it to `group`, if the source and target still exist.
pub fn host_move(view: &VaultView, id: Uuid, group: Option<Uuid>) -> Option<HostInput> {
    let host = view.hosts.iter().find(|h| h.id == id)?;

    if host.group_id == group || group.is_some_and(|g| !view.groups.iter().any(|x| x.id == g)) {
        return None;
    }

    Some(HostInput {
        name: host.name.clone(),
        address: host.address.clone(),
        port: host.port,
        group_id: group,
        notes: host.notes.clone(),
    })
}

/// The name of a host's group, or "No group".
pub fn group_name(view: &VaultView, host: &HostView) -> String {
    view.groups
        .iter()
        .find(|g| Some(g.id) == host.group_id)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "No group".into())
}

/// Options for choosing a group, starting with "No group".
pub fn group_options(view: &VaultView) -> Vec<(Option<Uuid>, String)> {
    let mut options: Vec<(Option<Uuid>, String)> = view
        .groups
        .iter()
        .map(|g| (Some(g.id), g.name.clone()))
        .collect();

    options.sort_by(|a, b| a.1.cmp(&b.1));
    options.insert(0, (None, "No group".into()));
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use vaults::GroupView;

    fn host(name: &str, address: &str, group: Option<Uuid>) -> HostView {
        HostView {
            id: Uuid::new_v4(),
            name: name.into(),
            address: address.into(),
            port: 22,
            host_key: None,
            group_id: group,
            notes: String::new(),
            os: None,
            mcp: None,
            methods: vec![],
        }
    }

    fn view() -> (VaultView, Uuid, Uuid) {
        let (prod, empty) = (Uuid::new_v4(), Uuid::new_v4());

        let view = VaultView {
            hosts: vec![
                host("web", "10.0.0.1", Some(prod)),
                host("API", "10.0.0.2", Some(prod)),
                host("loose", "192.168.1.5", None),
                // Its group no longer exists: it counts as "No group".
                host("orphan", "10.9.9.9", Some(Uuid::new_v4())),
            ],
            tunnels: vec![],
            groups: vec![
                GroupView {
                    id: prod,
                    name: "prod".into(),
                },
                GroupView {
                    id: empty,
                    name: "Empty".into(),
                },
            ],
            keys: vec![],
            warnings: vec![],
        };

        (view, prod, empty)
    }

    #[test]
    fn hosts_with_accepted_key_sorted() {
        let (mut view, _, _) = view();
        assert!(trusted_hosts(&view).is_empty());

        view.hosts[0].host_key = Some("SHA256:web".into());
        view.hosts[1].host_key = Some("SHA256:api".into());

        let hosts = trusted_hosts(&view);
        assert_eq!(hosts.len(), 2);
        assert_eq!(
            (hosts[0].0.name.as_str(), hosts[0].1),
            ("API", "SHA256:api")
        );
        assert_eq!(
            (hosts[1].0.name.as_str(), hosts[1].1),
            ("web", "SHA256:web")
        );
    }

    fn names(groups: &[TreeGroup]) -> Vec<(String, Vec<String>)> {
        groups
            .iter()
            .map(|g| {
                (
                    g.name.clone(),
                    g.hosts.iter().map(|h| h.name.clone()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn tree_sorted_by_group() {
        let (view, _, _) = view();

        assert_eq!(
            names(&tree(&view, "", false)),
            [
                ("Empty".to_string(), vec![]),
                (
                    "prod".to_string(),
                    vec!["API".to_string(), "web".to_string()]
                ),
                (
                    "No group".to_string(),
                    vec!["loose".to_string(), "orphan".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn search_filters_by_name_or_address_and_hides_empty() {
        let (view, _, _) = view();

        assert_eq!(
            names(&tree(&view, "192.168", false)),
            [("No group".to_string(), vec!["loose".to_string()])]
        );
        assert_eq!(names(&tree(&view, "WEB", false)).len(), 1);
        // While dragging, all targets are visible.
        assert_eq!(tree(&view, "WEB", true).len(), 3);
    }

    #[test]
    fn move_only_if_changed_and_target_exists() {
        let (view, prod, empty) = view();
        let web = view.hosts[0].id;

        assert!(host_move(&view, web, Some(prod)).is_none());
        assert!(host_move(&view, web, Some(Uuid::new_v4())).is_none());
        assert_eq!(
            host_move(&view, web, Some(empty)).unwrap().group_id,
            Some(empty)
        );
        assert_eq!(host_move(&view, web, None).unwrap().name, "web");
    }
}
