use lithic_lithovm::compile;
use lithovm::{ExecutionContext, ExecutionOutcome, FailureKind, Storage, Vm};
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    caller_bound_salt, child_contract_address, code_hash, CallRequest, DeployOutcome,
    DeployRequest, HostFailureKind, HostOutcome, InMemoryState, Initializer, TransactionalHost,
};

fn word(n: u64) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}
fn value(ty: ValueType, n: u64) -> Value {
    Value::Word(ty, word(n))
}
fn bytes(source: &str) -> Vec<u8> {
    hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap()
}
fn selector(code: &[u8], name: &str) -> [u8; 32] {
    function_selector(
        parse(code)
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap(),
    )
}
fn call(address: [u8; 32], name: &str, arguments: Vec<Value>) -> CallRequest<Value> {
    CallRequest {
        contract: address,
        function: name.into(),
        arguments,
        caller: word(9),
        value: word(0),
        gas_limit: 1000000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    }
}
fn deploy(
    host: &mut TransactionalHost<InMemoryState>,
    code: Vec<u8>,
    nonce: u64,
    initializer: Option<Initializer<Value>>,
) -> [u8; 32] {
    let result = host.deploy_values(DeployRequest {
        deployer: word(9),
        nonce,
        bytecode: code,
        initializer,
        value: word(0),
        gas_limit: 1000000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    });
    let DeployOutcome::Success(result) = result else {
        panic!("{result:?}")
    };
    result.contract
}
fn child_init() -> Option<Initializer<Value>> {
    Some(Initializer {
        function: "initialize".into(),
        arguments: vec![value(ValueType::Bool, 1)],
    })
}
const CHILD: &str = "contract Child { state { creator: address; amount: u256; } event Initialized { creator: address } pub fn initialize(accept: bool) -> bool { self.creator = msg.sender; self.amount = msg.value; emit Initialized { creator: msg.sender }; return accept; } pub fn creator() -> address { return self.creator; } }";
const FACTORY: &str = "contract Factory { state { last: address; } event Created { child: address } pub fn create(template: address, salt: bytes32, initializer: bytes32, child_accept: bool, parent_accept: bool) -> address { let child: address = create_contract(template, salt, initializer, 5, child_accept); self.last = child; emit Created { child: child }; require(parent_accept); return child; } }";
fn create(
    factory: [u8; 32],
    template: [u8; 32],
    initializer: [u8; 32],
    child: u64,
    parent: u64,
) -> CallRequest<Value> {
    call(
        factory,
        "create",
        vec![
            Value::Word(ValueType::Address, template),
            value(ValueType::Bytes32, 17),
            Value::Word(ValueType::Bytes32, initializer),
            value(ValueType::Bool, child),
            value(ValueType::Bool, parent),
        ],
    )
}

