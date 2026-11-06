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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let native_record = args.first().is_some_and(|arg| arg == "--native-record");
    if native_record {
        args.remove(0);
    }
    if args.len() != 4 {
        return Err(
            "usage: lithverify [--native-record] REQUEST.json CHAIN_ID CONTRACT_ADDRESS CHAIN_CODE_OR_RECORD.bin".into(),
        );
    }
    let request: Request =
        serde_json::from_slice(&bounded_read(&args[0])?).map_err(|e| e.to_string())?;
    let chain_id = args[1].parse::<u64>().map_err(|e| e.to_string())?;
    let bytes = bounded_read(&args[3])?;
    let bytes = if native_record {
        lithovm_ffi::inspected_contract_bytecode(&bytes)?
    } else {
        bytes
    };
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
