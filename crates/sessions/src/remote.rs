// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! SFTP connections of the tabs. The connection runs in a task that leaves the client in the
//! connection's `SftpSlot`; the file operations and the queue take it from there.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sftp::Remote;
use tokio::task::{AbortHandle, JoinHandle};

use crate::shell::connect_ssh;
use crate::{Ctx, Link, Status};

const CLOSED_CHECK: Duration = Duration::from_secs(2);

/// An open connection and the tab to notify if it is lost.
pub(crate) struct Open {
    pub remote: Arc<Remote>,
    ctx: Ctx,
}

/// State shared between the connection's task and the tab's operations.
#[derive(Default)]
pub(crate) struct SftpSlot {
    /// `None` while connecting or once the connection has been lost.
    pub open: Mutex<Option<Arc<Open>>>,
    /// The copy in progress, so it can be cancelled.
    pub transfer: Mutex<Option<AbortHandle>>,
}

impl SftpSlot {
    /// Cancels the copy in progress and releases the connection.
    pub fn stop(&self) {
        if let Some(transfer) = self.transfer.lock().unwrap().take() {
            transfer.abort();
        }

        self.open.lock().unwrap().take();
    }
}

/// After a failure: if it was because the connection dropped, notifies the tab and releases the client.
pub(crate) fn lost(slot: &SftpSlot, open: &Open) {
    if open.remote.ssh.is_closed() {
        slot.open.lock().unwrap().take();
        open.ctx.status(Status::Closed);
    }
}

/// Starts a tab's SFTP connection. Returns its slot and the task, to abort it.
pub(crate) fn start(ctx: Ctx) -> (Link, JoinHandle<()>) {
    let slot = Arc::new(SftpSlot::default());
    let runtime = ctx.sessions.runtime.clone();
    let task = runtime.spawn(run(ctx, slot.clone()));

    (Link::Sftp(slot), task)
}

async fn run(ctx: Ctx, slot: Arc<SftpSlot>) {
    let ssh = match connect_ssh(&ctx.sessions, ctx.vault, ctx.host, ctx.method).await {
        Ok(ssh) => ssh,
        Err(e) => return ctx.status(e.into()),
    };

    let sftp = match ssh.sftp().await {
        Ok(sftp) => sftp,
        Err(e) => {
            let _ = ssh.close().await;
            return ctx.status(Status::Failed {
                error: e.to_string(),
            });
        }
    };

    let open = Arc::new(Open {
        remote: Arc::new(Remote { ssh, sftp }),
        ctx: ctx.clone(),
    });

    *slot.open.lock().unwrap() = Some(open.clone());
    ctx.status(Status::Connected);

    // The connection lives in the slot. The task watches whether it drops (the server closes it or
    // the keepalive considers it lost) so it can notify even when the tab is not doing anything.
    while !open.remote.ssh.is_closed() {
        tokio::time::sleep(CLOSED_CHECK).await;
    }

    lost(&slot, &open);
}
