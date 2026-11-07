use lithic_lithovm::compile;
use lithovm::{ExecutionContext, ExecutionOutcome, Storage, Vm};

fn word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}
fn bytes() -> Vec<u8> {
    let artifact = compile(include_str!(
        "../../../../sdk/contracts/standards/finance_token_v11.lithic"
    ))
    .unwrap();
    hex::decode(&artifact.bytecode[2..]).unwrap()
}
fn call(bytes: &[u8], storage: &mut Storage, caller: u64, name: &str, args: &[[u8; 32]]) -> bool {
    let before = storage.clone();
    let outcome = Vm::default().execute_transactionally(
        bytes,
        name,
        args,
        10000,
        storage,
        &ExecutionContext {
            caller: word(caller),
            ..Default::default()
        },
    );
    match outcome {
        ExecutionOutcome::Success(_) => true,
        ExecutionOutcome::Failure(_) => {
            assert_eq!(*storage, before, "failed {name} must roll back");
            false
        }
    }
}

#[test]
fn all_sixteen_feature_combinations_obey_authorization_and_rollback() {
    let bytes = bytes();
    for flags in 0..16 {
        let mint = flags & 1 != 0;
        let burn = flags & 2 != 0;
        let pause = flags & 4 != 0;
        let owner = flags & 8 != 0;
        let mut storage = Storage::default();
        assert!(call(
            &bytes,
            &mut storage,
            9,
            "initialize",
            &[
                word(1),
                word(6),
                word(1000),
                word(mint as u64),
                word(burn as u64),
                word(pause as u64),
                word(owner as u64)
            ]
        ));
        assert_eq!(storage.get("decimals"), Some(&word(6)));
        assert_eq!(storage.get("owner"), Some(&word(if owner { 1 } else { 0 })));
        assert_eq!(storage.get_map("balances", &[word(1)]), Some(&word(1000)));
        assert!(storage.get_map("balances", &[word(9)]).is_none());
        assert!(!call(&bytes, &mut storage, 2, "mint", &[word(2), word(1)]));
        assert_eq!(
            call(&bytes, &mut storage, 1, "mint", &[word(2), word(10)]),
            mint && owner
        );
        assert_eq!(call(&bytes, &mut storage, 1, "burn", &[word(10)]), burn);
        assert_eq!(call(&bytes, &mut storage, 1, "pause", &[]), pause && owner);
        if pause && owner {
            assert!(!call(
                &bytes,
                &mut storage,
                1,
                "transfer",
                &[word(2), word(1)]
            ));
            assert!(!call(&bytes, &mut storage, 1, "mint", &[word(2), word(1)]));
            assert!(!call(&bytes, &mut storage, 1, "burn", &[word(1)]));
            assert!(call(&bytes, &mut storage, 1, "unpause", &[]));
        }
        assert!(call(
            &bytes,
            &mut storage,
            1,
            "transfer",
            &[word(2), word(1)]
        ));
        assert!(!call(
            &bytes,
            &mut storage,
            1,
            "transfer",
            &[word(0), word(1)]
        ));
        assert!(!call(&bytes, &mut storage, 0, "mint", &[word(2), word(1)]));
    }
}

#[test]
fn initialization_and_owner_changes_preserve_creator_identity() {
    let bytes = bytes();
    let mut storage = Storage::default();
    let mut args = [
        word(1),
        word(256),
        word(1000),
        word(1),
        word(1),
        word(1),
        word(1),
    ];
    assert!(!call(&bytes, &mut storage, 9, "initialize", &args));
    args[1] = word(18);
    assert!(call(&bytes, &mut storage, 9, "initialize", &args));
    assert!(!call(&bytes, &mut storage, 9, "initialize", &args));
    assert!(!call(
        &bytes,
        &mut storage,
        9,
        "transfer_ownership",
        &[word(2)]
    ));
    assert!(call(
        &bytes,
        &mut storage,
        1,
        "transfer_ownership",
        &[word(2)]
    ));
    assert!(!call(&bytes, &mut storage, 1, "pause", &[]));
    assert!(call(&bytes, &mut storage, 2, "renounce_ownership", &[]));
    assert!(!call(&bytes, &mut storage, 2, "mint", &[word(2), word(1)]));
}

#[test]
fn delegated_transfer_and_burn_handle_finite_and_unlimited_allowances() {
    let bytes = bytes();
    let mut storage = Storage::default();
    assert!(call(
        &bytes,
        &mut storage,
        9,
        "initialize",
        &[
            word(1),
            word(18),
            word(1000),
            word(1),
            word(1),
            word(1),
            word(1)
        ]
    ));
    assert!(call(
        &bytes,
        &mut storage,
        1,
        "approve",
        &[word(2), word(100)]
    ));
    assert!(call(
        &bytes,
        &mut storage,
        2,
        "transfer_from",
        &[word(1), word(3), word(40)]
    ));
    assert_eq!(
        storage.get_map("allowances", &[word(1), word(2)]),
        Some(&word(60))
    );
    assert!(call(
        &bytes,
        &mut storage,
        2,
        "burn_from",
        &[word(1), word(30)]
    ));
    assert_eq!(storage.get("total_supply"), Some(&word(970)));
    assert!(!call(
        &bytes,
        &mut storage,
        2,
        "burn_from",
        &[word(1), word(31)]
    ));
    assert!(call(
        &bytes,
        &mut storage,
        1,
        "approve",
        &[word(2), [255; 32]]
    ));
    assert!(call(
        &bytes,
        &mut storage,
        2,
        "burn_from",
        &[word(1), word(10)]
    ));
    assert_eq!(
        storage.get_map("allowances", &[word(1), word(2)]),
        Some(&[255; 32])
    );
    assert!(!call(
        &bytes,
        &mut storage,
        2,
        "transfer_from",
        &[word(1), word(3), word(10000)]
    ));
    assert!(call(&bytes, &mut storage, 1, "pause", &[]));
    assert!(!call(
        &bytes,
        &mut storage,
        2,
        "burn_from",
        &[word(1), word(1)]
    ));
    assert!(call(
        &bytes,
        &mut storage,
        1,
        "approve",
        &[word(2), word(0)]
    ));
}
