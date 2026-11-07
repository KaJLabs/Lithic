use lithic_lithovm::compile;
use lithovm::{FailureKind, Storage};
use lithovm_host::{
    code_hash, CallRequest, DeployedContract, HostFailureKind, HostOutcome, InMemoryState,
    TransactionalHost,
};
use std::collections::BTreeMap;

fn word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn address(value: u8) -> [u8; 32] {
    let mut address = [0; 32];
    address[31] = value;
    address
}

fn bytecode(source: &str) -> Vec<u8> {
    let artifact = compile(source).unwrap();
    hex::decode(artifact.bytecode.trim_start_matches("0x")).unwrap()
}

fn contract(
    source: &str,
    entrypoints: impl IntoIterator<Item = ([u8; 32], &'static str)>,
) -> DeployedContract {
    let bytecode = bytecode(source);
    DeployedContract {
        code_hash: code_hash(&bytecode),
        bytecode,
        storage: Storage::default(),
        entrypoints: entrypoints
            .into_iter()
            .map(|(selector, name)| (selector, name.to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn request(parent: [u8; 32], child: [u8; 32], selector: [u8; 32]) -> CallRequest {
    CallRequest {
        contract: parent,
        function: "run".into(),
        arguments: vec![address(9), child, selector, word(10)],
        gas_limit: 1_000,
        caller: address(8),
        value: word(0),
        block_height: 42,
        block_timestamp: 1_700_000_000,
        chain_id: 700_777,
    }
}

const PARENT: &str = "contract Parent { state { value: u64; } event Updated { value: u64 } pub fn run(recipient: address, target: address, selector: bytes32, amount: u256) -> u64 { self.value = 7; emit Updated { value: self.value }; transfer_native(recipient, amount); call_contract(target, selector, amount); return self.value; } }";
const CHILD: &str = "contract Child { state { value: u64; } event ChildUpdated { value: u64 } pub fn fail() -> u64 { self.value = 9; emit ChildUpdated { value: self.value }; revert(); } pub fn succeed() -> u64 { self.value = 11; emit ChildUpdated { value: self.value }; return self.value; } }";

#[test]
fn nested_failure_rolls_back_storage_value_transfer_and_events() {
    let parent = address(1);
    let child = address(2);
    let recipient = address(9);
    let fail_selector = [0xf1; 32];
    let mut state = InMemoryState::default();
    state.deploy(parent, contract(PARENT, []));
    state.deploy(child, contract(CHILD, [(fail_selector, "fail")]));
    state.set_balance(parent, word(100));
    state.set_balance(child, word(5));
    let mut host = TransactionalHost::new(state.clone());

    let HostOutcome::Failure(failure) = host.execute(request(parent, child, fail_selector)) else {
        panic!("nested revert should fail the host transaction");
    };
    assert_eq!(failure.kind, HostFailureKind::Vm(FailureKind::Revert));
    assert_eq!(failure.failed_contract, child);
    assert!(failure.gas_used > 0);
    assert_eq!(host.state(), &state);
    assert_eq!(host.state().balance(&recipient), word(0));
    assert_eq!(
        host.state().contract(&parent).unwrap().storage.get("value"),
        None
    );
    assert_eq!(
        host.state().contract(&child).unwrap().storage.get("value"),
        None
    );
}

#[test]
fn recovery_commits_nested_state_balances_and_ordered_events_once() {
    let parent = address(1);
    let child = address(2);
    let recipient = address(9);
    let selector = [0x51; 32];
    let mut state = InMemoryState::default();
    state.deploy(parent, contract(PARENT, []));
    state.deploy(child, contract(CHILD, [(selector, "succeed")]));
    state.set_balance(parent, word(100));
    state.set_balance(child, word(5));
    let mut host = TransactionalHost::new(state);

    let HostOutcome::Success(receipt) = host.execute(request(parent, child, selector)) else {
        panic!("recovery call should commit");
    };
    assert!(receipt.gas_used > receipt.result.gas_used);
    assert_eq!(receipt.events.len(), 2);
    assert_eq!(receipt.events[0].contract, parent);
    assert_eq!(receipt.events[1].contract, child);
    assert_eq!(host.state().balance(&recipient), word(10));
    assert_eq!(host.state().balance(&parent), word(80));
    assert_eq!(host.state().balance(&child), word(15));
    assert_eq!(
        host.state().contract(&parent).unwrap().storage.get("value"),
        Some(&word(7))
    );
    assert_eq!(
        host.state().contract(&child).unwrap().storage.get("value"),
        Some(&word(11))
    );
}

#[test]
fn native_transfer_to_contract_uses_the_shared_balance_namespace() {
    let parent = address(1);
    let child = address(2);
    let selector = [0x52; 32];
    let mut state = InMemoryState::default();
    state.deploy(parent, contract(PARENT, []));
    state.deploy(child, contract(CHILD, [(selector, "succeed")]));
    state.set_balance(parent, word(100));
    state.set_balance(child, word(5));
    let mut host = TransactionalHost::new(state);
    let mut call = request(parent, child, selector);
    call.arguments[0] = child;

    assert!(matches!(host.execute(call), HostOutcome::Success(_)));
    assert_eq!(host.state().balance(&parent), word(80));
    assert_eq!(host.state().balance(&child), word(25));
}

#[test]
fn unknown_selector_and_reentrancy_fail_without_committing() {
    let parent = address(1);
    let child = address(2);
    let unknown = [0xaa; 32];
    let mut state = InMemoryState::default();
    state.deploy(parent, contract(PARENT, []));
    state.deploy(child, contract(CHILD, []));
    state.set_balance(parent, word(100));
    state.set_balance(child, word(5));
    let original = state.clone();
    let mut host = TransactionalHost::new(state);
    let HostOutcome::Failure(failure) = host.execute(request(parent, child, unknown)) else {
        panic!("unknown selector should fail");
    };
    assert_eq!(failure.kind, HostFailureKind::UnknownSelector);
    assert_eq!(host.state(), &original);

    let recurse_selector = [0xbb; 32];
    host.state_mut()
        .deploy(parent, contract(PARENT, [(recurse_selector, "run")]));
    let before_reentrancy = host.state().clone();
    let HostOutcome::Failure(failure) = host.execute(request(parent, parent, recurse_selector))
    else {
        panic!("reentrancy should be rejected");
    };
    assert_eq!(failure.kind, HostFailureKind::Reentrancy);
    assert_eq!(host.state(), &before_reentrancy);
}

#[test]
fn corrupted_persisted_code_hash_fails_closed() {
    let parent = address(1);
    let child = address(2);
    let selector = [0x53; 32];
    let mut state = InMemoryState::default();
    let mut corrupted = contract(PARENT, []);
    corrupted.code_hash = [0xff; 32];
    state.deploy(parent, corrupted);
    state.deploy(child, contract(CHILD, [(selector, "succeed")]));
    state.set_balance(parent, word(100));
    let original = state.clone();
    let mut host = TransactionalHost::new(state);

    let HostOutcome::Failure(failure) = host.execute(request(parent, child, selector)) else {
        panic!("corrupted code must fail");
    };
    assert_eq!(failure.kind, HostFailureKind::State);
    assert!(failure.message.contains("code hash"));
    assert_eq!(host.state(), &original);
}
