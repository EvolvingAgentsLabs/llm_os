//! LLM-OS v1.0 runtime library.
//!
//! ## Core modules
//! - [`parser`] — streaming ISA opcode parser / state machine (14 opcodes incl. `<|state|>`).
//! - [`dispatch`] — opcode → cartridge syscall routing.
//! - [`cartridge`] — cartridge manifest types, registry, JSON-schema validation.
//! - [`iod`] — the I/O daemon main loop.
//!
//! ## Phase 1: In-process inference (collapse HTTP boundary)
//! - [`llama_ffi`] — thin FFI wrapper over llama.cpp's C API.
//! - [`sampler`] — in-process token generation with grammar enforcement.
//!
//! ## Phase 2: Deterministic KV paging
//! - [`kv_pager`] — positional token eviction with anchors (no LLM).
//! - [`swap`] — ISA-aware state extraction (reused by kv_pager).
//!
//! ## Phase 3: WASM cartridge sandbox
//! - [`wasm_host`] — wasmtime-based cartridge execution with curated host functions.
//!
//! ## Supporting modules
//! - [`tool_parser`] — fallback `<|call|>` arg parser (defense in depth).
//! - [`cloud`] — cloud-fallback HTTP adapter for `<|fault|>{"needs_cloud":true}`.
//! - [`capability`] — logit-bias opcode enforcement (exact with single-token ISA).
//! - [`scheduler`] — preemptive task budget enforcement.
//! - [`multitask`] — concurrent task execution via KV slot pool.
//! - [`handlers`] — in-process cartridge handlers (legacy, behind feature flag).

pub mod capability;
pub mod cartridge;
pub mod cloud;
pub mod dialect;
pub mod dispatch;
pub mod handlers;
pub mod iod;
pub mod kv_pager;
pub mod llama_ffi;
pub mod mock_server;
pub mod multitask;
pub mod parser;
pub mod roclaw;
pub mod sampler;
pub mod scheduler;
pub mod schema_to_gbnf;
pub mod subagent;
pub mod swap;
pub mod tool_parser;
pub mod wasm_host;

pub use cartridge::{Cartridge, CartridgeRegistry, Manifest, MethodSpec};
pub use kv_pager::{CompactionResult, EvictionPlan, KvPager, PagerConfig};
pub use llama_ffi::{ContextParams, LlamaContext, LlamaModel, ModelParams};
pub use parser::{Opcode, OpcodeStream, Statement};
pub use sampler::{Sampler, SamplerConfig, Segment};
pub use swap::IsaState;
pub use wasm_host::{CartridgeCaps, WasmEngine, WasmError};
