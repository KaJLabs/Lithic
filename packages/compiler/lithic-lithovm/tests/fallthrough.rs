use lithic_lithovm::compile;
use lithovm::Vm;

fn word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

#[test]
fn branch_fallthrough_and_early_return_preserve_outer_locals() {
    let artifact = compile("contract C { pub fn run(flag: bool, early: bool) -> u64 { let mut result: u64 = 2; if flag { let local: u64 = 3; result = result + local; } if early { return result; } else {} result = result + 1; return result; } }").unwrap();
    let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
    for (flag, early, expected) in [(0, 0, 3), (0, 1, 2), (1, 0, 6), (1, 1, 5)] {
        let result = Vm::default()
            .execute(&bytes, "run", &[word(flag), word(early)], 1000)
            .unwrap();
        assert_eq!(result.return_value, word(expected));
    }
}

#[test]
fn missing_returns_unreachable_code_and_branch_local_escape_still_reject() {
    for source in [
        "contract C { pub fn run(flag: bool) -> u64 { if flag { return 1; } } }",
        "contract C { pub fn run(flag: bool) -> u64 { if flag { return 1; } else { revert(); } return 2; } }",
        "contract C { pub fn run(flag: bool) -> u64 { if flag { let local: u64 = 1; } return local; } }",
    ] { assert!(compile(source).is_err(), "accepted {source}"); }
}
