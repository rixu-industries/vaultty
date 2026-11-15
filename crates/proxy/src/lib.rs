// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Local proxies: SOCKS5, SOCKS4/4a, HTTP and fixed-port forwarding. They do not know how their
//! connections leave: each one is requested from a `Dialer`, which today opens an SSH channel and
//! tomorrow may use another protocol. This way the same proxy works with any transport.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

/// How long the client handshake plus the connection to the destination may take. It includes
/// reconnecting the transport if it had dropped, so it is generous.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Cap on an HTTP request header: anything larger means a broken or malicious client.
const MAX_HEAD: usize = 16 * 1024;

/// Which protocol the proxy speaks with the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Protocol {
    Socks5,
    /// SOCKS4 and 4a (with a domain name).
    Socks4,
    /// `CONNECT` for HTTPS and any TCP, and plain HTTP requests (`GET http://…`).
    Http,
    /// Everything that arrives goes to this destination, with no negotiation.
    Forward {
        host: String,
        port: u16,
    },
}

/// Opens a TCP connection to the destination through the transport (an SSH channel, for example).
pub trait Dialer: Send + Sync + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    fn dial(&self, host: &str, port: u16) -> impl Future<Output = io::Result<Self::Stream>> + Send;
}

/// Serves connections from `listener` until the future is dropped. Dropping it also cuts all
/// open connections.
pub async fn serve<D: Dialer>(listener: TcpListener, protocol: Protocol, dialer: Arc<D>) {
    let mut clients = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((client, _)) => {
                    let (protocol, dialer) = (protocol.clone(), dialer.clone());

                    // A client that fails does not affect the others: its error matters to no one else.
                    clients.spawn(async move {
                        let _ = handle(client, &protocol, &*dialer).await;
                    });
                }
                // No free file descriptors, for example: wait a bit instead of spinning idle.
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            },
            // Reap finished tasks so the `JoinSet` does not grow without limit.
            Some(_) = clients.join_next() => {}
        }
    }
}

