use lithic_lithovm::compile;
use lithovm::{with_external_fuel, FailureKind};
use lithovm_bytecode::{parse, values::Value, ValueType};
use lithovm_host::{
    CallRequest, DeployOutcome, DeployRequest, HostFailureKind, HostOutcome, InMemoryState,
    TransactionalHost,
};

fn word(n: u64) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}
fn val(ty: ValueType, n: u64) -> Value {
    Value::Word(ty, word(n))
}
fn deploy(source: &str) -> (TransactionalHost<InMemoryState>, [u8; 32], Vec<u8>) {
    let artifact = compile(source).unwrap();
    assert_eq!(artifact.bytecode_version, 16);
    let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
    let mut host = TransactionalHost::new(InMemoryState::default());
    let outcome = host.deploy_values(DeployRequest {
        deployer: word(9),
        nonce: 0,
        bytecode: bytes.clone(),
        initializer: None,
        value: word(0),
        gas_limit: 100000,
        chain_id: 700780,
        block_height: 1,
        block_timestamp: 100,
    });
    let DeployOutcome::Success(r) = outcome else {
        panic!("{outcome:?}")
    };
    (host, r.contract, bytes)
}
fn request(contract: [u8; 32], function: &str, arguments: Vec<Value>) -> CallRequest<Value> {
    CallRequest {
        contract,
        function: function.into(),
        arguments,
        caller: word(9),
        value: word(0),
        gas_limit: 100000,
        chain_id: 700780,
        block_height: 2,
        block_timestamp: 100,
    }
}
fn run(
    host: &mut TransactionalHost<InMemoryState>,
    request: CallRequest<Value>,
) -> lithovm_host::HostSuccess<Value> {
    let outcome = host.execute_values(request);
    let HostOutcome::Success(r) = outcome else {
        panic!("{outcome:?}")
    };
    r
}
fn binary_hex(value: &str) -> Vec<u8> {
    hex::decode(value.strip_prefix("0x").unwrap()).unwrap()
}

const VERIFY:&str="contract Verify { state { valid: bool; } event Checked { valid: bool } pub fn verify(digest: bytes32, signature: bytes, signer: address) -> bool { let valid: bool = secp256k1_verify(digest, signature, signer); self.valid = valid; emit Checked { valid: valid }; return valid; } pub fn identity() -> address { return contract.address; } }";

#[test]
fn strict_signature_verification_matches_proposed_independent_vectors() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../standards/lep100-tests/fixtures/proposed-signing-vectors.json"
    ))
    .unwrap();
    let (mut host, address, bytes) = deploy(VERIFY);
    assert_eq!(
        run(&mut host, request(address, "identity", vec![]))
            .result
            .return_value,
        Value::Word(ValueType::Address, address)
    );
    for version in 11..16 {
        let mut old = bytes.clone();
        old[7] = version;
        assert!(parse(&old).is_err());
    }
    for fixture in fixtures["vectors"].as_array().unwrap() {
        let digest: [u8; 32] = binary_hex(fixture["digest"].as_str().unwrap())
            .try_into()
            .unwrap();
        for signature in fixture["signatures"].as_array().unwrap() {
            let mut signer = [0; 32];
            signer[12..].copy_from_slice(&binary_hex(signature["address"].as_str().unwrap()));
            let signature = binary_hex(signature["signature"].as_str().unwrap());
            let args = vec![
                Value::Word(ValueType::Bytes32, digest),
                Value::Bytes(signature.clone()),
                Value::Word(ValueType::Address, signer),
            ];
            assert_eq!(
                run(&mut host, request(address, "verify", args.clone()))
                    .result
                    .return_value,
                val(ValueType::Bool, 1)
            );
            let mut wrong = args.clone();
            if let Value::Word(_, w) = &mut wrong[0] {
                w[0] ^= 1;
            }
            assert_eq!(
                run(&mut host, request(address, "verify", wrong))
                    .result
                    .return_value,
                val(ValueType::Bool, 0)
            );
            let mut wrong = args;
            wrong[2] = val(ValueType::Address, 1);
            assert_eq!(
                run(&mut host, request(address, "verify", wrong))
                    .result
                    .return_value,
                val(ValueType::Bool, 0)
            );
            let order =
                binary_hex("0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141");
            let mut high = signature.clone();
            let mut borrow = 0_i16;
            for index in (0..32).rev() {
                let v = order[index] as i16 - signature[index + 32] as i16 - borrow;
                high[index + 32] = v as u8;
                borrow = i16::from(v < 0);
            }
            high[64] = if high[64] == 27 { 28 } else { 27 };
            let mut bad = vec![vec![], vec![0; 64], vec![0; 65], vec![0; 66], high];
            for v in [0, 1, 26, 29, 35, 255] {
                let mut s = signature.clone();
                s[64] = v;
                bad.push(s);
            }
            for range in [0..32, 32..64] {
                for bytes in [vec![0; 32], order.clone()] {
                    let mut s = signature.clone();
                    s[range.clone()].copy_from_slice(&bytes);
                    bad.push(s);
                }
            }
            for invalid in bad {
                let args = vec![
                    Value::Word(ValueType::Bytes32, digest),
                    Value::Bytes(invalid),
                    Value::Word(ValueType::Address, signer),
                ];
                assert_eq!(
                    run(&mut host, request(address, "verify", args))
                        .result
                        .return_value,
                    val(ValueType::Bool, 0)
                );
            }
        }
    }
}

