use lithic_lithovm::compile;
use lithovm::{with_external_fuel, FailureKind};
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    CallRequest, DeployOutcome, DeployRequest, HostFailureKind, HostOutcome, InMemoryState,
    TransactionalHost,
};
use sha3::{Digest, Keccak256};

const BINARY: &str = "contract Binary { state { digest: bytes32; } event Hashed { data: bytes, digest: bytes32 } pub fn echo(data: bytes) -> bytes { let copy: bytes = data; return copy; } pub fn hash(data: bytes, accept: bool) -> bytes32 { let digest: bytes32 = keccak256(data); self.digest = digest; emit Hashed { data: data, digest: digest }; require(accept); return self.digest; } pub fn same(a: bytes, b: bytes) -> bool { return a == b; } }";

fn word(n: u64) -> [u8; 32] {
    let mut result = [0; 32];
    result[24..].copy_from_slice(&n.to_be_bytes());
    result
}

fn deploy(
    host: &mut TransactionalHost<InMemoryState>,
    source: &str,
    nonce: u64,
) -> ([u8; 32], Vec<u8>) {
    let artifact = compile(source).unwrap();
    assert_eq!(artifact.bytecode_version, 15);
    assert_eq!(artifact.target, "lithovm-native-v15");
    let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
    let outcome = host.deploy_values(DeployRequest {
        deployer: word(9),
        nonce,
        bytecode: bytes.clone(),
        initializer: None,
        value: word(0),
        gas_limit: 100000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    });
    let DeployOutcome::Success(result) = outcome else {
        panic!("deploy failed: {outcome:?}")
    };
    (result.contract, bytes)
}

fn call(contract: [u8; 32], function: &str, arguments: Vec<Value>) -> CallRequest<Value> {
    CallRequest {
        contract,
        function: function.into(),
        arguments,
        caller: word(9),
        value: word(0),
        gas_limit: 100000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    }
}

fn success(
    host: &mut TransactionalHost<InMemoryState>,
    request: CallRequest<Value>,
) -> lithovm_host::HostSuccess<Value> {
    let outcome = host.execute_values(request);
    let HostOutcome::Success(result) = outcome else {
        panic!("call failed: {outcome:?}")
    };
    result
}

#[test]
fn compiled_binary_identity_equality_and_keccak_vectors_execute_through_host() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (address, _) = deploy(&mut host, BINARY, 0);
    for (data, expected) in [
        (
            vec![],
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
        ),
        (
            b"abc".to_vec(),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45",
        ),
    ] {
        let result = success(
            &mut host,
            call(
                address,
                "hash",
                vec![
                    Value::Bytes(data.clone()),
                    Value::Word(ValueType::Bool, word(1)),
                ],
            ),
        );
        let digest = Value::Word(
            ValueType::Bytes32,
            hex::decode(expected).unwrap().try_into().unwrap(),
        );
        assert_eq!(result.result.return_value, digest);
        assert_eq!(result.events[0].event.fields[0].2, Value::Bytes(data));
        assert_eq!(result.events[0].event.fields[1].2, digest);
    }
    for data in [vec![0, 255, 128, 0], vec![255; 4096]] {
        assert_eq!(
            success(
                &mut host,
                call(address, "echo", vec![Value::Bytes(data.clone())])
            )
            .result
            .return_value,
            Value::Bytes(data.clone())
        );
        let result = success(
            &mut host,
            call(
                address,
                "hash",
                vec![
                    Value::Bytes(data.clone()),
                    Value::Word(ValueType::Bool, word(1)),
                ],
            ),
        );
        assert_eq!(
            result.result.return_value,
            Value::Word(ValueType::Bytes32, Keccak256::digest(&data).into())
        );
    }
    for (a, b, same) in [
        (vec![], vec![], true),
        (vec![0, 255], vec![0, 255], true),
        (vec![0], vec![0, 0], false),
        (vec![255], vec![0], false),
    ] {
        assert_eq!(
            success(
                &mut host,
                call(address, "same", vec![Value::Bytes(a), Value::Bytes(b)])
            )
            .result
            .return_value,
            Value::Word(ValueType::Bool, word(u64::from(same)))
        );
    }
}

unsafe extern "C" fn record_fuel(context: usize, amount: u64) -> i32 {
    let total = &mut *(context as *mut u64);
    *total = total.checked_add(amount).unwrap();
    1
}

