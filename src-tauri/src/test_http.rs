//! Throwaway localhost HTTP server for offline round-trip tests.
//!
//! Shared by `http_retry`'s own retry/backoff coverage and by each gated adapter's
//! offline round-trip test (FMP / FRED / BLS / Tavily / fmp_news). A test scripts a
//! fixed sequence of canned replies — one per inbound connection — points the unit
//! under test at [`MockHttp::base_url`], and asserts the wire path (URL build →
//! `send_with_retry` → `interpret_response` → domain output) end to end without a live
//! API key. Compiled only under `cfg(test)`.

#![cfg(test)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One canned reply the mock server writes for a single inbound connection.
pub enum Canned {
    /// A complete HTTP/1.1 reply with a matching `Content-Length`.
    Reply {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static str,
    },
    /// Declares `content_length` bytes but writes only `partial`, then lets the
    /// connection close — simulating a body dropped mid-stream so the client's
    /// `Response::text()` errors. Served as HTTP 200 (see `write_canned`) so the
    /// status is non-retryable: the *only* path to a second attempt is the body
    /// re-read branch, which is exactly what the dropped-body test pins.
    DropBody {
        content_length: usize,
        partial: &'static str,
    },
}

/// A throwaway localhost HTTP server that serves a fixed script of replies — one
/// per inbound connection — and counts the connections it accepted. The listener
/// is bound on the caller's thread (so the port is live before `serve` returns),
/// then handed to a detached worker that consumes the whole script and exits.
/// Size each script to the expected number of attempts so no `accept()` blocks
/// past the run.
pub struct MockHttp {
    pub base_url: String,
    attempts: Arc<AtomicUsize>,
    targets: Arc<Mutex<Vec<String>>>,
}

impl MockHttp {
    pub fn serve(script: Vec<Canned>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let attempts = Arc::new(AtomicUsize::new(0));
        let targets = Arc::new(Mutex::new(Vec::new()));
        let counter = Arc::clone(&attempts);
        let recorder = Arc::clone(&targets);
        std::thread::spawn(move || serve_script(listener, script, counter, recorder));
        Self {
            base_url: format!("http://127.0.0.1:{port}/"),
            attempts,
            targets,
        }
    }

    /// Connections accepted so far == requests the unit under test actually made.
    /// Every increment lands before the matching reply is written, so reading
    /// this once the call has returned sees the final count.
    pub fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }

    /// The request targets — the `/path?query` portion of each request line (e.g.
    /// `/quote?symbol=%5EGSPC`) — in arrival order, one per accepted connection. Each is
    /// recorded *before* its reply is written, so reading this once the call has returned
    /// sees them all. Lets a test assert the endpoint path/query the adapter actually
    /// built, not merely that *a* request reached the mock — so a typo in a `*_PATH` const
    /// fails the test rather than silently hitting the mock anyway.
    pub fn request_targets(&self) -> Vec<String> {
        self.targets
            .lock()
            .expect("targets mutex is not poisoned")
            .clone()
    }

    /// The path component of each request target — the `/path` with any `?query` stripped
    /// — in arrival order. For a test asserting the endpoint path *exactly*: a suffix
    /// regression like `/quote-v2` fails `assert_eq!(request_paths(), ["/quote"])` where it
    /// would slip past a `starts_with`. Use [`Self::request_targets`] when the query
    /// matters (a per-call var reaching the wire).
    pub fn request_paths(&self) -> Vec<String> {
        self.request_targets()
            .into_iter()
            .map(|t| match t.split_once('?') {
                Some((path, _)) => path.to_string(),
                None => t,
            })
            .collect()
    }
}

fn serve_script(
    listener: TcpListener,
    script: Vec<Canned>,
    counter: Arc<AtomicUsize>,
    recorder: Arc<Mutex<Vec<String>>>,
) {
    for canned in script {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(_) => break,
        };
        counter.fetch_add(1, Ordering::SeqCst);
        let head = drain_request(&mut stream);
        if let Some(target) = request_target(&head) {
            recorder
                .lock()
                .expect("targets mutex is not poisoned")
                .push(target);
        }
        let _ = write_canned(&mut stream, canned);
        // `stream` drops at the end of the iteration: for `DropBody` that is the
        // mid-body EOF; for `Reply` the `Content-Length` body is already complete.
    }
}

/// Read the inbound request up to its header terminator, returning the bytes seen so the
/// caller can record the request target. Reading to the terminator also lets the client's
/// write complete before we reply. Best-effort and bounded by a read timeout — the GET
/// adapters issue header-only requests, and a POST body (BLS / Tavily) is tiny enough to
/// sit in the socket buffer unread without blocking the client's write (the request line
/// we record precedes the body, so an unread body costs no target detail).
fn drain_request(stream: &mut TcpStream) -> Vec<u8> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut seen: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                seen.extend_from_slice(&buf[..n]);
                if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    seen
}

/// Extract the request target — the second token of the request line (e.g. the
/// `/quote?symbol=%5EGSPC` in `GET /quote?symbol=%5EGSPC HTTP/1.1`) — from the raw
/// request head, if present.
fn request_target(head: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(head).ok()?;
    let first_line = text.lines().next()?;
    first_line.split_whitespace().nth(1).map(str::to_string)
}

fn write_canned(stream: &mut TcpStream, canned: Canned) -> std::io::Result<()> {
    match canned {
        Canned::Reply {
            status,
            headers,
            body,
        } => {
            let mut resp = format!(
                "HTTP/1.1 {status} STATUS\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (k, v) in headers {
                resp.push_str(k);
                resp.push_str(": ");
                resp.push_str(v);
                resp.push_str("\r\n");
            }
            resp.push_str("\r\n");
            resp.push_str(body);
            stream.write_all(resp.as_bytes())
        }
        Canned::DropBody {
            content_length,
            partial,
        } => {
            let resp = format!(
                "HTTP/1.1 200 STATUS\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n{partial}"
            );
            stream.write_all(resp.as_bytes())
        }
    }
}
