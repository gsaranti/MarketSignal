//! Minimal blocking HTTPS server for the Schwab OAuth loopback capture
//! (`docs/schwab-integration.md §Authorization`).
//!
//! Replaces `tiny_http`'s `ssl-rustls` server, which hard-pinned the EOL rustls 0.20 /
//! ring 0.16 stack (RUSTSEC-2024-0336 unfixed on that branch); this serves the same
//! one-shot capture on the rustls 0.23 + ring 0.17 stack the app's outbound HTTP
//! already uses, so removing `tiny_http` deletes the old TLS subtree entirely.
//!
//! Deliberately tiny in scope — it exists to hand the OAuth capture loop each inbound
//! request's *target* (path + query) and write one fixed, connection-closing response.
//! No routing, no keep-alive, no ALPN (so browsers settle on HTTP/1.1), no body
//! reading (the redirect is a GET). Per-connection failures are tolerated by design:
//! with a per-run self-signed certificate the browser *aborts the first TLS handshake*
//! at the interstitial and retries on a new connection once the user clicks through,
//! and browsers also open speculative connections they close without a request — none
//! of that may kill the capture. The overall wait is bounded by the caller's deadline,
//! each connection by its own I/O timeout and header-size cap.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// How long to sleep between polls of the non-blocking `accept` — `std`'s listener has
/// no timed accept, and the capture needs to honor its deadline without a second thread.
const ACCEPT_POLL: Duration = Duration::from_millis(50);
/// Per-connection read/write timeout, so one stalled connection can't eat the whole
/// capture window.
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Cap on a request's line + headers. The OAuth redirect is a short GET; anything
/// larger is not our redirect.
const MAX_REQUEST_BYTES: usize = 16 * 1024;

/// The one-shot loopback HTTPS server: a bound listener plus the TLS config built from
/// the caller's (per-run, self-signed) certificate. Dropping it closes the port.
pub struct LoopbackHttpsServer {
    listener: TcpListener,
    tls: Arc<ServerConfig>,
}

impl LoopbackHttpsServer {
    /// Bind `addr` and prepare the TLS acceptor over `cert`/`key` (DER, as rcgen
    /// emits). No ALPN is advertised, so the browser negotiates HTTP/1.1; protocol
    /// versions and cipher suites are rustls 0.23's defaults (TLS 1.2 + 1.3).
    pub fn bind(
        addr: &str,
        cert: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self> {
        let tls = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .context("building the loopback TLS config")?;
        let listener = TcpListener::bind(addr)
            .with_context(|| format!("binding loopback OAuth server on {addr}"))?;
        listener
            .set_nonblocking(true)
            .context("configuring the loopback listener")?;
        Ok(Self {
            listener,
            tls: Arc::new(tls),
        })
    }

    /// The bound address — the port, when bound with port 0 (tests).
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener
            .local_addr()
            .context("reading the loopback server address")
    }

    /// Block until the next *well-formed* HTTPS request arrives, or `Ok(None)` at the
    /// deadline. Connection-level failures — an aborted handshake at the self-signed
    /// interstitial, a speculative connection closed without a request, an oversized or
    /// stalled read — are swallowed and the wait continues; only a listener-level fault
    /// is an error.
    pub fn next_request(&self, deadline: Instant) -> Result<Option<CapturedRequest>> {
        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }
            let stream = match self.listener.accept() {
                Ok((stream, _peer)) => stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_POLL);
                    continue;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e).context("awaiting the OAuth redirect"),
            };
            match self.read_request(stream, deadline) {
                Ok(request) => return Ok(Some(request)),
                // Tolerated per the module contract; the real redirect arrives on a
                // fresh connection.
                Err(_) => continue,
            }
        }
    }

    /// Drive the TLS handshake and read one request's line + headers off `stream`,
    /// returning the request target and the live stream to answer on. Honors the
    /// caller's `deadline` between reads, so a connection dripping bytes can overshoot
    /// it by at most one `IO_TIMEOUT`, never `MAX_REQUEST_BYTES × IO_TIMEOUT`.
    fn read_request(&self, stream: TcpStream, deadline: Instant) -> Result<CapturedRequest> {
        // Accepted sockets don't reliably inherit blocking mode from the listener
        // (unspecified by POSIX), so set it explicitly, then bound all I/O.
        stream
            .set_nonblocking(false)
            .context("configuring the accepted connection")?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let conn = ServerConnection::new(self.tls.clone())
            .context("starting the loopback TLS session")?;
        // `StreamOwned` drives the handshake implicitly on first read.
        let mut tls = StreamOwned::new(conn, stream);

        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        while !header_block_complete(&buf) {
            if Instant::now() >= deadline {
                bail!("capture deadline passed mid-request");
            }
            if buf.len() >= MAX_REQUEST_BYTES {
                bail!("request headers exceed the loopback cap");
            }
            let n = tls.read(&mut chunk).context("reading the loopback request")?;
            if n == 0 {
                bail!("connection closed before a full request");
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let target = parse_request_target(&buf)?;
        Ok(CapturedRequest {
            target,
            stream: tls,
        })
    }
}

/// One accepted, fully-read request: its target (path + query, exactly as the browser
/// sent it) and the connection to answer on.
pub struct CapturedRequest {
    target: String,
    stream: StreamOwned<ServerConnection, TcpStream>,
}

impl CapturedRequest {
    /// The request target — `/?code=…&state=…` for the redirect this server exists for.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Answer with a `200 text/html` response and close the connection cleanly —
    /// `Connection: close` (no keep-alive to hold the browser on) plus a TLS
    /// `close_notify` so the browser sees an orderly EOF, not truncation. Best-effort
    /// by design: the capture's control flow never depends on the browser having
    /// received the answer, so write failures are discarded.
    pub fn respond(mut self, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len(),
        );
        let _ = self.stream.write_all(response.as_bytes());
        self.stream.conn.send_close_notify();
        let _ = self.stream.flush();
        let _ = self.stream.sock.shutdown(std::net::Shutdown::Write);
    }
}

/// Whether `buf` holds a complete request line + header block (the `\r\n\r\n`
/// terminator). The redirect is a GET, so nothing past the headers is needed.
fn header_block_complete(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n")
}

/// Pull the request target (the second token of the request line) out of a raw
/// header block: `GET /?code=… HTTP/1.1` → `/?code=…`.
fn parse_request_target(buf: &[u8]) -> Result<String> {
    let line_end = buf
        .windows(2)
        .position(|w| w == b"\r\n")
        .unwrap_or(buf.len());
    let line = String::from_utf8_lossy(&buf[..line_end]);
    line.split_whitespace()
        .nth(1)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("malformed request line on the loopback server"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_block_completion_requires_the_blank_line() {
        assert!(!header_block_complete(b"GET / HTTP/1.1\r\nHost: x\r\n"));
        assert!(header_block_complete(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"));
    }

    #[test]
    fn request_target_is_the_second_request_line_token() {
        assert_eq!(
            parse_request_target(b"GET /?code=ABC&state=n HTTP/1.1\r\nHost: x\r\n\r\n").unwrap(),
            "/?code=ABC&state=n"
        );
        // A garbage request line is an error (the connection gets dropped and the
        // capture keeps waiting), never a phantom target.
        assert!(parse_request_target(b"NONSENSE\r\n\r\n").is_err());
        assert!(parse_request_target(b"\r\n\r\n").is_err());
    }

    // The full TLS round-trip (bind → handshake → request → response → close) is
    // exercised end-to-end by `schwab_oauth`'s capture tests, which drive this server
    // with a real HTTPS client.
}
