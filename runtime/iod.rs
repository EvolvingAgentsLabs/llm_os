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
use crate::scheduler::{Budget, PreemptReason, Scheduler, SchedulerState};
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
    /// Optional path. If set, every task appends one JSONL line with the
    /// trace shape documented in `docs/fine-tune-recipe.md` §1. Used to
    /// build the v0.1 fine-tune dataset.
    pub trace_path: Option<String>,
    /// Hard token budget per task — preemptive scheduler forces
    /// `<|halt|>partial` when exceeded. v0.5+.
    pub max_tokens_per_task: u64,
    /// Optional task slot id used by the multi-task layer for KV
    /// isolation against llama-server's slot pool. v0.5+.
    pub slot_id: Option<i32>,
    /// Use Ollama's native `/api/generate` endpoint instead of
    /// llama-server's `/v1/completions`. Ollama does not enforce GBNF
    /// grammar at the sampler level, so the model must follow the ISA
    /// format via instruction-following alone.
    pub ollama: bool,
    /// Model name for Ollama backend (e.g. "qwen2.5:1.5b"). Ignored
    /// when `ollama` is false (llama-server loads model at startup).
    pub model: String,
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
            trace_path: None,
            max_tokens_per_task: 32_768,
            slot_id: None,
            ollama: false,
            model: String::new(),
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
    /// llama-server slot id for multi-task KV isolation (v0.5+).
    /// `-1` means "any free slot".
    #[serde(skip_serializing_if = "Option::is_none")]
    id_slot: Option<i32>,
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

/// Ollama `/api/generate` streaming response (NDJSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaStreamDelta {
    #[serde(default)]
    response: String,
    #[serde(default)]
    done: bool,
}

