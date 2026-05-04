//! Streaming ISA opcode parser.
//!
//! Consumes a token stream from llama-server's `/v1/completions` SSE
//! response and emits a sequence of [`Statement`] events to the daemon.
//! State-machine driven; no allocation per token outside event payloads.
//!
//! The grammar (`grammar/isa.gbnf`) already constrains what the model can
//! emit. This parser does **not** re-validate the grammar — it assumes the
//! token stream is grammar-conformant and just classifies statement
//! boundaries. A grammar violation here means a bug in the daemon's
//! request setup (missing or wrong `grammar` field).

use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    Read,
    Write,
    Call,
    Yield,
    Fork,
    Wait,
    Loop,
    Break,
    Halt,
    Think,
    Commit,
    Fault,
    Policy,
    /// 14th opcode — daemon-injected after KV compaction to rehydrate ISA
    /// state (loop depth, pending results, open forks). Informational only:
    /// no transition, no daemon response needed.
    State,
}

impl Opcode {
    /// Returns true if this opcode requires a daemon-injected response
    /// (a `<|result|>…<|/result|>` or `<|ack|>` block) before the model
    /// can continue. The daemon pauses sampling, executes the side effect,
    /// injects the response, and resumes.
    pub fn needs_response(self) -> bool {
        matches!(
            self,
            Opcode::Read
                | Opcode::Write
                | Opcode::Call
                | Opcode::Wait
                | Opcode::Fault
                | Opcode::Policy
        )
    }

    /// Returns true if this opcode was injected by the daemon (not emitted
    /// by the LLM). State is daemon-injected after KV compaction.
    pub fn is_daemon_injected(self) -> bool {
        matches!(self, Opcode::State)
    }
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Opcode::Read => "read",
            Opcode::Write => "write",
            Opcode::Call => "call",
            Opcode::Yield => "yield",
            Opcode::Fork => "fork",
            Opcode::Wait => "wait",
            Opcode::Loop => "loop",
            Opcode::Break => "break",
            Opcode::Halt => "halt",
            Opcode::Think => "think",
            Opcode::Commit => "commit",
            Opcode::Fault => "fault",
            Opcode::Policy => "policy",
            Opcode::State => "state",
        };
        write!(f, "{s}")
    }
}

/// One classified statement emitted from a single line of model output.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Read { fd: u32, len: u32 },
    Write { fd: u32, payload: Value },
    Call { cart: String, method: String, args: Value },
    Yield,
    Fork { goal: String },
    Wait { fd: u32 },
    LoopOpen { goal: String },
    Break,
    Halt { status: HaltStatus },
    Think { body: String },
    Commit { payload: Value },
    Fault { payload: Value },
    Policy,
    /// Daemon-injected ISA state preamble after KV compaction.
    /// Payload: `{"loop_depth":N, "pending_results":["fdN"], "open_forks":[], "open_loops":["goal"]}`
    State { payload: Value },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltStatus {
    Success,
    Failure,
    Partial,
}

impl fmt::Display for HaltStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            HaltStatus::Success => "success",
            HaltStatus::Failure => "failure",
            HaltStatus::Partial => "partial",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("malformed statement: {0}")]
    Malformed(String),
    #[error("invalid integer in {field}: {value}")]
    IntFormat { field: String, value: String },
    #[error("invalid JSON payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown halt status: {0}")]
    HaltStatus(String),
}