#[test]
fn byte_operations_are_bounded_and_word_conversions_are_canonical() {
    let source="contract Bytes { pub fn join(a: bytes, b: bytes) -> bytes { return bytes_concat(a, b); } pub fn slice(data: bytes, start: u64, size: u64) -> bytes { return bytes_slice(data, start, size); } pub fn get(data: bytes, index: u64) -> u64 { return bytes_get(data, index); } pub fn length(data: bytes) -> u64 { return bytes_len(data); } pub fn encode(n: u64) -> bytes { return word_bytes(n); } pub fn decode(data: bytes) -> u64 { return word_u64(bytes_word(data)); } pub fn address(data: bytes) -> address { return word_address(bytes_word(data)); } pub fn amount(data: bytes) -> u256 { return word_u256(bytes_word(data)); } }";
    let (mut host, address, _) = deploy(source);
    let bytes = Value::Bytes(vec![0, 255, 128]);
    assert_eq!(
        run(&mut host, request(address, "length", vec![bytes.clone()]))
            .result
            .return_value,
        val(ValueType::U64, 3)
    );
    assert_eq!(
        run(
            &mut host,
            request(address, "get", vec![bytes.clone(), val(ValueType::U64, 1)])
        )
        .result
        .return_value,
        val(ValueType::U64, 255)
    );
    assert_eq!(
        run(
            &mut host,
            request(
                address,
                "slice",
                vec![
                    bytes.clone(),
                    val(ValueType::U64, 1),
                    val(ValueType::U64, 2)
                ]
            )
        )
        .result
        .return_value,
        Value::Bytes(vec![255, 128])
    );
    assert_eq!(
        run(
            &mut host,
            request(
                address,
                "slice",
                vec![
                    bytes.clone(),
                    val(ValueType::U64, 3),
                    val(ValueType::U64, 0)
                ]
            )
        )
        .result
        .return_value,
        Value::Bytes(vec![])
    );
    assert_eq!(
        run(
            &mut host,
            request(
                address,
                "join",
                vec![Value::Bytes(vec![255; 2048]), Value::Bytes(vec![128; 2048])]
            )
        )
        .result
        .return_value,
        Value::Bytes([vec![255; 2048], vec![128; 2048]].concat())
    );
    for n in [0, 1, u64::MAX] {
        let encoded = run(
            &mut host,
            request(address, "encode", vec![val(ValueType::U64, n)]),
        )
        .result
        .return_value;
        assert_eq!(encoded, Value::Bytes(word(n).to_vec()));
        assert_eq!(
            run(&mut host, request(address, "decode", vec![encoded]))
                .result
                .return_value,
            val(ValueType::U64, n)
        );
    }
    assert_eq!(
        run(
            &mut host,
            request(address, "amount", vec![Value::Bytes(vec![255; 32])])
        )
        .result
        .return_value,
        Value::Word(ValueType::U256, [255; 32])
    );
    for (name, args) in [
        (
            "join",
            vec![Value::Bytes(vec![0; 4096]), Value::Bytes(vec![0])],
        ),
        (
            "slice",
            vec![
                bytes.clone(),
                val(ValueType::U64, 2),
                val(ValueType::U64, 2),
            ],
        ),
        (
            "slice",
            vec![
                bytes.clone(),
                val(ValueType::U64, u64::MAX),
                val(ValueType::U64, 1),
            ],
        ),
        ("get", vec![bytes.clone(), val(ValueType::U64, 3)]),
        ("get", vec![bytes, val(ValueType::U64, u64::MAX)]),
        ("decode", vec![Value::Bytes(vec![0; 31])]),
        ("decode", vec![Value::Bytes(vec![255; 32])]),
        ("address", vec![Value::Bytes(vec![255; 32])]),
    ] {
        let before = host.state().clone();
        assert!(matches!(
            host.execute_values(request(address, name, args)),
            HostOutcome::Failure(_)
        ));
        assert_eq!(host.state(), &before);
    }
}

