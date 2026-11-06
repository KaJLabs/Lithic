#![no_main]
//! A local, unapproved account fixture; no chain or custody activity.
use libfuzzer_sys::fuzz_target;
use lithic_lithovm::compile;
use lithovm::{FailureKind, Storage};
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    code_hash, CallRequest, DeployedContract, HostFailureKind, HostOutcome, InMemoryState,
    TransactionalHost,
};
use serde_json::Value as Json;

fn word(n: u64) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}
fn unhex(s: &str) -> Vec<u8> {
    hex::decode(s.strip_prefix("0x").unwrap()).unwrap()
}
fn address(s: &str) -> [u8; 32] {
    let mut w = [0; 32];
    w[12..].copy_from_slice(&unhex(s));
    w
}

type Fixture = (InMemoryState, [u8; 32], Vec<u8>);
thread_local! {
    static FIXTURE: Fixture = {
        let account=address("0x1111111111111111111111111111111111111111");
        let source=include_str!("../../sdk/contracts/standards/lep10015_native_account_candidate.lithic");
        let vectors:Json=serde_json::from_str(include_str!("../../packages/standards/lep100-tests/fixtures/proposed-signing-vectors.json")).unwrap();
        let bytes=unhex(&compile(source).unwrap().bytecode);
        let program=parse(&bytes).unwrap();
        let entries=program.functions.iter().map(|f|(function_selector(f),f.name.clone())).collect();
        let mut state=InMemoryState::default();
        state.deploy(account,DeployedContract{code_hash:code_hash(&bytes),bytecode:bytes,storage:Storage::default(),entrypoints:entries});
        state.set_balance(account,word(100));
        let mut signers=word(32).to_vec();signers.extend(word(2));
        for signer in ["0x7e5f4552091a69125d5dfcb7b8c2659029395bdf","0x2b5ad5c4795c026514f8317c7a215e218dccd6cf"] {signers.extend(address(signer));signers.extend(word(1));}
        let mut host=TransactionalHost::new(state);
        let outcome=host.execute_values(CallRequest{contract:account,function:"initialize".into(),arguments:vec![Value::Bytes(signers),Value::Word(ValueType::U64,word(2))],caller:word(9),value:word(0),gas_limit:1000000,block_height:1,block_timestamp:100,chain_id:700780});
        assert!(matches!(outcome,HostOutcome::Success(_)),"{outcome:?}");
        let mut deployed=host.state().contract(&account).unwrap().clone();
        deployed.storage.set_word("next_nonce",word(42));host.state_mut().deploy(account,deployed);
        (host.state().clone(),account,unhex(vectors["vectors"][0]["canonicalTransaction"].as_str().unwrap()))
    };
}

fuzz_target!(|input: &[u8]| {
    if input.len() < 4 || input.len() > 8196 {
        return;
    }
    let gas = u16::from_le_bytes([input[0], input[1]]) as u64 * 4;
    let split = u16::from_le_bytes([input[2], input[3]]) as usize;
    if split > 4096 || split > input.len() - 4 || input.len() - 4 - split > 4096 {
        return;
    }
    FIXTURE.with(|(before, account, expected_transaction)| {
        let transaction = input[4..4 + split].to_vec();
        let signatures = input[4 + split..].to_vec();
        let request = CallRequest {
            contract: *account,
            function: "execute".into(),
            arguments: vec![Value::Bytes(transaction.clone()), Value::Bytes(signatures)],
            caller: word(9),
            value: word(0),
            gas_limit: gas,
            block_height: 2,
            block_timestamp: 100,
            chain_id: 700780,
        };
        let mut host = TransactionalHost::new(before.clone());
        let result = host.execute_values(request.clone());
        let mut replay = TransactionalHost::new(before.clone());
        assert_eq!(replay.execute_values(request), result);
        assert_eq!(replay.state(), host.state());
        match result {
            HostOutcome::Success(result) => {
                // Only the fixed transaction is signed by the fixture's keys.
                assert_eq!(&transaction, expected_transaction);
                assert_eq!(
                    result.result.return_value,
                    Value::Word(ValueType::Bool, word(1))
                );
                assert_eq!(
                    host.state()
                        .contract(account)
                        .unwrap()
                        .storage
                        .get("next_nonce"),
                    Some(&word(43))
                );
                assert_eq!(host.state().balance(account), word(99));
                assert!(result.gas_used <= gas);
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
