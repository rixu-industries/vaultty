// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Open tabs (terminal and SFTP), tunnels and their connections. They do not depend on the UI: the
//! connections run in tokio tasks and every change is reported with an `Event`, so the UI
//! (desktop or mobile) repaints whatever changed.
//!
//! Credentials leave `vaults` only to open the SSH session and are dropped as soon as it is open.

mod files;
mod queue;
mod remote;
mod shell;
mod tunnels;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sftp::Side;
use terminal::Emulator;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use uuid::Uuid;
use vaults::{AccessKind, Vaults};

pub use queue::Queue;
pub use ssh::Output;
pub use tunnels::TunnelStatus;

use crate::remote::SftpSlot;
use crate::shell::Input;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabKind {
    Terminal,
    Sftp,
}

/// The state of a tab's connection.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Connecting,
    Connected,
    /// First connection: the server's fingerprint must be confirmed.
    UnknownHostKey {
        fingerprint: String,
    },
    Failed {
        error: String,
    },
    Closed,
}

/// Why the connection failed. Typed, so each UI can report it its own way (the
/// desktop app and the MCP server each word it differently).
#[derive(Debug)]
pub enum ConnectError {
    /// The host or access method no longer exists (or the vault was locked).
    Gone,
    /// The method uses a key that is no longer in the keychain.
    KeyGone,
    Ssh(ssh::Error),
}

impl From<ConnectError> for Status {
    fn from(e: ConnectError) -> Self {
        let error = match e {
            ConnectError::Ssh(ssh::Error::UnknownHostKey(fingerprint)) => {
                return Status::UnknownHostKey { fingerprint };
            }
            ConnectError::Gone => "The host or access method no longer exists.".into(),
            ConnectError::KeyGone => "The key of this method no longer exists.".into(),
            ConnectError::Ssh(e) => e.to_string(),
        };

        Status::Failed { error }
    }
}

/// What the UI knows about a tab.
#[derive(Clone, Debug, PartialEq)]
pub struct TabInfo {
    pub id: Uuid,
    pub vault: Uuid,
    pub host: Uuid,
    pub method: Uuid,
    pub kind: TabKind,
    pub status: Status,
}

/// What changes and the UI must repaint.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The tab list or the state of one of the tabs.
    Tabs,
    /// Output arrived for the tab's terminal.
    Screen(Uuid),
    /// The tab's transfer queue changed.
    Queue(Uuid),
    /// A transfer copying into the `side` panel finished: that panel must be listed again.
    Reload { tab: Uuid, side: Side },
    /// The remote program asks to copy this text to the clipboard.
    Clipboard(String),
    /// A tunnel was started, stopped or changed state.
    Tunnels,
    /// Something new was saved for a vault host (its operating system): the view must be reloaded.
    VaultChanged(Uuid),
}

/// How the UI is notified. Called from any thread.
pub type Notify = Arc<dyn Fn(Event) + Send + Sync>;

/// What links the tab to its connection: the input of a shell or the client of an SFTP.
enum Link {
    Shell(UnboundedSender<Input>),
    Sftp(Arc<SftpSlot>),
}

/// A tab's current connection. Reconnecting replaces it with one with a different `id`, and whatever
/// the previous one reports is ignored.
struct Conn {
    id: Uuid,
    link: Link,
    task: JoinHandle<()>,
}

impl Conn {
    /// Stops the connection and, for an SFTP, also the transfer in progress.
    fn stop(&self) {
        self.task.abort();

        if let Link::Sftp(slot) = &self.link {
            slot.stop();
        }
    }
}

/// What lives as long as the tab, across connections.
enum Extra {
    /// The terminal's screen and history.
    Terminal(Arc<Mutex<Emulator>>),
    /// Transfer queue and last opened folder in each panel (local, remote).
    Sftp {
        queue: Arc<Queue>,
        folders: [Option<String>; 2],
    },
}

struct Tab {
    info: TabInfo,
    conn: Option<Conn>,
    extra: Extra,
}

impl Tab {
    fn stop(&self) {
        if let Some(conn) = &self.conn {
            conn.stop();
        }

        if let Extra::Sftp { queue, .. } = &self.extra {
            queue.close();
        }
    }
}

