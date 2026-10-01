//! Offline worker. The indexer supplies independent chain code and identity.
use lithic_lithovm::verification::{verify, ChainCode, Request};
use std::io::Read;

fn bounded_read(path: &str) -> Result<Vec<u8>, String> {
    const LIMIT: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("input exceeds 4 MiB worker limit".into());
    }
    Ok(bytes)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "usage: lithverify REQUEST.json CHAIN_ID CONTRACT_ADDRESS CHAIN_CODE.bin".into(),
        );
    }
    let request: Request =
        serde_json::from_slice(&bounded_read(&args[0])?).map_err(|e| e.to_string())?;
    let chain_id = args[1].parse::<u64>().map_err(|e| e.to_string())?;
    let bytes = bounded_read(&args[3])?;
    let verified = verify(
        &request,
        ChainCode {
            chain_id,
            contract_address: &args[2],
            bytecode: &bytes,
        },
    )?;
    println!(
        "{}",
        serde_json::to_string(&verified).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lithverify: {error}");
        std::process::exit(1);
    }
}
