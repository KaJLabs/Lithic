#![no_main]

use libfuzzer_sys::fuzz_target;
use lithic_lithovm::compile;
use lithovm::{ExecutionContext, ExecutionOutcome, FailureKind, Storage, Vm};
use std::collections::BTreeMap;

const SOURCE: &str = "contract Ledger { state { balances: map<address, u256>; } pub fn credit(account: address, amount: u256) -> u256 { self.balances[account] = self.balances[account] + amount; return self.balances[account]; } pub fn debit(account: address, amount: u256) -> u256 { let balance: u256 = self.balances[account]; require(balance >= amount); self.balances[account] = balance - amount; return self.balances[account]; } }";
const MAX_OPERATIONS: usize = 64;

fn address(value: u8) -> [u8; 32] {
    let mut word = [0; 32];
    word[31] = value;
    word
}

fn word(value: u128) -> [u8; 32] {
    let mut word = [0; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fuzz_target!(|data: &[u8]| {
    let artifact = compile(SOURCE).expect("stateful fuzz fixture must compile");
    let bytecode = hex::decode(&artifact.bytecode[2..]).expect("compiler bytecode must be hex");
    let vm = Vm::default();
    let context = ExecutionContext::default();
    let mut storage = Storage::default();
    let mut model = BTreeMap::<u8, u128>::new();

    for operation in data.chunks_exact(10).take(MAX_OPERATIONS) {
        let credit = operation[0] & 1 == 0;
        let account = operation[1];
        let amount = u64::from_be_bytes(operation[2..10].try_into().unwrap()) as u128;
        let current = model.get(&account).copied().unwrap_or(0);
        let outcome = vm.execute_transactionally(
            &bytecode,
            if credit { "credit" } else { "debit" },
            &[address(account), word(amount)],
            10_000,
            &mut storage,
            &context,
        );

        if credit {
            let expected = current + amount;
            model.insert(account, expected);
            let ExecutionOutcome::Success(result) = outcome else {
                panic!("bounded credit unexpectedly failed");
            };
            assert_eq!(result.return_value, word(expected));
        } else if amount <= current {
            let expected = current - amount;
            model.insert(account, expected);
            let ExecutionOutcome::Success(result) = outcome else {
                panic!("funded debit unexpectedly failed");
            };
            assert_eq!(result.return_value, word(expected));
        } else {
            assert!(matches!(
                outcome,
                ExecutionOutcome::Failure(ref failure) if failure.kind == FailureKind::Revert
            ));
        }

        for (account, expected) in &model {
            assert_eq!(
                storage.get_map("balances", &[address(*account)]),
                Some(&word(*expected))
            );
        }
    }
});