unsafe extern "C" fn fuel(context: usize, amount: u64) -> i32 {
    let state = &mut *(context as *mut (u64, bool));
    state.0 += amount;
    i32::from(!state.1 || amount != 3000)
}

#[test]
fn signature_gas_and_external_fuel_failures_discard_staged_effects() {
    let (mut host, address, _) = deploy(VERIFY);
    let before = host.state().clone();
    let request = request(
        address,
        "verify",
        vec![
            Value::Word(ValueType::Bytes32, [0; 32]),
            Value::Bytes(vec![0; 65]),
            val(ValueType::Address, 1),
        ],
    );
    let mut external = (0_u64, false);
    let result = unsafe {
        with_external_fuel(&mut external as *mut _ as usize, fuel, || {
            run(&mut host, request.clone())
        })
    };
    assert_eq!(external.0, result.gas_used);
    for gas in [0, 10, 100, 3000, result.gas_used - 1] {
        let mut limited = TransactionalHost::new(before.clone());
        let mut call = request.clone();
        call.gas_limit = gas;
        let HostOutcome::Failure(f) = limited.execute_values(call) else {
            panic!("accepted gas {gas}")
        };
        assert_eq!(f.kind, HostFailureKind::Vm(FailureKind::OutOfGas));
        assert_eq!(f.gas_used, gas);
        assert_eq!(limited.state(), &before);
    }
    let mut limited = TransactionalHost::new(before.clone());
    let mut external = (0_u64, true);
    let result = unsafe {
        with_external_fuel(&mut external as *mut _ as usize, fuel, || {
            limited.execute_values(request)
        })
    };
    assert!(matches!(result, HostOutcome::Failure(_)));
    assert_eq!(limited.state(), &before);
}

#[test]
fn intrinsic_type_and_nested_argument_errors_fail_the_entire_build() {
    for expr in [
        "bytes_len(n)",
        "word_bytes(data)",
        "bytes_slice(data, 0)",
        "bytes_slice(data, data, 0)",
        "secp256k1_verify(data, data, n)",
        "bytes_get(data, 0, 1)",
        "bytes_word()",
    ] {
        let source =
            format!("contract Bad {{ pub fn x(data: bytes, n: u64) -> u64 {{ return {expr}; }} }}");
        assert!(compile(&source).is_err(), "accepted {expr}");
    }
    let source="contract Nested { pub fn x(target: address, selector: bytes32, data: bytes) -> bytes { let answer: bytes = invoke(target, selector, 0, bytes_slice(bytes_concat(data, data), 0, bytes_len(data))); return answer; } }";
    assert_eq!(compile(source).unwrap().bytecode_version, 16);
    let old = "contract Old { pub fn echo(bytes_len: u64) -> u64 { return bytes_len; } }";
    assert_eq!(compile(old).unwrap().bytecode_version, 11);
}
