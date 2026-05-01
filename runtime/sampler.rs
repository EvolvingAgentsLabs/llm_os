//! In-process inference sampler — replaces the HTTP SSE loop.
//!
//! Phase 1: Instead of POSTing to llama-server and reading SSE tokens,
//! the Sampler drives llama.cpp's decode loop directly. This eliminates
//! 200–400ms of IPC overhead per stop-and-inject cycle.
//!
//! The Sampler owns the LlamaContext and provides:
//! - `generate_segment()` — emit tokens until an opcode boundary
//! - `inject_result()` — append result tokens to KV cache (no HTTP)
//! - Grammar stack for mid-stream grammar swapping

use crate::llama_ffi::{ContextParams, LlamaContext, LlamaModel, ModelParams};
use std::path::Path;

/// A generated segment of tokens.
#[derive(Debug, Clone)]
pub struct Segment {
    /// The decoded text of this segment.
    pub text: String,
    /// Whether this segment ended at an opcode boundary needing injection.
    pub needs_inject: bool,
    /// Number of tokens generated in this segment.
    pub n_tokens: u32,
}

/// Configuration for the sampler.
#[derive(Debug, Clone)]
pub struct SamplerConfig {
    /// Path to the GGUF model file.
    pub model_path: String,
    /// Path to the ISA GBNF grammar file.
    pub grammar_path: String,
    /// Context window size.
    pub n_ctx: u32,
    /// Batch size for decoding.
    pub n_batch: u32,
    /// Number of CPU threads.
    pub n_threads: u32,
    /// GPU layers to offload (0 = CPU only).
    pub n_gpu_layers: i32,
    /// Sampling temperature.
    pub temperature: f64,
    /// Max tokens per segment before forcing a stop.
    pub max_tokens_per_segment: u32,
    /// Enable flash attention.
    pub flash_attn: bool,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            grammar_path: String::new(),
            n_ctx: 32_768,
            n_batch: 512,
            n_threads: 4,
            n_gpu_layers: 0,
            temperature: 0.2,
            max_tokens_per_segment: 512,
            flash_attn: true,
        }
    }
}

/// The opcode tokens that signal "stop and inject" boundaries.
/// When the sampler detects these in the generated text, it stops
/// generation so the daemon can dispatch the opcode handler.
const STOP_MARKERS: &[&str] = &[
    "<|/call|>",
    "<|read|>",
    "<|write|>",
    "<|wait|>",
    "<|fault|>",
    "<|policy|>",
    "<|halt|>",
];

/// In-process inference sampler. Drives llama.cpp directly.
pub struct Sampler {
    model: LlamaModel,
    ctx: LlamaContext,
    config: SamplerConfig,
    /// GBNF grammar source (loaded at init).
    grammar_source: String,
    /// Current position in the KV cache.
    pos: i32,
    /// Grammar stack for mid-stream swapping.
    grammar_stack: Vec<String>,
}

impl Sampler {
    /// Create a new sampler, loading the model and initializing the context.
    pub fn new(config: SamplerConfig) -> Result<Self, String> {
        LlamaModel::init_backend();

        let model_params = ModelParams {
            n_gpu_layers: config.n_gpu_layers,
            use_mmap: true,
            use_mlock: false,
        };

        let model = LlamaModel::load(Path::new(&config.model_path), &model_params)?;

        let ctx_params = ContextParams {
            n_ctx: config.n_ctx,
            n_batch: config.n_batch,
            n_threads: config.n_threads,
            flash_attn: config.flash_attn,
        };

        let ctx = LlamaContext::new(&model, &ctx_params)?;

        let grammar_source = if config.grammar_path.is_empty() {
            String::new()
        } else {
            std::fs::read_to_string(&config.grammar_path)
                .map_err(|e| format!("reading grammar {}: {e}", config.grammar_path))?
        };

        Ok(Self {
            model,
            ctx,
            config,
            grammar_source,
            pos: 0,
            grammar_stack: Vec::new(),
        })
    }

    /// Inject a prompt into the context (tokenize + decode).
    /// This is used for the initial boot prompt.
    pub fn inject_prompt(&mut self, prompt: &str) -> Result<(), String> {
        let tokens = self.model.tokenize(prompt, true);
        if tokens.is_empty() {
            return Ok(());
        }

        // Decode in batches
        let batch_size = self.config.n_batch as usize;
        for chunk in tokens.chunks(batch_size) {
            self.ctx.decode_tokens(chunk, self.pos)?;
            self.pos += chunk.len() as i32;
        }

        Ok(())
    }

