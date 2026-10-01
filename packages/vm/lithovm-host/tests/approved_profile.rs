use lithic_lithovm::compile;
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    caller_bound_salt, child_contract_address, code_hash, CallRequest, DeployOutcome,
    DeployRequest, HostOutcome, InMemoryState, Initializer, TransactionalHost,
};

fn word(n: u8) -> [u8; 32] {
    let mut w = [0; 32];
    w[31] = n;
    w
}
fn code(s: &str) -> Vec<u8> {
    hex::decode(&compile(s).unwrap().bytecode[2..]).unwrap()
}
fn request(
    bytecode: Vec<u8>,
    nonce: u64,
    initializer: Option<Initializer<Value>>,
) -> DeployRequest<Value> {
    DeployRequest {
        deployer: word(9),
        nonce,
        bytecode,
        initializer,
        value: word(0),
        gas_limit: 1_000_000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    }
}
fn deploy(h: &mut TransactionalHost<InMemoryState>, r: DeployRequest<Value>) -> [u8; 32] {
    match h.deploy_values(r) {
        DeployOutcome::Success(r) => r.contract,
        other => panic!("{other:?}"),
    }
}

#[test]
fn declaring_initialize_requires_that_entrypoint_atomically() {
    let mut h = TransactionalHost::new(InMemoryState::default());
    let bytecode = code("contract C { pub fn initialize() -> bool { return true; } pub fn bypass() -> bool { return true; } }");
    for initializer in [
        None,
        Some(Initializer {
            function: "bypass".into(),
            arguments: vec![],
        }),
    ] {
        let before = h.state().clone();
        assert!(
            matches!(
                h.deploy_values(request(bytecode.clone(), 0, initializer)),
                DeployOutcome::Failure(_)
            ),
            "accepted missing or bypassed initializer"
        );
        assert_eq!(h.state(), &before);
    }
    deploy(
        &mut h,
        request(
            bytecode,
            0,
            Some(Initializer {
                function: "initialize".into(),
                arguments: vec![],
            }),
        ),
    );
    deploy(
        &mut h,
        request(
            code("contract Plain { pub fn run() -> bool { return true; } }"),
            1,
            None,
        ),
    );
}

#[test]
fn copied_factory_salt_does_not_block_another_caller() {
    let mut h = TransactionalHost::new(InMemoryState::default());
    let child_code = code("contract Child { pub fn initialize() -> bool { return true; } pub fn bypass() -> bool { return true; } }");
    let program = parse(&child_code).unwrap();
    let selector = function_selector(
        program
            .functions
            .iter()
            .find(|f| f.name == "initialize")
            .unwrap(),
    );
    let bypass = function_selector(
        program
            .functions
            .iter()
            .find(|f| f.name == "bypass")
            .unwrap(),
    );
    let hash = code_hash(&child_code);
    let template = deploy(
        &mut h,
        request(
            child_code,
            0,
            Some(Initializer {
                function: "initialize".into(),
                arguments: vec![],
            }),
        ),
    );
    let factory = deploy(&mut h, request(code("contract Factory { pub fn create(t: address, s: bytes32, i: bytes32) -> address { let child: address = create_contract(t, s, i, 0); return child; } }"), 1, None));
    let call = |caller| CallRequest {
        contract: factory,
        function: "create".into(),
        arguments: vec![
            Value::Word(ValueType::Address, template),
            Value::Word(ValueType::Bytes32, word(42)),
            Value::Word(ValueType::Bytes32, selector),
        ],
        caller: word(caller),
        value: word(0),
        gas_limit: 1_000_000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    };
    let mut bypass_request = call(66);
    bypass_request.arguments[2] = Value::Word(ValueType::Bytes32, bypass);
    let before = h.state().clone();
    assert!(matches!(
        h.execute_values(bypass_request),
        HostOutcome::Failure(_)
    ));
    assert_eq!(h.state(), &before);
    let HostOutcome::Success(attacker) = h.execute_values(call(66)) else {
        panic!("first creation failed")
    };
    let HostOutcome::Success(victim) = h.execute_values(call(10)) else {
        panic!("copied salt blocked victim")
    };
    assert_ne!(attacker.result.return_value, victim.result.return_value);
    for (caller, receipt) in [(66, &attacker), (10, &victim)] {
        assert_eq!(
            receipt.result.return_value,
            Value::Word(
                ValueType::Address,
                child_contract_address(
                    factory,
                    caller_bound_salt(word(caller), word(42)),
                    hash,
                    700777
                )
            )
        );
        lithovm_host::deployment_status::included(
            &receipt.deployments,
            &lithovm_host::deployment_status::Inclusion {
                chain_id: 700777,
                transaction_hash: word(1),
                block_hash: word(2),
                block_height: 1,
                transaction_gas_used: receipt.gas_used,
            },
        )
        .unwrap();
    }
    let before = h.state().clone();
    assert!(matches!(
        h.execute_values(call(10)),
        HostOutcome::Failure(_)
    ));
    assert_eq!(h.state(), &before);
}

#[test]
fn finance_factory_cannot_be_published_uninitialized_or_taken_over() {
    let mut h = TransactionalHost::new(InMemoryState::default());
    let factory_code = code(include_str!(
        "../../../../sdk/contracts/standards/finance_factory_v14.lithic"
    ));
    let before = h.state().clone();
    assert!(matches!(
        h.deploy_values(request(factory_code.clone(), 0, None)),
        DeployOutcome::Failure(_)
    ));
    assert_eq!(h.state(), &before);
    let args = vec![
        Value::Word(ValueType::Address, word(20)),
        Value::Word(ValueType::Bytes32, word(21)),
    ];
    let factory = deploy(
        &mut h,
        request(
            factory_code,
            0,
            Some(Initializer {
                function: "initialize".into(),
                arguments: args.clone(),
            }),
        ),
    );
    let before = h.state().clone();
    assert!(matches!(
        h.execute_values(CallRequest {
            contract: factory,
            function: "initialize".into(),
            arguments: args,
            caller: word(66),
            value: word(0),
            gas_limit: 1_000_000,
            block_height: 1,
            block_timestamp: 2,
            chain_id: 700777
        }),
        HostOutcome::Failure(_)
    ));
    assert_eq!(h.state(), &before);
}
