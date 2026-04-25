//! I/O daemon — the streaming SSE consumer that drives ISA dispatch.
//!
//! Owns one TCP connection to llama-server's `/v1/completions` per task.
//! On each model-emitted line, classifies the opcode via [`crate::parser`],
//! routes to the cartridge dispatcher, injects the result back into the
//! prompt, and resumes generation.
//!
//! v0.01 design notes:
//! - **Per-request grammar.** The bootloader does NOT set a server-side
//!   grammar; the daemon includes `grammar` in each `/v1/completions` body.
//!   Lets boot use no grammar (just `<|ready|>`) while subsequent calls are
//!   ISA-constrained.
//! - **Stop-and-inject loop.** Each request runs until the model emits a
//!   "needs response" opcode end-marker. Daemon stops the request, runs
//!   the cartridge, appends the result to the prompt, re-POSTs with
//!   `cache_prompt: true` so the KV cache is preserved.
//! - **Loop-depth tracking.** Mirrors the grammar's invariant: `<|halt|>`
//!   only at depth 0; `<|break|>` only inside loops. Daemon-side guard for
//!   the case where the grammar slips.

use crate::cartridge::CartridgeRegistry;
use crate::cloud::{CloudConfig, Message as CloudMessage};
use crate::dispatch::{dispatch as dispatch_call, DispatchError};
use crate::parser::{parse_statement, HaltStatus, OpcodeStream, Statement};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