async fn handle<D: Dialer>(
    mut client: TcpStream,
    protocol: &Protocol,
    dialer: &D,
) -> io::Result<()> {
    let _ = client.set_nodelay(true);

    let negotiate = async {
        match protocol {
            Protocol::Socks5 => socks5(&mut client, dialer).await,
            Protocol::Socks4 => socks4(&mut client, dialer).await,
            Protocol::Http => http(&mut client, dialer).await,
            Protocol::Forward { host, port } => dialer.dial(host, *port).await,
        }
    };

    let mut remote = tokio::time::timeout(HANDSHAKE_TIMEOUT, negotiate)
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;

    copy_bidirectional(&mut client, &mut remote).await?;
    Ok(())
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

// ---- SOCKS5 (RFC 1928), no authentication and CONNECT only ----

async fn socks5<D: Dialer>(client: &mut TcpStream, dialer: &D) -> io::Result<D::Stream> {
    let [version, count] = read_array(client).await?;

    if version != 5 {
        return Err(invalid("not SOCKS5"));
    }

    let mut methods = vec![0; count as usize];
    client.read_exact(&mut methods).await?;

    // 0x00: no authentication. The proxy listens locally; if the client does not accept it, there is nothing else to offer.
    if !methods.contains(&0) {
        client.write_all(&[5, 0xff]).await?;
        return Err(invalid("no supported auth method"));
    }

    client.write_all(&[5, 0]).await?;

    let [_, command, _, kind] = read_array(client).await?;

    let host = match kind {
        1 => std::net::Ipv4Addr::from(read_array::<4>(client).await?).to_string(),
        3 => {
            let [len] = read_array(client).await?;
            let mut name = vec![0; len as usize];
            client.read_exact(&mut name).await?;
            String::from_utf8(name).map_err(|_| invalid("bad domain"))?
        }
        4 => std::net::Ipv6Addr::from(read_array::<16>(client).await?).to_string(),
        _ => {
            socks5_reply(client, 8).await?;
            return Err(invalid("bad address type"));
        }
    };

    let port = u16::from_be_bytes(read_array(client).await?);

    if command != 1 {
        socks5_reply(client, 7).await?;
        return Err(invalid("only CONNECT is supported"));
    }

    match dialer.dial(&host, port).await {
        Ok(remote) => {
            socks5_reply(client, 0).await?;
            Ok(remote)
        }
        Err(e) => {
            // 5: connection refused. It is the closest match to "the other side could not open it".
            socks5_reply(client, 5).await?;
            Err(e)
        }
    }
}

/// Reply with the local address set to zero: clients do not use it for CONNECT.
async fn socks5_reply(client: &mut TcpStream, code: u8) -> io::Result<()> {
    client.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

// ---- SOCKS4 and 4a ----

async fn socks4<D: Dialer>(client: &mut TcpStream, dialer: &D) -> io::Result<D::Stream> {
    let [version, command, p1, p2, a, b, c, d] = read_array(client).await?;

    if version != 4 {
        return Err(invalid("not SOCKS4"));
    }

    // The user field is not used, but it must be read.
    read_until_nul(client).await?;

    let port = u16::from_be_bytes([p1, p2]);

    // 4a: an IP `0.0.0.x` (x ≠ 0) means the host name follows.
    let host = if [a, b, c] == [0, 0, 0] && d != 0 {
        String::from_utf8(read_until_nul(client).await?).map_err(|_| invalid("bad domain"))?
    } else {
        std::net::Ipv4Addr::new(a, b, c, d).to_string()
    };

    let result = match command {
        1 => dialer.dial(&host, port).await,
        _ => Err(invalid("only CONNECT is supported")),
    };

    // 0x5A granted, 0x5B rejected. Clients do not use the rest of the reply.
    let code = if result.is_ok() { 0x5a } else { 0x5b };
    client.write_all(&[0, code, 0, 0, 0, 0, 0, 0]).await?;

    result
}

async fn read_until_nul(client: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();

    loop {
        let [byte] = read_array(client).await?;

        if byte == 0 {
            return Ok(out);
        }

        if out.len() >= 255 {
            return Err(invalid("field too long"));
        }

        out.push(byte);
    }
}

async fn read_array<const N: usize>(client: &mut TcpStream) -> io::Result<[u8; N]> {
    let mut buf = [0; N];
    client.read_exact(&mut buf).await?;
    Ok(buf)
}

// ---- HTTP ----

const BAD_REQUEST: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const BAD_GATEWAY: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// Headers meant for the proxy, not for the server.
const HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
];

async fn http<D: Dialer>(client: &mut TcpStream, dialer: &D) -> io::Result<D::Stream> {
    let (head, rest) = read_head(client).await?;

    let Some(request) = parse_http(&head) else {
        client.write_all(BAD_REQUEST).await?;
        return Err(invalid("bad HTTP request"));
    };

    let mut remote = match dialer.dial(&request.host, request.port).await {
        Ok(remote) => remote,
        Err(e) => {
            client.write_all(BAD_GATEWAY).await?;
            return Err(e);
        }
    };

    match &request.forward {
        // CONNECT: from here on it is just raw bytes (usually TLS).
        None => {
            client
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await?
        }
        Some(head) => remote.write_all(head).await?,
    }

    remote.write_all(&rest).await?;
    Ok(remote)
}

/// Reads up to the end of the header. Returns the header and whatever came after it (the body,
/// or the beginning of the TLS).
async fn read_head(client: &mut TcpStream) -> io::Result<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0; 4096];

    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            let head = String::from_utf8(buf).map_err(|_| invalid("bad HTTP head"))?;
            return Ok((head, rest));
        }

        if buf.len() > MAX_HEAD {
            return Err(invalid("HTTP head too long"));
        }

        let n = client.read(&mut chunk).await?;

        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        buf.extend_from_slice(&chunk[..n]);
    }
}

#[derive(Debug, PartialEq)]
struct HttpRequest {
    host: String,
    port: u16,
    /// The header to send to the server; `None` for a `CONNECT`.
    forward: Option<Vec<u8>>,
}

