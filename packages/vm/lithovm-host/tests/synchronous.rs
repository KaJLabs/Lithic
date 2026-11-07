use lithic_lithovm::compile;
use lithovm::{ExecutionContext, ExecutionOutcome, FailureKind, Storage, Vm};
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    CallRequest, DeployOutcome, DeployRequest, HostFailureKind, HostOutcome, InMemoryState,
    Initializer, TransactionalHost,
};

fn word(n: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&n.to_be_bytes());
    word
}
fn value(ty: ValueType, n: u64) -> Value {
    Value::Word(ty, word(n))
}
fn bytes(source: &str) -> Vec<u8> {
    hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap()
}
fn deploy(
    host: &mut TransactionalHost<InMemoryState>,
    source: &str,
    nonce: u64,
) -> ([u8; 32], Vec<u8>) {
    let bytes = bytes(source);
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
fn selector(bytes: &[u8], name: &str) -> Value {
    Value::Word(
        ValueType::Bytes32,
        function_selector(
            parse(bytes)
                .unwrap()
                .functions
                .iter()
                .find(|f| f.name == name)
                .unwrap(),
        ),
    )
}
fn request(contract: [u8; 32], function: &str, arguments: Vec<Value>) -> CallRequest<Value> {
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
const PARENT: &str = "contract Parent { state { result: u64; } event Before { n: u64 } event After { n: u64 } pub fn run(target: address, selector: bytes32, n: u64, accept: bool) -> u64 { self.result = 7; emit Before { n: self.result }; let result: u64 = invoke(target, selector, 5, n); self.result = result; emit After { n: result }; require(accept); return result + 1; } }";
const CHILD: &str = "contract Child { state { result: u64; caller: address; received: u256; } event Child { n: u64 } pub fn run(n: u64) -> u64 { self.result = n + 2; self.caller = msg.sender; self.received = msg.value; emit Child { n: self.result }; return self.result; } pub fn fail(n: u64) -> u64 { self.result = n; emit Child { n: n }; revert(); } pub fn wrong() -> bool { return true; } }";

#[test]
fn typed_calls_resume_with_return_data_and_source_ordered_events() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, child_bytes) = deploy(&mut host, CHILD, 0);
    let (parent, parent_bytes) = deploy(&mut host, PARENT, 1);
    assert_eq!(parent_bytes[7], 13);
    host.state_mut().set_balance(parent, word(10));
    let before = host.state().clone();
    let call = request(
        parent,
        "run",
        vec![
            Value::Word(ValueType::Address, child),
            selector(&child_bytes, "run"),
            value(ValueType::U64, 40),
            value(ValueType::Bool, 1),
        ],
    );
    let outcome = host.execute_values(call.clone());
    let HostOutcome::Success(result) = outcome else {
        panic!("invoke failed: {outcome:?}")
    };
    assert_eq!(result.result.return_value, value(ValueType::U64, 43));
    assert_eq!(
        result
            .events
            .iter()
            .map(|e| e.event.name.as_str())
            .collect::<Vec<_>>(),
        ["Before", "Child", "After"]
    );
    assert_eq!(result.events[1].contract, child);
    assert_eq!(
        host.state().contract(&child).unwrap().storage.get("caller"),
        Some(&parent)
    );
    assert_eq!(
        host.state()
            .contract(&child)
            .unwrap()
            .storage
            .get("received"),
        Some(&word(5))
    );
    assert_eq!(
        host.state()
            .contract(&parent)
            .unwrap()
            .storage
            .get("result"),
        Some(&word(42))
    );
    assert_eq!(host.state().balance(&parent), word(5));
    assert_eq!(host.state().balance(&child), word(5));
    for gas in 0..result.gas_used {
        let mut host = TransactionalHost::new(before.clone());
        let mut limited = call.clone();
        limited.gas_limit = gas;
        let HostOutcome::Failure(failure) = host.execute_values(limited) else {
            panic!("accepted gas {gas}")
        };
        assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::OutOfGas));
        assert_eq!(failure.gas_used, gas);
        assert_eq!(host.state(), &before);
    }
    let mut exact = TransactionalHost::new(before);
    let mut call = call;
    call.gas_limit = result.gas_used;
    assert!(matches!(
        exact.execute_values(call),
        HostOutcome::Success(_)
    ));
}

