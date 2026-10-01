use lithic_lithovm::compile;
use lithovm::{ExecutionContext, ExecutionOutcome, FailureKind, Storage, Vm};
use lithovm_bytecode::{parse, values::Value, ValueType};

const SOURCE: &str = r#"contract Metadata {
    state { name: string; }
    event Named { name: string }
    pub fn echo(name: string) -> string { return name; }
    pub fn name() -> string { return self.name; }
    pub fn set(name: string, accept: bool) -> string {
        let mut next: string = name;
        if accept { next = name; }
        self.name = next;
        emit Named { name: self.name };
        require(accept);
        return self.name;
    }
    pub fn equal(a: string, b: string) -> bool { return a == b; }
}"#;

#[test]
fn map_only_programs_require_explicit_storage() {
    let artifact = compile("contract C { state { balances: map<address, u256>; } pub fn balance(account: address) -> u256 { return self.balances[account]; } }").unwrap();
    let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
    let vm = Vm::default();
    assert!(vm.execute(&bytes, "balance", &[[0; 32]], 1000).is_err());
    assert!(vm
        .execute_with_context(
            &bytes,
            "balance",
            &[[0; 32]],
            1000,
            &ExecutionContext::default()
        )
        .is_err());
    assert!(vm
        .execute_with_storage(&bytes, "balance", &[[0; 32]], 1000, &mut Storage::default())
        .is_ok());
}

fn bytes() -> Vec<u8> {
    let artifact = compile(SOURCE).unwrap();
    assert_eq!(artifact.target, "lithovm-native-v12");
    assert_eq!(artifact.bytecode_version, 12);
    hex::decode(&artifact.bytecode[2..]).unwrap()
}
fn boolean(value: bool) -> Value {
    let mut word = [0; 32];
    word[31] = u8::from(value);
    Value::Word(ValueType::Bool, word)
}
fn call(
    bytes: &[u8],
    storage: &mut Storage,
    name: &str,
    args: &[Value],
    gas: u64,
) -> ExecutionOutcome<Value> {
    Vm::default().execute_values_transactionally(
        bytes,
        name,
        args,
        gas,
        storage,
        &ExecutionContext::default(),
    )
}

#[test]
fn strings_roundtrip_through_arguments_locals_storage_events_and_returns() {
    let bytes = bytes();
    let mut storage = Storage::default();
    for name in [
        String::new(),
        "A\u{e9} e\u{301}\0\u{1faa8}".into(),
        "\u{e9}".repeat(2048),
    ] {
        let value = Value::String(name.clone());
        let ExecutionOutcome::Success(result) = call(
            &bytes,
            &mut storage,
            "set",
            &[value.clone(), boolean(true)],
            100000,
        ) else {
            panic!("set failed")
        };
        assert_eq!(result.return_value, value);
        assert_eq!(
            result.events[0].fields[0],
            ("name".into(), ValueType::String, value.clone())
        );
        assert_eq!(storage.get_string("name"), Some(name.as_str()));
        let ExecutionOutcome::Success(result) = call(&bytes, &mut storage, "name", &[], 100000)
        else {
            panic!("read failed")
        };
        assert_eq!(result.return_value, value);
    }
    for (a, b, expected) in [
        ("same", "same", true),
        ("\u{e9}", "e\u{301}", false),
        ("", "x", false),
    ] {
        let ExecutionOutcome::Success(result) = call(
            &bytes,
            &mut storage,
            "equal",
            &[Value::String(a.into()), Value::String(b.into())],
            1000,
        ) else {
            panic!("eq failed")
        };
        assert_eq!(result.return_value, boolean(expected));
    }
}