fn parse_http(head: &str) -> Option<HttpRequest> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);

    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = host_port(target, 443)?;
        return Some(HttpRequest {
            host,
            port,
            forward: None,
        });
    }

    // Normal request to a proxy: the full URL is given (`GET http://host/path`). Only the path is
    // sent to the server.
    let url = target
        .get(..7)
        .filter(|s| s.eq_ignore_ascii_case("http://"))
        .map(|_| &target[7..])?;
    let (authority, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };
    let (host, port) = host_port(authority, 80)?;

    let mut out = format!("{method} {path} {version}\r\n");

    for line in lines.filter(|l| !l.is_empty()) {
        let name = line.split(':').next().unwrap_or_default().trim();

        if !HOP_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)) {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }

    // One connection, one request: if the client sent the next one to another host over the same
    // connection, it would end up at the wrong server.
    out.push_str("Connection: close\r\n\r\n");

    Some(HttpRequest {
        host,
        port,
        forward: Some(out.into_bytes()),
    })
}

/// `host`, `host:port`, or `[ipv6]:port`.
fn host_port(authority: &str, default: u16) -> Option<(String, u16)> {
    // Without user info: `user:pass@host` is not sent to the server.
    let authority = authority.rsplit('@').next()?;

    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => {
            let (host, after) = rest.split_once(']')?;
            (host, after.strip_prefix(':'))
        }
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };

    let port = match port {
        Some(port) => port.parse().ok().filter(|p| *p != 0)?,
        None => default,
    };

    (!host.is_empty()).then(|| (host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Connects over real TCP and records where it was asked to go.
    #[derive(Default)]
    struct Direct {
        asked: Mutex<Vec<(String, u16)>>,
    }

    impl Dialer for Direct {
        type Stream = TcpStream;

        async fn dial(&self, host: &str, port: u16) -> io::Result<TcpStream> {
            self.asked.lock().unwrap().push((host.to_string(), port));
            TcpStream::connect(("127.0.0.1", port)).await
        }
    }

    /// Server that echoes back what it receives, prefixed with `>`.
    async fn echo() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();

                tokio::spawn(async move {
                    let mut buf = [0; 1024];

                    while let Ok(n) = s.read(&mut buf).await {
                        if n == 0 || s.write_all(b">").await.is_err() {
                            break;
                        }

                        let _ = s.write_all(&buf[..n]).await;
                    }
                });
            }
        });

        port
    }

    async fn proxy(protocol: Protocol) -> (u16, Arc<Direct>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let dialer = Arc::new(Direct::default());

        tokio::spawn(serve(listener, protocol, dialer.clone()));
        (port, dialer)
    }

    async fn read_n(s: &mut TcpStream, n: usize) -> Vec<u8> {
        let mut buf = vec![0; n];
        s.read_exact(&mut buf).await.unwrap();
        buf
    }

    #[tokio::test]
    async fn socks5_with_domain() {
        let target = echo().await;
        let (port, dialer) = proxy(Protocol::Socks5).await;
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();

        s.write_all(&[5, 1, 0]).await.unwrap();
        assert_eq!(read_n(&mut s, 2).await, [5, 0]);

        let mut req = vec![5, 1, 0, 3, 9];
        req.extend_from_slice(b"db.intern");
        req.extend_from_slice(&target.to_be_bytes());
        s.write_all(&req).await.unwrap();
        assert_eq!(read_n(&mut s, 10).await[..2], [5, 0]);

        s.write_all(b"hola").await.unwrap();
        assert_eq!(read_n(&mut s, 5).await, b">hola");
        assert_eq!(
            dialer.asked.lock().unwrap()[0],
            ("db.intern".into(), target)
        );
    }

    #[tokio::test]
    async fn socks5_rejects_bad_method_and_reports_failure() {
        let (port, _) = proxy(Protocol::Socks5).await;

        // Only username and password: not accepted.
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(&[5, 1, 2]).await.unwrap();
        assert_eq!(read_n(&mut s, 2).await, [5, 0xff]);

        // To a closed port: connection refused reply.
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead = closed.local_addr().unwrap().port();
        drop(closed);

        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(&[5, 1, 0]).await.unwrap();
        read_n(&mut s, 2).await;
        let mut req = vec![5, 1, 0, 1, 127, 0, 0, 1];
        req.extend_from_slice(&dead.to_be_bytes());
        s.write_all(&req).await.unwrap();
        assert_eq!(read_n(&mut s, 10).await[..2], [5, 5]);
    }

    #[tokio::test]
    async fn socks4a() {
        let target = echo().await;
        let (port, dialer) = proxy(Protocol::Socks4).await;
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();

        let mut req = vec![4, 1];
        req.extend_from_slice(&target.to_be_bytes());
        req.extend_from_slice(&[0, 0, 0, 1]);
        req.extend_from_slice(b"user\0web.local\0");
        s.write_all(&req).await.unwrap();
        assert_eq!(read_n(&mut s, 8).await[..2], [0, 0x5a]);

        s.write_all(b"x").await.unwrap();
        assert_eq!(read_n(&mut s, 2).await, b">x");
        assert_eq!(dialer.asked.lock().unwrap()[0].0, "web.local");
    }

    #[tokio::test]
    async fn http_connect() {
        let target = echo().await;
        let (port, dialer) = proxy(Protocol::Http).await;
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();

        // What follows the header must also arrive.
        let req = format!("CONNECT site.test:{target} HTTP/1.1\r\nHost: site.test\r\n\r\nhi");
        s.write_all(req.as_bytes()).await.unwrap();

        let ok = b"HTTP/1.1 200 Connection established\r\n\r\n";
        assert_eq!(read_n(&mut s, ok.len()).await, ok);
        assert_eq!(read_n(&mut s, 3).await, b">hi");
        assert_eq!(
            dialer.asked.lock().unwrap()[0],
            ("site.test".into(), target)
        );
    }

    #[tokio::test]
    async fn fixed_forward() {
        let target = echo().await;
        let (port, dialer) = proxy(Protocol::Forward {
            host: "10.0.0.5".into(),
            port: target,
        })
        .await;
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();

        s.write_all(b"ping").await.unwrap();
        assert_eq!(read_n(&mut s, 5).await, b">ping");
        assert_eq!(dialer.asked.lock().unwrap()[0].0, "10.0.0.5");
    }

    #[test]
    fn plain_http_rewrites_request() {
        let head = "GET http://example.com:8080/a?b=1 HTTP/1.1\r\nHost: example.com:8080\r\nProxy-Connection: keep-alive\r\nAccept: */*\r\n\r\n";
        let req = parse_http(head).unwrap();

        assert_eq!((req.host.as_str(), req.port), ("example.com", 8080));
        assert_eq!(
            String::from_utf8(req.forward.unwrap()).unwrap(),
            "GET /a?b=1 HTTP/1.1\r\nHost: example.com:8080\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );

        assert_eq!(
            parse_http("GET http://example.com HTTP/1.1\r\n\r\n").map(|r| r.port),
            Some(80)
        );
        assert!(parse_http("GET /relative HTTP/1.1\r\n\r\n").is_none());
        assert!(parse_http("garbage").is_none());
    }

    #[test]
    fn splits_host_and_port() {
        assert_eq!(host_port("a.b:22", 1), Some(("a.b".into(), 22)));
        assert_eq!(host_port("a.b", 443), Some(("a.b".into(), 443)));
        assert_eq!(host_port("[::1]:8080", 1), Some(("::1".into(), 8080)));
        assert_eq!(host_port("[::1]", 80), Some(("::1".into(), 80)));
        assert_eq!(host_port("u:p@h:9", 1), Some(("h".into(), 9)));
        assert_eq!(host_port("h:0", 1), None);
        assert_eq!(host_port("h:x", 1), None);
        assert_eq!(host_port(":80", 1), None);
    }
}