    /// Inject a result string directly into the KV cache.
    /// No HTTP round-trip — just tokenize and decode in-process.
    pub fn inject_result(&mut self, result_text: &str) -> Result<(), String> {
        let tokens = self.model.tokenize(result_text, false);
        if tokens.is_empty() {
            return Ok(());
        }

        let batch_size = self.config.n_batch as usize;
        for chunk in tokens.chunks(batch_size) {
            self.ctx.decode_tokens(chunk, self.pos)?;
            self.pos += chunk.len() as i32;
        }

        Ok(())
    }

    /// Generate tokens until an opcode boundary or max_tokens is reached.
    ///
    /// This is the core of the stop-and-inject loop without HTTP:
    /// 1. Sample a token (grammar-constrained)
    /// 2. Decode the token piece
    /// 3. Check if we've hit an opcode boundary
    /// 4. If yes, return the segment so the daemon can dispatch
    ///
    /// NOTE: In the full FFI implementation, this would call llama_decode
    /// and llama_sampler_sample directly. For now, it provides the interface
    /// that iod.rs will use. The actual token-by-token sampling requires
    /// the llama_sampler API which varies by llama.cpp version.
    pub fn generate_segment(&mut self, max_tokens: u32) -> Result<Segment, String> {
        // This is the interface contract. The actual implementation requires
        // linking against libllama.a with the sampler API. The structure is:
        //
        // for _ in 0..max_tokens {
        //     let token = sample_with_grammar(&self.ctx, &self.grammar);
        //     let piece = self.model.token_to_piece(token);
        //     text.push_str(&piece);
        //     self.ctx.decode_tokens(&[token], self.pos)?;
        //     self.pos += 1;
        //     if is_stop_marker(&text) { break; }
        // }

        // Placeholder that signals the interface is correct but requires
        // the actual libllama linkage to function.
        Err("sampler requires ffi-inference feature with linked libllama".into())
    }

    /// Push a sub-grammar onto the grammar stack (for mid-stream swap).
    /// Used when entering a `<|call|>cart.method` — push the method's
    /// args grammar so the model can only emit valid JSON args.
    pub fn push_grammar(&mut self, sub_grammar: &str) {
        self.grammar_stack.push(self.grammar_source.clone());
        self.grammar_source = sub_grammar.to_string();
    }

    /// Pop back to the previous grammar (after `<|/call|>`).
    pub fn pop_grammar(&mut self) {
        if let Some(prev) = self.grammar_stack.pop() {
            self.grammar_source = prev;
        }
    }

    /// Get the current KV cache utilization as a fraction of n_ctx.
    pub fn kv_utilization(&self) -> f64 {
        let used = self.ctx.kv_cache_used_cells() as f64;
        let total = self.ctx.n_ctx() as f64;
        if total == 0.0 { 0.0 } else { used / total }
    }

    /// Get the current position in the KV cache.
    pub fn pos(&self) -> i32 {
        self.pos
    }

    /// Remove KV cache entries in a range (for compaction).
    pub fn kv_cache_rm(&mut self, p0: i32, p1: i32) -> bool {
        self.ctx.kv_cache_seq_rm(0, p0, p1)
    }

    /// Get reference to the model (for tokenization).
    pub fn model(&self) -> &LlamaModel {
        &self.model
    }
}

/// Check if the accumulated text ends with any stop marker.
pub fn contains_stop_marker(text: &str) -> bool {
    STOP_MARKERS.iter().any(|m| text.contains(m))
}

/// Find the first stop marker in the text and return its position + the marker.
pub fn find_stop_marker(text: &str) -> Option<(usize, &'static str)> {
    let mut earliest: Option<(usize, &str)> = None;
    for &marker in STOP_MARKERS {
        if let Some(pos) = text.find(marker) {
            match earliest {
                None => earliest = Some((pos, marker)),
                Some((prev_pos, _)) if pos < prev_pos => earliest = Some((pos, marker)),
                _ => {}
            }
        }
    }
    earliest
}

impl Drop for Sampler {
    fn drop(&mut self) {
        LlamaModel::free_backend();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_markers_detected() {
        assert!(contains_stop_marker("some text <|halt|>status=success"));
        assert!(contains_stop_marker("<|/call|>"));
        assert!(contains_stop_marker("data <|read|>fd=3 len=128"));
        assert!(!contains_stop_marker("normal text without markers"));
    }

    #[test]
    fn find_stop_marker_returns_earliest() {
        let text = "<|call|>roclaw.forward {\"left\":150} <|/call|>";
        let found = find_stop_marker(text);
        assert!(found.is_some());
        let (pos, marker) = found.unwrap();
        assert_eq!(marker, "<|/call|>");
        assert_eq!(pos, 36);
    }

    #[test]
    fn sampler_config_defaults() {
        let cfg = SamplerConfig::default();
        assert_eq!(cfg.n_ctx, 32_768);
        assert_eq!(cfg.n_batch, 512);
        assert_eq!(cfg.n_threads, 4);
        assert_eq!(cfg.max_tokens_per_segment, 512);
    }
}