#[test]
fn hashing_is_metered_and_every_insufficient_budget_rolls_back_state_and_events() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (address, _) = deploy(&mut host, BINARY, 0);
    let before = host.state().clone();
    let request = call(
        address,
        "hash",
        vec![
            Value::Bytes(vec![255; 33]),
            Value::Word(ValueType::Bool, word(1)),
        ],
    );
    let mut total = 0_u64;
    let result = unsafe {
        with_external_fuel(&mut total as *mut u64 as usize, record_fuel, || {
            success(&mut host, request.clone())
        })
    };
    assert_eq!(total, result.gas_used);
    for gas in 0..result.gas_used {
        let mut host = TransactionalHost::new(before.clone());
        let mut limited = request.clone();
        limited.gas_limit = gas;
        let outcome = host.execute_values(limited);
        let HostOutcome::Failure(failure) = outcome else {
            panic!("accepted gas {gas}")
        };
        assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::OutOfGas));
        assert_eq!(failure.gas_used, gas);
        assert_eq!(host.state(), &before);
    }
    let mut exact = TransactionalHost::new(before.clone());
    let mut exact_call = request.clone();
    exact_call.gas_limit = result.gas_used;
    success(&mut exact, exact_call);
    let mut reverted = TransactionalHost::new(before.clone());
    let mut rejected = request;
    rejected.arguments[1] = Value::Word(ValueType::Bool, word(0));
    assert!(matches!(
        reverted.execute_values(rejected),
        HostOutcome::Failure(_)
    ));
    assert_eq!(reverted.state(), &before);
    for invalid in [
        Value::Bytes(vec![0; 4097]),
        Value::String("abc".into()),
        Value::Word(ValueType::Bytes, [0; 32]),
    ] {
        assert!(matches!(
            reverted.execute_values(call(address, "echo", vec![invalid])),
            HostOutcome::Failure(_)
        ));
        assert_eq!(reverted.state(), &before);
    }
}

#[test]
fn synchronous_binary_arguments_and_returns_preserve_rollback() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, child_bytes) = deploy(&mut host, BINARY, 0);
    let parent = "contract Parent { state { marker: u64; } pub fn forward(target: address, selector: bytes32, data: bytes, accept: bool) -> bytes { self.marker = 7; let result: bytes = invoke(target, selector, 0, data); require(accept); return result; } }";
    let (parent, _) = deploy(&mut host, parent, 1);
    let selector = function_selector(
        parse(&child_bytes)
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == "echo")
            .unwrap(),
    );
    let before = host.state().clone();
    let data = Value::Bytes(vec![0, 255, 128]);
    let args = vec![
        Value::Word(ValueType::Address, child),
        Value::Word(ValueType::Bytes32, selector),
        data.clone(),
        Value::Word(ValueType::Bool, word(1)),
    ];
    assert_eq!(
        success(&mut host, call(parent, "forward", args.clone()))
            .result
            .return_value,
        data
    );
    let mut reverted = TransactionalHost::new(before.clone());
    let mut rejected = args;
    rejected[3] = Value::Word(ValueType::Bool, word(0));
    assert!(matches!(
        reverted.execute_values(call(parent, "forward", rejected)),
        HostOutcome::Failure(_)
    ));
    assert_eq!(reverted.state(), &before);
}

#[test]
fn hash_gas_grows_by_input_words_and_requires_bytecode_v15() {
    let source =
        "contract Hash { pub fn hash(data: bytes) -> bytes32 { return keccak256(data); } }";
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (address, bytes) = deploy(&mut host, source, 0);
    assert_eq!(parse(&bytes).unwrap().encode().unwrap(), bytes);
    for version in 1..15 {
        let mut downgraded = bytes.clone();
        downgraded[7] = version;
        assert!(parse(&downgraded).is_err(), "accepted v{version}");
    }
    let mut last = 0;
    for size in [0_u64, 1, 31, 32, 33, 64, 65, 4096] {
        let result = success(
            &mut host,
            call(
                address,
                "hash",
                vec![Value::Bytes(vec![255; size as usize])],
            ),
        );
        let adjusted = result.gas_used - 2 * size - 6 * size.div_ceil(32);
        if last != 0 {
            assert_eq!(adjusted, last);
        }
        last = adjusted;
    }
}

#[test]
fn cumulative_binary_events_enforce_the_actual_encoded_byte_bound() {
    let source = "contract Log { state { marker: u64; } event Data { data: bytes } pub fn emit(data: bytes, count: u64) -> bool { self.marker = 1; repeat count { emit Data { data: data }; } return true; } }";
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (address, _) = deploy(&mut host, source, 0);
    let before = host.state().clone();
    let mut request = call(
        address,
        "emit",
        vec![
            Value::Bytes(vec![255; 4096]),
            Value::Word(ValueType::U64, word(16)),
        ],
    );
    request.gas_limit = 1000000;
    let outcome = host.execute_values(request.clone());
    let HostOutcome::Failure(failure) = outcome else {
        panic!("accepted oversized event output")
    };
    assert!(
        failure.message.contains("event output exceeds"),
        "{failure:?}"
    );
    assert_eq!(host.state(), &before);
    request.arguments[1] = Value::Word(ValueType::U64, word(15));
    assert_eq!(success(&mut host, request).events.len(), 15);
}
