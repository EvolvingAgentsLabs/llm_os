//! LLM-OS v0.5 runtime library.
//!
//! Modules:
//! - [`tool_parser`] — fallback `<|call|>` arg parser (defense in depth).
//! - [`cartridge`] — cartridge manifest types, registry, JSON-schema validation.
//! - [`parser`] — streaming ISA opcode parser / state machine (14 opcodes incl. `<|state|>`).
//! - [`dispatch`] — opcode → cartridge syscall routing.
//! - [`swap`] — ISA-aware context compactor with state recovery (§2 NEXT_STEPS).
//! - [`cloud`] — cloud-fallback HTTP adapter for `<|fault|>{"needs_cloud":true}`.
//! - [`iod`] — the I/O daemon main loop (streaming SSE consumer).

pub mod capability;
pub mod cartridge;
pub mod cloud;
pub mod dialect;
pub mod dispatch;
pub mod handlers;
pub mod iod;
pub mod mock_server;
pub mod multitask;
pub mod parser;
pub mod roclaw;
pub mod scheduler;
pub mod schema_to_gbnf;
pub mod subagent;
pub mod swap;
pub mod tool_parser;

pub use cartridge::{Cartridge, CartridgeRegistry, Manifest, MethodSpec};
pub use parser::{Opcode, OpcodeStream, Statement};
pub use swap::IsaState;
