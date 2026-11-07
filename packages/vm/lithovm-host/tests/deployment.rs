use lithic_lithovm::compile;
use lithovm_host::{
    code_hash, contract_address, DeployOutcome, DeployRequest, HostFailureKind, InMemoryState,
    Initializer, TransactionalHost,
};

fn address(value: u8) -> [u8; 32] {
    let mut address = [0; 32];
    address[31] = value;
    address
}

fn word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn decimal_word(value: &str) -> [u8; 32] {
    let mut word = [0u8; 32];
    for digit in value.bytes().map(|byte| byte - b'0') {
        let mut carry = digit as u16;
        for byte in word.iter_mut().rev() {
            let next = (*byte as u16) * 10 + carry;
            *byte = next as u8;
            carry = next >> 8;
        }
        assert_eq!(carry, 0);
    }
    word
}

fn compile_bytecode(source: &str) -> Vec<u8> {
    let artifact = compile(source).unwrap();
    hex::decode(&artifact.bytecode[2..]).unwrap()
}

fn request(bytecode: Vec<u8>, deployer: [u8; 32], nonce: u64) -> DeployRequest {
    DeployRequest {
        deployer,
        nonce,
        bytecode,
        initializer: Some(Initializer {
            function: "initialize".into(),
            arguments: vec![deployer],
        }),
        value: word(25),
        gas_limit: 2_000,
        block_height: 42,
        block_timestamp: 1_700_000_000,
        chain_id: 700_777,
    }
}

#[test]
fn deterministic_address_is_domain_separated_and_canonical() {
    let deployer = address(7);
    let hash = code_hash(b"artifact");
    let first = contract_address(deployer, 9, hash, 700_777);
    assert_eq!(first, contract_address(deployer, 9, hash, 700_777));
    assert_ne!(first, contract_address(deployer, 10, hash, 700_777));
    assert_ne!(first, contract_address(deployer, 9, hash, 1));
    assert_eq!(first[..12], [0; 12]);
}

#[test]
fn scalar_host_rejects_dynamic_deployment_even_without_initializer() {
    let bytes =
        compile_bytecode("contract C { pub fn echo(name: string) -> string { return name; } }");
    let mut request = request(bytes, address(7), 0);
    request.initializer = None;
    let mut host = TransactionalHost::new(InMemoryState::default());
    let DeployOutcome::Failure(failure) = host.deploy(request) else {
        panic!("stored unusable dynamic contract")
    };
    assert_eq!(failure.kind, HostFailureKind::InvalidBytecode);
    assert_eq!(failure.gas_used, 0);
    assert!(failure.message.contains("dynamic-value host adapter"));
}

