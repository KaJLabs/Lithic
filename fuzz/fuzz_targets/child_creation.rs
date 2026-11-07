#![no_main]

use libfuzzer_sys::fuzz_target;
use lithic_lithovm::compile;
use lithovm::FailureKind;
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    caller_bound_salt, child_contract_address, code_hash, CallRequest, DeployOutcome,
    DeployRequest, HostFailureKind, HostOutcome, InMemoryState, Initializer, TransactionalHost,
};

type Fixture = (InMemoryState, [u8; 32], [u8; 32], [u8; 32], [u8; 32]);
fn word(n: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&n.to_be_bytes());
    word
}
thread_local! {
    static FIXTURE: Fixture = {
        let mut host = TransactionalHost::new(InMemoryState::default());
        let mut addresses = Vec::new();
        let mut initializer = [0; 32];
        let mut hash = [0; 32];
        for (nonce, source) in [
            "contract Child { state { creator: address; amount: u256; } event Initialized { creator: address } pub fn initialize(accept: bool) -> bool { self.creator = msg.sender; self.amount = msg.value; emit Initialized { creator: msg.sender }; return accept; } }",
            "contract Factory { state { last: address; } event Created { child: address } pub fn create(template: address, salt: bytes32, initializer: bytes32, child_accept: bool, parent_accept: bool, amount: u256) -> address { let child: address = create_contract(template, salt, initializer, amount, child_accept); self.last = child; emit Created { child: child }; require(parent_accept); return child; } }",
        ].iter().enumerate() {
            let bytes = hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap();
            if nonce == 0 { initializer = function_selector(&parse(&bytes).unwrap().functions[0]); hash = code_hash(&bytes); }
            let init = (nonce == 0).then(|| Initializer { function: "initialize".into(), arguments: vec![Value::Word(ValueType::Bool, word(1))] });
            let DeployOutcome::Success(result) = host.deploy_values(DeployRequest { deployer: word(9), nonce: nonce as u64, bytecode: bytes, initializer: init, value: word(0), gas_limit: 100000, block_height: 1, block_timestamp: 2, chain_id: 700777 }) else { panic!("fixture") };
            addresses.push(result.contract);
        }
        host.state_mut().set_balance(addresses[1], word(10));
        (host.into_state(), addresses[1], addresses[0], initializer, hash)
    };
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 7 {
        return;
    }
    FIXTURE.with(|(state, factory, template, initializer, hash)| {
        let gas = u16::from_le_bytes([data[0], data[1]]) as u64;
        let amount = (data[2] % 16) as u64;
        let child_accept = data[3] & 1 != 0;
        let parent_accept = data[4] & 1 != 0;
        let salt = word(data[5] as u64);
        let prefund = (data[6] % 16) as u64;
        let child =
            child_contract_address(*factory, caller_bound_salt(word(9), salt), *hash, 700777);
        let mut before = state.clone();
        before.set_balance(child, word(prefund));
        let request = CallRequest {
            contract: *factory,
            function: "create".into(),
            arguments: vec![
                Value::Word(ValueType::Address, *template),
                Value::Word(ValueType::Bytes32, salt),
                Value::Word(ValueType::Bytes32, *initializer),
                Value::Word(ValueType::Bool, word(child_accept as u64)),
                Value::Word(ValueType::Bool, word(parent_accept as u64)),
                Value::Word(ValueType::U256, word(amount)),
            ],
            caller: word(9),
            value: word(0),
            gas_limit: gas,
            block_height: 1,
            block_timestamp: 2,
            chain_id: 700777,
        };
        let mut host = TransactionalHost::new(before.clone());
        let outcome = host.execute_values(request.clone());
        let mut replay = TransactionalHost::new(before.clone());
        assert_eq!(replay.execute_values(request.clone()), outcome);
        assert_eq!(replay.state(), host.state());
        match outcome {
            HostOutcome::Success(result) => {
                assert!(child_accept && parent_accept && amount <= 10);
                assert!(result.gas_used <= gas);
                assert_eq!(
                    result.result.return_value,
                    Value::Word(ValueType::Address, child)
                );
                assert_eq!(host.state().balance(factory), word(10 - amount));
                assert_eq!(host.state().balance(&child), word(prefund + amount));
                let deployed = host.state().contract(&child).unwrap();
                assert_eq!(deployed.code_hash, *hash);
                assert_eq!(deployed.storage.get("creator"), Some(factory));
                assert_eq!(deployed.storage.get("amount"), Some(&word(amount)));
                assert_eq!(
                    host.state().contract(factory).unwrap().storage.get("last"),
                    Some(&child)
                );
                assert_eq!(host.state().contract(template), before.contract(template));
                assert_eq!(
                    result
                        .events
                        .iter()
                        .map(|e| e.event.name.as_str())
                        .collect::<Vec<_>>(),
                    ["Initialized", "Created"]
                );
                let after = host.state().clone();
                let HostOutcome::Failure(failure) = host.execute_values(request) else {
                    panic!("collision accepted")
                };
                assert_eq!(failure.kind, HostFailureKind::ContractExists);
                assert_eq!(host.state(), &after);
            }
            HostOutcome::Failure(failure) => {
                assert!(failure.gas_used <= gas);
                if failure.kind == HostFailureKind::Vm(FailureKind::OutOfGas) {
                    assert_eq!(failure.gas_used, gas);
                }
                assert_eq!(host.state(), &before);
            }
        }
    });
});