/// Parse a single complete line of model output into a [`Statement`].
///
/// The line includes the opcode token but not a trailing newline. Multi-line
/// constructs (`<|think|>…<|/think|>`, `<|loop|>…`/`<|break|>`) are emitted
/// as their open/close events separately by [`OpcodeStream`].
pub fn parse_statement(line: &str) -> Result<Statement, ParseError> {
    let line = line.trim_end_matches('\n');

    if let Some(rest) = strip_prefix(line, "<|read|>") {
        return parse_read(rest);
    }
    if let Some(rest) = strip_prefix(line, "<|write|>") {
        return parse_write(rest);
    }
    if let Some(rest) = strip_prefix(line, "<|call|>") {
        return parse_call(rest);
    }
    if line == "<|yield|>" {
        return Ok(Statement::Yield);
    }
    if let Some(rest) = strip_prefix(line, "<|fork|>") {
        return parse_fork(rest);
    }
    if let Some(rest) = strip_prefix(line, "<|wait|>") {
        return parse_wait(rest);
    }
    if let Some(rest) = strip_prefix(line, "<|loop|>") {
        return parse_loop_open(rest);
    }
    if line == "<|break|>" {
        return Ok(Statement::Break);
    }
    if let Some(rest) = strip_prefix(line, "<|halt|>") {
        return parse_halt(rest);
    }
    if let Some(rest) = strip_prefix(line, "<|commit|>") {
        return Ok(Statement::Commit {
            payload: serde_json::from_str(rest)?,
        });
    }
    if let Some(rest) = strip_prefix(line, "<|fault|>") {
        return Ok(Statement::Fault {
            payload: serde_json::from_str(rest)?,
        });
    }
    if line == "<|policy|>" {
        return Ok(Statement::Policy);
    }
    // <|state|>{...}<|/state|> — daemon-injected ISA state after compaction.
    if let Some(rest) = strip_prefix(line, "<|state|>") {
        return parse_state(rest);
    }

    Err(ParseError::Malformed(line.to_string()))
}

fn strip_prefix<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    s.strip_prefix(p)
}

fn parse_int(s: &str, field: &str) -> Result<u32, ParseError> {
    s.parse::<u32>().map_err(|_| ParseError::IntFormat {
        field: field.to_string(),
        value: s.to_string(),
    })
}

// `fd=N len=N` — both required.
fn parse_read(rest: &str) -> Result<Statement, ParseError> {
    // Expected shape: "fd=N len=N"
    let (fd_s, len_s) = parse_kvp_pair(rest, "fd", "len")?;
    Ok(Statement::Read {
        fd: parse_int(fd_s, "fd")?,
        len: parse_int(len_s, "len")?,
    })
}

// `fd=N <json>` — fd then a single JSON value.
fn parse_write(rest: &str) -> Result<Statement, ParseError> {
    let trimmed = rest.trim_start();
    let after_fd = strip_prefix(trimmed, "fd=").ok_or_else(|| {
        ParseError::Malformed(format!("write missing fd=: {rest}"))
    })?;
    let space = after_fd.find(' ').ok_or_else(|| {
        ParseError::Malformed(format!("write missing space after fd: {rest}"))
    })?;
    let fd_s = &after_fd[..space];
    let json_s = after_fd[space..].trim_start();
    Ok(Statement::Write {
        fd: parse_int(fd_s, "fd")?,
        payload: serde_json::from_str(json_s)?,
    })
}

// `cart.method <json> <|/call|>` — stripped of opening; need to also strip
// the trailing close tag.
fn parse_call(rest: &str) -> Result<Statement, ParseError> {
    let body = rest
        .trim_end()
        .strip_suffix("<|/call|>")
        .ok_or_else(|| ParseError::Malformed(format!("call missing <|/call|>: {rest}")))?
        .trim_end();
    let dot = body.find('.').ok_or_else(|| {
        ParseError::Malformed(format!("call missing '.' between cart and method: {body}"))
    })?;
    let cart = &body[..dot];
    let after_dot = &body[dot + 1..];
    // Tolerate missing args: `cart.method <|/call|>` → default to `{}`.
    // Models without GBNF grammar enforcement may omit args for no-arg
    // methods (e.g. `sim_world.observe <|/call|>`).
    let (method, args_s) = match after_dot.find(char::is_whitespace) {
        Some(space) => (&after_dot[..space], after_dot[space..].trim()),
        None => (after_dot.trim(), "{}"),
    };
    let args_s = if args_s.is_empty() { "{}" } else { args_s };
    Ok(Statement::Call {
        cart: cart.to_string(),
        method: method.to_string(),
        args: serde_json::from_str(args_s)?,
    })
}

