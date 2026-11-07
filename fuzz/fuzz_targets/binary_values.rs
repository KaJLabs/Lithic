#![no_main]

use libfuzzer_sys::fuzz_target;
use lithic_lithovm::compile;
use lithovm::FailureKind;
use lithovm_bytecode::{
    values::{self, Value},
    ValueType,
};
use lithovm_host::{
    CallRequest, DeployOutcome, DeployRequest, HostFailureKind, HostOutcome, InMemoryState,
    TransactionalHost,
};

fn word(n: u64) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[24..].copy_from_slice(&n.to_be_bytes());
    bytes
}

thread_local! {
    static FIXTURE: (InMemoryState, [u8; 32]) = {
        let source = "contract Binary { state { digest: bytes32; } event Data { data: bytes } pub fn run(data: bytes, accept: bool) -> bytes { self.digest = keccak256(data); emit Data { data: data }; require(accept); return data; } }";
        let mut host = TransactionalHost::new(InMemoryState::default());
        let bytes = hex::decode(&compile(source).unwrap().bytecode[2..]).unwrap();
        let DeployOutcome::Success(deployed) = host.deploy_values(DeployRequest {
            deployer: word(9), nonce: 0, bytecode: bytes, initializer: None,
            value: word(0), gas_limit: 100000, block_height: 1, block_timestamp: 2, chain_id: 700777,
        }) else { panic!("fixture deployment failed") };
        (host.state().clone(), deployed.contract)
    };
}

fuzz_target!(|input: &[u8]| {
    if input.len() < 3 || input.len() > 4101 {
        return;
    }
    let gas = u16::from_le_bytes([input[0], input[1]]) as u64;
    let accept = input[2] & 1 != 0;
    let data = Value::Bytes(input[3..].to_vec());
    let arguments = vec![
        data.clone(),
        Value::Word(ValueType::Bool, word(u64::from(accept))),
    ];
    if let Ok(encoded) = values::encode(&arguments) {
        assert_eq!(values::decode(&encoded).unwrap(), arguments);
    }
    FIXTURE.with(|(before, contract)| {
        let request = CallRequest {
            contract: *contract,
            function: "run".into(),
            arguments,
            gas_limit: gas,
            caller: word(9),
            value: word(0),
            block_height: 1,
            block_timestamp: 2,
            chain_id: 700777,
        };
        let mut host = TransactionalHost::new(before.clone());
        let result = host.execute_values(request.clone());
        let mut replay = TransactionalHost::new(before.clone());
        assert_eq!(replay.execute_values(request), result);
        assert_eq!(replay.state(), host.state());
        match result {
            HostOutcome::Success(result) => {
                assert!(accept && input.len() - 3 <= values::MAX_BYTES_LENGTH);
                assert!(result.gas_used <= gas);
                assert_eq!(result.result.return_value, data);
                assert_eq!(result.events.len(), 1);
                assert_eq!(result.events[0].event.fields[0].2, data);
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