#[test]
fn creation_is_atomic_metered_prefunding_safe_and_collision_resistant() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let code = bytes(CHILD);
    let template = deploy(&mut host, code.clone(), 0, child_init());
    let factory_code = bytes(FACTORY);
    assert_eq!(factory_code[7], 14);
    let factory = deploy(&mut host, factory_code, 1, None);
    host.state_mut().set_balance(factory, word(10));
    let child = child_contract_address(
        factory,
        caller_bound_salt(word(9), word(17)),
        code_hash(&code),
        700777,
    );
    host.state_mut().set_balance(child, word(3));
    let before = host.state().clone();
    for (child_accept, parent_accept) in [(0, 1), (1, 0)] {
        let HostOutcome::Failure(failure) = host.execute_values(create(
            factory,
            template,
            selector(&code, "initialize"),
            child_accept,
            parent_accept,
        )) else {
            panic!("accepted failure")
        };
        assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::Revert));
        assert_eq!(host.state(), &before);
    }
    let request = create(factory, template, selector(&code, "initialize"), 1, 1);
    let HostOutcome::Success(result) = host.execute_values(request.clone()) else {
        panic!("creation failed")
    };
    assert_eq!(
        result.result.return_value,
        Value::Word(ValueType::Address, child)
    );
    assert_eq!(host.state().balance(&child), word(8));
    assert_eq!(result.deployments.len(), 1);
    let registration = &result.deployments[0];
    assert_eq!(registration.creator, factory);
    assert_eq!(registration.contract, child);
    assert_eq!(registration.code_hash, code_hash(&code));
    assert_eq!(
        registration.origin,
        lithovm_host::DeploymentOrigin::Child {
            template,
            salt: caller_bound_salt(word(9), word(17))
        }
    );
    let statuses = lithovm_host::deployment_status::included(
        &result.deployments,
        &lithovm_host::deployment_status::Inclusion {
            chain_id: 700777,
            transaction_hash: word(40),
            block_hash: word(41),
            block_height: 1,
            transaction_gas_used: result.gas_used,
        },
    )
    .unwrap();
    assert_eq!(statuses[0]["state"], "included");
    assert_eq!(statuses[0]["verificationState"], "unverified");
    assert_eq!(host.state().balance(&factory), word(5));
    assert_eq!(
        host.state()
            .contract(&child)
            .unwrap()
            .storage
            .get("creator"),
        Some(&factory)
    );
    assert_eq!(
        host.state().contract(&child).unwrap().storage.get("amount"),
        Some(&word(5))
    );
    assert_eq!(
        result
            .events
            .iter()
            .map(|e| e.event.name.as_str())
            .collect::<Vec<_>>(),
        ["Initialized", "Created"]
    );
    let after = host.state().clone();
    let HostOutcome::Failure(failure) = host.execute_values(request.clone()) else {
        panic!("accepted collision")
    };
    assert_eq!(failure.kind, HostFailureKind::ContractExists);
    assert_eq!(host.state(), &after);
    for gas in 0..result.gas_used {
        let mut limited = TransactionalHost::new(before.clone());
        let mut request = request.clone();
        request.gas_limit = gas;
        let HostOutcome::Failure(failure) = limited.execute_values(request) else {
            panic!("accepted {gas}")
        };
        assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::OutOfGas));
        assert_eq!(failure.gas_used, gas);
        assert_eq!(limited.state(), &before);
    }
    let mut exact = TransactionalHost::new(before);
    let mut request = request;
    request.gas_limit = result.gas_used;
    assert!(matches!(
        exact.execute_values(request),
        HostOutcome::Success(_)
    ));
    assert_ne!(
        child,
        child_contract_address(
            factory,
            caller_bound_salt(word(9), word(18)),
            code_hash(&code),
            700777
        )
    );
    assert_ne!(
        child,
        child_contract_address(
            word(10),
            caller_bound_salt(word(9), word(17)),
            code_hash(&code),
            700777
        )
    );
    assert_ne!(
        child,
        child_contract_address(
            factory,
            caller_bound_salt(word(9), word(17)),
            code_hash(&code),
            9005
        )
    );
}

