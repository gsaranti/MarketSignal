//! Throwaway localhost HTTP server for offline round-trip tests: a test scripts
//! a fixed sequence of canned replies, one per inbound connection, and points
//! the unit under test at [`MockHttp::base_url`].

#![cfg(test)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One canned reply for a single inbound connection.
pub enum Canned {
    /// A complete HTTP/1.1 reply with a matching `Content-Length`.
    Reply {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static str,
    },
    /// Declares `content_length` bytes but writes only `partial`, then closes,
    /// so the client's `Response::text()` errors. Served as HTTP 200 so the
    /// status is non-retryable: the only path to a second attempt is the body
    /// re-read branch.
    DropBody {
        content_length: usize,
        partial: &'static str,
    },
    /// Sleeps `for_ms` after reading the request, then serves `then`, so a test
    /// can pin a client's header-wait deadline. Size the sleep past the deadline
    /// under test; the worker's late write to the closed socket is ignored.
    Delay { for_ms: u64, then: Box<Canned> },
    /// Writes a `status` head and `partial` (close-delimited, no
    /// `Content-Length`), sleeps `for_ms`, then closes, so a test can pin a
    /// client's per-read idle deadline.
    StallMidBody {
        status: u16,
        partial: &'static str,
        for_ms: u64,
    },
    /// Writes one close-delimited chunk at a time, sleeping the paired delay
    /// after each. Distinguishes a total request deadline from a per-read idle
    /// deadline: every gap can stay under the idle bound while the complete
    /// response outlives it.
    DripBody {
        status: u16,
        chunks: Vec<(&'static str, u64)>,
    },
}

/// The listener is bound on the caller's thread, so the port is live before
/// `serve` returns, then handed to a detached worker that consumes the whole
/// script and exits. Size each script to the expected number of attempts so no
/// `accept()` blocks past the run.
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

    /// Connections accepted so far. Every increment lands before the matching
    /// reply is written, so reading this once the call has returned sees the
    /// final count.
    pub fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }

    /// The `/path?query` portion of each request line, in arrival order, each
    /// recorded before its reply is written. Lets a test assert the endpoint
    /// the adapter actually built, so a typo in a `*_PATH` const fails the test
    /// rather than silently hitting the mock anyway.
    pub fn request_targets(&self) -> Vec<String> {
        self.targets
            .lock()
            .expect("targets mutex is not poisoned")
            .clone()
    }

    /// [`Self::request_targets`] with any `?query` stripped, for asserting the
    /// endpoint path exactly (a `starts_with` would let `/quote-v2` slip past).
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
        // `stream` drops here: for `DropBody` that is the mid-body EOF.
    }
}

/// Reads the whole request (head plus any declared `Content-Length` body) so
/// the client's write completes before the reply: a model adapter's POST body
/// is large enough to fill the socket buffer and stall the client if left
/// unread. Best-effort, bounded by a read timeout.
fn drain_request(stream: &mut TcpStream) -> Vec<u8> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut seen: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let mut wanted: Option<usize> = None;
    loop {
        if let Some(total) = wanted {
            if seen.len() >= total {
                break;
            }
        }
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                seen.extend_from_slice(&buf[..n]);
                if wanted.is_none() {
                    if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                        wanted = Some(end + 4 + content_length(&seen[..end]));
                    }
                }
            }
            Err(_) => break,
        }
    }
    seen
}

/// The request head's declared `Content-Length`, or 0 when absent or unreadable.
fn content_length(head: &[u8]) -> usize {
    String::from_utf8_lossy(head)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0)
}

/// The second token of the request line (`/quote?symbol=%5EGSPC` in
/// `GET /quote?symbol=%5EGSPC HTTP/1.1`), if present.
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
        Canned::Delay { for_ms, then } => {
            std::thread::sleep(Duration::from_millis(for_ms));
            write_canned(stream, *then)
        }
        Canned::StallMidBody {
            status,
            partial,
            for_ms,
        } => {
            let head = format!("HTTP/1.1 {status} STATUS\r\nConnection: close\r\n\r\n{partial}");
            stream.write_all(head.as_bytes())?;
            stream.flush()?;
            std::thread::sleep(Duration::from_millis(for_ms));
            Ok(())
        }
        Canned::DripBody { status, chunks } => {
            let head = format!("HTTP/1.1 {status} STATUS\r\nConnection: close\r\n\r\n");
            stream.write_all(head.as_bytes())?;
            stream.flush()?;
            for (chunk, delay_after_ms) in chunks {
                stream.write_all(chunk.as_bytes())?;
                stream.flush()?;
                if delay_after_ms > 0 {
                    std::thread::sleep(Duration::from_millis(delay_after_ms));
                }
            }
            Ok(())
        }
    }
}
