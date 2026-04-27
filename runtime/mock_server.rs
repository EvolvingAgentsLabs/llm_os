//! Mock llama-server for tests.
//!
//! Bare-minimum HTTP server speaking the subset of llama-server's
//! `/v1/completions` and `/tokenize` endpoints that the daemon uses.
//! Lets every part of the daemon — segment loop, cartridge dispatch,
//! result-block reinjection, preemption, multi-task — be exercised
//! against scripted responses, with no real model required.
//!
//! Usage in tests:
//!
//! ```no_run
//! use llm_os_runtime::mock_server::{MockServer, ScriptedResponse};
//! let mock = MockServer::new();
//! mock.push(ScriptedResponse::completion("<|read|>fd=3 len=4\n"));
//! mock.push(ScriptedResponse::completion("<|halt|>status=success\n"));
//! let port = mock.start();
//! // Now point Daemon at http://127.0.0.1:{port} and run a task.
//! ```
//!
//! What this is NOT:
//! - A general-purpose HTTP server. We hand-roll the smallest fragment
//!   of HTTP/1.1 we need.
//! - A faithful llama-server emulator. Sampler params, grammar, and
//!   `cache_prompt` are accepted-and-ignored.
//! - Production code — `Mutex<VecDeque>` for the response queue is fine
//!   for test concurrency but not for serving real workloads.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// One pre-canned response the mock will hand out in FIFO order.
#[derive(Debug, Clone)]
pub enum ScriptedResponse {
    /// One full completion stream. The daemon sees this as the body of
    /// one `/v1/completions` POST. The string is split into a single
    /// SSE chunk for simplicity; tests that need multi-chunk streaming
    /// can use [`ScriptedResponse::CompletionChunks`].
    Completion(String),
    /// Stream multiple SSE chunks before `[DONE]`. Useful for tests that
    /// exercise the daemon's multi-chunk accumulation.
    CompletionChunks(Vec<String>),
    /// Tokenize response — the next POST to `/tokenize` returns these
    /// token ids regardless of input.
    Tokenize(Vec<i64>),
}

impl ScriptedResponse {
    pub fn completion<S: Into<String>>(s: S) -> Self {
        ScriptedResponse::Completion(s.into())
    }
    pub fn completion_chunks<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ScriptedResponse::CompletionChunks(iter.into_iter().map(Into::into).collect())
    }
    pub fn tokenize(ids: Vec<i64>) -> Self {
        ScriptedResponse::Tokenize(ids)
    }
}

#[derive(Default)]
pub struct MockServer {
    queue: Arc<Mutex<VecDeque<ScriptedResponse>>>,
    received_bodies: Arc<Mutex<Vec<String>>>,
}

impl MockServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a scripted response to the queue.
    pub fn push(&self, r: ScriptedResponse) {
        self.queue.lock().unwrap().push_back(r);
    }

    /// Inspect what the daemon POSTed to us (in arrival order). Useful
    /// for assertions like "daemon included `<|result|>...<|/result|>`
    /// in the second segment's prompt".
    pub fn received_bodies(&self) -> Vec<String> {
        self.received_bodies.lock().unwrap().clone()
    }

    /// Bind to an ephemeral port, spawn the listener thread, return the
    /// port number. Caller passes `http://127.0.0.1:{port}` to the
    /// daemon.
    pub fn start(&self) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local_addr").port();
        let queue = Arc::clone(&self.queue);
        let bodies = Arc::clone(&self.received_bodies);
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let stream = match incoming {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let q = Arc::clone(&queue);
                let b = Arc::clone(&bodies);
                thread::spawn(move || {
                    let _ = handle_one(stream, q, b);
                });
            }
        });
        port
    }
}

