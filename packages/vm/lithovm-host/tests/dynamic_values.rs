use lithic_lithovm::compile;
use lithovm::FailureKind;
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    code_hash, contract_address, CallRequest, DeployOutcome, DeployRequest, HostFailureKind,
    HostOutcome, InMemoryState, Initializer, TransactionalHost,
};

fn word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}
fn typed(ty: ValueType, value: u64) -> Value {
    Value::Word(ty, word(value))
}
fn bytecode(source: &str) -> Vec<u8> {
    hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap()
}
fn deploy_request(bytes: Vec<u8>, nonce: u64, arguments: Vec<Value>) -> DeployRequest<Value> {
    DeployRequest {
        deployer: word(7),
        nonce,
        bytecode: bytes,
        initializer: Some(Initializer {
            function: "initialize".into(),
            arguments,
        }),
        value: word(5),
        gas_limit: 1000000,
        block_height: 42,
        block_timestamp: 1000,
        chain_id: 700777,
    }
}
fn call_request(contract: [u8; 32], function: &str, arguments: Vec<Value>) -> CallRequest<Value> {
    CallRequest {
        contract,
        function: function.into(),
        arguments,
        gas_limit: 1000000,
        caller: word(7),
        value: word(0),
        block_height: 43,
        block_timestamp: 1001,
        chain_id: 700777,
    }
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

const METADATA: &str = "contract Metadata { state { name: string; } event Named { name: string } pub fn initialize(name: string, accept: bool) -> string { self.name = name; emit Named { name: self.name }; require(accept); return self.name; } pub fn name() -> string { return self.name; } }";

#[test]
fn dynamic_initialization_is_atomic_and_preserves_prefunding() {
    let bytes = bytecode(METADATA);
    let name = Value::String("\u{e9}\0\u{1faa8}".into());
    let request = deploy_request(
        bytes.clone(),
        1,
        vec![name.clone(), typed(ValueType::Bool, 1)],
    );
    let address = contract_address(word(7), 1, code_hash(&bytes), 700777);
    let mut initial = InMemoryState::default();
    initial.set_balance(word(7), word(100));
    initial.set_balance(address, word(20));
    let mut host = TransactionalHost::new(initial.clone());
    let DeployOutcome::Success(success) = host.deploy_values(request.clone()) else {
        panic!("deployment failed")
    };
    assert_eq!(success.contract, address);
    assert_eq!(success.initializer_result.unwrap().return_value, name);
    assert_eq!(success.events[0].event.fields[0].2, name);
    assert_eq!(host.state().balance(&address), word(25));
    assert_eq!(host.state().balance(&word(7)), word(95));
    let HostOutcome::Success(result) = host.execute_values(call_request(address, "name", vec![]))
    else {
        panic!("getter failed")
    };
    assert_eq!(result.result.return_value, name);

    for gas in 0..success.gas_used {
        let mut host = TransactionalHost::new(initial.clone());
        let mut low = request.clone();
        low.gas_limit = gas;
        let DeployOutcome::Failure(failure) = host.deploy_values(low) else {
            panic!("accepted insufficient gas")
        };
        assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::OutOfGas));
        assert_eq!(failure.gas_used, gas);
        assert_eq!(host.state(), &initial);
    }
    let mut host = TransactionalHost::new(initial.clone());
    let mut rejected = request.clone();
    rejected.initializer.as_mut().unwrap().arguments[1] = typed(ValueType::Bool, 0);
    let DeployOutcome::Failure(failure) = host.deploy_values(rejected) else {
        panic!("accepted revert")
    };
    assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::Revert));
    assert_eq!(host.state(), &initial);
    assert!(matches!(
        host.deploy_values(request.clone()),
        DeployOutcome::Success(_)
    ));
    let before = host.state().clone();
    assert!(matches!(
        host.deploy_values(request),
        DeployOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
}

#[test]
fn deferred_child_failure_and_aggregate_event_limits_roll_back_the_whole_tree() {
    let child = bytecode("contract Child { state { name: string; } event Named { name: string } pub fn initialize(name: string) -> string { self.name = name; return self.name; } pub fn fail() -> string { emit Named { name: self.name }; revert(); } pub fn succeed() -> string { repeat 8 { emit Named { name: self.name }; } return self.name; } }");
    let parent = bytecode("contract Parent { const ONE: u256 = 1; state { name: string; } event Named { name: string } pub fn initialize(name: string) -> string { self.name = name; return self.name; } pub fn run(name: string, child: address, selector: bytes32, count: u64) -> string { self.name = name; repeat count { emit Named { name: self.name }; } call_contract(child, selector, ONE); return self.name; } }");
    let mut state = InMemoryState::default();
    state.set_balance(word(7), word(100));
    let mut host = TransactionalHost::new(state);
    let text = Value::String("x".repeat(4096));
    let DeployOutcome::Success(c) =
        host.deploy_values(deploy_request(child.clone(), 1, vec![text.clone()]))
    else {
        panic!("child deploy failed")
    };
    let DeployOutcome::Success(p) =
        host.deploy_values(deploy_request(parent, 2, vec![Value::String("old".into())]))
    else {
        panic!("parent deploy failed")
    };
    let before = host.state().clone();
    let mut request = call_request(
        p.contract,
        "run",
        vec![
            text.clone(),
            Value::Word(ValueType::Address, c.contract),
            selector(&child, "fail"),
            typed(ValueType::U64, 1),
        ],
    );
    let HostOutcome::Failure(failure) = host.execute_values(request.clone()) else {
        panic!("accepted child revert")
    };
    assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::Revert));
    assert_eq!(failure.failed_contract, c.contract);
    assert_eq!(host.state(), &before);
    request.arguments[2] = selector(&child, "succeed");
    request.arguments[3] = typed(ValueType::U64, 8);
    let HostOutcome::Failure(failure) = host.execute_values(request.clone()) else {
        panic!("accepted oversized transaction event output")
    };
    assert!(failure.message.contains("host event output exceeds"));
    assert_eq!(host.state(), &before);
    request.arguments[3] = typed(ValueType::U64, 7);
    let HostOutcome::Success(success) = host.execute_values(request.clone()) else {
        panic!("recovery failed")
    };
    assert_eq!(success.result.return_value, text);
    assert_eq!(success.events.len(), 15);
    assert_eq!(success.events[0].contract, p.contract);
    assert_eq!(success.events[7].contract, c.contract);
    assert_eq!(host.state().balance(&p.contract), word(4));
    assert_eq!(host.state().balance(&c.contract), word(6));
    // Exhaustion in the child must also undo parent state, events and value debit.
    let mut rollback_host = TransactionalHost::new(before.clone());
    request.gas_limit = success.gas_used - 1;
    let HostOutcome::Failure(failure) = rollback_host.execute_values(request) else {
        panic!("accepted child OOG")
    };
    assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::OutOfGas));
    assert_eq!(rollback_host.state(), &before);
}

