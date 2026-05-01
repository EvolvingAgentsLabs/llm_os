//! Thin FFI wrapper over llama.cpp's C API.
//!
//! Phase 1 of the improvement plan: collapse the HTTP boundary between
//! iod and llama-server into a single in-process call. This module
//! provides safe Rust wrappers over the llama.cpp C functions.
//!
//! Build requirements:
//! - llama.cpp vendored as a git submodule at `runtime/vendor/llama.cpp`
//! - `build.rs` compiles it as a static library (`libllama.a`)
//! - Feature flag: `ffi-inference` (default enabled)
//!
//! When `ffi-inference` is disabled, the daemon falls back to HTTP
//! communication with a standalone llama-server (legacy mode).

#![allow(non_camel_case_types, dead_code, unused_imports)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_int};
use std::path::Path;
use std::ptr;

// ─── C API types (opaque handles) ───────────────────────────────────────────

/// Opaque pointer to a llama model.
pub enum llama_model {}
/// Opaque pointer to a llama context (KV cache + sampling state).
pub enum llama_context {}
/// Opaque pointer to a compiled GBNF grammar.
pub enum llama_grammar {}
/// Opaque pointer to a sampler chain.
pub enum llama_sampler {}

pub type llama_token = c_int;
pub type llama_pos = c_int;
pub type llama_seq_id = c_int;

/// Token batch for decoding.
#[repr(C)]
pub struct llama_batch {
    pub n_tokens: c_int,
    pub token: *mut llama_token,
    pub embd: *mut c_float,
    pub pos: *mut llama_pos,
    pub n_seq_id: *mut c_int,
    pub seq_id: *mut *mut llama_seq_id,
    pub logits: *mut i8,
}

/// Model loading parameters.
#[repr(C)]
#[derive(Clone)]
pub struct llama_model_params {
    pub n_gpu_layers: c_int,
    pub use_mmap: bool,
    pub use_mlock: bool,
}

/// Context parameters.
#[repr(C)]
#[derive(Clone)]
pub struct llama_context_params {
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_threads: u32,
    pub n_threads_batch: u32,
    pub flash_attn: bool,
}

// ─── C API bindings (extern) ────────────────────────────────────────────────

#[cfg(feature = "ffi-inference")]
extern "C" {
    pub fn llama_backend_init();
    pub fn llama_backend_free();

    pub fn llama_model_default_params() -> llama_model_params;
    pub fn llama_context_default_params() -> llama_context_params;

    pub fn llama_load_model_from_file(
        path: *const c_char,
        params: llama_model_params,
    ) -> *mut llama_model;
    pub fn llama_free_model(model: *mut llama_model);

    pub fn llama_new_context_with_model(
        model: *mut llama_model,
        params: llama_context_params,
    ) -> *mut llama_context;
    pub fn llama_free(ctx: *mut llama_context);

    pub fn llama_n_ctx(ctx: *const llama_context) -> u32;
    pub fn llama_n_vocab(model: *const llama_model) -> c_int;

    pub fn llama_decode(ctx: *mut llama_context, batch: llama_batch) -> c_int;

    pub fn llama_batch_init(n_tokens: c_int, embd: c_int, n_seq_max: c_int) -> llama_batch;
    pub fn llama_batch_free(batch: llama_batch);

    pub fn llama_kv_cache_seq_rm(
        ctx: *mut llama_context,
        seq_id: llama_seq_id,
        p0: llama_pos,
        p1: llama_pos,
    ) -> bool;
    pub fn llama_kv_cache_seq_cp(
        ctx: *mut llama_context,
        seq_id_src: llama_seq_id,
        seq_id_dst: llama_seq_id,
        p0: llama_pos,
        p1: llama_pos,
    );
    pub fn llama_get_kv_cache_used_cells(ctx: *const llama_context) -> c_int;

    pub fn llama_token_to_piece(
        model: *const llama_model,
        token: llama_token,
        buf: *mut c_char,
        length: c_int,
        lstrip: c_int,
        special: bool,
    ) -> c_int;

    pub fn llama_tokenize(
        model: *const llama_model,
        text: *const c_char,
        text_len: c_int,
        tokens: *mut llama_token,
        n_tokens_max: c_int,
        add_special: bool,
        parse_special: bool,
    ) -> c_int;

    pub fn llama_token_bos(model: *const llama_model) -> llama_token;
    pub fn llama_token_eos(model: *const llama_model) -> llama_token;

    pub fn llama_grammar_init(
        rules: *const *const c_char,
        n_rules: usize,
        start_rule_index: usize,
    ) -> *mut llama_grammar;
    pub fn llama_grammar_free(grammar: *mut llama_grammar);
}

// ─── Safe Rust wrappers ─────────────────────────────────────────────────────

/// Safe wrapper around a loaded llama model.
pub struct LlamaModel {
    ptr: *mut llama_model,
}

unsafe impl Send for LlamaModel {}
unsafe impl Sync for LlamaModel {}

