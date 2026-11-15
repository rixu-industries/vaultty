// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! The HTTP server (MCP Streamable HTTP, at `/mcp`). `rmcp` rejects requests with another
//! `Host` (protects against DNS rebinding); the Bearer token keeps any program on the machine from getting in.

use std::future::Future;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};

use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use sessions::Sessions;
use tokio_util::sync::CancellationToken;

use crate::jobs::Jobs;
use crate::settings::{Settings, VaultAccess};
use crate::tools::{Ask, Tools};

/// The running server. It stops when dropped.
pub struct Server {
    stop: CancellationToken,
    access: Arc<Mutex<Vec<VaultAccess>>>,
}

impl Server {
    /// Changes the vaults and permissions without restarting: takes effect from the next call.
    pub fn set_vaults(&self, vaults: Vec<VaultAccess>) {
        *self.access.lock().unwrap() = vaults;
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// Opens the port now, so that a "port in use" error surfaces here. What it returns apart is the server
/// itself: it has to be spawned on the tokio runtime and ends when `Server` is dropped. `ask` asks the user
/// before running a command on hosts set to "Ask every time".
pub fn start(
    settings: &Settings,
    token: String,
    sessions: Arc<Sessions>,
    ask: Ask,
) -> Result<(Server, impl Future<Output = ()> + Send + 'static), String> {
    let listeners = bind(&settings.host, settings.port)
        .map_err(|e| format!("Could not open {}:{}: {e}", settings.host, settings.port))?;

    let stop = CancellationToken::new();
    let access = Arc::new(Mutex::new(settings.vaults.clone()));

    let tools = Tools {
        sessions,
        access: access.clone(),
        ask,
        jobs: Arc::new(Jobs::new(stop.clone())),
    };

    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts([settings.host.clone()])
        .with_cancellation_token(stop.child_token());

    let service = StreamableHttpService::new(
        move || Ok(tools.clone()),
        LocalSessionManager::default().into(),
        config,
    );

    let token = Arc::new(token);
    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn(move |req, next| {
            authorize(token.clone(), req, next)
        }));

    let shutdown = stop.clone();
    let run = async move {
        let mut all = tokio::task::JoinSet::new();

        for listener in listeners {
            // Converting to tokio has to happen inside the runtime, not when the port is opened.
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                continue;
            };

            let serve = axum::serve(listener, app.clone())
                .with_graceful_shutdown(shutdown.clone().cancelled_owned());

            all.spawn(async move {
                let _ = serve.await;
            });
        }

        all.join_all().await;
    };

    Ok((Server { stop, access }, run))
}

/// Whether `host` only reaches this computer. Anything else puts the token, which travels in plain
/// HTTP, on the network.
pub fn is_loopback(host: &str) -> bool {
    let host = host.trim();

    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// One port on each address of the host: `localhost` is `127.0.0.1` and `::1`, and each client tries
/// the one it wants. If one is in use, it fails as a whole; otherwise another program would receive part of the requests,
/// along with the token. Only an address the system does not have (IPv6 disabled) is skipped.
fn bind(host: &str, port: u16) -> std::io::Result<Vec<std::net::TcpListener>> {
    let mut listeners = Vec::new();

    for addr in (host, port).to_socket_addrs()? {
        match std::net::TcpListener::bind(addr) {
            Ok(l) => {
                l.set_nonblocking(true)?;
                listeners.push(l);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrNotAvailable => {}
            Err(e) => return Err(e),
        }
    }

    if listeners.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "the host has no available address",
        ));
    }

    Ok(listeners)
}

async fn authorize(token: Arc<String>, req: Request, next: Next) -> Result<Response, StatusCode> {
    let sent = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();

    if same(sent.as_bytes(), token.as_bytes()) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Compares without stopping at the first differing byte, so timing gives no hints about the token.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use vaults::Vaults;

    #[test]
    fn only_this_computer_is_loopback() {
        assert!(is_loopback("localhost"));
        assert!(is_loopback(" 127.0.0.1 "));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("0.0.0.0"));
        assert!(!is_loopback("192.168.1.20"));
        assert!(!is_loopback("my-pc.lan"));
    }

    #[test]
    fn compares_tokens() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
        assert!(!same(b"", b"abc"));
    }

    /// HTTP request written by hand: no HTTP client is needed just for this.
    async fn post(port: u16, host: &str, token: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        s.write_all(req.as_bytes()).await.unwrap();

        let mut out = String::new();
        let _ = s.read_to_string(&mut out).await;
        out
    }

    #[tokio::test]
    async fn requires_token_and_host_and_answers_initialize() {
        let settings = Settings {
            host: "127.0.0.1".into(),
            port: 47699,
            ..Settings::default()
        };

        // A vault list that does not exist: the real one is not touched.
        let sessions = Sessions::new(
            Arc::new(Vaults::at(
                std::env::temp_dir().join("vaultty-test-none.json"),
            )),
            tokio::runtime::Handle::current(),
            Arc::new(|_| {}),
        );
        let ask: Ask = Arc::new(|_| Box::pin(async { false }));

        let (server, run) =
            start(&settings, "secret".into(), sessions.clone(), ask.clone()).unwrap();
        tokio::spawn(run);

        let ok = post(47699, "127.0.0.1", "secret").await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.contains("serverInfo"), "{ok}");

        let wrong_token = post(47699, "127.0.0.1", "other").await;
        assert!(wrong_token.starts_with("HTTP/1.1 401"), "{wrong_token}");

        let wrong_host = post(47699, "evil.example", "secret").await;
        assert!(!wrong_host.starts_with("HTTP/1.1 200"), "{wrong_host}");

        // While it is running, the port is in use.
        assert!(start(&settings, "x".into(), sessions, ask).is_err());

        drop(server);
    }
}
