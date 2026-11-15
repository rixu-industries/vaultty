// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Tunnels: local proxies (SOCKS, HTTP, port forwarding) that go out through a host in the vault. The
//! sidebar list, the detail view of a tunnel and its form. Starting them is the job of `sessions`.

pub mod detail;
pub mod form;
pub mod list;

use crate::app::theme::colors;
use gpui_kit::Hsla;
use sessions::TunnelStatus;
use vaults::{TunnelKind, TunnelView, VaultView};

/// Tunnels sorted by name and filtered by name or port.
pub fn list(view: &VaultView, query: &str) -> Vec<TunnelView> {
    let query = query.to_lowercase();
    let mut tunnels: Vec<TunnelView> = view
        .tunnels
        .iter()
        .filter(|t| {
            query.is_empty()
                || t.name.to_lowercase().contains(&query)
                || t.port.to_string().contains(&query)
        })
        .cloned()
        .collect();

    tunnels.sort_by_key(|t| t.name.to_lowercase());
    tunnels
}

/// Short name of the type, as shown in the list, the detail view and the form.
pub fn kind_name(kind: &TunnelKind) -> &'static str {
    match kind {
        TunnelKind::Socks5 => "SOCKS5",
        TunnelKind::Socks4 => "SOCKS4",
        TunnelKind::Http => "HTTP",
        TunnelKind::Forward { .. } => "Port forward",
    }
}

/// Color of the status dot: off, starting, running or failed.
pub fn status_color(enabled: bool, status: Option<&TunnelStatus>) -> Hsla {
    let c = colors();

    match status {
        _ if !enabled => c.disabled_foreground,
        Some(TunnelStatus::Running) => c.success,
        Some(TunnelStatus::Failed { .. }) => c.destructive,
        Some(TunnelStatus::Starting) | None => c.warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tunnel(name: &str, port: u16) -> TunnelView {
        TunnelView {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: TunnelKind::Socks5,
            bind: "127.0.0.1".into(),
            port,
            host_id: Uuid::new_v4(),
            method_id: Uuid::new_v4(),
            enabled: true,
            broken: false,
        }
    }

    #[test]
    fn list_sorted_and_filtered_by_name_or_port() {
        let view = VaultView {
            tunnels: vec![tunnel("work", 1080), tunnel("Home", 8080)],
            ..VaultView::default()
        };

        let names: Vec<_> = list(&view, "").iter().map(|t| t.name.clone()).collect();
        assert_eq!(names, ["Home", "work"]);
        assert_eq!(list(&view, "WOR").len(), 1);
        assert_eq!(list(&view, "808")[0].name, "Home");
    }
}
