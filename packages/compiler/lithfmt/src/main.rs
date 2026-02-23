use anyhow::Result;
use clap::Parser;
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(name="lithfmt", version, about="Lithic formatter (scaffold)")]
struct Cli {
  /// Input file (.lithic)
  input: PathBuf,
}

fn main() -> Result<()> {
  let cli = Cli::parse();
  let src = fs::read_to_string(&cli.input)?;
  // Scaffold: trim trailing spaces and ensure newline
  let formatted = src
    .lines()
    .map(|l| l.trim_end())
    .collect::<Vec<_>>()
    .join("\n") + "\n";
  fs::write(&cli.input, formatted)?;
  Ok(())
}
