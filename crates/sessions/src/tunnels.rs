// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Running tunnels: each one listens on its port with a proxy (`proxy`) and routes the connections
//! through its host. The proxy knows nothing about transports: this is where it is decided how the
//! host is reached. Today it is always SSH; another protocol (telnet…) would be another `Dialer`,
//! chosen by the access method.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use uuid::Uuid;
use vaults::{TunnelKind, TunnelView};

use crate::shell::connect_ssh;
use crate::{ConnectError, Event, Sessions, Status};

/// State of a running tunnel. A disabled one has no state.
#[derive(Clone, Debug, PartialEq)]
pub enum TunnelStatus {
    Starting,
    Running,
    Failed { error: String },
}

/// What defines a running tunnel: if it changes, the tunnel is started again.
#[derive(Clone, PartialEq)]
struct Spec {
    kind: TunnelKind,
    bind: String,
    port: u16,
    host: Uuid,
    method: Uuid,
}

impl From<&TunnelView> for Spec {
    fn from(t: &TunnelView) -> Self {
        Self {
            kind: t.kind.clone(),
            bind: t.bind.clone(),
            port: t.port,
            host: t.host_id,
            method: t.method_id,
        }
    }
}

pub(crate) struct Running {
    vault: Uuid,
    /// Changes every time it starts: whatever a previous task reports is ignored.
    run: Uuid,
    spec: Spec,
    status: TunnelStatus,
    task: JoinHandle<()>,
}

impl Sessions {
    /// Starts the active tunnels of the vault and stops those that were disabled, deleted or
    /// changed. With the vault locked, it stops them all. Called after every change to the vault.
    pub fn sync_tunnels(self: &Arc<Self>, vault: Uuid) {
        let wanted: HashMap<Uuid, Spec> = self
            .vaults
            .tunnels(vault)
            .unwrap_or_default()
            .iter()
            .filter(|t| t.enabled)
            .map(|t| (t.id, Spec::from(t)))
            .collect();

        let mut tunnels = self.tunnels.lock().unwrap();
        let mut changed = false;

        tunnels.retain(|id, t| {
            let keep = t.vault != vault || wanted.get(id) == Some(&t.spec);

            if !keep {
                t.task.abort();
                changed = true;
            }

            keep
        });

        for (id, spec) in wanted {
            if tunnels.contains_key(&id) {
                continue;
            }

            let run = Uuid::new_v4();
            let task = self
                .runtime
                .spawn(run_tunnel(self.clone(), id, run, vault, spec.clone()));

            tunnels.insert(
                id,
                Running {
                    vault,
                    run,
                    spec,
                    status: TunnelStatus::Starting,
                    task,
                },
            );
            changed = true;
        }

        drop(tunnels);

        if changed {
            self.emit(Event::Tunnels);
        }
    }

    /// Stops all tunnels of the vault (when it is locked or removed).
    pub(crate) fn stop_tunnels(&self, vault: Uuid) {
        let mut tunnels = self.tunnels.lock().unwrap();
        let before = tunnels.len();

        tunnels.retain(|_, t| {
            if t.vault == vault {
                t.task.abort();
            }

            t.vault != vault
        });

        if tunnels.len() != before {
            drop(tunnels);
            self.emit(Event::Tunnels);
        }
    }

    /// `None` if the tunnel is not running (disabled or with the vault locked).
    pub fn tunnel_status(&self, id: Uuid) -> Option<TunnelStatus> {
        self.tunnels
            .lock()
            .unwrap()
            .get(&id)
            .map(|t| t.status.clone())
    }

    fn set_tunnel_status(&self, id: Uuid, run: Uuid, status: TunnelStatus) {
        let mut tunnels = self.tunnels.lock().unwrap();

        match tunnels.get_mut(&id) {
            Some(t) if t.run == run && t.status != status => t.status = status,
            _ => return,
        }

        drop(tunnels);
        self.emit(Event::Tunnels);
    }
}

async fn run_tunnel(sessions: Arc<Sessions>, id: Uuid, run: Uuid, vault: Uuid, spec: Spec) {
    let fail = |error: String| sessions.set_tunnel_status(id, run, TunnelStatus::Failed { error });

    let listener = match TcpListener::bind((spec.bind.as_str(), spec.port)).await {
        Ok(listener) => listener,
        Err(e) => {
            return fail(format!(
                "Could not listen on {}:{}: {e}",
                spec.bind, spec.port
            ));
        }
    };

    let dialer = Arc::new(SshDialer {
        sessions: sessions.clone(),
        id,
        run,
        vault,
        host: spec.host,
        method: spec.method,
        session: tokio::sync::Mutex::new(None),
    });

    // Connects right away, without waiting for the first client: an error (credentials, fingerprint)
    // shows up immediately. If it fails (the app started before the network was up), the tunnel keeps
    // listening and each new connection tries again.
    let _ = dialer.session().await;

    let protocol = match spec.kind {
        TunnelKind::Socks5 => proxy::Protocol::Socks5,
        TunnelKind::Socks4 => proxy::Protocol::Socks4,
        TunnelKind::Http => proxy::Protocol::Http,
        TunnelKind::Forward { host, port } => proxy::Protocol::Forward { host, port },
    };

    proxy::serve(listener, protocol, dialer).await;
}

/// Routes the proxy's connections over SSH (`direct-tcpip`). All of them share one session; if it drops,
/// it is reopened with the next connection.
struct SshDialer {
    sessions: Arc<Sessions>,
    id: Uuid,
    run: Uuid,
    vault: Uuid,
    host: Uuid,
    method: Uuid,
    session: tokio::sync::Mutex<Option<Arc<ssh::Session>>>,
}

impl SshDialer {
    async fn session(&self) -> io::Result<Arc<ssh::Session>> {
        let mut slot = self.session.lock().await;

        if let Some(session) = slot.as_ref().filter(|s| !s.is_closed()) {
            return Ok(session.clone());
        }

        slot.take();

        match connect_ssh(&self.sessions, self.vault, self.host, self.method).await {
            Ok(session) => {
                let session = Arc::new(session);
                *slot = Some(session.clone());
                self.status(TunnelStatus::Running);
                Ok(session)
            }
            Err(e) => {
                let error = describe(e);
                self.status(TunnelStatus::Failed {
                    error: error.clone(),
                });
                Err(io::Error::other(error))
            }
        }
    }

    fn status(&self, status: TunnelStatus) {
        self.sessions.set_tunnel_status(self.id, self.run, status);
    }
}

impl proxy::Dialer for SshDialer {
    type Stream = ssh::TcpStream;

    async fn dial(&self, host: &str, port: u16) -> io::Result<ssh::TcpStream> {
        let session = self.session().await?;

        match session.direct_tcpip(host, port).await {
            Ok(stream) => Ok(stream),
            // The session dropped just now: it is reopened once.
            Err(_) if session.is_closed() => self
                .session()
                .await?
                .direct_tcpip(host, port)
                .await
                .map_err(io::Error::other),
            Err(e) => Err(io::Error::other(e)),
        }
    }
}

fn describe(e: ConnectError) -> String {
    match Status::from(e) {
        Status::UnknownHostKey { .. } => {
            "The server fingerprint has not been accepted yet: open a terminal to this host once."
                .into()
        }
        Status::Failed { error } => error,
        other => format!("{other:?}"),
    }
}