/// Safe wrapper around a llama context (KV cache).
pub struct LlamaContext {
    ptr: *mut llama_context,
    model: *const llama_model,
    n_ctx: u32,
}

unsafe impl Send for LlamaContext {}

/// Parameters for model loading.
#[derive(Debug, Clone)]
pub struct ModelParams {
    pub n_gpu_layers: i32,
    pub use_mmap: bool,
    pub use_mlock: bool,
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            n_gpu_layers: 0,
            use_mmap: true,
            use_mlock: false,
        }
    }
}

/// Parameters for context creation.
#[derive(Debug, Clone)]
pub struct ContextParams {
    pub n_ctx: u32,
    pub n_batch: u32,
    pub n_threads: u32,
    pub flash_attn: bool,
}

impl Default for ContextParams {
    fn default() -> Self {
        Self {
            n_ctx: 32_768,
            n_batch: 512,
            n_threads: 4,
            flash_attn: true,
        }
    }
}

#[cfg(feature = "ffi-inference")]
impl LlamaModel {
    /// Initialize the llama backend (call once at startup).
    pub fn init_backend() {
        unsafe { llama_backend_init() }
    }

    /// Free the llama backend (call once at shutdown).
    pub fn free_backend() {
        unsafe { llama_backend_free() }
    }

    /// Load a model from a GGUF file.
    pub fn load(path: &Path, params: &ModelParams) -> Result<Self, String> {
        let path_str = path.to_str().ok_or("invalid path encoding")?;
        let c_path = CString::new(path_str).map_err(|e| format!("path CString: {e}"))?;

        let c_params = llama_model_params {
            n_gpu_layers: params.n_gpu_layers,
            use_mmap: params.use_mmap,
            use_mlock: params.use_mlock,
        };

        let ptr = unsafe { llama_load_model_from_file(c_path.as_ptr(), c_params) };
        if ptr.is_null() {
            return Err(format!("failed to load model from {path_str}"));
        }
        Ok(Self { ptr })
    }

    /// Get the model's vocabulary size.
    pub fn n_vocab(&self) -> i32 {
        unsafe { llama_n_vocab(self.ptr as *const _) }
    }

    /// Get the BOS token id.
    pub fn token_bos(&self) -> llama_token {
        unsafe { llama_token_bos(self.ptr as *const _) }
    }

    /// Get the EOS token id.
    pub fn token_eos(&self) -> llama_token {
        unsafe { llama_token_eos(self.ptr as *const _) }
    }

    /// Tokenize a string.
    pub fn tokenize(&self, text: &str, add_special: bool) -> Vec<llama_token> {
        let c_text = CString::new(text).unwrap_or_else(|_| CString::new("").unwrap());
        let text_len = text.len() as c_int;

        // First call to get required buffer size
        let n_max = text_len + 128; // generous overalloc
        let mut tokens = vec![0i32; n_max as usize];

        let n = unsafe {
            llama_tokenize(
                self.ptr as *const _,
                c_text.as_ptr(),
                text_len,
                tokens.as_mut_ptr(),
                n_max,
                add_special,
                true, // parse_special: recognize ISA tokens
            )
        };

        if n < 0 {
            // Buffer too small, retry with exact size
            let needed = (-n) as usize;
            tokens.resize(needed, 0);
            let n2 = unsafe {
                llama_tokenize(
                    self.ptr as *const _,
                    c_text.as_ptr(),
                    text_len,
                    tokens.as_mut_ptr(),
                    needed as c_int,
                    add_special,
                    true,
                )
            };
            tokens.truncate(n2.max(0) as usize);
        } else {
            tokens.truncate(n as usize);
        }
        tokens
    }

    /// Decode a single token to its string piece.
    pub fn token_to_piece(&self, token: llama_token) -> String {
        let mut buf = vec![0u8; 64];
        let n = unsafe {
            llama_token_to_piece(
                self.ptr as *const _,
                token,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as c_int,
                0,    // lstrip
                true, // special
            )
        };
        if n < 0 {
            // Buffer too small
            buf.resize((-n) as usize, 0);
            let n2 = unsafe {
                llama_token_to_piece(
                    self.ptr as *const _,
                    token,
                    buf.as_mut_ptr() as *mut c_char,
                    buf.len() as c_int,
                    0,
                    true,
                )
            };
            buf.truncate(n2.max(0) as usize);
        } else {
            buf.truncate(n as usize);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    pub fn as_ptr(&self) -> *mut llama_model {
        self.ptr
    }
}

#[cfg(feature = "ffi-inference")]
impl Drop for LlamaModel {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { llama_free_model(self.ptr) }
        }
    }
}

