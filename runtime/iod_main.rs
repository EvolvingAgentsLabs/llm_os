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
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    let cfg = DaemonConfig {
        server_url: args.server,
        grammar_path: args.grammar,
        cart_root: args.cart,
        task_budget: Duration::from_secs(args.budget),
        max_loop_depth: 4,
        temperature: args.temperature,
        max_predict_per_segment: 512,
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
