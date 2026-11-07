use lithic_lithovm::compile;
use lithovm_bytecode::{parse, Function, Instruction, Program, ReturnValue, ValueType};

#[test]
fn rejects_unsupported_binary_semantics_in_the_complete_contract() {
    for source in [
        "contract Bad { state { data: bytes; } pub fn x() -> u64 { return 1; } }",
        "contract Bad { state { data: map<address, bytes>; } pub fn x() -> u64 { return 1; } }",
        "contract Bad { state { data: map<bytes, u64>; } pub fn x() -> u64 { return 1; } }",
        "contract Bad { const DATA: bytes = 0x01; pub fn x() -> u64 { return 1; } }",
        "contract Bad { pub fn x() -> bytes { return 0x01; } }",
        "contract Bad { pub fn x(data: string) -> bytes32 { return keccak256(data); } }",
        "contract Bad { pub fn x(data: bytes32) -> bytes32 { return keccak256(data); } }",
        "contract Bad { pub fn x(data: bytes) -> bytes32 { return keccak256(); } }",
        "contract Bad { pub fn x(a: bytes, b: bytes) -> bytes32 { return keccak256(a, b); } }",
        "contract Bad { pub fn x(a: bytes, b: string) -> bool { return a == b; } }",
        "contract Bad { pub fn x(a: bytes) -> bytes { return a + a; } }",
        "contract Bad { pub fn x(a: bytes, target: address, selector: bytes32) -> bytes { call_contract(target, selector, 0); return a; } }",
    ] {
        assert!(compile(source).is_err(), "accepted unsupported source: {source}");
    }
}

#[test]
fn binary_local_and_hash_in_scalar_entrypoint_select_v15() {
    let source = "contract Hash { pub fn hash(target: address, selector: bytes32) -> bytes32 { let data: bytes = invoke(target, selector, 0); return keccak256(data); } }";
    let artifact = compile(source).unwrap();
    assert_eq!(artifact.bytecode_version, 15);
    assert_eq!(artifact.entrypoints[0].signature, "hash(address,bytes32)");
    let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
    for version in [11, 12, 13, 14] {
        let mut downgraded = bytes.clone();
        downgraded[7] = version;
        assert!(parse(&downgraded).is_err());
    }
    let source =
        "contract Hash { event Data { data: bytes } pub fn scalar() -> u64 { return 1; } }";
    assert_eq!(compile(source).unwrap().bytecode_version, 15);
    // Existing scalar and string programs keep their exact target version.
    assert_eq!(
        compile("contract Old { pub fn scalar() -> u64 { return 1; } }")
            .unwrap()
            .bytecode_version,
        11
    );
    assert_eq!(
        compile("contract Old { pub fn echo(data: string) -> string { return data; } }")
            .unwrap()
            .bytecode_version,
        12
    );
}

#[test]
fn bytecode_validator_rejects_wrong_hash_operands_and_stack_shapes() {
    for code in [
        vec![Instruction::Keccak256],
        vec![
            Instruction::Parameter(0),
            Instruction::Keccak256,
            Instruction::Keccak256,
        ],
        vec![
            Instruction::Constant(ValueType::U64, [0; 32]),
            Instruction::Keccak256,
        ],
        vec![
            Instruction::Parameter(0),
            Instruction::Parameter(0),
            Instruction::Keccak256,
        ],
    ] {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "hash".into(),
                parameters: vec![ValueType::Bytes],
                return_type: ValueType::Bytes32,
                return_value: ReturnValue::Expression(code),
            }],
        };
        assert!(program.encode().is_err());
    }
    let nested = format!(
        "contract Bad {{ pub fn hash(data: bytes) -> bytes32 {{ return {}data{}; }} }}",
        "keccak256(".repeat(200),
        ")".repeat(200)
    );
    assert!(compile(&nested).is_err());
}
