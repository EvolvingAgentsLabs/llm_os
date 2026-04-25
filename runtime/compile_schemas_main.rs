//! `compile_schemas` binary — walks the cart root, compiles every
//! per-method args schema to GBNF, and writes `<method>.args.gbnf`
//! alongside each `<method>.args.schema.json`.
//!
//! Used both as a build step (run after editing schemas to refresh
//! artifacts) and as a v0.5 prep step (the artifacts will be loaded by
//! iod for mid-stream grammar swap).

use anyhow::{Context, Result};
use clap::Parser;
use llm_os_runtime::cartridge::CartridgeRegistry;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "compile_schemas", about = "Materialize per-method GBNF artifacts")]
struct Args {
    /// Cartridge root directory.
    #[arg(long, default_value = "cart")]
    cart: String,

    /// If set, write artifacts to this dir instead of next to schemas.
    #[arg(long)]
    out_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    let registry = CartridgeRegistry::discover(&args.cart)
        .with_context(|| format!("discovering cartridges in {}", args.cart))?;

    let mut written = 0;
    let mut skipped = 0;
    for cart_name in registry.names() {
        let cart = registry.get(cart_name).unwrap();
        for (method, gbnf) in cart.all_grammars() {
            let manifest_method = cart
                .manifest
                .methods
                .get(method)
                .expect("method present in manifest");
            let schema_rel = &manifest_method.args_schema;
            let schema_path = cart.root.join(schema_rel);
            let dest = if let Some(out_dir) = &args.out_dir {
                out_dir
                    .join(cart_name)
                    .join(format!("{method}.args.gbnf"))
            } else {
                schema_path.with_extension("gbnf")
            };
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, gbnf)?;
            written += 1;
            log::info!("wrote {}", dest.display());
        }
        // Methods that didn't compile get counted.
        skipped += cart.manifest.methods.len() - cart.all_grammars().count();
    }
    log::info!("compiled {written} sub-grammars; {skipped} methods fell back to runtime validation");
    Ok(())
}
