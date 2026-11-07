#![no_main]

use libfuzzer_sys::fuzz_target;
use lithic_lithovm::compile;
use lithovm::FailureKind;
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    CallRequest, DeployOutcome, DeployRequest, HostFailureKind, HostOutcome, InMemoryState,
    TransactionalHost,
};

type Fixture = (InMemoryState, [u8; 32], [u8; 32], [u8; 32]);

fn word(n: u64) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[24..].copy_from_slice(&n.to_be_bytes());
    bytes
}

thread_local! {
    static FIXTURE: Fixture = {
        let mut host = TransactionalHost::new(InMemoryState::default());
        let mut contracts = Vec::new();
        let mut selector = [0; 32];
        for (nonce, source) in [
            "contract Child { state { n: u64; } event Child { n: u64 } pub fn run(n: u64) -> u64 { self.n = n + 2; emit Child { n: self.n }; return self.n; } }",
            "contract Parent { state { n: u64; } event Before { n: u64 } event After { n: u64 } pub fn run(target: address, selector: bytes32, n: u64, accept: bool, amount: u256) -> u64 { self.n = n; emit Before { n: n }; let answer: u64 = invoke(target, selector, amount, n); self.n = answer; emit After { n: answer }; require(accept); return answer + 1; } }",
        ].iter().enumerate() {
            let bytes = hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap();
            if nonce == 0 { selector = function_selector(&parse(&bytes).unwrap().functions[0]); }
            let DeployOutcome::Success(result) = host.deploy_values(DeployRequest {
                deployer: word(9), nonce: nonce as u64, bytecode: bytes, initializer: None,
                value: word(0), gas_limit: 100000, block_height: 1, block_timestamp: 2, chain_id: 700777,
            }) else { panic!("fixture deployment failed") };
            contracts.push(result.contract);
        }
        host.state_mut().set_balance(contracts[1], word(10));
        (host.state().clone(), contracts[1], contracts[0], selector)
    };
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 6 {
        return;
    }
    FIXTURE.with(|(before, parent, child, selector)| {
        let number = u16::from_le_bytes([data[0], data[1]]) as u64;
        let gas = u16::from_le_bytes([data[2], data[3]]) as u64;
        let accept = data[4] & 1 != 0;
        let amount = (data[5] % 16) as u64;
        let call = CallRequest {
            contract: *parent,
            function: "run".into(),
            arguments: vec![
                Value::Word(ValueType::Address, *child),
                Value::Word(ValueType::Bytes32, *selector),
                Value::Word(ValueType::U64, word(number)),
                Value::Word(ValueType::Bool, word(accept as u64)),
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
        let outcome = host.execute_values(call.clone());
        let mut replay = TransactionalHost::new(before.clone());
        assert_eq!(replay.execute_values(call), outcome);
        assert_eq!(replay.state(), host.state());
        match outcome {
            HostOutcome::Success(result) => {
                assert!(accept && amount <= 10);
                assert!(result.gas_used <= gas);
                assert_eq!(
                    result.result.return_value,
                    Value::Word(ValueType::U64, word(number + 3))
                );
                assert_eq!(host.state().balance(parent), word(10 - amount));
                assert_eq!(host.state().balance(child), word(amount));
                for address in [parent, child] {
                    assert_eq!(
                        host.state().contract(address).unwrap().storage.get("n"),
                        Some(&word(number + 2))
                    );
                }
                assert_eq!(
                    result
                        .events
                        .iter()
                        .map(|e| e.event.name.as_str())
                        .collect::<Vec<_>>(),
                    ["Before", "Child", "After"]
                );
            }
            HostOutcome::Failure(failure) => {
                assert_eq!(host.state(), before);
                assert!(failure.gas_used <= gas);
                if failure.kind == HostFailureKind::Vm(FailureKind::OutOfGas) {
                    assert_eq!(failure.gas_used, gas);
                }
            }
        }
    });
});