// `goal=<label>`
fn parse_fork(rest: &str) -> Result<Statement, ParseError> {
    let goal = strip_prefix(rest, "goal=")
        .ok_or_else(|| ParseError::Malformed(format!("fork missing goal=: {rest}")))?
        .trim()
        .to_string();
    Ok(Statement::Fork { goal })
}

fn parse_wait(rest: &str) -> Result<Statement, ParseError> {
    let fd_s = strip_prefix(rest, "fd=")
        .ok_or_else(|| ParseError::Malformed(format!("wait missing fd=: {rest}")))?
        .trim();
    Ok(Statement::Wait {
        fd: parse_int(fd_s, "fd")?,
    })
}

fn parse_loop_open(rest: &str) -> Result<Statement, ParseError> {
    let goal = strip_prefix(rest, "goal=")
        .ok_or_else(|| ParseError::Malformed(format!("loop missing goal=: {rest}")))?
        .trim()
        .to_string();
    Ok(Statement::LoopOpen { goal })
}

fn parse_halt(rest: &str) -> Result<Statement, ParseError> {
    let status_s = strip_prefix(rest, "status=")
        .ok_or_else(|| ParseError::Malformed(format!("halt missing status=: {rest}")))?
        .trim();
    let status = match status_s {
        "success" => HaltStatus::Success,
        "failure" => HaltStatus::Failure,
        "partial" => HaltStatus::Partial,
        other => return Err(ParseError::HaltStatus(other.to_string())),
    };
    Ok(Statement::Halt { status })
}

// `<|state|>{...}<|/state|>` — strip close tag, parse JSON.
fn parse_state(rest: &str) -> Result<Statement, ParseError> {
    let body = rest
        .trim_end()
        .strip_suffix("<|/state|>")
        .ok_or_else(|| ParseError::Malformed(format!("state missing <|/state|>: {rest}")))?
        .trim();
    Ok(Statement::State {
        payload: serde_json::from_str(body)?,
    })
}

// `key1=v1 key2=v2`
fn parse_kvp_pair<'a>(
    s: &'a str,
    k1: &str,
    k2: &str,
) -> Result<(&'a str, &'a str), ParseError> {
    let first_prefix = format!("{k1}=");
    let second_prefix = format!(" {k2}=");
    let after_k1 = strip_prefix(s, &first_prefix).ok_or_else(|| {
        ParseError::Malformed(format!("missing {k1}=: {s}"))
    })?;
    let split = after_k1.find(&*second_prefix).ok_or_else(|| {
        ParseError::Malformed(format!("missing {k2}=: {s}"))
    })?;
    let v1 = &after_k1[..split];
    let v2 = &after_k1[split + second_prefix.len()..];
    Ok((v1.trim(), v2.trim()))
}

/// Stateful scanner over a stream of model-emitted text. Tracks loop depth
/// and accumulates partial think-block bodies. Feed bytes via [`Self::feed`];
/// drain complete events via [`Self::next_statement`].
pub struct OpcodeStream {
    buf: String,
    in_think: bool,
    think_acc: String,
    loop_depth: u32,
}

