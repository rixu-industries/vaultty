// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! SSH connections. It knows nothing about the UI or the vault: it receives secrets on loan and never
//! returns them, so the app and a future MCP server can use it without exposing them.
//!
//! The server key is never accepted blindly: if it is not known, `connect` fails with
//! `UnknownHostKey` and its fingerprint, and the caller decides (the app asks; an agent cannot accept).

use std::sync::Arc;
use std::time::Duration;

use russh::Disconnect;
use russh::client::{self, Handle, Msg};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate, decode_secret_key};
pub use russh::{Channel, ChannelMsg};

/// TCP connection opened through the server (`direct-tcpip`), for tunnels.
pub type TcpStream = russh::ChannelStream<Msg>;
pub use russh_sftp::client::SftpSession;
pub use russh_sftp::client::error::Error as SftpError;
pub use russh_sftp::client::fs::{File, Metadata};
pub use russh_sftp::protocol::{OpenFlags, StatusCode};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OS_TIMEOUT: Duration = Duration::from_secs(3);
/// On Linux and BSD this is `/etc/os-release`; on macOS, `uname -s`; on Windows (cmd), `ver`.
const OS_COMMAND: &str = "cat /etc/os-release || uname -s || ver";

/// Cap on each `exec` output. The latest part is kept: in a long command, the end (the error, the
/// summary) is what matters.
const MAX_OUTPUT: usize = 64 * 1024;

/// Output of a standalone command so far. It fills while the command runs, so it can be read partway.
#[derive(Debug, Default, Clone)]
pub struct Output {
    /// Raw bytes: a chunk may split a UTF-8 character in half.
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `None` while it runs, or if it never arrived (for example, if a signal killed the command).
    pub status: Option<u32>,
    /// Some output exceeded `MAX_OUTPUT` and lost its beginning.
    pub truncated: bool,
}

/// Appends to the end and, if it goes over the cap, removes from the beginning.
fn keep_tail(buf: &mut Vec<u8>, data: &[u8]) -> bool {
    buf.extend_from_slice(data);

    let extra = buf.len().saturating_sub(MAX_OUTPUT);
    buf.drain(..extra);

    extra > 0
}

/// Connection errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Network or SSH protocol failure.
    #[error("SSH error: {0}")]
    Ssh(#[from] russh::Error),
    /// The server did not respond in time.
    #[error("the server is not responding")]
    Timeout,
    /// First connection to this server: its fingerprint must be confirmed and the connection retried.
    #[error("unknown server, fingerprint {0}")]
    UnknownHostKey(String),
    /// The fingerprint does not match the saved one: it may be an attack or a server reinstall.
    #[error("the server fingerprint has changed (expected {expected}, got {found})")]
    HostKeyChanged { expected: String, found: String },
    /// The private key could not be read, or the passphrase is wrong.
    #[error("invalid private key: {0}")]
    BadKey(String),
    /// The server rejected the username and password or key.
    #[error("the server rejected the credentials")]
    AuthFailed,
    /// The server does not offer SFTP or failed to start it.
    #[error("could not open SFTP: {0}")]
    Sftp(String),
}

/// How the session authenticates. Secrets are borrowed and never stored.
pub enum Auth<'a> {
    Password(&'a str),
    Key {
        private_key: &'a str,
        passphrase: Option<&'a str>,
    },
}

/// An authenticated session. Channels come from it (shells and, later, standalone commands).
pub struct Session {
    handle: Handle<HostCheck>,
}

/// Opens a session. `known_key` is the server's saved fingerprint (`SHA256:…`), or `None` if this is
/// the first time; in that case it fails with `UnknownHostKey` before sending any credentials.
pub async fn connect(
    address: &str,
    port: u16,
    user: &str,
    auth: Auth<'_>,
    known_key: Option<&str>,
) -> Result<Session, Error> {
    let check = HostCheck {
        expected: known_key.map(str::to_owned),
    };

    let config = Arc::new(config());

    let mut handle = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect(config, (address, port), check),
    )
    .await
    .map_err(|_| Error::Timeout)??;

    let result = match auth {
        Auth::Password(password) => handle.authenticate_password(user, password).await?,
        Auth::Key {
            private_key,
            passphrase,
        } => {
            let key = decode_secret_key(private_key, passphrase)
                .map_err(|e| Error::BadKey(e.to_string()))?;

            // RSA keys need a hash the server accepts; the others ignore it.
            let hash = handle.best_supported_rsa_hash().await?.flatten();

            handle
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?
        }
    };

    if !result.success() {
        return Err(Error::AuthFailed);
    }

    Ok(Session { handle })
}