fn handle_one(
    mut stream: TcpStream,
    queue: Arc<Mutex<VecDeque<ScriptedResponse>>>,
    bodies: Arc<Mutex<Vec<String>>>,
) -> std::io::Result<()> {
    let peer = stream.try_clone()?;
    let mut reader = BufReader::new(peer);

    // Parse request line.
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let request_line = request_line.trim().to_string();
    let mut method = String::new();
    let mut path = String::new();
    let mut parts = request_line.split_whitespace();
    if let Some(m) = parts.next() {
        method = m.to_string();
    }
    if let Some(p) = parts.next() {
        path = p.to_string();
    }

    // Parse headers; capture Content-Length.
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
        {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }

    // Read body (if any).
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body_s = String::from_utf8_lossy(&body).to_string();
    bodies.lock().unwrap().push(body_s);

    // Pop next scripted response that matches the path semantically.
    let next = {
        let mut q = queue.lock().unwrap();
        if path.contains("tokenize") {
            // Look ahead for the next Tokenize response.
            let pos = q.iter().position(|r| matches!(r, ScriptedResponse::Tokenize(_)));
            pos.and_then(|i| q.remove(i))
        } else {
            // /v1/completions or unknown — pop the next Completion(Chunks).
            let pos = q.iter().position(|r| {
                matches!(
                    r,
                    ScriptedResponse::Completion(_) | ScriptedResponse::CompletionChunks(_)
                )
            });
            pos.and_then(|i| q.remove(i))
        }
    };

    if method != "POST" {
        // Anything other than POST gets a 405.
        let resp = "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes());
        return Ok(());
    }

    match next {
        Some(ScriptedResponse::Completion(text)) => {
            write_sse_stream(&mut stream, &[&text])?;
        }
        Some(ScriptedResponse::CompletionChunks(chunks)) => {
            let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
            write_sse_stream(&mut stream, &refs)?;
        }
        Some(ScriptedResponse::Tokenize(ids)) => {
            let json = serde_json::json!({"tokens": ids}).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(), json
            );
            stream.write_all(resp.as_bytes())?;
        }
        None => {
            // Queue starved — return 503 so tests fail loudly.
            let body = "{\"error\":\"mock queue empty\"}";
            let resp = format!(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            stream.write_all(resp.as_bytes())?;
        }
    }
    Ok(())
}

fn write_sse_stream(stream: &mut TcpStream, chunks: &[&str]) -> std::io::Result<()> {
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n";
    stream.write_all(header.as_bytes())?;
    for chunk in chunks {
        let event = serde_json::json!({"content": chunk, "stop": false});
        let line = format!("data: {}\n\n", event);
        write_chunked(stream, line.as_bytes())?;
    }
    let final_event = serde_json::json!({"content": "", "stop": true});
    let line = format!("data: {}\n\ndata: [DONE]\n\n", final_event);
    write_chunked(stream, line.as_bytes())?;
    write_chunked(stream, b"")?;
    Ok(())
}

fn write_chunked(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let size_line = format!("{:x}\r\n", payload.len());
    stream.write_all(size_line.as_bytes())?;
    if !payload.is_empty() {
        stream.write_all(payload)?;
    }
    stream.write_all(b"\r\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn mock_returns_503_when_queue_empty() {
        let mock = MockServer::new();
        let port = mock.start();
        thread::sleep(std::time::Duration::from_millis(50));
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"prompt":"hi"}"#;
        let req = format!(
            "POST /v1/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.contains("503"));
    }

    #[test]
    fn mock_serves_completion_then_records_body() {
        let mock = MockServer::new();
        mock.push(ScriptedResponse::completion("<|halt|>status=success\n"));
        let port = mock.start();
        thread::sleep(std::time::Duration::from_millis(50));
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"prompt":"hello world"}"#;
        let req = format!(
            "POST /v1/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.contains("200 OK"));
        assert!(buf.contains("<|halt|>"));
        let recorded = mock.received_bodies();
        assert_eq!(recorded.len(), 1);
        assert!(recorded[0].contains("hello world"));
    }

    #[test]
    fn mock_serves_tokenize() {
        let mock = MockServer::new();
        mock.push(ScriptedResponse::Tokenize(vec![10, 11, 12]));
        let port = mock.start();
        thread::sleep(std::time::Duration::from_millis(50));
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"content":"<|read|>"}"#;
        let req = format!(
            "POST /tokenize HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        assert!(buf.contains("200 OK"));
        assert!(buf.contains("\"tokens\":[10,11,12]"));
    }
}
