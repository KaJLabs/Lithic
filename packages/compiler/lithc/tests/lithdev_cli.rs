use serde_json::Value;
use std::{path::PathBuf, process::Command};

#[test]
fn real_cli_reads_typed_argument_file_and_rejects_ambiguous_or_wrong_values() {
    use std::io::Write;
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../apps/examples/frontend/lithovm-storage.lithic");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("lithdev-args-{}-{unique}.json", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(br#"[{"type":"u64","value":"18446744073709551615"}]"#)
        .unwrap();
    drop(file);
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_lithdev"))
            .args([
                "prepare-call",
                source.to_str().unwrap(),
                "--chain-id",
                "700777",
                "--contract",
                "0x1111111111111111111111111111111111111111",
                "--function",
                "set",
                "--args",
                path.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let result = invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["nativeSignature"], "set(u64)");
    assert_eq!(value["deploymentStatus"], "not-submitted");
    for invalid in [
        r#"[{"type":"u64","value":42}]"#,
        r#"[{"type":"u64","type":"u64","value":"1"}]"#,
        r#"[{"type":"u256","value":"1"}]"#,
        r#"[{"type":"u64","value":"18446744073709551616"}]"#,
    ] {
        std::fs::write(&path, invalid).unwrap();
        let result = invoke();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn real_cli_prepares_native_artifact_and_rejects_mainnet_profile() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../apps/examples/frontend/lithovm-constants.lithic");
    let output = Command::new(env!("CARGO_BIN_EXE_lithdev"))
        .args([
            "prepare-deploy",
            source.to_str().unwrap(),
            "--chain-id",
            "700777",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["artifact"]["contractName"], "Roles");
    assert_eq!(value["executionAuthorized"], false);
    assert_eq!(value["value"], "0");
    assert!(value["data"].as_str().unwrap().len() > 200);
    let rejected = Command::new(env!("CARGO_BIN_EXE_lithdev"))
        .args([
            "prepare-deploy",
            source.to_str().unwrap(),
            "--chain-id",
            "9005",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("700777"));
}
