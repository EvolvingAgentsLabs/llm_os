//! Cartridge manifest types, registry, and per-method JSON-schema validation.
//!
//! A cartridge is a sealed bundle of methods (the syscalls callable via
//! `<|call|>cart.method {…}`) plus the schemas that constrain their args
//! and (in v0.1+) results. Each cartridge declares its own subset of the
//! 13 ISA opcodes it is permitted to be the target of (currently only
//! `<|call|>` matters since cartridges are dispatch destinations).
//!
//! Layout — see [`docs/cartridge-manifest.md`](../docs/cartridge-manifest.md):
//!
//! ```text
//! cart/
//!   index.md
//!   <domain>/
//!     index.md
//!     <name>/
//!       manifest.json
//!       full.md            (per-cartridge spec, lazy-loaded)
//!       schemas/*.json     (per-method JSON Schemas)
//! ```

use jsonschema::JSONSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum CartridgeError {
    #[error("manifest not found at {0}")]
    ManifestMissing(PathBuf),
    #[error("manifest parse failed: {0}")]
    ManifestParse(#[from] serde_json::Error),
    #[error("schema compile failed for {method}: {reason}")]
    SchemaCompile {
        method: String,
        reason: String,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown method '{0}'")]
    UnknownMethod(String),
    #[error("schema validation failed for '{method}': {errors:?}")]
    SchemaViolation { method: String, errors: Vec<String> },
}

/// Cartridge manifest. Top-level shape stored at `cart/<domain>/<name>/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// Free-text description shown to the model in the cartridge index.
    #[serde(default)]
    pub description: String,
    /// ISA opcodes the cartridge is permitted to be a dispatch target of.
    /// For v0.01 this is informational; opcode-level capability enforcement
    /// via logit bias lands in v0.1.
    #[serde(default = "default_opcodes")]
    pub allowed_opcodes: Vec<String>,
    /// Routing hint for tier-aware dispatch. Mirrors skillos_mini's
    /// `preferred_tier`. v0.01 surfaces this in `dispatch.rs` but always
    /// routes locally; v0.1 wires the proactive cloud path.
    #[serde(default = "default_tier")]
    pub preferred_tier: String,
    pub methods: HashMap<String, MethodSpec>,
    /// Cross-blackboard invariant checks fired on `<|halt|>`. Names resolve
    /// against the `BUILTIN_VALIDATORS` registry. Mirrors skillos_mini's
    /// post-flow validators.
    #[serde(default)]
    pub post_validators: Vec<String>,
    /// Reserved for v0.5 — flags a cartridge that itself runs sub-inference
    /// via `__skillos.llm`-style proxy. Always `false` in v0.01.
    #[serde(default)]
    pub subagent: bool,
}

fn default_opcodes() -> Vec<String> {
    vec!["call".to_string()]
}
fn default_tier() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodSpec {
    /// Path to the args JSON Schema, relative to cartridge dir.
    pub args_schema: String,
    /// Optional path to the result JSON Schema. v0.01 doesn't enforce
    /// result schemas; v0.1 compiles them to GBNF sub-grammars.
    #[serde(default)]
    pub result_schema: Option<String>,
    /// Free-text description of the method, included in cartridge full.md.
    #[serde(default)]
    pub description: String,
}

/// A loaded cartridge: manifest + compiled JSON Schemas + compiled
/// per-method GBNF sub-grammars (v0.1).
pub struct Cartridge {
    pub manifest: Manifest,
    pub root: PathBuf,
    schemas: HashMap<String, JSONSchema>,
    /// Raw schema JSON per method, for extracting required arg names
    /// in the Ollama boot prompt (no GBNF to enforce format).
    raw_schemas: HashMap<String, Value>,
    /// Per-method compiled GBNF, keyed by method name. Empty for methods
    /// whose schema can't be expressed in the v0.1 GBNF subset (those
    /// fall through to runtime validation only).
    grammars: HashMap<String, String>,
}

impl Cartridge {
    pub fn load<P: AsRef<Path>>(dir: P) -> Result<Self, CartridgeError> {
        let dir = dir.as_ref();
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.exists() {
            return Err(CartridgeError::ManifestMissing(manifest_path));
        }
        let manifest: Manifest = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;

        let mut schemas = HashMap::new();
        let mut raw_schemas = HashMap::new();
        let mut grammars = HashMap::new();
        for (name, spec) in &manifest.methods {
            let schema_path = dir.join(&spec.args_schema);
            let schema_json: Value =
                serde_json::from_str(&std::fs::read_to_string(&schema_path)?)?;
            let compiled = JSONSchema::options()
                .with_draft(jsonschema::Draft::Draft7)
                .compile(&schema_json)
                .map_err(|e| CartridgeError::SchemaCompile {
                    method: name.clone(),
                    reason: format!("{e}"),
                })?;
            schemas.insert(name.clone(), compiled);
            raw_schemas.insert(name.clone(), schema_json.clone());

            // v0.1: try to compile a GBNF sub-grammar. Falls through silently
            // if the schema uses constructs outside the v0.1 subset; the
            // runtime jsonschema validator still catches violations.
            let grammar_root = format!("{}-{name}-args", manifest.name);
            match crate::schema_to_gbnf::compile(&schema_json, &grammar_root) {
                Ok(gbnf) => {
                    grammars.insert(name.clone(), gbnf);
                }
                Err(e) => {
                    log::debug!(
                        "{}.{name} schema not GBNF-compilable in v0.1 subset: {e}",
                        manifest.name
                    );
                }
            }
        }

        Ok(Cartridge {
            manifest,
            root: dir.to_path_buf(),
            schemas,
            raw_schemas,
            grammars,
        })
    }

    /// Compiled GBNF for a method's args schema, or `None` if the schema
    /// uses constructs outside the v0.1 subset.
    pub fn args_grammar(&self, method: &str) -> Option<&str> {
        self.grammars.get(method).map(String::as_str)
    }

    /// All compiled grammars, for materialization to disk.
    pub fn all_grammars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.grammars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Returns a compact args hint for a method, e.g. `{household_size: int, dietary_notes?: str}`.
    /// Used in the Ollama boot prompt so the model knows exact field names.
    pub fn args_hint(&self, method: &str) -> Option<String> {
        let schema = self.raw_schemas.get(method)?;
        let props = schema.get("properties")?.as_object()?;
        let required: Vec<&str> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let mut parts = Vec::new();
        for (k, v) in props {
            let ty = v
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("any");
            let short_ty = match ty {
                "integer" => "int",
                "string" => "str",
                "number" => "num",
                "boolean" => "bool",
                "array" => "arr",
                "object" => "obj",
                other => other,
            };
            if required.contains(&k.as_str()) {
                parts.push(format!("{k}:{short_ty}"));
            } else {
                parts.push(format!("{k}?:{short_ty}"));
            }
        }
        Some(format!("{{{}}}", parts.join(", ")))
    }

    pub fn validate_args(&self, method: &str, args: &Value) -> Result<(), CartridgeError> {
        let schema = self
            .schemas
            .get(method)
            .ok_or_else(|| CartridgeError::UnknownMethod(method.to_string()))?;
        let result = schema.validate(args);
        if let Err(errs) = result {
            let messages: Vec<String> = errs.map(|e| format!("{e}")).collect();
            return Err(CartridgeError::SchemaViolation {
                method: method.to_string(),
                errors: messages,
            });
        }
        Ok(())
    }
}

/// In-memory cartridge registry. Keys are cartridge names from the manifest.
pub struct CartridgeRegistry {
    cartridges: HashMap<String, Cartridge>,
}

impl Default for CartridgeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CartridgeRegistry {
    pub fn new() -> Self {
        Self {
            cartridges: HashMap::new(),
        }
    }

    /// Walk a cart root (e.g. `cart/`), load every cartridge it finds.
    /// Recurses one level into domain subdirs (`cart/<domain>/<name>/`).
    pub fn discover<P: AsRef<Path>>(root: P) -> Result<Self, CartridgeError> {
        let mut reg = Self::new();
        let root = root.as_ref();
        if !root.exists() {
            return Ok(reg);
        }
        for domain in std::fs::read_dir(root)? {
            let domain = domain?;
            if !domain.file_type()?.is_dir() {
                continue;
            }
            for cart_dir in std::fs::read_dir(domain.path())? {
                let cart_dir = cart_dir?;
                if !cart_dir.file_type()?.is_dir() {
                    continue;
                }
                if !cart_dir.path().join("manifest.json").exists() {
                    continue;
                }
                let cart = Cartridge::load(cart_dir.path())?;
                reg.cartridges.insert(cart.name().to_string(), cart);
            }
        }
        Ok(reg)
    }

    pub fn get(&self, name: &str) -> Option<&Cartridge> {
        self.cartridges.get(name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.cartridges.keys().map(String::as_str).collect()
    }

    pub fn len(&self) -> usize {
        self.cartridges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cartridges.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_through_serde() {
        let raw = r#"{
            "name": "demo",
            "version": "0.0.1",
            "description": "test cartridge",
            "allowed_opcodes": ["call"],
            "preferred_tier": "auto",
            "methods": {
                "echo": {
                    "args_schema": "schemas/echo.args.json",
                    "description": "return what was sent"
                }
            }
        }"#;
        let m: Manifest = serde_json::from_str(raw).unwrap();
        assert_eq!(m.name, "demo");
        assert_eq!(m.preferred_tier, "auto");
        assert_eq!(m.methods.get("echo").unwrap().args_schema, "schemas/echo.args.json");
        assert!(!m.subagent);
    }

    #[test]
    fn defaults_apply_when_optional_fields_missing() {
        let raw = r#"{
            "name": "minimal",
            "version": "0.0.1",
            "methods": {}
        }"#;
        let m: Manifest = serde_json::from_str(raw).unwrap();
        assert_eq!(m.allowed_opcodes, vec!["call".to_string()]);
        assert_eq!(m.preferred_tier, "auto");
        assert!(m.post_validators.is_empty());
    }
}