#[cfg(feature = "ffi-inference")]
impl LlamaContext {
    /// Create a new context for a model.
    pub fn new(model: &LlamaModel, params: &ContextParams) -> Result<Self, String> {
        let c_params = llama_context_params {
            n_ctx: params.n_ctx,
            n_batch: params.n_batch,
            n_threads: params.n_threads,
            n_threads_batch: params.n_threads,
            flash_attn: params.flash_attn,
        };

        let ptr = unsafe { llama_new_context_with_model(model.ptr, c_params) };
        if ptr.is_null() {
            return Err("failed to create llama context".into());
        }
        Ok(Self {
            ptr,
            model: model.ptr as *const _,
            n_ctx: params.n_ctx,
        })
    }

    /// Get the context window size.
    pub fn n_ctx(&self) -> u32 {
        self.n_ctx
    }

    /// Get number of KV cache cells currently used.
    pub fn kv_cache_used_cells(&self) -> i32 {
        unsafe { llama_get_kv_cache_used_cells(self.ptr as *const _) }
    }

    /// Remove KV cache entries in [p0, p1) for a sequence.
    /// Use p1 = -1 to remove from p0 to end.
    pub fn kv_cache_seq_rm(&mut self, seq_id: i32, p0: i32, p1: i32) -> bool {
        unsafe { llama_kv_cache_seq_rm(self.ptr, seq_id, p0, p1) }
    }

    /// Copy KV cache from one sequence to another in [p0, p1).
    pub fn kv_cache_seq_cp(&mut self, src: i32, dst: i32, p0: i32, p1: i32) {
        unsafe { llama_kv_cache_seq_cp(self.ptr, src, dst, p0, p1) }
    }

    /// Decode a batch of tokens (extends the KV cache).
    pub fn decode_tokens(&mut self, tokens: &[llama_token], pos_start: i32) -> Result<(), String> {
        if tokens.is_empty() {
            return Ok(());
        }

        let mut batch = unsafe { llama_batch_init(tokens.len() as c_int, 0, 1) };

        // Fill batch manually
        for (i, &token) in tokens.iter().enumerate() {
            unsafe {
                *batch.token.add(i) = token;
                *batch.pos.add(i) = pos_start + i as i32;
                *batch.n_seq_id.add(i) = 1;
                *(*batch.seq_id.add(i)) = 0; // seq_id = 0
                *batch.logits.add(i) = if i == tokens.len() - 1 { 1 } else { 0 };
            }
        }
        batch.n_tokens = tokens.len() as c_int;

        let ret = unsafe { llama_decode(self.ptr, batch) };
        unsafe { llama_batch_free(batch) };

        if ret != 0 {
            Err(format!("llama_decode failed with code {ret}"))
        } else {
            Ok(())
        }
    }

    pub fn as_ptr(&self) -> *mut llama_context {
        self.ptr
    }
}

#[cfg(feature = "ffi-inference")]
impl Drop for LlamaContext {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { llama_free(self.ptr) }
        }
    }
}

// ─── Stub implementations when FFI is disabled ──────────────────────────────

#[cfg(not(feature = "ffi-inference"))]
impl LlamaModel {
    pub fn init_backend() {}
    pub fn free_backend() {}
    pub fn load(_path: &Path, _params: &ModelParams) -> Result<Self, String> {
        Err("ffi-inference feature not enabled; use --legacy-server".into())
    }
    pub fn n_vocab(&self) -> i32 { 0 }
    pub fn token_bos(&self) -> llama_token { 0 }
    pub fn token_eos(&self) -> llama_token { 0 }
    pub fn tokenize(&self, _text: &str, _add_special: bool) -> Vec<llama_token> { vec![] }
    pub fn token_to_piece(&self, _token: llama_token) -> String { String::new() }
    pub fn as_ptr(&self) -> *mut llama_model { ptr::null_mut() }
}

#[cfg(not(feature = "ffi-inference"))]
impl LlamaContext {
    pub fn new(_model: &LlamaModel, _params: &ContextParams) -> Result<Self, String> {
        Err("ffi-inference feature not enabled".into())
    }
    pub fn n_ctx(&self) -> u32 { 0 }
    pub fn kv_cache_used_cells(&self) -> i32 { 0 }
    pub fn kv_cache_seq_rm(&mut self, _seq_id: i32, _p0: i32, _p1: i32) -> bool { false }
    pub fn kv_cache_seq_cp(&mut self, _src: i32, _dst: i32, _p0: i32, _p1: i32) {}
    pub fn decode_tokens(&mut self, _tokens: &[llama_token], _pos_start: i32) -> Result<(), String> {
        Err("ffi-inference feature not enabled".into())
    }
    pub fn as_ptr(&self) -> *mut llama_context { ptr::null_mut() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_params_default() {
        let p = ModelParams::default();
        assert_eq!(p.n_gpu_layers, 0);
        assert!(p.use_mmap);
        assert!(!p.use_mlock);
    }

    #[test]
    fn context_params_default() {
        let p = ContextParams::default();
        assert_eq!(p.n_ctx, 32_768);
        assert_eq!(p.n_batch, 512);
        assert!(p.flash_attn);
    }
}