impl Session {
    /// Opens an interactive shell with a terminal of the given size. Output is read with
    /// `channel.wait()`, and input is sent with `channel.data(...)`.
    pub async fn shell(&self, cols: u32, rows: u32) -> Result<Channel<Msg>, Error> {
        let channel = self.handle.channel_open_session().await?;
        channel
            .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;
        Ok(channel)
    }

    /// Opens SFTP on a new channel of this session.
    pub async fn sftp(&self) -> Result<SftpSession, Error> {
        let channel = self.handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;

        SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| Error::Sftp(e.to_string()))
    }

    /// The server's operating system (`ubuntu`, `debian`, `macos`, `windows`…), via a standalone
    /// command. `None` if it is not recognized or does not respond in time; it is not a session error.
    pub async fn os(&self) -> Option<String> {
        let run = async {
            let mut channel = self.handle.channel_open_session().await.ok()?;
            channel.exec(true, OS_COMMAND).await.ok()?;
            let mut out = Vec::new();

            // Capped, in case the server keeps sending.
            while out.len() < 16 * 1024 {
                match channel.wait().await? {
                    ChannelMsg::Data { data } => out.extend_from_slice(&data),
                    ChannelMsg::Eof | ChannelMsg::Close => break,
                    _ => {}
                }
            }

            parse_os(&String::from_utf8_lossy(&out))
        };

        tokio::time::timeout(OS_TIMEOUT, run).await.ok().flatten()
    }

    /// Runs a standalone command, without a terminal, storing its output in `out` until it ends.
    /// If `stop` finishes first, it sends `KILL` to the remote process: without a terminal, cutting the
    /// connection does not kill it. Returns `true` if it ended on its own and `false` if it was stopped.
    pub async fn exec(
        &self,
        command: &str,
        out: &std::sync::Mutex<Output>,
        stop: impl Future<Output = ()>,
    ) -> Result<bool, Error> {
        let mut channel = self.handle.channel_open_session().await?;
        channel.exec(true, command).await?;

        tokio::pin!(stop);

        // The exit code can arrive after EOF, so read until the channel closes.
        loop {
            let msg = tokio::select! {
                msg = channel.wait() => msg,
                () = &mut stop => {
                    // If the server does not accept signals, at least close the channel.
                    let _ = channel.signal(russh::Sig::KILL).await;
                    let _ = channel.close().await;
                    return Ok(false);
                }
            };

            let mut out = out.lock().unwrap();

            match msg {
                Some(ChannelMsg::Data { data }) => {
                    out.truncated |= keep_tail(&mut out.stdout, &data);
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    out.truncated |= keep_tail(&mut out.stderr, &data);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => out.status = Some(exit_status),
                Some(ChannelMsg::Close) | None => return Ok(true),
                Some(_) => {}
            }
        }
    }

    /// Opens a TCP connection from the server to `host:port`, like `ssh -L`. Several can be open at
    /// once over the same session.
    pub async fn direct_tcpip(&self, host: &str, port: u16) -> Result<TcpStream, Error> {
        // The origin is informational only for the server: it is not told who connected to the proxy.
        let channel = self
            .handle
            .channel_open_direct_tcpip(host, port.into(), "127.0.0.1", 0)
            .await?;

        Ok(channel.into_stream())
    }

    /// The connection has been cut (by the server, the network, or `close`).
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    pub async fn close(&self) -> Result<(), Error> {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "es")
            .await?;
        Ok(())
    }
}

/// Without keepalive, a network that drops without warning (wifi, VPN, sleep) goes unnoticed until
/// the system gives up on the TCP connection, minutes later. This way it is cut after ~45 s without a reply.
fn config() -> client::Config {
    client::Config {
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        // Nagle disabled: SFTP and the terminal send small messages that wait for a reply.
        nodelay: true,
        ..Default::default()
    }
}

/// Checks the server key during the handshake, before authenticating.
struct HostCheck {
    expected: Option<String>,
}

