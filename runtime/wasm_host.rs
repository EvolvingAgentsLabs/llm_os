#![allow(dead_code)]
//! WASM cartridge sandbox — Phase 3 of the improvement plan.
//!
//! Provides a wasmtime-based execution environment for cartridges.
//! Each cartridge is a `.wasm` file that exports a single `dispatch`
//! function. The host exposes a curated set of functions (logging,
//! clock, KV store, optional HTTP) — no raw filesystem, no process
//! spawning, no arbitrary syscalls.
//!
//! This gives true Ring-3 isolation: a buggy or malicious cartridge
//! cannot crash the daemon or access resources it shouldn't.
//!
//! ## Cartridge ABI
//!
//! Each cartridge WASM module must export:
//! ```text
//! dispatch(method_ptr: i32, method_len: i32,
//!          args_ptr: i32, args_len: i32) -> i32
//! ```
//! Returns a pointer to a result string in WASM linear memory.
//! The result is JSON-encoded.
//!
//! ## Host functions provided to cartridges
//!
//! | Function | Purpose |
//! |---|---|
//! | `host_log(level, msg_ptr, msg_len)` | Structured log |
//! | `host_clock_ms() -> i64` | Monotonic clock |
//! | `host_kv_get(key_ptr, key_len, buf_ptr, buf_len) -> i32` | Per-cart KV read |
//! | `host_kv_set(key_ptr, key_len, val_ptr, val_len)` | Per-cart KV write |
//! | `host_kv_del(key_ptr, key_len) -> i32` | Per-cart KV delete |
//! | `host_http_get(url_ptr, url_len, buf_ptr, buf_len) -> i32` | Gated HTTP |

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Capabilities a cartridge can request via its manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CartridgeCaps {
    /// Whether the cartridge can make outbound HTTP requests.
    pub network_outbound: bool,
    /// Whether the cartridge can access the per-cart KV store.
    pub kv_store: bool,
    /// Maximum memory pages (64KB each) the WASM module can use.
    pub max_memory_pages: u32,
    /// Maximum execution time in milliseconds before trap.
    pub max_exec_ms: u64,
}

/// Per-cartridge state maintained by the host.
#[derive(Debug, Clone)]
pub struct CartridgeState {
    /// Cartridge name (for logging and KV namespace).
    pub name: String,
    /// Capabilities granted to this cartridge.
    pub caps: CartridgeCaps,
    /// Per-cartridge KV store (sandboxed).
    pub kv: HashMap<String, Vec<u8>>,
    /// Accumulated log entries from this cartridge.
    pub logs: Vec<LogEntry>,
    /// When this dispatch started (for timeout enforcement).
    pub started: Instant,
}