#[test]
fn every_insufficient_gas_budget_and_revert_roll_back_strings_and_events() {
    let bytes = bytes();
    let mut storage = Storage::default();
    assert!(matches!(
        call(
            &bytes,
            &mut storage,
            "set",
            &[Value::String("old".into()), boolean(true)],
            1000
        ),
        ExecutionOutcome::Success(_)
    ));
    let before = storage.clone();
    let args = [Value::String("new\u{e9}".into()), boolean(true)];
    let ExecutionOutcome::Success(success) = call(&bytes, &mut storage.clone(), "set", &args, 1000)
    else {
        panic!("set failed")
    };
    for gas in 0..success.gas_used {
        let ExecutionOutcome::Failure(failure) = call(&bytes, &mut storage, "set", &args, gas)
        else {
            panic!("accepted insufficient gas {gas}")
        };
        assert_eq!(failure.kind, FailureKind::OutOfGas);
        assert_eq!(failure.gas_used, gas);
        assert_eq!(storage, before);
    }
    let ExecutionOutcome::Failure(failure) = call(
        &bytes,
        &mut storage,
        "set",
        &[args[0].clone(), boolean(false)],
        1000,
    ) else {
        panic!("accepted revert")
    };
    assert_eq!(failure.kind, FailureKind::Revert);
    assert_eq!(storage, before);
    assert!(matches!(
        call(&bytes, &mut storage, "set", &args, success.gas_used),
        ExecutionOutcome::Success(_)
    ));
}

#[test]
fn byte_limits_types_and_version_boundaries_are_enforced() {
    let bytes = bytes();
    let mut storage = Storage::default();
    for args in [
        vec![Value::String("a".repeat(4097))],
        vec![Value::String("\u{e9}".repeat(2049))],
        vec![boolean(true)],
        vec![],
    ] {
        let before = storage.clone();
        assert!(matches!(
            call(&bytes, &mut storage, "echo", &args, 100000),
            ExecutionOutcome::Failure(_)
        ));
        assert_eq!(storage, before);
    }
    let mut old = bytes.clone();
    old[7] = 11;
    assert!(parse(&old).is_err());
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_err());
    }
    assert!(Vm::default()
        .execute_with_storage(&bytes, "name", &[], 1000, &mut storage)
        .is_err());
    assert!(compile(
        "contract C { state { x: map<string, u64>; } pub fn x() -> u64 { return 0; } }"
    )
    .is_err());
    assert!(compile(
        "contract C { state { x: map<address, string>; } pub fn x() -> u64 { return 0; } }"
    )
    .is_err());
    let scalar = compile("contract C { pub fn x() -> u64 { return 1; } }").unwrap();
    assert_eq!(scalar.bytecode_version, 11);
    assert_eq!(scalar.target, "lithovm-native-v11");
}

#[test]
fn identity_gas_counts_utf8_bytes_not_characters() {
    let bytes = bytes();
    for name in ["", "a", "\u{e9}", "\u{1faa8}"] {
        let ExecutionOutcome::Success(result) = call(
            &bytes,
            &mut Storage::default(),
            "echo",
            &[Value::String(name.into())],
            1000,
        ) else {
            panic!("echo failed")
        };
        assert_eq!(result.gas_used, 12 + 2 * name.len() as u64);
    }
}

#[test]
fn aggregate_input_and_event_limits_fail_without_committing_state() {
    let source = "contract C { state { name: string; } event Named { name: string } pub fn run(name: string, count: u64) -> string { self.name = name; repeat count { emit Named { name: self.name }; } return self.name; } }";
    let artifact = compile(source).unwrap();
    let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
    let mut storage = Storage::default();
    let text = Value::String("a".repeat(4096));
    let mut count = [0; 32];
    count[31] = 15;
    assert!(matches!(
        call(
            &bytes,
            &mut storage,
            "run",
            &[text.clone(), Value::Word(ValueType::U64, count)],
            1000000
        ),
        ExecutionOutcome::Success(_)
    ));
    let before = storage.clone();
    count[31] = 16;
    let ExecutionOutcome::Failure(failure) = call(
        &bytes,
        &mut storage,
        "run",
        &[text.clone(), Value::Word(ValueType::U64, count)],
        1000000,
    ) else {
        panic!("accepted oversized event output")
    };
    assert!(failure.message.contains("output exceeds byte limit"));
    assert_eq!(storage, before);
    let ExecutionOutcome::Failure(failure) =
        call(&bytes, &mut storage, "run", &vec![text; 16], 1000000)
    else {
        panic!("accepted oversized argument envelope")
    };
    assert_eq!(failure.kind, FailureKind::InvalidRequest);
    assert!(failure.message.contains("envelope exceeds byte limit"));
    assert_eq!(storage, before);
}