impl client::Handler for HostCheck {
    type Error = Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Error> {
        let found = fingerprint(key);

        match &self.expected {
            Some(expected) if *expected == found => Ok(true),
            Some(expected) => Err(Error::HostKeyChanged {
                expected: expected.clone(),
                found,
            }),
            None => Err(Error::UnknownHostKey(found)),
        }
    }
}

/// SHA-256 fingerprint in OpenSSH format (`SHA256:…`), the one shown by `ssh-keygen -l`.
fn fingerprint(key: &PublicKeyOrCertificate) -> String {
    match key {
        PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256),
        PublicKeyOrCertificate::Certificate(cert) => cert.public_key().fingerprint(HashAlg::Sha256),
    }
    .to_string()
}

/// Extracts the OS from the output of `OS_COMMAND`: the `ID` from os-release, or the name from `uname`/`ver`.
fn parse_os(out: &str) -> Option<String> {
    if let Some(id) = out.lines().find_map(|l| l.trim().strip_prefix("ID=")) {
        let id = id.trim().trim_matches(['"', '\'']).to_lowercase();
        // This is text from the server: only something shaped like an id is accepted.
        let valid = !id.is_empty()
            && id.len() <= 32
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        return valid.then_some(id);
    }

    let out = out.trim();

    match out {
        "Darwin" => Some("macos".into()),
        "Linux" | "FreeBSD" | "OpenBSD" | "NetBSD" => Some(out.to_lowercase()),
        _ if out.starts_with("Microsoft Windows") => Some("windows".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use russh::client::Handler;
    use russh::keys::PublicKey;

    use super::*;

    const KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAdYHjkNnTeFKiCV3Kvn9jrkT0OCneD4EOEPKRy0UvQ9";

    fn server_key() -> PublicKeyOrCertificate {
        PublicKeyOrCertificate::PublicKey {
            key: PublicKey::from_openssh(KEY).unwrap(),
            hash_alg: None,
        }
    }

    async fn check(expected: Option<&str>) -> Result<bool, Error> {
        let mut check = HostCheck {
            expected: expected.map(str::to_owned),
        };

        check.check_server_key(&server_key()).await
    }

    #[test]
    fn detects_dropped_connections() {
        let c = config();
        assert_eq!(c.keepalive_interval, Some(Duration::from_secs(15)));
        assert_eq!(c.keepalive_max, 3);
    }

    #[test]
    fn keeps_the_end_of_the_output() {
        let mut buf = Vec::new();

        assert!(!keep_tail(&mut buf, b"start"));
        assert!(keep_tail(&mut buf, &vec![b'x'; MAX_OUTPUT]));
        assert_eq!(buf.len(), MAX_OUTPUT);
        assert!(buf.iter().all(|b| *b == b'x'));

        assert!(keep_tail(&mut buf, b"end"));
        assert!(buf.ends_with(b"end"));
        assert_eq!(buf.len(), MAX_OUTPUT);
    }

    #[test]
    fn recognizes_the_os() {
        let ubuntu = "PRETTY_NAME=\"Ubuntu 24.04 LTS\"\nID=ubuntu\nID_LIKE=debian\n";
        assert_eq!(parse_os(ubuntu).as_deref(), Some("ubuntu"));
        assert_eq!(parse_os("ID=\"rhel\"\n").as_deref(), Some("rhel"));
        assert_eq!(parse_os("Darwin\n").as_deref(), Some("macos"));
        assert_eq!(parse_os("FreeBSD\n").as_deref(), Some("freebsd"));
        assert_eq!(
            parse_os("\r\nMicrosoft Windows [Version 10.0.19045.3803]\r\n").as_deref(),
            Some("windows")
        );
        assert_eq!(parse_os(""), None);
        assert_eq!(parse_os("ID=<script>\n"), None);
    }

    #[tokio::test]
    async fn host_key_check() {
        let found = fingerprint(&server_key());
        assert!(found.starts_with("SHA256:"));

        assert!(check(Some(&found)).await.unwrap());

        assert!(matches!(check(None).await, Err(Error::UnknownHostKey(f)) if f == found));

        assert!(matches!(
            check(Some("SHA256:other")).await,
            Err(Error::HostKeyChanged { found: f, .. }) if f == found
        ));
    }
}
