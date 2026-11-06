use lithic_lithovm::compile;
use lithovm_bytecode::{
    function_selector, parse,
    values::{self, Value},
    ValueType,
};
use lithovm_host::{
    CallRequest, DeployOutcome, DeployRequest, HostOutcome, InMemoryState, TransactionalHost,
};

fn word(n: u64) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}
fn deploy(
    host: &mut TransactionalHost<InMemoryState>,
    source: &str,
    nonce: u64,
) -> ([u8; 32], Vec<u8>) {
    let code = hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap();
    let outcome = host.deploy_values(DeployRequest {
        deployer: word(9),
        nonce,
        bytecode: code.clone(),
        initializer: None,
        value: word(0),
        gas_limit: 1000000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700780,
    });
    let DeployOutcome::Success(r) = outcome else {
        panic!("{outcome:?}")
    };
    (r.contract, code)
}
fn payload(code: &[u8], name: &str, args: &[Value]) -> Vec<u8> {
    let function = parse(code)
        .unwrap()
        .functions
        .into_iter()
        .find(|f| f.name == name)
        .unwrap();
    let mut bytes = function_selector(&function).to_vec();
    bytes.extend(values::encode(args).unwrap());
    bytes
}
fn call(
    parent: [u8; 32],
    name: &str,
    target: [u8; 32],
    data: Vec<u8>,
    amount: u64,
) -> CallRequest<Value> {
    CallRequest {
        contract: parent,
        function: name.into(),
        arguments: vec![
            Value::Word(ValueType::Address, target),
            Value::Word(ValueType::U256, word(amount)),
            Value::Bytes(data),
        ],
        caller: word(9),
        value: word(0),
        gas_limit: 1000000,
        block_height: 1,
        block_timestamp: 2,
        chain_id: 700780,
    }
}
const PARENT:&str="contract Parent { state { marker: u64; } pub fn run(target: address, amount: u256, data: bytes) -> bytes { self.marker = 1; let result: bytes = invoke_payload(target, amount, data); return result; } pub fn check(target: address, amount: u256, data: bytes) -> bytes { let result: bytes = invoke_static_payload(target, amount, data); return result; } pub fn revert_after(target: address, amount: u256, data: bytes) -> bytes { self.marker = 1; let result: bytes = invoke_payload(target, amount, data); revert(); } }";
const CHILD:&str="contract Child { const ONE: u256 = 1; const ZERO: u256 = 0; state { n: u64; values: map<u64, u64>; } event Changed { n: u64 } pub fn read() -> u64 { return self.n; } pub fn write(n: u64) -> bool { self.n = n; return true; } pub fn map_write() -> bool { self.values[1] = 2; return true; } pub fn log() -> bool { emit Changed { n: 1 }; return true; } pub fn pay(to: address) -> bool { transfer_native(to, ONE); return true; } pub fn defer(target: address, selector: bytes32) -> bool { call_contract(target, selector, ZERO); return true; } }";

#[test]
fn raw_native_calls_preserve_typed_results_value_transfer_and_rollback() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, code) = deploy(&mut host, CHILD, 0);
    let (parent, _) = deploy(&mut host, PARENT, 1);
    host.state_mut().set_balance(parent, word(100));
    let before = host.state().clone();
    let data = payload(&code, "write", &[Value::Word(ValueType::U64, word(42))]);
    let request = call(parent, "run", child, data.clone(), 5);
    let outcome = host.execute_values(request.clone());
    let HostOutcome::Success(result) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        result.result.return_value,
        Value::Bytes(values::encode(&[Value::Word(ValueType::Bool, word(1))]).unwrap())
    );
    assert_eq!(
        host.state().contract(&child).unwrap().storage.get("n"),
        Some(&word(42))
    );
    assert_eq!(host.state().balance(&child), word(5));
    assert_eq!(host.state().balance(&parent), word(95));
    for gas in [0, 50, result.gas_used - 1] {
        let mut limited = TransactionalHost::new(before.clone());
        let mut req = request.clone();
        req.gas_limit = gas;
        assert!(matches!(
            limited.execute_values(req),
            HostOutcome::Failure(_)
        ));
        assert_eq!(limited.state(), &before);
    }
    let mut failed = TransactionalHost::new(before.clone());
    assert!(matches!(
        failed.execute_values(call(parent, "revert_after", child, data, 5)),
        HostOutcome::Failure(_)
    ));
    assert_eq!(failed.state(), &before);
}

