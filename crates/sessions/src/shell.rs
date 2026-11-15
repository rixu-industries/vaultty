// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Shell of the terminal tabs. The session runs in its own task: what the server sends goes into
//! the tab's `Emulator` (which lasts from one connection to the next), and the interface is
//! notified so it can paint it.

use std::sync::{Arc, Mutex};

use ssh::ChannelMsg;
use terminal::Emulator;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;
use uuid::Uuid;
use vaults::AccessKind;

use crate::{ConnectError, Ctx, Event, Link, Sessions, Status};

/// Input typed in the terminal that must be passed to the server.
pub(crate) enum Input {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

/// Starts the shell of a tab. Returns how to pass it input, and the task to abort it.
pub(crate) fn start(ctx: Ctx, screen: Arc<Mutex<Emulator>>) -> (Link, JoinHandle<()>) {
    let (input, rx) = unbounded_channel();
    let runtime = ctx.sessions.runtime.clone();
    let task = runtime.spawn(run(ctx, screen, rx));

    (Link::Shell(input), task)
}

async fn run(ctx: Ctx, screen: Arc<Mutex<Emulator>>, mut input: UnboundedReceiver<Input>) {
    let status = match connect_ssh(&ctx.sessions, ctx.vault, ctx.host, ctx.method).await {
        Ok(session) => {
            let status = pipe(&ctx, &session, &screen, &mut input).await;
            let _ = session.close().await;
            status
        }
        Err(e) => e.into(),
    };

    let notice = match &status {
        Status::Failed { error } => format!("\r\n\x1b[31m{error}\x1b[0m\r\n"),
        Status::Closed => "\r\n\x1b[90mSession closed.\x1b[0m\r\n".into(),
        _ => String::new(),
    };

    if !notice.is_empty() {
        screen.lock().unwrap().feed(notice.as_bytes());
        ctx.sessions.emit(Event::Screen(ctx.tab));
    }

    ctx.status(status);
}

/// Takes the credentials from the vault, connects and releases them as soon as the session is open.
pub(crate) async fn connect_ssh(
    sessions: &Sessions,
    vault: Uuid,
    host: Uuid,
    method: Uuid,
) -> Result<ssh::Session, ConnectError> {
    let gone = || ConnectError::Gone;

    let host_id = host;
    let (host, key) = sessions
        .vaults
        .target(vault, host, method)
        .ok_or_else(gone)?;

    let method = host
        .access_methods
        .iter()
        .find(|m| m.id == method)
        .ok_or_else(gone)?;

    let (user, auth) = match &method.kind {
        AccessKind::Password { username, password } => (username, ssh::Auth::Password(password)),
        AccessKind::Key { username, .. } => {
            let key = key.as_ref().ok_or(ConnectError::KeyGone)?;

            let auth = ssh::Auth::Key {
                private_key: &key.private_key,
                passphrase: key.passphrase.as_deref(),
            };

            (username, auth)
        }
    };
    let session = ssh::connect(
        &host.address,
        host.port,
        user,
        auth,
        host.host_key.as_deref(),
    )
    .await
    .map_err(ConnectError::Ssh)?;
    // Only the first time: it is saved for the icon and not asked for again.
    if host.os.is_none()
        && let Some(os) = session.os().await
    {
        // If it fails (the vault was locked in the meantime), the generic icon stays.
        if sessions
            .vaults
            .edit_host(vault, host_id, |h| h.os = Some(os))
            .is_ok()
        {
            sessions.emit(Event::VaultChanged(vault));
        }
    }
    Ok(session)
}

/// Forwards what is typed to the server and what arrives from it to the screen, until it closes.
async fn pipe(
    ctx: &Ctx,
    session: &ssh::Session,
    screen: &Mutex<Emulator>,
    input: &mut UnboundedReceiver<Input>,
) -> Status {
    let (cols, rows) = {
        let screen = screen.lock().unwrap();
        (screen.columns() as u32, screen.lines() as u32)
    };

    let mut channel = match session.shell(cols, rows).await {
        Ok(channel) => channel,
        Err(e) => {
            return Status::Failed {
                error: e.to_string(),
            };
        }
    };
    ctx.status(Status::Connected);

    loop {
        tokio::select! {
            typed = input.recv() => {
                let sent = match typed {
                    Some(Input::Data(data)) => channel.data(data.as_slice()).await,
                    Some(Input::Resize { cols, rows }) => channel.window_change(cols, rows, 0, 0).await,
                    None => return Status::Closed,
                };

                if let Err(e) = sent {
                    return Status::Failed { error: e.to_string() };
                }
            }
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    let out = screen.lock().unwrap().feed(&data);

                    // Whatever the program asks the terminal (cursor position…) is answered right away.
                    if !out.reply.is_empty() && channel.data(out.reply.as_slice()).await.is_err() {
                        return Status::Closed;
                    }

                    if let Some(text) = out.clipboard {
                        ctx.sessions.emit(Event::Clipboard(text));
                    }

                    ctx.sessions.emit(Event::Screen(ctx.tab));
                }
                Some(ChannelMsg::Close) | None => return Status::Closed,
                _ => {}
            },
        }
    }
}