#[test]
fn finance_metadata_initializes_atomically_for_all_sixteen_feature_profiles() {
    let bytes = bytecode(include_str!(
        "../../../../sdk/contracts/standards/finance_token_v12.lithic"
    ));
    for flags in 0..16 {
        let mut state = InMemoryState::default();
        state.set_balance(word(7), word(100));
        let mut host = TransactionalHost::new(state);
        let name = Value::String("Client \u{1faa8} Token".into());
        let symbol = Value::String("TEST".into());
        let mut arguments = vec![
            name.clone(),
            symbol.clone(),
            typed(ValueType::Address, 8),
            typed(ValueType::U64, 18),
            typed(ValueType::U256, 1000),
        ];
        arguments.extend((0..4).map(|bit| typed(ValueType::Bool, (flags >> bit) & 1)));
        let DeployOutcome::Success(deployed) =
            host.deploy_values(deploy_request(bytes.clone(), flags, arguments.clone()))
        else {
            panic!("profile {flags} failed initialization")
        };
        for (function, args, expected) in [
            ("name", vec![], name),
            ("symbol", vec![], symbol),
            ("decimals", vec![], typed(ValueType::U64, 18)),
            ("total_supply", vec![], typed(ValueType::U256, 1000)),
            (
                "balance_of",
                vec![typed(ValueType::Address, 8)],
                typed(ValueType::U256, 1000),
            ),
            (
                "balance_of",
                vec![typed(ValueType::Address, 7)],
                typed(ValueType::U256, 0),
            ),
            (
                "owner",
                vec![],
                typed(ValueType::Address, if flags & 8 != 0 { 8 } else { 0 }),
            ),
        ] {
            let HostOutcome::Success(result) =
                host.execute_values(call_request(deployed.contract, function, args))
            else {
                panic!("profile {flags}: {function} failed")
            };
            assert_eq!(result.result.return_value, expected);
        }
        let before = host.state().clone();
        assert!(matches!(
            host.execute_values(call_request(deployed.contract, "initialize", arguments)),
            HostOutcome::Failure(_)
        ));
        assert_eq!(host.state(), &before);
    }
}