#[test]
fn finance_factory_initializes_fresh_children_for_all_feature_profiles() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let code = bytes(include_str!(
        "../../../../sdk/contracts/standards/finance_token_v12.lithic"
    ));
    let mut args = vec![
        Value::String("Template".into()),
        Value::String("OLD".into()),
        value(ValueType::Address, 8),
        value(ValueType::U64, 18),
        value(ValueType::U256, 777),
    ];
    args.extend((0..4).map(|_| value(ValueType::Bool, 1)));
    let template = deploy(
        &mut host,
        code.clone(),
        0,
        Some(Initializer {
            function: "initialize".into(),
            arguments: args,
        }),
    );
    let factory = deploy(
        &mut host,
        bytes(include_str!(
            "../../../../sdk/contracts/standards/finance_factory_v14.lithic"
        )),
        1,
        Some(Initializer {
            function: "initialize".into(),
            arguments: vec![
                Value::Word(ValueType::Address, template),
                Value::Word(ValueType::Bytes32, selector(&code, "initialize")),
            ],
        }),
    );
    let template_before = host.state().contract(&template).unwrap().clone();
    for flags in 0..16 {
        let name = Value::String(format!("Client \u{1faa8}\0 {flags}"));
        let symbol = Value::String("TEST".into());
        let mut args = vec![
            value(ValueType::Bytes32, flags),
            name.clone(),
            symbol.clone(),
            value(ValueType::U64, 6),
            value(ValueType::U256, 1000000),
        ];
        args.extend((0..4).map(|bit| value(ValueType::Bool, (flags >> bit) & 1)));
        let HostOutcome::Success(result) = host.execute_values(call(factory, "create", args))
        else {
            panic!("profile {flags}")
        };
        let Value::Word(ValueType::Address, child) = result.result.return_value else {
            panic!("not an address")
        };
        assert_eq!(
            child,
            child_contract_address(
                factory,
                caller_bound_salt(word(9), word(flags)),
                code_hash(&code),
                700777
            )
        );
        let last = result.events.last().unwrap();
        assert_eq!(last.contract, factory);
        assert_eq!(last.event.name, "TokenCreated");
        assert_eq!(
            last.event.fields[0].2,
            Value::Word(ValueType::Address, child)
        );
        assert_eq!(last.event.fields[1].2, value(ValueType::Address, 9));
        assert_eq!(last.event.fields[2].2, name);
        for (function, args, expected) in [
            ("name", vec![], name),
            ("symbol", vec![], symbol),
            ("decimals", vec![], value(ValueType::U64, 6)),
            ("total_supply", vec![], value(ValueType::U256, 1000000)),
            (
                "owner",
                vec![],
                value(ValueType::Address, if flags & 8 != 0 { 9 } else { 0 }),
            ),
            (
                "balance_of",
                vec![value(ValueType::Address, 9)],
                value(ValueType::U256, 1000000),
            ),
            (
                "balance_of",
                vec![Value::Word(ValueType::Address, factory)],
                value(ValueType::U256, 0),
            ),
        ] {
            let HostOutcome::Success(read) = host.execute_values(call(child, function, args))
            else {
                panic!("getter")
            };
            assert_eq!(read.result.return_value, expected);
        }
        assert_eq!(host.state().contract(&template).unwrap(), &template_before);
    }
    let HostOutcome::Success(count) = host.execute_values(call(factory, "created", vec![])) else {
        panic!("count")
    };
    assert_eq!(count.result.return_value, value(ValueType::U64, 16));
    // R1 H2 regression on the actual FinanceFactory/token artifacts: a second
    // user can reuse salt 0 after the first user, without inheriting ownership.
    let mut args = vec![
        value(ValueType::Bytes32, 0),
        Value::String("Other".into()),
        Value::String("OTHER".into()),
        value(ValueType::U64, 18),
        value(ValueType::U256, 99),
    ];
    args.extend((0..4).map(|_| value(ValueType::Bool, 1)));
    let mut request = call(factory, "create", args);
    request.caller = word(100);
    let HostOutcome::Success(other) = host.execute_values(request.clone()) else {
        panic!("copied salt blocked second Finance user")
    };
    let Value::Word(ValueType::Address, child) = other.result.return_value else {
        panic!("not address")
    };
    assert_eq!(
        child,
        child_contract_address(
            factory,
            caller_bound_salt(word(100), word(0)),
            code_hash(&code),
            700777
        )
    );
    let HostOutcome::Success(owner) = host.execute_values(call(child, "owner", vec![])) else {
        panic!("owner")
    };
    assert_eq!(owner.result.return_value, value(ValueType::Address, 100));
    let before = host.state().clone();
    assert!(matches!(
        host.execute_values(request),
        HostOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
}

#[test]
fn malformed_creation_and_legacy_modes_fail_closed() {
    let code = bytes(FACTORY);
    for version in [11, 12, 13] {
        let mut bad = code.clone();
        bad[7] = version;
        assert!(parse(&bad).is_err());
    }
    for end in 0..code.len() {
        assert!(parse(&code[..end]).is_err());
    }
    for expression in [
        "let x: bool = create_contract(t, s, i, 0);",
        "let mut x: address = create_contract(t, s, i, 0);",
        "let x = create_contract(t, s, i, 0);",
        "let x: address = create_contract(t, s, i, 0); call_contract(t, i, 0);",
    ] {
        assert!(compile(&format!("contract F {{ pub fn run(t: address, s: bytes32, i: bytes32) -> bool {{ {expression} return true; }} }}")).is_err());
    }
    let mut storage = Storage::default();
    assert!(matches!(
        Vm::default().execute_values_transactionally(
            &code,
            "create",
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
fn fabricated_contract_event_is_not_a_registration_and_record_limit_rolls_back() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let fake = deploy(&mut host, bytes("contract Fake { event TokenCreated { token: address } pub fn fake(token: address) -> bool { emit TokenCreated { token: token }; return true; } }"), 0, None);
    let HostOutcome::Success(result) =
        host.execute_values(call(fake, "fake", vec![value(ValueType::Address, 7)]))
    else {
        panic!("event test")
    };
    assert_eq!(result.events.len(), 1);
    assert!(result.deployments.is_empty());
    let template_code = bytes(CHILD);
    let template = deploy(&mut host, template_code.clone(), 1, child_init());
    let mut source = String::from("contract Many {");
    for n in 0..65 {
        source.push_str(&format!("const S{n}: bytes32 = 0x{n:064x};"));
    }
    source.push_str("pub fn run(t: address, init: bytes32) -> bool {");
    for n in 0..65 {
        source.push_str(&format!(
            "let c{n}: address = create_contract(t, S{n}, init, 0, true);"
        ));
    }
    source.push_str("return true; } }");
    let factory = deploy(&mut host, bytes(&source), 2, None);
    let before = host.state().clone();
    let HostOutcome::Failure(failure) = host.execute_values(call(
        factory,
        "run",
        vec![
            Value::Word(ValueType::Address, template),
            Value::Word(ValueType::Bytes32, selector(&template_code, "initialize")),
        ],
    )) else {
        panic!("accepted 65 registrations")
    };
    assert!(failure.message.contains("deployment record limit"));
    assert_eq!(host.state(), &before);
}

#[test]
fn invalid_templates_initializers_balances_and_code_sizes_discard_creation() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let code = bytes(CHILD);
    let template = deploy(&mut host, code.clone(), 0, child_init());
    let factory = deploy(&mut host, bytes(FACTORY), 1, None);
    host.state_mut().set_balance(factory, word(10));
    let before = host.state().clone();
    for (target, entry, expected) in [
        (
            word(100),
            selector(&code, "initialize"),
            HostFailureKind::MissingContract,
        ),
        (template, [255; 32], HostFailureKind::UnknownSelector),
        (
            template,
            selector(&code, "creator"),
            HostFailureKind::InvalidEntrypoint,
        ),
    ] {
        let HostOutcome::Failure(failure) =
            host.execute_values(create(factory, target, entry, 1, 1))
        else {
            panic!("accepted invalid create")
        };
        assert_eq!(failure.kind, expected);
        assert_eq!(host.state(), &before);
    }
    let mut invalid_args = create(factory, template, selector(&code, "initialize"), 1, 1);
    // A different child signature rejects the factory's bool argument.
    let wrong = bytes("contract Wrong { pub fn initialize(n: u64) -> bool { return true; } }");
    let wrong_template = deploy(
        &mut host,
        wrong.clone(),
        2,
        Some(Initializer {
            function: "initialize".into(),
            arguments: vec![value(ValueType::U64, 0)],
        }),
    );
    invalid_args.arguments[0] = Value::Word(ValueType::Address, wrong_template);
    invalid_args.arguments[2] = Value::Word(ValueType::Bytes32, selector(&wrong, "initialize"));
    let before = host.state().clone();
    assert!(matches!(
        host.execute_values(invalid_args),
        HostOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
    host.state_mut().set_balance(factory, word(4));
    let before = host.state().clone();
    let HostOutcome::Failure(failure) = host.execute_values(create(
        factory,
        template,
        selector(&code, "initialize"),
        1,
        1,
    )) else {
        panic!("accepted insufficient balance")
    };
    assert_eq!(failure.kind, HostFailureKind::InsufficientBalance);
    assert_eq!(host.state(), &before);
    let mut large =
        String::from("contract Large { pub fn initialize(ok: bool) -> bool { return ok; }");
    for n in 0..900 {
        large.push_str(&format!(
            "pub fn f{n}(x: u64) -> u64 {{ let y: u64 = x + 1; return y + 2; }}"
        ));
    }
    large.push('}');
    let large_code = bytes(&large);
    assert!(large_code.len() > lithovm_host::MAX_CHILD_CODE_BYTES);
    let large_template = deploy(&mut host, large_code.clone(), 3, child_init());
    let before = host.state().clone();
    let HostOutcome::Failure(failure) = host.execute_values(create(
        factory,
        large_template,
        selector(&large_code, "initialize"),
        1,
        1,
    )) else {
        panic!("accepted oversized child")
    };
    assert_eq!(failure.kind, HostFailureKind::InvalidBytecode);
    assert_eq!(host.state(), &before);
}

#[test]
fn newly_created_child_can_be_called_and_top_level_deploy_rolls_back_both() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let code = bytes(CHILD);
    let template = deploy(&mut host, code.clone(), 0, child_init());
    let factory_code = bytes("contract Factory { pub fn initialize(template: address, salt: bytes32, init: bytes32, getter: bytes32, accept: bool) -> address { let child: address = create_contract(template, salt, init, 0, true); let observed: address = invoke(child, getter, 0); require(accept); return observed; } }");
    let mut request = DeployRequest {
        deployer: word(9),
        nonce: 1,
        bytecode: factory_code.clone(),
        initializer: Some(Initializer {
            function: "initialize".into(),
            arguments: vec![
                Value::Word(ValueType::Address, template),
                value(ValueType::Bytes32, 1),
                Value::Word(ValueType::Bytes32, selector(&code, "initialize")),
                Value::Word(ValueType::Bytes32, selector(&code, "creator")),
                value(ValueType::Bool, 0),
            ],
        }),
        value: word(0),
        gas_limit: 1000000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700777,
    };
    let before = host.state().clone();
    assert!(matches!(
        host.deploy_values(request.clone()),
        DeployOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
    request.initializer.as_mut().unwrap().arguments[4] = value(ValueType::Bool, 1);
    let DeployOutcome::Success(result) = host.deploy_values(request) else {
        panic!("recovery deployment failed")
    };
    assert_eq!(
        result.initializer_result.unwrap().return_value,
        Value::Word(ValueType::Address, result.contract)
    );
    let child = child_contract_address(
        result.contract,
        caller_bound_salt(word(9), word(1)),
        code_hash(&code),
        700777,
    );
    assert_eq!(result.deployments.len(), 2);
    assert_eq!(result.deployments[0].contract, result.contract);
    assert_eq!(result.deployments[1].contract, child);
    assert!(host.state().contract(&child).is_some());
}