/// Per-task daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// llama-server base URL, e.g. `http://127.0.0.1:8080`.
    pub server_url: String,
    /// Path to `grammar/isa.gbnf`, read once and embedded in each request.
    pub grammar_path: String,
    /// Cartridge root, scanned at startup.
    pub cart_root: String,
    /// Wall-clock budget per task. `<|halt|>partial` is forced past this.
    pub task_budget: Duration,
    /// Max nested loop depth before the daemon raises a fault. Mirrors
    /// `isa-spec.md` §6 deferred items.
    pub max_loop_depth: u32,
    /// Default sampler temperature for ISA generation.
    pub temperature: f64,
    /// Cap on tokens per sub-request before forcing a checkpoint.
    pub max_predict_per_segment: u32,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            server_url: "http://127.0.0.1:8080".into(),
            grammar_path: "grammar/isa.gbnf".into(),
            cart_root: "cart".into(),
            task_budget: Duration::from_secs(600),
            max_loop_depth: 4,
            temperature: 0.2,
            max_predict_per_segment: 512,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompletionRequest {
    prompt: String,
    grammar: String,
    stream: bool,
    cache_prompt: bool,
    n_predict: u32,
    temperature: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamDelta {
    /// Latest text fragment produced by this SSE event.
    #[serde(default)]
    content: String,
    /// True if this is the terminating event for the request.
    #[serde(default)]
    stop: bool,
}

/// Outcome of running one task to completion.
#[derive(Debug)]
pub struct TaskOutcome {
    pub status: HaltStatus,
    pub steps: u32,
    pub final_prompt: String,
}

pub struct Daemon {
    pub cfg: DaemonConfig,
    pub registry: CartridgeRegistry,
    pub grammar: String,
    pub cloud: CloudConfig,
}

impl Daemon {
    pub fn new(cfg: DaemonConfig) -> Result<Self> {
        let grammar = std::fs::read_to_string(&cfg.grammar_path)
            .with_context(|| format!("reading grammar at {}", cfg.grammar_path))?;
        let registry = CartridgeRegistry::discover(&cfg.cart_root)
            .with_context(|| format!("discovering cartridges in {}", cfg.cart_root))?;
        let cloud = CloudConfig::from_env();
        log::info!(
            "iod ready: {} cartridges loaded, grammar={} bytes",
            registry.len(),
            grammar.len()
        );
        Ok(Self {
            cfg,
            registry,
            grammar,
            cloud,
        })
    }

    /// Run one task to `<|halt|>`. Drives the stop-and-inject loop.
    pub fn run_task(&self, user_goal: &str) -> Result<TaskOutcome> {
        let mut prompt = self.boot_prompt(user_goal);
        let mut stream = OpcodeStream::new();
        let mut local_context: Vec<CloudMessage> = vec![CloudMessage {
            role: "user".into(),
            content: user_goal.into(),
        }];
        let mut steps: u32 = 0;
        let started = std::time::Instant::now();

        loop {
            if started.elapsed() > self.cfg.task_budget {
                log::warn!("task budget exceeded; forcing halt");
                return Ok(TaskOutcome {
                    status: HaltStatus::Partial,
                    steps,
                    final_prompt: prompt,
                });
            }

            let segment = self.complete_one_segment(&prompt)?;
            log::debug!("segment ({} bytes): {:?}", segment.len(), trim_for_log(&segment));
            prompt.push_str(&segment);
            stream.feed(&segment);

            // Drain all complete statements emitted by this segment.
            // Most segments emit exactly one, but think-blocks may not be
            // complete yet (waiting for `<|/think|>`).
            while let Some(stmt) = stream.next_statement() {
                let stmt = match stmt {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("parse error: {e}");
                        return Ok(TaskOutcome {
                            status: HaltStatus::Failure,
                            steps,
                            final_prompt: prompt,
                        });
                    }
                };
                steps += 1;
                log::info!("step {steps}: {stmt:?}");

                match self.handle_statement(&stmt, &mut prompt, &mut local_context)? {
                    StatementOutcome::Halt(s) => {
                        return Ok(TaskOutcome {
                            status: s,
                            steps,
                            final_prompt: prompt,
                        });
                    }
                    StatementOutcome::Continue => {}
                }

                if stream.loop_depth() > self.cfg.max_loop_depth {
                    log::warn!(
                        "loop depth {} exceeds max_loop_depth {}; raising fault",
                        stream.loop_depth(),
                        self.cfg.max_loop_depth
                    );
                    inject_result(
                        &mut prompt,
                        &json!({"error":"max_loop_depth_exceeded"}),
                    );
                }
            }
        }
    }

    fn handle_statement(
        &self,
        stmt: &Statement,
        prompt: &mut String,
        local_context: &mut Vec<CloudMessage>,
    ) -> Result<StatementOutcome> {
        match stmt {
            Statement::Read { fd, len } => {
                // v0.01: only fd 0 (stdin) is hooked; everything else stubs.
                let result = if *fd == 0 {
                    json!({"ok": true, "bytes": 0, "data": ""})
                } else {
                    json!({"ok": false, "error": format!("unmapped fd {fd}"), "len_requested": len})
                };
                inject_result(prompt, &result);
            }
            Statement::Write { fd, payload } => {
                // fd 1 → daemon stdout; fd 2 → stderr; everything else stubs.
                match fd {
                    1 => println!("{payload}"),
                    2 => eprintln!("{payload}"),
                    _ => log::info!("write fd={fd} (stubbed): {payload}"),
                }
                inject_ack(prompt);
            }
            Statement::Call { cart, method, args } => {
                let result = match dispatch_call(&self.registry, cart, method, args) {
                    Ok(v) => v,
                    Err(DispatchError::SchemaViolation { errors, .. }) => {
                        json!({"ok": false, "error": "schema_violation", "details": errors})
                    }
                    Err(e) => json!({"ok": false, "error": format!("{e}")}),
                };
                local_context.push(CloudMessage {
                    role: "assistant".into(),
                    content: format!("call {cart}.{method} → {result}"),
                });
                inject_result(prompt, &result);
            }
            Statement::Wait { fd } => {
                inject_result(prompt, &json!({"ok": false, "error": format!("wait fd={fd} not implemented in v0.01")}));
            }
            Statement::Fault { payload } => {
                let result = self.handle_fault(payload, local_context);
                inject_result(prompt, &result);
            }
            Statement::Policy => {
                // v0.01: return the static set of opcodes + cartridges.
                let allowed: Vec<String> = self
                    .registry
                    .names()
                    .iter()
                    .map(|n| format!("call.{n}.*"))
                    .collect();
                let result = json!({
                    "allowed_opcodes": ["read","write","call","yield","fork","wait","loop","break","halt","think","commit","fault","policy"],
                    "allowed_cartridge_calls": allowed,
                });
                inject_result(prompt, &result);
            }
            Statement::LoopOpen { goal } => {
                log::info!("loop open: goal={goal}");
            }
            Statement::Break => {
                log::info!("break");
            }
            Statement::Halt { status } => {
                return Ok(StatementOutcome::Halt(*status));
            }
            Statement::Yield => {
                // v0.01: scheduler isn't implemented; yield is logged.
                log::info!("yield (no-op in v0.01)");
            }
            Statement::Fork { goal } => {
                log::info!("fork goal={goal} (logged only in v0.01)");
            }
            Statement::Think { body } => {
                log::debug!("think: {} chars", body.len());
            }
            Statement::Commit { payload } => {
                log::info!("commit: {payload}");
            }
        }
        Ok(StatementOutcome::Continue)
    }

    fn handle_fault(
        &self,
        payload: &Value,
        local_context: &[CloudMessage],
    ) -> Value {
        let needs_cloud = payload
            .get("needs_cloud")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !needs_cloud {
            return json!({"handled": false, "reason": "no needs_cloud flag; v0.01 has no other handlers"});
        }
        match crate::cloud::forward_fault(&self.cloud, local_context, payload) {
            Ok(reply) => json!({"cloud_response": reply}),
            Err(e) => json!({"handled": false, "error": format!("cloud fallback failed: {e}")}),
        }
    }

    fn boot_prompt(&self, user_goal: &str) -> String {
        format!(
            "You are LLM-OS v0.01. Your output MUST conform to the ISA grammar at all times.\n\
             Available cartridges: {cartridges}\n\
             Goal from user: {goal}\n\n\
             Begin emitting ISA opcodes:\n",
            cartridges = self.registry.names().join(", "),
            goal = user_goal,
        )
    }

    /// POST one streaming request to `/v1/completions`. Returns the full
    /// segment text (concatenation of all SSE `content` fragments).
    fn complete_one_segment(&self, prompt: &str) -> Result<String> {
        let body = CompletionRequest {
            prompt: prompt.to_string(),
            grammar: self.grammar.clone(),
            stream: true,
            cache_prompt: true,
            n_predict: self.cfg.max_predict_per_segment,
            temperature: self.cfg.temperature,
        };

        let url = format!("{}/v1/completions", self.cfg.server_url);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(120))
            .build();
        let resp = agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| anyhow!("llama-server POST failed: {e}"))?;

        let reader = BufReader::new(resp.into_reader());
        let mut acc = String::new();
        for line in reader.lines() {
            let line = line.context("reading SSE line")?;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let payload = line.strip_prefix("data: ").unwrap_or(line);
            if payload == "[DONE]" {
                break;
            }
            let parsed: StreamDelta = match serde_json::from_str(payload) {
                Ok(v) => v,
                Err(_) => continue, // tolerate non-JSON keep-alives.
            };
            if !parsed.content.is_empty() {
                acc.push_str(&parsed.content);
            }
            if parsed.stop {
                break;
            }
        }
        Ok(acc)
    }
}

