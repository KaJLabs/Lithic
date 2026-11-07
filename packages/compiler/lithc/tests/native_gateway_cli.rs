use serde_json::Value;
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn native_receipt_and_persisted_record_workers_use_real_factory_fixture() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-factory.json")).unwrap();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lithic-gateway-{}-{unique}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let receipt = dir.join("receipt.json");
    let request = dir.join("request.json");
    let record = dir.join("record.bin");
    fs::write(&receipt, serde_json::to_vec(&fixture["receipt"]).unwrap()).unwrap();
    fs::write(
        &request,
        serde_json::to_vec(&fixture["verification"]).unwrap(),
    )
    .unwrap();
    let mut bytes = hex::decode(
        fixture["nativeRecord"]
            .as_str()
            .unwrap()
            .strip_prefix("0x")
            .unwrap(),
    )
    .unwrap();
    fs::write(&record, &bytes).unwrap();
    let decoded = Command::new(env!("CARGO_BIN_EXE_lithdev"))
        .arg("decode-receipt")
        .arg(&receipt)
        .args([
            "--chain-id",
            "700777",
            "--gateway",
            fixture["gateway"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&decoded.stdout).unwrap(),
        fixture["observation"]
    );
    let verify = || {
        Command::new(env!("CARGO_BIN_EXE_lithverify"))
            .arg("--native-record")
            .arg(&request)
            .args(["700777", fixture["contractAddress"].as_str().unwrap()])
            .arg(&record)
            .output()
            .unwrap()
    };
    let result = verify();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap(),
        fixture["verified"]
    );
    bytes[0] ^= 1;
    fs::write(&record, bytes).unwrap();
    let rejected = verify();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    // Preserve the old preparation gate: receipt decoding does not authorize a runtime.
    let rejected = Command::new(env!("CARGO_BIN_EXE_lithdev"))
        .arg("decode-receipt")
        .arg(&receipt)
        .args([
            "--chain-id",
            "9005",
            "--gateway",
            fixture["gateway"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    fs::remove_dir_all(dir).unwrap();
}