/// Ollama `/api/generate` request body.
#[derive(Debug, Clone, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    options: OllamaOptions,
    /// Disable thinking/reasoning mode for models that support it (e.g.
    /// Qwen 3.5). When true the model spends tokens on internal CoT and
    /// produces empty `response` fields — useless for ISA generation.
    /// Always sent as `false` for ISA generation.
    think: bool,
    /// Stop sequences — generation halts when any of these are emitted.
    /// Used to implement stop-and-inject: model stops at `<|/call|>` etc.
    /// so the daemon can dispatch handlers and inject results.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct OllamaOptions {
    num_predict: u32,
    temperature: f64,
    /// Repetition penalty — penalises tokens that have already appeared.
    /// Helps prevent small models from looping on the same call.
    repeat_penalty: f64,
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
        let mut raw_stream = String::new();
        let mut cartridges_used: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut sched = Scheduler::new(Budget {
            wall: self.cfg.task_budget,
            max_tokens: self.cfg.max_tokens_per_task,
            soft_preempt_ratio: 0.8,
        });
        // Track the last call signature to detect repetition loops
        // (common with small models on Ollama without GBNF).
        let mut last_call_sig: Option<String> = None;
        let mut repeat_count: u32 = 0;
        const MAX_REPEATS: u32 = 2;

        let outcome = loop {
            // v0.5: scheduler-driven preemption replaces the bare wall-clock
            // check. Wall + token budgets unified, soft warning at 80%.
            match sched.check() {
                SchedulerState::HardPreempt { reason } => {
                    log::warn!(
                        "scheduler hard-preempt ({:?}): forcing partial halt at step {steps}",
                        reason
                    );
                    // Inject a partial-halt directly into the prompt so
                    // the trace shows the task ended at OS request, not
                    // at model preference.
                    prompt.push_str("<|halt|>status=partial\n");
                    raw_stream.push_str("<|halt|>status=partial\n");
                    break TaskOutcome {
                        status: HaltStatus::Partial,
                        steps,
                        final_prompt: prompt.clone(),
                    };
                }
                SchedulerState::SoftPreempt | SchedulerState::Run => {}
            }

            let segment = self.complete_one_segment(&prompt)?;
            // Ollama stop sequences are unreliable — the model may
            // hallucinate daemon-injected blocks (<|result|>…<|/result|>
            // and <|ack|>). Strip them so the parser only sees opcodes
            // that the model intended to emit.
            let segment = strip_daemon_blocks(&segment);
            // Ollama mode: truncate at the first <|call|>…<|/call|> so the
            // daemon dispatches it and injects the real result before the
            // model continues. Without this the model hallucinate all steps
            // in one segment because stop sequences don't fire mid-stream.
            let segment = if self.cfg.ollama {
                truncate_at_first_call(&segment)
            } else {
                segment
            };
            log::debug!("segment ({} bytes): {:?}", segment.len(), trim_for_log(&segment));
            sched.account_segment_bytes(segment.len());
            prompt.push_str(&segment);
            raw_stream.push_str(&segment);
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
                            final_prompt: prompt.clone(),
                        });
                    }
                };
                steps += 1;
                log::info!("step {steps}: {stmt:?}");
                if let Statement::Call { cart, method, args } = &stmt {
                    cartridges_used.insert(cart.clone());
                    // Detect repetition loops: if the same call signature
                    // repeats MAX_REPEATS times, inject an error and nudge
                    // the model to write output and halt.
                    let sig = format!("{cart}.{method}:{args}");
                    if last_call_sig.as_deref() == Some(&sig) {
                        repeat_count += 1;
                        if repeat_count >= MAX_REPEATS {
                            log::warn!(
                                "repeat loop detected: {sig} called {} times; nudging model",
                                repeat_count + 1
                            );
                            inject_result(
                                &mut prompt,
                                &json!({"error": "repeated call detected — write your output and halt"}),
                            );
                            raw_stream.push_str("<|result|>{\"error\":\"repeated call\"}<|/result|>\n");
                            continue;
                        }
                    } else {
                        last_call_sig = Some(sig);
                        repeat_count = 0;
                    }
                }
                // Non-call statements (Think, Write, etc.) do NOT reset
                // repeat tracking — the model often inserts Think blocks
                // between repeated calls: Think → Call → Think → Call(same).

                match self.handle_statement(&stmt, &mut prompt, &mut local_context)? {
                    StatementOutcome::Halt(s) => {
                        let outcome = TaskOutcome {
                            status: s,
                            steps,
                            final_prompt: prompt.clone(),
                        };
                        return self.write_trace(
                            user_goal,
                            &outcome,
                            &raw_stream,
                            &cartridges_used,
                            started.elapsed(),
                        )
                        .map(|_| outcome);
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
        };

        // Write trace for budget-exceeded / parse-failure exits too.
        let _ = self.write_trace(
            user_goal,
            &outcome,
            &raw_stream,
            &cartridges_used,
            started.elapsed(),
        );
        Ok(outcome)
    }

    fn write_trace(
        &self,
        goal: &str,
        outcome: &TaskOutcome,
        raw_stream: &str,
        cartridges: &std::collections::BTreeSet<String>,
        wall: Duration,
    ) -> Result<()> {
        let path = match &self.cfg.trace_path {
            Some(p) => p.clone(),
            None => return Ok(()),
        };
        let entry = json!({
            "goal":          goal,
            "prompt":        outcome.final_prompt,
            "stream":        raw_stream,
            "status":        outcome.status.to_string(),
            "steps":         outcome.steps,
            "wall_seconds":  wall.as_secs_f64(),
            "cartridges":    cartridges.iter().collect::<Vec<_>>(),
            "ts":            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        let line = serde_json::to_string(&entry)? + "\n";
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening trace file {path}"))?;
        f.write_all(line.as_bytes()).context("writing trace line")?;
        log::debug!("trace appended to {path}");
        Ok(())
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
                let result = self.handle_call(cart, method, args, local_context);
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
            Statement::State { payload } => {
                // Daemon-injected state preamble after KV compaction.
                // The model doesn't emit this; the compactor prepends it
                // so the GBNF state machine can resume coherently.
                log::info!("state preamble: {payload}");
            }
        }
        Ok(StatementOutcome::Continue)
    }

    /// Dispatch a `<|call|>cart.method` — v0.1 wires the **proactive
    /// tier-based cloud route** (refinement §2.5). Order:
    ///
    /// 1. If the cartridge's manifest sets `preferred_tier: "cloud"`,
    ///    proxy the call to the cloud model via [`crate::cloud`]. The
    ///    cloud returns a JSON value; we inject it as the result.
    /// 2. Else dispatch to the local handler. On any error, surface as a
    ///    `{"ok":false,...}` JSON for the model to decide whether to
    ///    retry (refinement §1.2 — single retry per step at the daemon
    ///    level, then `<|fault|>` if persistent).
    fn handle_call(
        &self,
        cart: &str,
        method: &str,
        args: &Value,
        local_context: &[CloudMessage],
    ) -> Value {
        // Look up tier without holding the cartridge ref into the cloud branch.
        let tier = self
            .registry
            .get(cart)
            .map(|c| c.manifest.preferred_tier.clone())
            .unwrap_or_else(|| "auto".into());

        if tier == "cloud" && self.cloud.key.is_some() {
            log::info!("call {cart}.{method}: routing to cloud (preferred_tier=cloud)");
            return self.proxy_call_to_cloud(cart, method, args, local_context);
        }

        match dispatch_call(&self.registry, cart, method, args) {
            Ok(v) => v,
            Err(DispatchError::SchemaViolation {
                errors,
                expected_grammar,
                ..
            }) => {
                // v0.5: surface the compiled GBNF so the model sees the
                // exact shape it should have emitted. Increases retry
                // success rate vs v0.01's bare schema-violation message.
                let mut payload = json!({
                    "ok": false,
                    "error": "schema_violation",
                    "details": errors,
                });
                if let Some(g) = expected_grammar {
                    payload["expected_grammar"] = Value::String(g);
                }
                payload
            }
            Err(e) => json!({"ok": false, "error": format!("{e}")}),
        }
    }

    fn proxy_call_to_cloud(
        &self,
        cart: &str,
        method: &str,
        args: &Value,
        local_context: &[CloudMessage],
    ) -> Value {
        let payload = json!({
            "intent": "execute_cartridge_method",
            "cartridge": cart,
            "method": method,
            "args": args,
        });
        match crate::cloud::forward_fault(&self.cloud, local_context, &payload) {
            Ok(reply) => {
                // Cloud may return a raw JSON value or a code-fenced block.
                // Try direct parse first; fall back to extracting JSON from
                // text via the tool_parser helper.
                if let Ok(v) = serde_json::from_str::<Value>(&reply) {
                    v
                } else {
                    let extracted = crate::tool_parser::extract_json_object(&reply);
                    serde_json::from_str(&extracted).unwrap_or(json!({
                        "ok": false,
                        "error": "cloud returned non-JSON",
                        "raw": reply,
                    }))
                }
            }
            Err(e) => json!({"ok": false, "error": format!("cloud routing failed: {e}")}),
        }
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
        if self.cfg.ollama {
            self.boot_prompt_ollama(user_goal)
        } else {
            format!(
                "You are LLM-OS v0.01. Your output MUST conform to the ISA grammar at all times.\n\
                 Available cartridges: {cartridges}\n\
                 Goal from user: {goal}\n\n\
                 Begin emitting ISA opcodes:\n",
                cartridges = self.registry.names().join(", "),
                goal = user_goal,
            )
        }
    }

    /// Enhanced boot prompt for Ollama (no GBNF grammar). Includes opcode
    /// reference, cartridge method listing, and a few-shot example so the
    /// model follows the exact ISA syntax through instruction-following.
    fn boot_prompt_ollama(&self, user_goal: &str) -> String {
        // Build per-cartridge method listing from the live registry.
        let mut cart_listing = String::new();
        let mut names: Vec<&str> = self.registry.names();
        names.sort();
        for name in &names {
            if let Some(cart) = self.registry.get(name) {
                for (m, spec) in &cart.manifest.methods {
                    let hint = cart.args_hint(m).unwrap_or_else(|| "{}".into());
                    let desc = if spec.description.is_empty() {
                        String::new()
                    } else {
                        format!(" — {}", spec.description)
                    };
                    cart_listing.push_str(&format!("  {name}.{m} {hint}{desc}\n"));
                }
            }
        }

        format!(
            "You are LLM-OS, a deterministic kernel. Output ONLY ISA opcodes. No prose, no markdown.\n\
             \n\
             OPCODES:\n\
             <|think|>reasoning<|/think|>\n\
             <|call|>cart.method {{\"key\":\"val\"}} <|/call|>   (ALWAYS include JSON args, use {{}} for no args)\n\
             <|write|>fd=1 {{\"msg\":\"text\"}}\n\
             <|loop|>goal=label  ...  <|break|>\n\
             <|halt|>status=success\n\
             \n\
             RULES:\n\
             - Every <|call|> MUST have JSON args: <|call|>cart.method {{}} <|/call|>  (use {{}} if no args needed)\n\
             - After <|call|>...<|/call|>, SYSTEM injects <|result|>...<|/result|>. Do NOT predict results.\n\
             - After <|write|>, SYSTEM injects <|ack|>. Do NOT predict acks.\n\
             \n\
             CARTRIDGES:\n\
             {cart_listing}\
             \n\
             EXAMPLE 1 (single call):\n\
             <|think|>I need to echo a message<|/think|>\n\
             <|call|>demo.echo {{\"text\":\"hello\"}} <|/call|>\n\
             <|result|>{{\"text\":\"hello\",\"len\":5}}<|/result|>\n\
             <|write|>fd=1 {{\"msg\":\"Echo returned: hello\"}}\n\
             <|ack|>\n\
             <|halt|>status=success\n\
             \n\
             EXAMPLE 2 (multiple calls — always end with write+halt):\n\
             <|think|>I need to echo then hash<|/think|>\n\
             <|call|>demo.echo {{\"text\":\"hi\"}} <|/call|>\n\
             <|result|>{{\"text\":\"hi\",\"len\":2}}<|/result|>\n\
             <|call|>demo.hash {{\"text\":\"hi\"}} <|/call|>\n\
             <|result|>{{\"hex\":\"49f68a5c8493ec2c0bf489821c21fc3b\"}}<|/result|>\n\
             <|write|>fd=1 {{\"msg\":\"hash of hi = 49f68a5c8493ec2c0bf489821c21fc3b\"}}\n\
             <|ack|>\n\
             <|halt|>status=success\n\
             \n\
             CRITICAL: After completing all calls, you MUST <|write|> then <|halt|>. Never repeat calls.\n\
             \n\
             YOUR TASK:\n\
             Goal: {goal}\n\
             Begin:\n",
            cart_listing = cart_listing,
            goal = user_goal,
        )
    }

    /// POST one streaming request. Routes to either llama-server
    /// (`/v1/completions` with GBNF grammar) or Ollama (`/api/generate`
    /// without grammar enforcement) based on `cfg.ollama`.
    fn complete_one_segment(&self, prompt: &str) -> Result<String> {
        if self.cfg.ollama {
            self.complete_via_ollama(prompt)
        } else {
            self.complete_via_llama_server(prompt)
        }
    }

    /// llama-server backend: SSE streaming with GBNF grammar enforcement.
    fn complete_via_llama_server(&self, prompt: &str) -> Result<String> {
        let body = CompletionRequest {
            prompt: prompt.to_string(),
            grammar: self.grammar.clone(),
            stream: true,
            cache_prompt: true,
            n_predict: self.cfg.max_predict_per_segment,
            temperature: self.cfg.temperature,
            id_slot: self.cfg.slot_id,
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

    /// Ollama backend: NDJSON streaming via `/api/generate`. No GBNF
    /// grammar enforcement — the model relies on instruction-following.
    /// Stop sequences implement the stop-and-inject loop: model stops at
    /// `<|/call|>` etc. so the daemon can dispatch handlers.
    fn complete_via_ollama(&self, prompt: &str) -> Result<String> {
        let body = OllamaRequest {
            model: self.cfg.model.clone(),
            prompt: prompt.to_string(),
            stream: true,
            options: OllamaOptions {
                num_predict: self.cfg.max_predict_per_segment,
                temperature: self.cfg.temperature,
                repeat_penalty: 1.3,
            },
            // Disable CoT thinking (Qwen 3.5 etc.) — ISA generation
            // needs direct opcode output, not internal reasoning.
            think: false,
            // Stop after opcodes that need daemon response. Ollama
            // includes the stop token in output, so the parser sees
            // the complete opcode. `<|result|>` prevents the model
            // from self-predicting daemon-injected results. `<|ack|>`
            // prevents self-predicting write acknowledgements.
            stop: vec![
                "<|/call|>".into(),
                "<|result|>".into(),
                "<|ack|>".into(),
            ],
        };

        let url = format!("{}/api/generate", self.cfg.server_url);
        // Ollama may need to reload the model on first call (~30s) and
        // generation can be slow on CPU-only machines. Use a generous
        // timeout to avoid cutting off valid long-running generation.
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(300))
            .build();
        let resp = agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| anyhow!("Ollama POST failed: {e}"))?;

        let reader = BufReader::new(resp.into_reader());
        let mut acc = String::new();
        for line in reader.lines() {
            let line = line.context("reading Ollama NDJSON line")?;
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            let parsed: OllamaStreamDelta = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !parsed.response.is_empty() {
                acc.push_str(&parsed.response);
            }
            if parsed.done {
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

/// Strip model-hallucinated daemon blocks from a segment and truncate
/// after halt statements. Handles two Ollama quirks:
///
/// 1. Unreliable stop sequences — model may generate daemon-injected
///    blocks (`<|result|>…<|/result|>`, `<|ack|>`) that should be
///    daemon-only.
/// 2. Runaway generation — model may continue past `<|halt|>status=…`
///    generating garbage that corrupts parsing.
///
/// Processing order: strip daemon blocks first, then truncate at halt.
fn strip_daemon_blocks(segment: &str) -> String {
    // Phase 1: strip <|result|>…<|/result|> and <|ack|> blocks.
    let mut cleaned = String::with_capacity(segment.len());
    let mut rest = segment;
    while !rest.is_empty() {
        // Find the earliest daemon block.
        let result_pos = rest.find("<|result|>");
        let ack_pos = rest.find("<|ack|>");
        let earliest = match (result_pos, ack_pos) {
            (Some(r), Some(a)) => Some(r.min(a)),
            (Some(r), None) => Some(r),
            (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        match earliest {
            None => {
                cleaned.push_str(rest);
                break;
            }
            Some(pos) => {
                cleaned.push_str(&rest[..pos]);
                if rest[pos..].starts_with("<|result|>") {
                    if let Some(end) = rest[pos..].find("<|/result|>") {
                        let skip = pos + end + "<|/result|>".len();
                        rest = &rest[skip..];
                        rest = rest.strip_prefix('\n').unwrap_or(rest);
                    } else {
                        break; // unclosed — drop remainder
                    }
                } else {
                    // <|ack|>
                    rest = &rest[pos + "<|ack|>".len()..];
                    rest = rest.strip_prefix('\n').unwrap_or(rest);
                }
            }
        }
    }

    // Phase 2: truncate at <|halt|>status=X — nothing after is valid.
    for status in ["success", "failure", "partial"] {
        let needle = format!("<|halt|>status={status}");
        if let Some(pos) = cleaned.find(&needle) {
            let end = pos + needle.len();
            cleaned.truncate(end);
            cleaned.push('\n');
            log::debug!("truncated segment at <|halt|>status={status}");
            break;
        }
    }

    if cleaned != segment {
        log::debug!(
            "cleaned segment: {} → {} bytes",
            segment.len(),
            cleaned.len()
        );
    }
    cleaned
}

/// For Ollama mode: truncate a cleaned segment at the end of the first
/// `<|call|>…<|/call|>` block. This forces the daemon to dispatch that call,
/// inject the real result, and re-generate — preventing the model from
/// hallucinating all subsequent steps in one shot (Ollama stop sequences are
/// unreliable and don't actually stop mid-stream).
///
/// Also handles a common model quirk: the model may emit `<|call|>` inside
/// an unclosed `<|think|>` block. When detected, inserts `<|/think|>\n`
/// before the call so the parser can close the think and parse the call.
///
/// If no `<|call|>` is found, returns the full segment unchanged.
fn truncate_at_first_call(segment: &str) -> String {
    if let Some(call_start) = segment.find("<|call|>") {
        if let Some(end_offset) = segment[call_start..].find("<|/call|>") {
            let cut = call_start + end_offset + "<|/call|>".len();
            let before_call = &segment[..call_start];
            let call_block = &segment[call_start..cut];

            let mut truncated = String::with_capacity(cut + 20);

            // Check if there's an unclosed <|think|> block before the call.
            // Count opens vs closes to determine if we're inside a think.
            let think_opens = before_call.matches("<|think|>").count();
            let think_closes = before_call.matches("<|/think|>").count();
            if think_opens > think_closes {
                // Close the dangling think block before the call.
                truncated.push_str(before_call);
                truncated.push_str("<|/think|>\n");
                truncated.push_str(call_block);
                log::debug!("inserted <|/think|> before <|call|> (unclosed think block)");
            } else {
                truncated.push_str(&segment[..cut]);
            }
            truncated.push('\n');
            log::debug!(
                "truncated segment at first <|/call|>: {} → {} bytes",
                segment.len(),
                truncated.len()
            );
            return truncated;
        }
    }
    segment.to_string()
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
