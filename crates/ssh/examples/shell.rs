// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Manual test: opens a remote shell and connects it to the console, line by line.
//!
//! cargo run -p ssh --example shell -- <host> <port> <user>
//! The password is read from the SSH_PASSWORD variable so it does not stay in the shell history. If
//! SSH_KEY also points to a private key, that key is used and SSH_PASSWORD is its passphrase.

use russh::ChannelMsg;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let [_, host, port, user] = args.as_slice() else {
        return Err("usage: shell <host> <port> <user>".into());
    };

    let port: u16 = port.parse()?;
    let password = std::env::var("SSH_PASSWORD")?;
    let key = std::env::var("SSH_KEY")
        .ok()
        .map(std::fs::read_to_string)
        .transpose()?;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    let auth = || match &key {
        Some(key) => ssh::Auth::Key {
            private_key: key,
            passphrase: Some(&password),
        },
        None => ssh::Auth::Password(&password),
    };

    // First connection: shows the fingerprint and reconnects if it is accepted.
    let session = match ssh::connect(host, port, user, auth(), None).await {
        Err(ssh::Error::UnknownHostKey(found)) => {
            println!("Unknown server, fingerprint {found}. Accept? (y/n)");

            if lines.next_line().await?.as_deref() != Some("y") {
                return Ok(());
            }

            ssh::connect(host, port, user, auth(), Some(&found)).await?
        }
        other => other?,
    };

    let mut channel = session.shell(80, 24).await?;
    let mut stdout = tokio::io::stdout();

    loop {
        tokio::select! {
            line = lines.next_line() => match line? {
                Some(line) => channel.data(format!("{line}\n").as_bytes()).await?,
                None => break,
            },
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) => {
                    stdout.write_all(&data).await?;
                    stdout.flush().await?;
                }
                Some(ChannelMsg::ExitStatus { .. }) | None => break,
                _ => {}
            },
        }
    }

    session.close().await?;
    Ok(())
}