#[test]
fn static_calls_reject_every_stateful_effect_and_nonzero_value() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, code) = deploy(&mut host, CHILD, 0);
    let (parent, _) = deploy(&mut host, "contract Checker { pub fn check(target: address, amount: u256, data: bytes) -> bytes { let result: bytes = invoke_static_payload(target, amount, data); return result; } }", 1);
    host.state_mut().set_balance(child, word(100));
    let before = host.state().clone();
    let success = host.execute_values(call(parent, "check", child, payload(&code, "read", &[]), 0));
    assert!(matches!(success, HostOutcome::Success(_)), "{success:?}");
    assert_eq!(host.state(), &before);
    for (entry, args, amount) in [
        ("write", vec![Value::Word(ValueType::U64, word(42))], 0),
        ("map_write", vec![], 0),
        ("log", vec![], 0),
        ("pay", vec![Value::Word(ValueType::Address, word(3))], 0),
        ("read", vec![], 1),
        (
            "defer",
            vec![
                Value::Word(ValueType::Address, child),
                Value::Word(
                    ValueType::Bytes32,
                    payload(&code, "read", &[])[..32].try_into().unwrap(),
                ),
            ],
            0,
        ),
    ] {
        let outcome = host.execute_values(call(
            parent,
            "check",
            child,
            payload(&code, entry, &args),
            amount,
        ));
        assert!(
            matches!(outcome, HostOutcome::Failure(_)),
            "{entry}: {outcome:?}"
        );
        assert_eq!(host.state(), &before);
    }
}

#[test]
fn readonly_flag_propagates_through_typed_and_payload_nested_calls() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, code) = deploy(&mut host, CHILD, 0);
    let (parent, _) = deploy(&mut host, PARENT, 1);
    let wrapper="contract Wrapper { pub fn typed(target: address, selector: bytes32) -> bool { let result: bool = invoke(target, selector, 0); return result; } pub fn raw(target: address, data: bytes) -> bytes { let result: bytes = invoke_payload(target, 0, data); return result; } }";
    let (wrapper, wrapper_code) = deploy(&mut host, wrapper, 2);
    let before = host.state().clone();
    let selector: [u8; 32] = payload(&code, "map_write", &[])[..32].try_into().unwrap();
    for data in [
        payload(
            &wrapper_code,
            "typed",
            &[
                Value::Word(ValueType::Address, child),
                Value::Word(ValueType::Bytes32, selector),
            ],
        ),
        payload(
            &wrapper_code,
            "raw",
            &[
                Value::Word(ValueType::Address, child),
                Value::Bytes(payload(&code, "map_write", &[])),
            ],
        ),
    ] {
        let outcome = host.execute_values(call(parent, "check", wrapper, data, 0));
        assert!(matches!(outcome, HostOutcome::Failure(_)), "{outcome:?}");
        assert_eq!(host.state(), &before);
    }
}

#[test]
fn malformed_payloads_and_reentrancy_fail_without_state_change() {
    let mut host = TransactionalHost::new(InMemoryState::default());
    let (child, code) = deploy(&mut host, CHILD, 0);
    let (parent, parent_code) = deploy(&mut host, PARENT, 1);
    let before = host.state().clone();
    let valid = payload(&code, "read", &[]);
    for data in [
        vec![],
        valid[..38].to_vec(),
        [valid.clone(), vec![0]].concat(),
        [vec![0; 32], values::encode(&[]).unwrap()].concat(),
        payload(&code, "read", &[Value::Word(ValueType::U64, word(1))]),
    ] {
        assert!(matches!(
            host.execute_values(call(parent, "run", child, data, 0)),
            HostOutcome::Failure(_)
        ));
        assert_eq!(host.state(), &before);
    }
    let recursive = payload(
        &parent_code,
        "run",
        &[
            Value::Word(ValueType::Address, child),
            Value::Word(ValueType::U256, word(0)),
            Value::Bytes(valid),
        ],
    );
    assert!(matches!(
        host.execute_values(call(parent, "run", parent, recursive, 0)),
        HostOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
}

#[test]
fn direct_readonly_vm_views_do_not_materialize_implicit_storage_defaults() {
    use lithovm::{ExecutionContext, ExecutionOutcome, Storage, Vm};
    let context = ExecutionContext {
        read_only: true,
        ..ExecutionContext::default()
    };
    let source="contract View { state { n: u64; } pub fn read() -> u64 { return self.n; } pub fn write() -> u64 { self.n = 1; return self.n; } }";
    let bytes = hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap();
    let mut storage = Storage::default();
    let before = storage.clone();
    let vm = Vm::default();
    assert!(matches!(
        vm.execute_transactionally(&bytes, "read", &[], 1000, &mut storage, &context),
        ExecutionOutcome::Success(_)
    ));
    assert_eq!(storage, before);
    assert!(vm
        .execute_with_storage_and_context(&bytes, "read", &[], 1000, &mut storage, &context)
        .is_ok());
    assert_eq!(storage, before);
    assert!(matches!(
        vm.execute_transactionally(&bytes, "write", &[], 1000, &mut storage, &context),
        ExecutionOutcome::Failure(_)
    ));
    assert_eq!(storage, before);
    let source =
        "contract View { state { text: string; } pub fn read() -> string { return self.text; } }";
    let bytes = hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap();
    assert!(matches!(
        vm.execute_values_transactionally(&bytes, "read", &[], 1000, &mut storage, &context),
        ExecutionOutcome::Success(_)
    ));
    assert_eq!(storage, before);
}