#[test]
fn parent_child_and_selection_failures_discard_all_staged_effects() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, child_bytes) = deploy(&mut host, CHILD, 0);
    let (parent, parent_bytes) = deploy(&mut host, PARENT, 1);
    host.state_mut().set_balance(parent, word(10));
    let before = host.state().clone();
    for (target, entry, accept, expected, failed_contract) in [
        (
            child,
            selector(&child_bytes, "run"),
            0,
            HostFailureKind::Vm(FailureKind::Revert),
            parent,
        ),
        (
            child,
            selector(&child_bytes, "fail"),
            1,
            HostFailureKind::Vm(FailureKind::Revert),
            child,
        ),
        (
            child,
            selector(&child_bytes, "wrong"),
            1,
            HostFailureKind::InvalidEntrypoint,
            child,
        ),
        (
            child,
            Value::Word(ValueType::Bytes32, [255; 32]),
            1,
            HostFailureKind::UnknownSelector,
            child,
        ),
        (
            parent,
            selector(&parent_bytes, "run"),
            1,
            HostFailureKind::Reentrancy,
            parent,
        ),
        (
            word(100),
            selector(&child_bytes, "run"),
            1,
            HostFailureKind::MissingContract,
            word(100),
        ),
    ] {
        let outcome = host.execute_values(request(
            parent,
            "run",
            vec![
                Value::Word(ValueType::Address, target),
                entry,
                value(ValueType::U64, 1),
                value(ValueType::Bool, accept),
            ],
        ));
        let HostOutcome::Failure(failure) = outcome else {
            panic!("unexpected success")
        };
        assert_eq!(failure.kind, expected);
        assert_eq!(failure.failed_contract, failed_contract);
        assert!(failure.gas_used > 0);
        assert_eq!(host.state(), &before);
    }
}

#[test]
fn strings_and_three_level_calls_preserve_typed_results() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (leaf, leaf_bytes) = deploy(
        &mut host,
        "contract Leaf { pub fn echo(name: string) -> string { return name; } }",
        0,
    );
    let (middle, middle_bytes) = deploy(&mut host, "contract Middle { pub fn run(leaf: address, selector: bytes32, name: string) -> string { let result: string = invoke(leaf, selector, 0, name); return result; } }", 1);
    let (root, _) = deploy(&mut host, "contract Root { pub fn run(middle: address, selector: bytes32, leaf: address, leaf_selector: bytes32, name: string) -> string { let result: string = invoke(middle, selector, 0, leaf, leaf_selector, name); return result; } }", 2);
    for text in [String::new(), "\u{e9}\0\u{1faa8}".into(), "a".repeat(4096)] {
        let expected = Value::String(text);
        let outcome = host.execute_values(request(
            root,
            "run",
            vec![
                Value::Word(ValueType::Address, middle),
                selector(&middle_bytes, "run"),
                Value::Word(ValueType::Address, leaf),
                selector(&leaf_bytes, "echo"),
                expected.clone(),
            ],
        ));
        let HostOutcome::Success(result) = outcome else {
            panic!("string invoke failed: {outcome:?}")
        };
        assert_eq!(result.result.return_value, expected);
    }
}

#[test]
fn transfers_execute_around_calls_and_refresh_refunded_balances() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, child_bytes) = deploy(&mut host, "contract Child { pub fn refund() -> bool { transfer_native(msg.sender, msg.value); return true; } }", 0);
    let (parent, _) = deploy(&mut host, "contract Parent { const FOUR: u256 = 4; const SIX: u256 = 6; pub fn run(target: address, selector: bytes32, recipient: address) -> bool { transfer_native(recipient, FOUR); let ok: bool = invoke(target, selector, SIX); transfer_native(recipient, SIX); return ok; } }", 1);
    host.state_mut().set_balance(parent, word(10));
    let outcome = host.execute_values(request(
        parent,
        "run",
        vec![
            Value::Word(ValueType::Address, child),
            selector(&child_bytes, "refund"),
            value(ValueType::Address, 100),
        ],
    ));
    assert!(matches!(outcome, HostOutcome::Success(_)), "{outcome:?}");
    assert_eq!(host.state().balance(&word(100)), word(10));
    assert_eq!(host.state().balance(&parent), word(0));
    assert_eq!(host.state().balance(&child), word(0));
}

