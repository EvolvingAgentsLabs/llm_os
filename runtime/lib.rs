//! LLM-OS v0.01 runtime library.
//!
//! Modules:
//! - [`tool_parser`] — fallback `<|call|>` arg parser (defense in depth).
//! - [`cartridge`] — cartridge manifest types, registry, JSON-schema validation.
//! - [`parser`] — streaming ISA opcode parser / state machine.
//! - [`dispatch`] — opcode → cartridge syscall routing.
//! - [`swap`] — context compactor (port of skillos_mini compactor.ts).
//! - [`cloud`] — cloud-fallback HTTP adapter for `<|fault|>{"needs_cloud":true}`.
//! - [`iod`] — the I/O daemon main loop (streaming SSE consumer).

pub mod cartridge;
pub mod cloud;
pub mod dialect;
pub mod dispatch;
pub mod handlers;
pub mod iod;
pub mod parser;
pub mod roclaw;
pub mod schema_to_gbnf;
pub mod swap;
pub mod tool_parser;

pub use cartridge::{Cartridge, CartridgeRegistry, Manifest, MethodSpec};
pub use parser::{Opcode, OpcodeStream, Statement};
