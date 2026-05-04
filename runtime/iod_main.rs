//! Binary entry point for the I/O daemon.
//!
//! Usage:
//!     iod --server http://127.0.0.1:8080 \
//!         --grammar grammar/isa.gbnf \
//!         --cart    cart \
//!         --goal    "plan a weekly menu"

use anyhow::Result;
use clap::Parser;
use llm_os_runtime::iod::{Daemon, DaemonConfig};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "iod", version, about = "LLM-OS I/O daemon (v0.01)")]
struct Args {
    /// llama-server base URL.
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    server: String,

    /// Path to the ISA GBNF grammar.
    #[arg(long, default_value = "grammar/isa.gbnf")]
    grammar: String,

    /// Cartridge root directory.
    #[arg(long, default_value = "cart")]
    cart: String,

    /// User goal (the prompt that drives this task).
    #[arg(long)]
    goal: String,

    /// Wall-clock task budget in seconds.
    #[arg(long, default_value = "600")]
    budget: u64,

    /// Sampler temperature for ISA generation.
    #[arg(long, default_value = "0.2")]
    temperature: f64,

    /// Optional JSONL path; appends one line per task for fine-tune dataset
    /// collection (see docs/fine-tune-recipe.md §1).
    #[arg(long)]
    trace: Option<String>,

    /// Use Ollama's /api/generate instead of llama-server's /v1/completions.
    /// Default server URL changes to http://127.0.0.1:11434.
    #[arg(long, default_value = "false")]
    ollama: bool,

    /// Model name for Ollama backend (e.g. "qwen3.5:2b").
    #[arg(long, default_value = "")]
    model: String,

    /// Max tokens per generation segment. Lower values force smaller
    /// segments, giving the daemon more chances to inject results between
    /// model calls. Default: 512 for llama-server, 128 for Ollama (to
    /// compensate for unreliable stop sequences).
    #[arg(long)]
    max_predict: Option<u32>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    // Default server URL differs between backends.
    let server_url = if args.server == "http://127.0.0.1:8080" && args.ollama {
        "http://127.0.0.1:11434".to_string()
    } else {
        args.server
    };

    let cfg = DaemonConfig {
        server_url,
        grammar_path: args.grammar,
        cart_root: args.cart,
        task_budget: Duration::from_secs(args.budget),
        max_loop_depth: 4,
        temperature: args.temperature,
        max_predict_per_segment: args.max_predict.unwrap_or(if args.ollama { 128 } else { 512 }),
        trace_path: args.trace,
        max_tokens_per_task: 100_000,
        slot_id: None,
        ollama: args.ollama,
        model: args.model,
    };

    let daemon = Daemon::new(cfg)?;
    let outcome = daemon.run_task(&args.goal)?;
    log::info!(
        "task complete: status={} steps={} prompt_len={}",
        outcome.status,
        outcome.steps,
        outcome.final_prompt.len()
    );
    // Exit code reflects halt status so callers (e.g. e2e harness) can branch.
    let code = match outcome.status {
        llm_os_runtime::parser::HaltStatus::Success => 0,
        llm_os_runtime::parser::HaltStatus::Partial => 2,
        llm_os_runtime::parser::HaltStatus::Failure => 1,
    };
    std::process::exit(code);
}