#[test]
fn unsupported_syntax_mixed_modes_downgrades_and_unhosted_calls_fail_closed() {
    let encoded = bytes(PARENT);
    for version in [11, 12] {
        let mut old = encoded.clone();
        old[7] = version;
        assert!(parse(&old).is_err());
    }
    for end in 0..encoded.len() {
        assert!(parse(&encoded[..end]).is_err());
    }
    for source in [
        "contract C { pub fn run(a: address, s: bytes32) -> bool { let x = invoke(a, s, 0); return x; } }",
        "contract C { pub fn run(a: address, s: bytes32) -> bool { let mut x: bool = invoke(a, s, 0); return x; } }",
        "contract C { pub fn run(a: address, s: bytes32) -> bool { let x: bool = invoke(a, s, 0); call_contract(a, s, msg.value); return x; } }",
        "contract C { pub fn run(a: address, s: bytes32) -> bool { let x: bool = invoke(a, s, 0); x = false; return x; } }",
    ] { assert!(compile(source).is_err(), "accepted {source}"); }
    let mut storage = Storage::default();
    assert!(matches!(
        Vm::default().execute_values_transactionally(
            &encoded,
            "run",
            &[],
            1000,
            &mut storage,
            &ExecutionContext::default()
        ),
        ExecutionOutcome::Failure(_)
    ));
    assert_eq!(storage, Storage::default());
}

#[test]
fn argument_mismatches_and_failed_initialization_are_atomic() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, child_bytes) = deploy(&mut host, CHILD, 0);
    for (nonce, arguments) in [(1, "true"), (2, "1, 2"), (3, "")] {
        let source = format!("contract Parent {{ state {{ changed: bool; }} pub fn run(target: address, selector: bytes32) -> u64 {{ self.changed = true; let result: u64 = invoke(target, selector, 5{}{}); return result; }} }}", if arguments.is_empty() { "" } else { ", " }, arguments);
        let (parent, _) = deploy(&mut host, &source, nonce);
        host.state_mut().set_balance(parent, word(10));
        let before = host.state().clone();
        let outcome = host.execute_values(request(
            parent,
            "run",
            vec![
                Value::Word(ValueType::Address, child),
                selector(&child_bytes, "run"),
            ],
        ));
        assert!(matches!(outcome, HostOutcome::Failure(_)), "{outcome:?}");
        assert_eq!(host.state(), &before);
    }
    host.state_mut().set_balance(word(9), word(10));
    let before = host.state().clone();
    let outcome = host.deploy_values(DeployRequest {
        deployer: word(9),
        nonce: 10,
        bytecode: bytes(PARENT),
        initializer: Some(lithovm_host::Initializer {
            function: "run".into(),
            arguments: vec![
                Value::Word(ValueType::Address, child),
                selector(&child_bytes, "run"),
                value(ValueType::U64, 40),
                value(ValueType::Bool, 0),
            ],
        }),
        value: word(10),
        gas_limit: 100000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    });
    assert!(matches!(outcome, DeployOutcome::Failure(_)), "{outcome:?}");
    assert_eq!(host.state(), &before);
}

