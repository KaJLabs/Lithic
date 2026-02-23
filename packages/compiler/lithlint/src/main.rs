use anyhow::{anyhow, Result};
use clap::Parser;
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(name="lithlint", version, about="Lithic linter (scaffold)")]
struct Cli {
  /// Input file (.lithic)
  input: PathBuf,
}

fn main() -> Result<()> {
  let cli = Cli::parse();
  let src = fs::read_to_string(&cli.input)?;

  // Scaffold checks:
  // - must declare contract
  // - if ai.request exists, require timeout handler mention
  // - warn if capabilities missing for AI
  if !src.lines().any(|l| l.trim_start().starts_with("contract ")) {
    return Err(anyhow!("No `contract <Name>` declaration found."));
  }

  let uses_ai = src.contains("ai.request");
  if uses_ai && !(src.contains(".timeout(") || src.contains("timeout(")) {
    eprintln!("[warn] ai.request found but no timeout handler detected.");
  }

  if uses_ai && !src.contains("requires") {
    eprintln!("[warn] ai.request found but no `requires AI_CALL` capability declaration detected.");
  }

  Ok(())
}