#[test]
fn deployment_preserves_prefunding_and_rejects_balance_overflow() {
    let deployer = address(7);
    let bytes = compile_bytecode(
        "contract C { pub fn initialize(owner: address) -> bool { return true; } }",
    );
    let target = contract_address(deployer, 15, code_hash(&bytes), 700_777);
    let mut state = InMemoryState::default();
    state.set_balance(deployer, word(100));
    state.set_balance(target, word(40));
    let mut host = TransactionalHost::new(state);
    assert!(matches!(
        host.deploy(request(bytes.clone(), deployer, 15)),
        DeployOutcome::Success(_)
    ));
    assert_eq!(host.state().balance(&target), word(65));
    assert_eq!(host.state().balance(&deployer), word(75));

    let overflow_target = contract_address(deployer, 16, code_hash(&bytes), 700_777);
    host.state_mut().set_balance(overflow_target, [255; 32]);
    let before = host.state().clone();
    assert!(matches!(
        host.deploy(request(bytes, deployer, 16)),
        DeployOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
}

#[test]
fn reverting_initializer_preserves_prefunding() {
    let deployer = address(7);
    let bytes =
        compile_bytecode("contract C { pub fn initialize(owner: address) -> bool { revert(); } }");
    let target = contract_address(deployer, 17, code_hash(&bytes), 700_777);
    let mut state = InMemoryState::default();
    state.set_balance(deployer, word(100));
    state.set_balance(target, word(40));
    let before = state.clone();
    let mut host = TransactionalHost::new(state);
    assert!(matches!(
        host.deploy(request(bytes, deployer, 17)),
        DeployOutcome::Failure(_)
    ));
    assert_eq!(host.state(), &before);
}

#[test]
fn lax_deploy_and_initialize_commit_atomically() {
    let deployer = address(7);
    let bytecode = compile_bytecode(include_str!(
        "../../../../sdk/contracts/standards/lax_lep100_v11.lithic"
    ));
    let expected_hash = code_hash(&bytecode);
    let expected_address = contract_address(deployer, 1, expected_hash, 700_777);
    let mut state = InMemoryState::default();
    state.set_balance(deployer, word(100));
    let mut host = TransactionalHost::new(state);

    let DeployOutcome::Success(receipt) = host.deploy(request(bytecode, deployer, 1)) else {
        panic!("LAX deployment fixture should succeed");
    };
    assert_eq!(receipt.contract, expected_address);
    assert_eq!(receipt.code_hash, expected_hash);
    assert!(receipt.initializer_result.is_some());
    assert_eq!(receipt.events.len(), 1);
    assert_eq!(receipt.events[0].contract, expected_address);
    assert_eq!(host.state().balance(&deployer), word(75));
    assert_eq!(host.state().balance(&expected_address), word(25));
    let deployed = host.state().contract(&expected_address).unwrap();
    assert_eq!(deployed.code_hash, expected_hash);
    let transfer_selector: [u8; 32] =
        hex::decode("f61367304e4e32065cad538b10a44bb599f78e0771530108572d225c74122c1f")
            .unwrap()
            .try_into()
            .unwrap();
    assert_eq!(
        deployed
            .entrypoints
            .get(&transfer_selector)
            .map(String::as_str),
        Some("transfer")
    );
    assert_eq!(
        deployed.storage.get("total_supply"),
        Some(&decimal_word("10000000000000000000000000000"))
    );
    assert_eq!(
        deployed.storage.get_map("balances", &[deployer]),
        Some(&decimal_word("10000000000000000000000000000"))
    );
}

#[test]
fn failed_initializer_discards_code_storage_and_value_transfer() {
    let deployer = address(7);
    let bytecode = compile_bytecode(
        "contract Failing { pub fn initialize(owner: address) -> bool { revert(); } }",
    );
    let expected_address = contract_address(deployer, 2, code_hash(&bytecode), 700_777);
    let mut state = InMemoryState::default();
    state.set_balance(deployer, word(100));
    let original = state.clone();
    let mut host = TransactionalHost::new(state);

    let DeployOutcome::Failure(failure) = host.deploy(request(bytecode, deployer, 2)) else {
        panic!("reverting initializer must fail deployment");
    };
    assert_eq!(
        failure.kind,
        HostFailureKind::Vm(lithovm::FailureKind::Revert)
    );
    assert_eq!(failure.failed_contract, expected_address);
    assert!(failure.gas_used > 0);
    assert_eq!(host.state(), &original);
    assert!(host.state().contract(&expected_address).is_none());
}

#[test]
fn invalid_artifact_collision_and_value_fail_closed() {
    let deployer = address(7);
    let mut state = InMemoryState::default();
    state.set_balance(deployer, word(100));
    let original = state.clone();
    let mut host = TransactionalHost::new(state);

    let mut invalid = request(vec![1, 2, 3], deployer, 3);
    invalid.value = [0; 32];
    let DeployOutcome::Failure(failure) = host.deploy(invalid) else {
        panic!("invalid bytecode must fail");
    };
    assert_eq!(failure.kind, HostFailureKind::InvalidBytecode);
    assert_eq!(host.state(), &original);

    let bytecode = compile_bytecode(include_str!(
        "../../../../sdk/contracts/standards/lax_lep100_v11.lithic"
    ));
    let deployed = request(bytecode.clone(), deployer, 5);
    assert!(matches!(
        host.deploy(deployed.clone()),
        DeployOutcome::Success(_)
    ));
    let committed = host.state().clone();
    let DeployOutcome::Failure(failure) = host.deploy(deployed) else {
        panic!("address collision must fail");
    };
    assert_eq!(failure.kind, HostFailureKind::ContractExists);
    assert_eq!(host.state(), &committed);

    let mut unfunded = request(bytecode, address(9), 6);
    unfunded.value = word(1);
    let before_unfunded = host.state().clone();
    let DeployOutcome::Failure(failure) = host.deploy(unfunded) else {
        panic!("unfunded deployment value must fail");
    };
    assert_eq!(failure.kind, HostFailureKind::InsufficientBalance);
    assert_eq!(host.state(), &before_unfunded);
}
