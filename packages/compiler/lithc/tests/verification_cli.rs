use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn offline_worker_verifies_code_and_returns_failure_for_tampering() {
    let dir = std::env::temp_dir().join(format!(
        "lithverify-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let source = "contract C { pub fn answer() -> u64 { return 42; } }";
    let artifact = lithic_lithovm::compile(source).unwrap();
    let request = serde_json::json!({
        "schemaVersion": "lithovm-verification-v1", "chainId": 700777,
        "contractAddress": "0x0000000000000000000000000000000000000007",
        "codeHash": artifact.code_hash, "artifact": artifact,
        "sources": [{"path":"C.lithic", "content":source, "keccak256":artifact.source_hash}]
    });
    let request_file = dir.join("request.json");
    let code_file = dir.join("code.bin");
    fs::write(&request_file, serde_json::to_vec(&request).unwrap()).unwrap();
    let bytes: Vec<u8> = artifact.bytecode.as_bytes()[2..]
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    fs::write(&code_file, bytes).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_lithverify"))
            .arg(&request_file)
            .args(["700777", "0x0000000000000000000000000000000000000007"])
            .arg(&code_file)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["rebuiltCodeHash"], artifact.code_hash);
    fs::write(&code_file, b"changed chain code").unwrap();
    let failed = run();
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    fs::remove_file(request_file).unwrap();
    fs::remove_file(code_file).unwrap();
    fs::remove_dir(dir).unwrap();
}