#[test]
fn aggregate_event_limit_rolls_back_and_allows_recovery() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, child_bytes) = deploy(&mut host, "contract Child { event Named { name: string } pub fn run(name: string) -> string { repeat 8 { emit Named { name: name }; } return name; } }", 0);
    let (parent, _) = deploy(&mut host, "contract Parent { state { name: string; } event Named { name: string } pub fn run(target: address, selector: bytes32, name: string, count: u64) -> string { self.name = name; repeat count { emit Named { name: name }; } let result: string = invoke(target, selector, 0, name); return result; } }", 1);
    let before = host.state().clone();
    let mut call = request(
        parent,
        "run",
        vec![
            Value::Word(ValueType::Address, child),
            selector(&child_bytes, "run"),
            Value::String("x".repeat(4096)),
            value(ValueType::U64, 8),
        ],
    );
    call.gas_limit = 1000000;
    let HostOutcome::Failure(failure) = host.execute_values(call.clone()) else {
        panic!("accepted excessive events")
    };
    assert!(failure.message.contains("host event output exceeds"));
    assert_eq!(host.state(), &before);
    call.arguments[3] = value(ValueType::U64, 7);
    let HostOutcome::Success(result) = host.execute_values(call) else {
        panic!("recovery failed")
    };
    assert_eq!(result.events.len(), 15);
    assert_eq!(result.events[6].contract, parent);
    assert_eq!(result.events[7].contract, child);
}

#[test]
fn finance_token_pull_uses_caller_allowance_and_rolls_back_after_return() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let token_bytes = bytes(include_str!(
        "../../../../sdk/contracts/standards/finance_token_v12.lithic"
    ));
    let mut initialization = vec![
        Value::String("Integration Token".into()),
        Value::String("TEST".into()),
        value(ValueType::Address, 9),
        value(ValueType::U64, 18),
        value(ValueType::U256, 1000),
    ];
    initialization.extend((0..4).map(|_| value(ValueType::Bool, 1)));
    let DeployOutcome::Success(deployed) = host.deploy_values(DeployRequest {
        deployer: word(9),
        nonce: 0,
        bytecode: token_bytes.clone(),
        initializer: Some(Initializer {
            function: "initialize".into(),
            arguments: initialization,
        }),
        value: word(0),
        gas_limit: 100000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    }) else {
        panic!("atomic token initialization failed")
    };
    let token = deployed.contract;
    let (puller, _) = deploy(&mut host, "contract Puller { pub fn pull(token: address, selector: bytes32, recipient: address, amount: u256, accept: bool) -> bool { let ok: bool = invoke(token, selector, 0, msg.sender, recipient, amount); require(ok); require(accept); return ok; } }", 1);
    let pull = |amount, accept| {
        request(
            puller,
            "pull",
            vec![
                Value::Word(ValueType::Address, token),
                selector(&token_bytes, "transfer_from"),
                value(ValueType::Address, 10),
                value(ValueType::U256, amount),
                value(ValueType::Bool, accept),
            ],
        )
    };
    let before = host.state().clone();
    assert!(matches!(
        host.execute_values(pull(40, 1)),
        HostOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
    assert!(matches!(
        host.execute_values(request(
            token,
            "approve",
            vec![
                Value::Word(ValueType::Address, puller),
                value(ValueType::U256, 50)
            ]
        )),
        HostOutcome::Success(_)
    ));
    let approved = host.state().clone();
    assert!(matches!(
        host.execute_values(pull(40, 0)),
        HostOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &approved);
    let HostOutcome::Success(result) = host.execute_values(pull(40, 1)) else {
        panic!("approved token pull failed")
    };
    assert_eq!(result.result.return_value, value(ValueType::Bool, 1));
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].contract, token);
    assert_eq!(result.events[0].event.name, "Transfer");
    for (function, arguments, expected) in [
        ("balance_of", vec![value(ValueType::Address, 9)], 960),
        ("balance_of", vec![value(ValueType::Address, 10)], 40),
        (
            "allowance",
            vec![
                value(ValueType::Address, 9),
                Value::Word(ValueType::Address, puller),
            ],
            10,
        ),
    ] {
        let HostOutcome::Success(result) = host.execute_values(request(token, function, arguments))
        else {
            panic!("getter failed")
        };
        assert_eq!(result.result.return_value, value(ValueType::U256, expected));
    }
    let after = host.state().clone();
    assert!(matches!(
        host.execute_values(pull(11, 1)),
        HostOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &after);
}