/// Which connection of which tab a task works on, so it can report its status.
#[derive(Clone)]
struct Ctx {
    sessions: Arc<Sessions>,
    tab: Uuid,
    conn: Uuid,
    vault: Uuid,
    host: Uuid,
    method: Uuid,
}

impl Ctx {
    fn status(&self, status: Status) {
        self.sessions.set_status(self.tab, self.conn, status);
    }
}

/// All the app's tabs.
pub struct Sessions {
    tabs: Mutex<Vec<Tab>>,
    tunnels: Mutex<HashMap<Uuid, tunnels::Running>>,
    vaults: Arc<Vaults>,
    runtime: Handle,
    notify: Notify,
}

impl Sessions {
    /// `runtime` is where the connections run; `notify` is how the UI is notified.
    pub fn new(vaults: Arc<Vaults>, runtime: Handle, notify: Notify) -> Arc<Self> {
        Arc::new(Self {
            tabs: Mutex::default(),
            tunnels: Mutex::default(),
            vaults,
            runtime,
            notify,
        })
    }

    pub fn vaults(&self) -> &Arc<Vaults> {
        &self.vaults
    }

    /// Launches a job on the connections' runtime. What it returns can be awaited from
    /// any executor (including the UI's).
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }

    fn emit(&self, event: Event) {
        (self.notify)(event);
    }

    /// The open sessions, in the order they were opened (the UI decides the tab order).
    pub fn list(&self) -> Vec<TabInfo> {
        self.tabs
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.info.clone())
            .collect()
    }

    pub fn info(&self, id: Uuid) -> Option<TabInfo> {
        self.tabs
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.info.id == id)
            .map(|t| t.info.clone())
    }

    /// Opens a tab at the end of the list and starts connecting. Returns its id.
    pub fn open(self: &Arc<Self>, vault: Uuid, host: Uuid, method: Uuid, kind: TabKind) -> Uuid {
        let id = Uuid::new_v4();

        let extra = match kind {
            TabKind::Terminal => Extra::Terminal(Arc::new(Mutex::new(Emulator::new(80, 24)))),
            TabKind::Sftp => Extra::Sftp {
                queue: Arc::new(Queue::new(id)),
                folders: [None, None],
            },
        };

        let info = TabInfo {
            id,
            vault,
            host,
            method,
            kind,
            status: Status::Connecting,
        };

        let mut tabs = self.tabs.lock().unwrap();
        tabs.push(Tab {
            info,
            conn: None,
            extra,
        });
        self.start(&mut tabs, id);
        id
    }

    /// Reconnects a tab, stopping the previous connection if it is still alive.
    pub fn reconnect(self: &Arc<Self>, id: Uuid) {
        let mut tabs = self.tabs.lock().unwrap();
        self.start(&mut tabs, id);
    }

    fn start(self: &Arc<Self>, tabs: &mut [Tab], id: Uuid) {
        let Some(tab) = tabs.iter_mut().find(|t| t.info.id == id) else {
            return;
        };

        if let Some(old) = tab.conn.take() {
            old.stop();
        }

        let ctx = Ctx {
            sessions: self.clone(),
            tab: id,
            conn: Uuid::new_v4(),
            vault: tab.info.vault,
            host: tab.info.host,
            method: tab.info.method,
        };

        let conn = ctx.conn;

        // The task cannot report anything until the lock is released, by which time the connection is
        // already stored.
        let (link, task) = match &tab.extra {
            Extra::Terminal(screen) => shell::start(ctx, screen.clone()),
            Extra::Sftp { queue, .. } => {
                // Whatever was waiting in the queue will no longer be copied with this connection.
                queue.fail_waiting(self);
                remote::start(ctx)
            }
        };

        tab.conn = Some(Conn {
            id: conn,
            link,
            task,
        });
        tab.info.status = Status::Connecting;
        self.emit(Event::Tabs);
    }

    /// Stores the state of connection `conn` of tab `id`, if it is still the current one.
    fn set_status(&self, id: Uuid, conn: Uuid, status: Status) {
        let mut tabs = self.tabs.lock().unwrap();

        let Some(tab) = tabs.iter_mut().find(|t| t.info.id == id) else {
            return;
        };

        if tab.conn.as_ref().map(|c| c.id) != Some(conn) {
            return;
        }

        if let Extra::Sftp { queue, .. } = &tab.extra
            && status != Status::Connected
        {
            queue.fail_waiting(self);
        }

        tab.info.status = status;
        drop(tabs);
        self.emit(Event::Tabs);
    }

    /// Closes the tabs matching `remove` and stops their connections.
    fn close_where(&self, remove: impl Fn(&TabInfo) -> bool) {
        let mut tabs = self.tabs.lock().unwrap();
        let before = tabs.len();

        tabs.retain(|t| {
            if remove(&t.info) {
                t.stop();
            }

            !remove(&t.info)
        });

        if tabs.len() != before {
            drop(tabs);
            self.emit(Event::Tabs);
        }
    }

    pub fn close(&self, id: Uuid) {
        self.close_where(|t| t.id == id);
    }

    /// When locking, removing or moving a vault. Also stops its tunnels.
    pub fn close_vault(&self, vault: Uuid) {
        self.close_where(|t| t.vault == vault);
        self.stop_tunnels(vault);
    }

    /// When deleting a host.
    pub fn close_host(&self, host: Uuid) {
        self.close_where(|t| t.host == host);
    }

    /// Tells the UI that the vault changed from elsewhere (the MCP server, for example).
    pub fn vault_changed(&self, vault: Uuid) {
        self.emit(Event::VaultChanged(vault));
    }

    /// Runs a standalone command on a host, without a tab: connects, runs it and closes. The output
    /// keeps accumulating in `out`. If `stop` finishes first, it is stopped (and the remote process is
    /// killed); returns `true` if the command finished on its own. The server's fingerprint must already
    /// be accepted: accepting it is up to the user, from the app.
    pub async fn run_command(
        &self,
        vault: Uuid,
        host: Uuid,
        method: Uuid,
        command: &str,
        out: &Mutex<ssh::Output>,
        stop: impl Future<Output = ()>,
    ) -> Result<bool, ConnectError> {
        tokio::pin!(stop);

        let connect = shell::connect_ssh(self, vault, host, method);

        let session = tokio::select! {
            session = connect => session?,
            () = &mut stop => return Ok(false),
        };

        let finished = session
            .exec(command, out, &mut stop)
            .await
            .map_err(ConnectError::Ssh);
        let _ = session.close().await;

        finished
    }

    /// The user accepts the server's fingerprint: it is saved to the host and the tab reconnects.
    pub fn trust_host_key(self: &Arc<Self>, id: Uuid, fingerprint: String) -> Result<(), String> {
        let info = self.info(id).ok_or("The tab no longer exists.")?;

        self.vaults
            .trust_host_key(info.vault, info.host, fingerprint)?;
        self.emit(Event::VaultChanged(info.vault));
        self.reconnect(id);
        Ok(())
    }

    /// The tab's terminal, to paint it or select text.
    pub fn terminal(&self, id: Uuid) -> Option<Arc<Mutex<Emulator>>> {
        match &self
            .tabs
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.info.id == id)?
            .extra
        {
            Extra::Terminal(screen) => Some(screen.clone()),
            Extra::Sftp { .. } => None,
        }
    }

    /// Passes what is typed to the tab's shell, if it is connected.
    pub fn write(&self, id: Uuid, bytes: Vec<u8>) {
        self.send(id, Input::Data(bytes));
    }

    /// `sudo` is asking for the password of the method's user, and the method is a password method.
    pub fn sudo_prompt(&self, id: Uuid) -> bool {
        self.sudo_password(id).is_some()
    }

    /// Types the method's password and Enter. The prompt is checked again: between the click and
    /// now the screen may have changed.
    pub fn fill_sudo(&self, id: Uuid) {
        if let Some(password) = self.sudo_password(id) {
            let mut bytes = password.into_bytes();
            bytes.push(b'\r');
            self.write(id, bytes);
        }
    }

    fn sudo_password(&self, id: Uuid) -> Option<String> {
        let user = self.terminal(id)?.lock().unwrap().sudo_prompt()?;
        let info = self.info(id)?;
        let (host, _) = self.vaults.target(info.vault, info.host, info.method)?;
        let method = host.access_methods.iter().find(|m| m.id == info.method)?;

        match &method.kind {
            AccessKind::Password { username, password } if *username == user => {
                Some(password.clone())
            }
            _ => None,
        }
    }

    /// The size is stored even without a connection: the next shell opens with it already applied.
    pub fn resize(&self, id: Uuid, columns: usize, lines: usize) {
        let Some(screen) = self.terminal(id) else {
            return;
        };

        if screen.lock().unwrap().resize(columns, lines) {
            self.send(
                id,
                Input::Resize {
                    cols: columns as u32,
                    rows: lines as u32,
                },
            );
            self.emit(Event::Screen(id));
        }
    }

    fn send(&self, id: Uuid, input: Input) {
        if let Some(Tab {
            conn:
                Some(Conn {
                    link: Link::Shell(sender),
                    ..
                }),
            ..
        }) = self.tabs.lock().unwrap().iter().find(|t| t.info.id == id)
        {
            // If the session has already ended, the input is simply lost.
            let _ = sender.send(input);
        }
    }

    fn slot(&self, id: Uuid) -> Option<Arc<SftpSlot>> {
        match &self
            .tabs
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.info.id == id)?
            .conn
        {
            Some(Conn {
                link: Link::Sftp(slot),
                ..
            }) => Some(slot.clone()),
            _ => None,
        }
    }

    /// Stops the tab's transfer in progress, if any.
    fn abort_transfer(&self, id: Uuid) {
        if let Some(slot) = self.slot(id)
            && let Some(transfer) = slot.transfer.lock().unwrap().take()
        {
            transfer.abort();
        }
    }

    /// The SFTP tab's transfer queue.
    pub fn queue(&self, id: Uuid) -> Result<Arc<Queue>, String> {
        match self
            .tabs
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.info.id == id)
            .map(|t| &t.extra)
        {
            Some(Extra::Sftp { queue, .. }) => Ok(queue.clone()),
            _ => Err("The tab no longer exists".into()),
        }
    }

    /// Last folder opened in a panel of SFTP tab `id`.
    fn folder(&self, id: Uuid, side: Side) -> Option<String> {
        match &self
            .tabs
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.info.id == id)?
            .extra
        {
            Extra::Sftp { folders, .. } => folders[side.index()].clone(),
            Extra::Terminal(_) => None,
        }
    }

    fn set_folder(&self, id: Uuid, side: Side, path: String) {
        if let Some(Tab {
            extra: Extra::Sftp { folders, .. },
            ..
        }) = self
            .tabs
            .lock()
            .unwrap()
            .iter_mut()
            .find(|t| t.info.id == id)
        {
            folders[side.index()] = Some(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A tab for a locked vault fails without connecting, reports it, and writes it to its terminal.
    #[test]
    fn tab_without_open_vault_fails_and_notifies() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let notify: Notify = Arc::new(move |e| seen.lock().unwrap().push(e));
        let sessions = Sessions::new(Arc::new(Vaults::load()), runtime.handle().clone(), notify);

        let id = sessions.open(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            TabKind::Terminal,
        );
        assert_eq!(sessions.info(id).unwrap().status, Status::Connecting);

        runtime.block_on(async {
            while matches!(sessions.info(id).unwrap().status, Status::Connecting) {
                tokio::task::yield_now().await;
            }
        });

        assert!(matches!(
            sessions.info(id).unwrap().status,
            Status::Failed { .. }
        ));
        assert!(events.lock().unwrap().contains(&Event::Screen(id)));

        let screen = sessions.terminal(id).unwrap();
        let palette = terminal::Palette {
            background: terminal::Rgb8::hex(0),
            foreground: terminal::Rgb8::hex(0xffffff),
            cursor: terminal::Rgb8::hex(0xffffff),
            selection: terminal::Rgb8::hex(0x444444),
            ansi: [terminal::Rgb8::hex(0); 16],
        };
        let text: String = screen.lock().unwrap().screen(&palette).lines[1]
            .iter()
            .map(|r| r.text.as_str())
            .collect();
        assert!(text.contains("no longer exists"), "{text}");

        sessions.close(id);
        assert!(sessions.list().is_empty());
    }
}