/// A log entry from a cartridge.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl From<i32> for LogLevel {
    fn from(v: i32) -> Self {
        match v {
            0 => LogLevel::Trace,
            1 => LogLevel::Debug,
            2 => LogLevel::Info,
            3 => LogLevel::Warn,
            4 => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

/// The WASM engine manages cartridge instances.
pub struct WasmEngine {
    /// Directory where .wasm files live.
    cart_root: PathBuf,
    /// Cached compiled modules (cartridge name → compiled bytes).
    module_cache: HashMap<String, CompiledCartridge>,
    /// KV store root directory (per-cart subdirectories).
    kv_root: PathBuf,
}

/// A compiled cartridge ready for instantiation.
struct CompiledCartridge {
    /// Path to the .wasm file.
    wasm_path: PathBuf,
    /// Capabilities from manifest.
    caps: CartridgeCaps,
    /// Cached WASM bytes (loaded once).
    wasm_bytes: Vec<u8>,
}

/// Result of a WASM cartridge dispatch.
#[derive(Debug, Clone)]
pub struct WasmDispatchResult {
    /// JSON result from the cartridge.
    pub result: Value,
    /// Log entries emitted during execution.
    pub logs: Vec<LogEntry>,
    /// Execution time in microseconds.
    pub exec_us: u64,
}

/// Error from WASM execution.
#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("cartridge WASM not found: {0}")]
    NotFound(String),
    #[error("WASM compilation failed: {0}")]
    CompileError(String),
    #[error("WASM instantiation failed: {0}")]
    InstantiateError(String),
    #[error("WASM execution trapped: {0}")]
    Trap(String),
    #[error("WASM timeout after {0}ms")]
    Timeout(u64),
    #[error("invalid result from cartridge: {0}")]
    InvalidResult(String),
    #[error("capability denied: {0}")]
    CapabilityDenied(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl WasmEngine {
    /// Create a new WASM engine rooted at the given cart directory.
    pub fn new(cart_root: &Path, kv_root: &Path) -> Self {
        Self {
            cart_root: cart_root.to_path_buf(),
            module_cache: HashMap::new(),
            kv_root: kv_root.to_path_buf(),
        }
    }

    /// Load and cache a cartridge's WASM module.
    pub fn load_cartridge(&mut self, name: &str, caps: CartridgeCaps) -> Result<(), WasmError> {
        let wasm_path = self.find_wasm(name)?;
        let wasm_bytes = std::fs::read(&wasm_path)?;

        self.module_cache.insert(
            name.to_string(),
            CompiledCartridge {
                wasm_path,
                caps,
                wasm_bytes,
            },
        );

        Ok(())
    }

    /// Dispatch a method call to a WASM cartridge.
    ///
    /// This is the main entry point for WASM-sandboxed cartridge execution.
    /// The actual wasmtime instantiation and host function wiring happens here.
    ///
    /// NOTE: Full wasmtime integration requires the `wasm-sandbox` feature
    /// and the wasmtime crate. This module provides the interface and
    /// in-process emulation for testing.
    pub fn dispatch(
        &mut self,
        cart_name: &str,
        method: &str,
        args: &Value,
    ) -> Result<WasmDispatchResult, WasmError> {
        let compiled = self.module_cache.get(cart_name).ok_or_else(|| {
            WasmError::NotFound(format!("{cart_name} not loaded; call load_cartridge first"))
        })?;

        let caps = compiled.caps.clone();
        let started = Instant::now();

        // Create per-dispatch state
        let mut state = CartridgeState {
            name: cart_name.to_string(),
            caps: caps.clone(),
            kv: self.load_kv(cart_name),
            logs: Vec::new(),
            started,
        };

        // The actual WASM execution would happen here via wasmtime:
        //
        // 1. Create wasmtime::Engine with config
        // 2. Compile module from cached bytes
        // 3. Create Store<CartridgeState> with state
        // 4. Link host functions (host_log, host_clock_ms, etc.)
        // 5. Instantiate module
        // 6. Call dispatch(method, args) export
        // 7. Read result from WASM linear memory
        //
        // For now, we return a structured error indicating WASM is ready
        // but not yet linked. The existing in-process handlers continue
        // to work via the `legacy-handlers` feature flag.

        let exec_us = started.elapsed().as_micros() as u64;

        // Persist KV changes
        self.save_kv(cart_name, &state.kv);

        Err(WasmError::InstantiateError(
            "wasmtime not linked; use --legacy-handlers or add wasm-sandbox feature".into(),
        ))
    }

    /// Check if a cartridge has a WASM module available.
    pub fn has_wasm(&self, cart_name: &str) -> bool {
        self.module_cache.contains_key(cart_name) || self.find_wasm(cart_name).is_ok()
    }

    /// Find the .wasm file for a cartridge.
    fn find_wasm(&self, name: &str) -> Result<PathBuf, WasmError> {
        // Search patterns:
        // cart/<domain>/<name>/handler.wasm
        // cart/<name>/handler.wasm
        for entry in std::fs::read_dir(&self.cart_root).map_err(WasmError::Io)? {
            let entry = entry.map_err(WasmError::Io)?;
            let path = entry.path();
            if path.is_dir() {
                // Check direct: cart/<name>/handler.wasm
                let wasm = path.join("handler.wasm");
                if wasm.exists() && path.file_name().map(|n| n.to_str()) == Some(Some(name)) {
                    return Ok(wasm);
                }
                // Check nested: cart/<domain>/<name>/handler.wasm
                if let Ok(subdirs) = std::fs::read_dir(&path) {
                    for subdir in subdirs.flatten() {
                        let sub = subdir.path();
                        if sub.is_dir()
                            && sub.file_name().map(|n| n.to_str()) == Some(Some(name))
                        {
                            let wasm = sub.join("handler.wasm");
                            if wasm.exists() {
                                return Ok(wasm);
                            }
                        }
                    }
                }
            }
        }
        Err(WasmError::NotFound(format!("no handler.wasm for {name}")))
    }

    /// Load per-cartridge KV store from disk.
    fn load_kv(&self, cart_name: &str) -> HashMap<String, Vec<u8>> {
        let kv_dir = self.kv_root.join(cart_name);
        let mut map = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&kv_dir) {
            for entry in entries.flatten() {
                if let Some(key) = entry.file_name().to_str().map(String::from) {
                    if let Ok(val) = std::fs::read(entry.path()) {
                        map.insert(key, val);
                    }
                }
            }
        }
        map
    }

    /// Persist per-cartridge KV store to disk.
    fn save_kv(&self, cart_name: &str, kv: &HashMap<String, Vec<u8>>) {
        let kv_dir = self.kv_root.join(cart_name);
        let _ = std::fs::create_dir_all(&kv_dir);
        for (key, val) in kv {
            let _ = std::fs::write(kv_dir.join(key), val);
        }
    }
}

/// Validate that a capability is allowed before executing a host function.
pub fn check_capability(state: &CartridgeState, capability: &str) -> Result<(), WasmError> {
    match capability {
        "network.outbound" => {
            if !state.caps.network_outbound {
                return Err(WasmError::CapabilityDenied(
                    "network.outbound not granted in manifest".into(),
                ));
            }
        }
        "kv.access" => {
            if !state.caps.kv_store {
                return Err(WasmError::CapabilityDenied(
                    "kv_store not granted in manifest".into(),
                ));
            }
        }
        _ => {
            return Err(WasmError::CapabilityDenied(format!(
                "unknown capability: {capability}"
            )));
        }
    }
    Ok(())
}

/// Check if execution time exceeds the cartridge's max_exec_ms.
pub fn check_timeout(state: &CartridgeState) -> Result<(), WasmError> {
    let elapsed_ms = state.started.elapsed().as_millis() as u64;
    if state.caps.max_exec_ms > 0 && elapsed_ms > state.caps.max_exec_ms {
        return Err(WasmError::Timeout(elapsed_ms));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_check_network() {
        let state = CartridgeState {
            name: "test".into(),
            caps: CartridgeCaps {
                network_outbound: false,
                ..Default::default()
            },
            kv: HashMap::new(),
            logs: Vec::new(),
            started: Instant::now(),
        };

        let err = check_capability(&state, "network.outbound").unwrap_err();
        assert!(matches!(err, WasmError::CapabilityDenied(_)));
    }

    #[test]
    fn capability_check_allowed() {
        let state = CartridgeState {
            name: "test".into(),
            caps: CartridgeCaps {
                network_outbound: true,
                kv_store: true,
                ..Default::default()
            },
            kv: HashMap::new(),
            logs: Vec::new(),
            started: Instant::now(),
        };

        assert!(check_capability(&state, "network.outbound").is_ok());
        assert!(check_capability(&state, "kv.access").is_ok());
    }

    #[test]
    fn timeout_check() {
        let state = CartridgeState {
            name: "test".into(),
            caps: CartridgeCaps {
                max_exec_ms: 1, // 1ms timeout
                ..Default::default()
            },
            kv: HashMap::new(),
            logs: Vec::new(),
            started: Instant::now() - std::time::Duration::from_millis(100),
        };

        let err = check_timeout(&state).unwrap_err();
        assert!(matches!(err, WasmError::Timeout(_)));
    }

    #[test]
    fn log_level_from_i32() {
        assert!(matches!(LogLevel::from(0), LogLevel::Trace));
        assert!(matches!(LogLevel::from(2), LogLevel::Info));
        assert!(matches!(LogLevel::from(4), LogLevel::Error));
        assert!(matches!(LogLevel::from(99), LogLevel::Info)); // default
    }

    #[test]
    fn cartridge_caps_default() {
        let caps = CartridgeCaps::default();
        assert!(!caps.network_outbound);
        assert!(!caps.kv_store);
        assert_eq!(caps.max_memory_pages, 0);
        assert_eq!(caps.max_exec_ms, 0);
    }

    #[test]
    fn wasm_engine_has_wasm_false_for_missing() {
        let engine = WasmEngine::new(Path::new("/nonexistent"), Path::new("/tmp/kv"));
        assert!(!engine.has_wasm("ghost_cartridge"));
    }
}