impl OpcodeStream {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            in_think: false,
            think_acc: String::new(),
            loop_depth: 0,
        }
    }

    pub fn loop_depth(&self) -> u32 {
        self.loop_depth
    }

    /// Append raw text from the SSE stream.
    pub fn feed(&mut self, chunk: &str) {
        self.buf.push_str(chunk);
    }

    /// Return the next complete classified statement, or `None` if more
    /// text is needed. Statements are line-terminated; multi-line think
    /// blocks accumulate into a single `Statement::Think` event.
    pub fn next_statement(&mut self) -> Option<Result<Statement, ParseError>> {
        loop {
            if self.in_think {
                if let Some(end) = self.buf.find("<|/think|>") {
                    let body_part = &self.buf[..end];
                    self.think_acc.push_str(body_part);
                    let after_close = end + "<|/think|>".len();
                    // Skip an optional newline after </think>.
                    let drop_to = if self.buf[after_close..].starts_with('\n') {
                        after_close + 1
                    } else {
                        after_close
                    };
                    let stmt = Statement::Think {
                        body: std::mem::take(&mut self.think_acc),
                    };
                    self.buf.drain(..drop_to);
                    self.in_think = false;
                    return Some(Ok(stmt));
                } else {
                    self.think_acc.push_str(&self.buf);
                    self.buf.clear();
                    return None;
                }
            }

            // Outside think mode: find next newline.
            let nl = match self.buf.find('\n') {
                Some(i) => i,
                None => return None,
            };
            let line = self.buf[..nl].to_string();
            self.buf.drain(..nl + 1);

            if line.is_empty() {
                continue;
            }

            // think open is special — body may span multiple lines.
            if let Some(after_open) = line.strip_prefix("<|think|>") {
                if let Some(close) = after_open.find("<|/think|>") {
                    // Single-line think.
                    let body = after_open[..close].to_string();
                    return Some(Ok(Statement::Think { body }));
                } else {
                    self.in_think = true;
                    self.think_acc.push_str(after_open);
                    self.think_acc.push('\n');
                    continue;
                }
            }

            let stmt = parse_statement(&line);
            if let Ok(ref s) = stmt {
                match s {
                    Statement::LoopOpen { .. } => self.loop_depth += 1,
                    Statement::Break => {
                        self.loop_depth = self.loop_depth.saturating_sub(1);
                    }
                    // Rehydrate ISA state from daemon-injected <|state|> preamble
                    // after KV compaction. This restores loop depth so the grammar
                    // state machine stays coherent across compaction boundaries.
                    Statement::State { payload } => {
                        if let Some(depth) = payload.get("loop_depth").and_then(|v| v.as_u64()) {
                            self.loop_depth = depth as u32;
                        }
                    }
                    _ => {}
                }
            }
            return Some(stmt);
        }
    }
}