enum StatementOutcome {
    Halt(HaltStatus),
    Continue,
}

fn inject_result(prompt: &mut String, value: &Value) {
    prompt.push_str("<|result|>");
    prompt.push_str(&serde_json::to_string(value).unwrap_or_else(|_| "null".into()));
    prompt.push_str("<|/result|>\n");
}

fn inject_ack(prompt: &mut String) {
    prompt.push_str("<|ack|>\n");
}

fn trim_for_log(s: &str) -> String {
    if s.len() <= 200 {
        s.to_string()
    } else {
        format!("{}…(+{} bytes)", &s[..200], s.len() - 200)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_result_appends_wrapped_json() {
        let mut p = String::from("hello\n");
        inject_result(&mut p, &json!({"ok": true}));
        assert!(p.ends_with("<|result|>{\"ok\":true}<|/result|>\n"));
    }

    #[test]
    fn inject_ack_appends_token_with_newline() {
        let mut p = String::new();
        inject_ack(&mut p);
        assert_eq!(p, "<|ack|>\n");
    }

    #[test]
    fn boot_prompt_lists_cartridges_and_goal() {
        // Construct a Daemon-shaped minimal stand-in for boot_prompt formatting.
        // We avoid Daemon::new because that needs a real grammar file on disk.
        let cfg = DaemonConfig::default();
        let registry = CartridgeRegistry::new();
        let d = Daemon {
            cfg,
            registry,
            grammar: String::new(),
            cloud: CloudConfig::from_env(),
        };
        let p = d.boot_prompt("plan dinner");
        assert!(p.contains("LLM-OS v0.01"));
        assert!(p.contains("plan dinner"));
    }
}
