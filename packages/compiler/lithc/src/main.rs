use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(name="lithc", version, about="Lithic compiler (scaffold)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile a .lithic file into LithoVM bytecode + LEP100 metadata (scaffold)
    Compile {
        /// Path to contract source (.lithic)
        input: PathBuf,
        /// Output directory
        #[arg(long, default_value = "./out")]
        out: PathBuf,
        /// Strict mode: enforce .lithic extension
        #[arg(long, default_value_t = true)]
        strict: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Lep100Metadata {
    lep: String,
    spec_version: String,
    contract_name: String,
    source_hash: String,
    generated_at: String,
    capabilities: Vec<String>,
    ai_services: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compile { input, out, strict } => compile(input, out, strict),
    }
}

fn compile(input: PathBuf, out: PathBuf, strict: bool) -> Result<()> {
    if strict {
        let ext_ok = input.extension().and_then(|e| e.to_str()) == Some("lithic");
        anyhow::ensure!(ext_ok, "Strict mode: input must end with .lithic");
    }

    let src = fs::read_to_string(&input).with_context(|| format!("read {}", input.display()))?;

    // Extremely small "parser" (scaffold):
    // - finds `contract <Name>`
    // - collects `requires ...` capabilities
    // - collects `ai.service <Name>` services
    let contract_name = parse_contract_name(&src).unwrap_or_else(|| "UnknownContract".to_string());
    let capabilities = parse_requires(&src);
    let ai_services = parse_ai_services(&src);

    // Source hash
    let mut h = Sha256::new();
    h.update(src.as_bytes());
    let source_hash = hex::encode(h.finalize());

    // Dummy bytecode: SHA256(source) prefix for deterministic artifact in scaffold
    let bytecode = make_dummy_bytecode(&source_hash);

    fs::create_dir_all(&out).with_context(|| format!("create out dir {}", out.display()))?;
    let bytecode_path = out.join(format!("{contract_name}.lithovm"));
    fs::write(&bytecode_path, &bytecode).with_context(|| format!("write {}", bytecode_path.display()))?;

    let meta = Lep100Metadata {
        lep: "LEP100".to_string(),
        spec_version: "100-1@draft".to_string(),
        contract_name: contract_name.clone(),
        source_hash,
        generated_at: chrono_like_now(),
        capabilities,
        ai_services,
    };

    let meta_path = out.join(format!("{contract_name}.lep100.json"));
    fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)
        .with_context(|| format!("write {}", meta_path.display()))?;

    println!("Compiled:");
    println!("  contract: {contract_name}");
    println!("  bytecode: {}", bytecode_path.display());
    println!("  metadata: {}", meta_path.display());
    Ok(())
}

fn parse_contract_name(src: &str) -> Option<String> {
    for line in src.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("contract ") {
            let name = rest.split_whitespace().next()?.trim().trim_matches('{').to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn parse_requires(src: &str) -> Vec<String> {
    for line in src.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("requires ") {
            return rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    vec![]
}

fn parse_ai_services(src: &str) -> Vec<String> {
    let mut out = vec![];
    for line in src.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("ai.service ") {
            if let Some(name) = rest.split_whitespace().next() {
                out.push(name.trim_matches('{').to_string());
            }
        }
    }
    out
}

fn make_dummy_bytecode(source_hash_hex: &str) -> Vec<u8> {
    // "LITHOVM" magic + 32 bytes hash
    let mut out = Vec::with_capacity(6 + 32);
    out.extend_from_slice(b"LITHOVM");
    let hash_bytes = hex::decode(source_hash_hex).unwrap_or_default();
    out.extend_from_slice(&hash_bytes[..hash_bytes.len().min(32)]);
    out
}

fn chrono_like_now() -> String {
    // No chrono dependency: keep scaffold minimal and deterministic-ish
    // Use UTC date at build-time-ish; good enough for scaffold. Replace in production.
    // Format: YYYY-MM-DDTHH:MM:SSZ (placeholder)
    "2026-02-23T00:00:00Z".to_string()
}