impl Default for OpcodeStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_read() {
        let s = parse_statement("<|read|>fd=3 len=128").unwrap();
        assert_eq!(s, Statement::Read { fd: 3, len: 128 });
    }

    #[test]
    fn parses_write() {
        let s = parse_statement(r#"<|write|>fd=1 {"msg":"ready"}"#).unwrap();
        assert_eq!(
            s,
            Statement::Write {
                fd: 1,
                payload: json!({"msg":"ready"})
            }
        );
    }

    #[test]
    fn parses_call_with_close_tag() {
        let s = parse_statement(
            r#"<|call|>roclaw.forward {"left":150,"right":150} <|/call|>"#,
        )
        .unwrap();
        assert_eq!(
            s,
            Statement::Call {
                cart: "roclaw".into(),
                method: "forward".into(),
                args: json!({"left":150,"right":150})
            }
        );
    }

    #[test]
    fn parses_loop_open_break_halt() {
        assert_eq!(
            parse_statement("<|loop|>goal=navigate").unwrap(),
            Statement::LoopOpen {
                goal: "navigate".into()
            }
        );
        assert_eq!(parse_statement("<|break|>").unwrap(), Statement::Break);
        assert_eq!(
            parse_statement("<|halt|>status=success").unwrap(),
            Statement::Halt {
                status: HaltStatus::Success
            }
        );
    }

    #[test]
    fn parses_fork_yield_commit_fault_policy() {
        assert_eq!(parse_statement("<|yield|>").unwrap(), Statement::Yield);
        assert_eq!(parse_statement("<|policy|>").unwrap(), Statement::Policy);
        assert_eq!(
            parse_statement("<|fork|>goal=compact").unwrap(),
            Statement::Fork {
                goal: "compact".into()
            }
        );
        assert_eq!(
            parse_statement(r#"<|commit|>{"plan":"x"}"#).unwrap(),
            Statement::Commit {
                payload: json!({"plan":"x"})
            }
        );
        assert_eq!(
            parse_statement(r#"<|fault|>{"kind":"oops"}"#).unwrap(),
            Statement::Fault {
                payload: json!({"kind":"oops"})
            }
        );
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse_statement("<|halt|>status=bogus").is_err());
        assert!(parse_statement("<|read|>fd=x len=128").is_err());
        assert!(parse_statement("garbage").is_err());
    }

    #[test]
    fn stream_emits_statements_line_by_line() {
        let mut st = OpcodeStream::new();
        st.feed("<|read|>fd=3 len=64\n");
        st.feed("<|halt|>status=success\n");
        let s1 = st.next_statement().unwrap().unwrap();
        assert_eq!(s1, Statement::Read { fd: 3, len: 64 });
        let s2 = st.next_statement().unwrap().unwrap();
        assert_eq!(s2, Statement::Halt { status: HaltStatus::Success });
        assert!(st.next_statement().is_none());
    }

    #[test]
    fn stream_handles_multi_line_think() {
        let mut st = OpcodeStream::new();
        st.feed("<|think|>line one\n");
        assert!(st.next_statement().is_none());
        st.feed("line two<|/think|>\n");
        let s = st.next_statement().unwrap().unwrap();
        assert!(matches!(s, Statement::Think { .. }));
        if let Statement::Think { body } = s {
            assert!(body.contains("line one"));
            assert!(body.contains("line two"));
        }
    }

    #[test]
    fn loop_depth_tracked() {
        let mut st = OpcodeStream::new();
        st.feed("<|loop|>goal=x\n");
        st.next_statement();
        assert_eq!(st.loop_depth(), 1);
        st.feed("<|loop|>goal=y\n");
        st.next_statement();
        assert_eq!(st.loop_depth(), 2);
        st.feed("<|break|>\n<|break|>\n");
        st.next_statement();
        st.next_statement();
        assert_eq!(st.loop_depth(), 0);
    }

    #[test]
    fn opcode_response_classification() {
        assert!(Opcode::Read.needs_response());
        assert!(Opcode::Call.needs_response());
        assert!(!Opcode::Yield.needs_response());
        assert!(!Opcode::Halt.needs_response());
        assert!(!Opcode::Loop.needs_response());
        assert!(!Opcode::State.needs_response());
        assert!(Opcode::State.is_daemon_injected());
    }

    #[test]
    fn parses_state() {
        let s = parse_statement(
            r#"<|state|>{"loop_depth":2,"pending_results":["fd3"],"open_forks":[]}<|/state|>"#,
        )
        .unwrap();
        match s {
            Statement::State { payload } => {
                assert_eq!(payload["loop_depth"], 2);
                assert_eq!(payload["pending_results"][0], "fd3");
            }
            other => panic!("expected State, got {:?}", other),
        }
    }

    #[test]
    fn stream_rehydrates_loop_depth_from_state() {
        let mut st = OpcodeStream::new();
        assert_eq!(st.loop_depth(), 0);
        // Simulate compaction: daemon injects state preamble with loop_depth=3
        st.feed(r#"<|state|>{"loop_depth":3,"pending_results":[],"open_forks":[]}<|/state|>"#);
        st.feed("\n");
        let s = st.next_statement().unwrap().unwrap();
        assert!(matches!(s, Statement::State { .. }));
        assert_eq!(st.loop_depth(), 3);
        // Now a break should decrement from 3
        st.feed("<|break|>\n");
        st.next_statement();
        assert_eq!(st.loop_depth(), 2);
    }
}
