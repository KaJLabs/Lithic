use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn source_file(label: &str, source: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "lithc-{label}-{}-{nonce}.lithic",
        std::process::id()
    ));
    fs::write(&path, source).expect("write Lithic fixture");
    path
}

#[test]
fn check_mode_accepts_a_clean_contract() {
    let path = source_file("clean", "contract C { state { value: u256; } }");
    let output = Command::new(env!("CARGO_BIN_EXE_lithc"))
        .args(["--emit", "check"])
        .arg(&path)
        .output()
        .expect("run lithc");
    fs::remove_file(&path).expect("remove Lithic fixture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("declaration checks clean"));
    assert!(output.stdout.is_empty());
}

#[test]
fn check_mode_rejects_an_unambiguous_name_collision() {
    let path = source_file(
        "duplicate",
        "contract C { state { value: u256; } state { value: bool; } }",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_lithc"))
        .args(["--emit", "check"])
        .arg(&path)
        .output()
        .expect("run lithc");
    fs::remove_file(&path).expect("remove Lithic fixture");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[E002] duplicate state field `value`"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("aborting due to 1 declaration error(s)"));
}

#[test]
fn evm_mode_emits_a_complete_artifact() {
    let path = source_file(
        "evm",
        "contract C { pub fn answer() -> u64 { return 42; } }",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_lithc"))
        .args(["--emit", "evm"])
        .arg(&path)
        .output()
        .expect("run lithc");
    fs::remove_file(&path).expect("remove Lithic fixture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"target\": \"evm-lithosphere-9005-v1\""));
    assert!(stdout.contains("\"signature\": \"answer()\""));
    assert!(stdout.contains("\"bytecode\": \"0x"));
    assert!(stdout.contains("\"deployedBytecode\": \"0x"));
}

#[test]
fn evm_mode_rejects_unsupported_stateful_source() {
    let path = source_file(
        "evm-state",
        "contract C { state { value: u64; } pub fn answer() -> u64 { return 42; } }",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_lithc"))
        .args(["--emit", "evm"])
        .arg(&path)
        .output()
        .expect("run lithc");
    fs::remove_file(&path).expect("remove Lithic fixture");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("state fields are not supported by the v1 EVM backend"));
    assert!(output.stdout.is_empty());
}

#[test]
fn lithovm_mode_emits_a_versioned_native_artifact() {
    let path = source_file(
        "lithovm",
        "contract C { pub fn answer() -> u64 { return 42; } }",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_lithc"))
        .args(["--emit", "lithovm"])
        .arg(&path)
        .output()
        .expect("run lithc");
    fs::remove_file(&path).expect("remove Lithic fixture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"target\": \"lithovm-native-v11\""));
    assert!(stdout.contains("\"artifactVersion\": 1"));
    assert!(stdout.contains("\"compilerVersion\": \"0.2.0\""));
    assert!(stdout.contains("\"bytecodeVersion\": 11"));
    assert!(stdout.contains("\"sourceHash\": \"0x"));
    assert!(stdout.contains("\"codeHash\": \"0x"));
    assert!(stdout.contains("\"entrypoints\": ["));
    assert!(stdout.contains("\"bytecode\": \"0x4c4954484f564d0b"));
}

#[test]
fn lithovm_mode_accepts_typed_map_storage() {
    let path = source_file(
        "lithovm-state",
        "contract C { state { values: map<address, u64>; } pub fn answer() -> u64 { return 42; } }",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_lithc"))
        .args(["--emit", "lithovm"])
        .arg(&path)
        .output()
        .expect("run lithc");
    fs::remove_file(&path).expect("remove Lithic fixture");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"bytecodeVersion\": 11"));
}
