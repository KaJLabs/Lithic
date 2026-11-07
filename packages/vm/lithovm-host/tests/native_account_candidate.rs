//! Local engineering coverage of an unapproved native account.
//! These tests are not standards certification or chain/deployment evidence.
use lithic_lithovm::compile;
use lithovm::Storage;
use lithovm_bytecode::values;
use lithovm_bytecode::{function_selector, parse, values::Value, ValueType};
use lithovm_host::{
    code_hash, CallRequest, DeployOutcome, DeployRequest, DeployedContract, HostOutcome,
    InMemoryState, Initializer, TransactionalHost,
};
use serde_json::Value as Json;
use std::collections::BTreeMap;

const SOURCE: &str =
    include_str!("../../../../sdk/contracts/standards/lep10015_native_account_candidate.lithic");
const VECTORS: &str =
    include_str!("../../../standards/lep100-tests/fixtures/proposed-signing-vectors.json");
fn word(n: u64) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}
fn hex_data(s: &str) -> Vec<u8> {
    hex::decode(s.strip_prefix("0x").unwrap()).unwrap()
}
fn address(s: &str) -> [u8; 32] {
    let mut w = [0; 32];
    w[12..].copy_from_slice(&hex_data(s));
    w
}
fn account() -> [u8; 32] {
    address("0x1111111111111111111111111111111111111111")
}
fn signer(index: usize) -> [u8; 32] {
    address(if index == 0 {
        "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf"
    } else {
        "0x2b5ad5c4795c026514f8317c7a215e218dccd6cf"
    })
}
fn fixtures() -> Json {
    serde_json::from_str(VECTORS).unwrap()
}
fn binary(data: Vec<u8>) -> Value {
    Value::Bytes(data)
}
fn bytes32(data: &str) -> Value {
    Value::Word(ValueType::Bytes32, hex_data(data).try_into().unwrap())
}
fn signer_list(signers: &[[u8; 32]]) -> Vec<u8> {
    descriptors(&signers.iter().map(|s| (*s, 1)).collect::<Vec<_>>())
}
fn descriptors(signers: &[([u8; 32], u64)]) -> Vec<u8> {
    let mut b = word(32).to_vec();
    b.extend(word(signers.len() as u64));
    for (s, scheme) in signers {
        b.extend(s);
        b.extend(word(*scheme));
    }
    b
}

thread_local! {static CODE: Vec<u8> = {
    let code=hex_data(&compile(SOURCE).unwrap().bytecode);
    assert!(code.len() <= 65536,"candidate exceeds the existing code-admission boundary");
    code
};}

fn request(function: &str, args: Vec<Value>, caller: [u8; 32], now: u64) -> CallRequest<Value> {
    CallRequest {
        contract: account(),
        function: function.into(),
        arguments: args,
        caller,
        value: word(0),
        gas_limit: 1_000_000,
        block_height: 2,
        block_timestamp: now,
        chain_id: 700780,
    }
}
fn success(
    host: &mut TransactionalHost<InMemoryState>,
    call: CallRequest<Value>,
) -> lithovm_host::HostSuccess<Value> {
    let out = host.execute_values(call);
    let HostOutcome::Success(r) = out else {
        panic!("{out:?}")
    };
    r
}
fn unchanged_failure(host: &mut TransactionalHost<InMemoryState>, call: CallRequest<Value>) {
    let before = host.state().clone();
    let result = host.execute_values(call);
    assert!(matches!(result, HostOutcome::Failure(_)), "{result:?}");
    assert_eq!(host.state(), &before);
}
fn fixture(nonce: u64, balance: u64) -> TransactionalHost<InMemoryState> {
    fixture_with_signers(nonce, balance, &[(signer(0), 1), (signer(1), 1)], 2)
}
fn fixture_with_signers(
    nonce: u64,
    balance: u64,
    signers: &[([u8; 32], u64)],
    threshold: u64,
) -> TransactionalHost<InMemoryState> {
    let mut state = InMemoryState::default();
    CODE.with(|code| {
        let entries = parse(code)
            .unwrap()
            .functions
            .iter()
            .map(|f| (function_selector(f), f.name.clone()))
            .collect::<BTreeMap<_, _>>();
        state.deploy(
            account(),
            DeployedContract {
                bytecode: code.clone(),
                code_hash: code_hash(code),
                storage: Storage::default(),
                entrypoints: entries,
            },
        );
    });
    state.set_balance(account(), word(balance));
    let mut host = TransactionalHost::new(state);
    success(
        &mut host,
        request(
            "initialize",
            vec![
                binary(descriptors(signers)),
                Value::Word(ValueType::U64, word(threshold)),
            ],
            signer(0),
            100,
        ),
    );
    // A synthetic fixture starts at each independently published vector's nonce.
    let mut deployed = host.state().contract(&account()).unwrap().clone();
    deployed.storage.set_word("next_nonce", word(nonce));
    host.state_mut().deploy(account(), deployed);
    host
}
fn nonce(host: &mut TransactionalHost<InMemoryState>) -> Value {
    success(host, request("nonce", vec![], signer(0), 100))
        .result
        .return_value
}

fn transaction(nonce: u64, calls: &[([u8; 32], u64)], after: u64, until: u64) -> Vec<u8> {
    transaction_calls(
        nonce,
        &calls
            .iter()
            .map(|(a, n)| (*a, *n, vec![]))
            .collect::<Vec<_>>(),
        after,
        until,
    )
}
fn transaction_calls(
    nonce: u64,
    calls: &[([u8; 32], u64, Vec<u8>)],
    after: u64,
    until: u64,
) -> Vec<u8> {
    // Independent canonical Solidity ABI construction, checked against fixed ethers vectors.
    let f = fixtures();
    let policy = hex_data(
        f["vectors"][0]["transaction"]["policyHash"]
            .as_str()
            .unwrap(),
    );
    let mut b = word(32).to_vec();
    for w in [
        account(),
        word(700780),
        word(nonce),
        word(256),
        word(after),
        word(until),
    ] {
        b.extend(w);
    }
    b.extend(policy);
    b.extend(word(0));
    b.extend(word(calls.len() as u64));
    let mut offset = calls.len() * 32;
    for (_, _, data) in calls {
        b.extend(word(offset as u64));
        offset += 160 + data.len().div_ceil(32) * 32;
    }
    for (target, value, data) in calls {
        for w in [
            *target,
            word(*value),
            word(128),
            word(0),
            word(data.len() as u64),
        ] {
            b.extend(w);
        }
        b.extend(data);
        b.extend(vec![0; data.len().div_ceil(32) * 32 - data.len()]);
    }
    b
}
fn empty_signatures() -> Value {
    binary([word(32), word(0)].concat())
}
fn signatures(items: &[Json]) -> Value {
    signature_bundle(
        &items
            .iter()
            .map(|item| {
                (
                    hex_data(item["signerId"].as_str().unwrap())
                        .try_into()
                        .unwrap(),
                    item["scheme"].as_u64().unwrap(),
                    hex_data(item["signature"].as_str().unwrap()),
                )
            })
            .collect::<Vec<_>>(),
    )
}
fn signature_bundle(items: &[([u8; 32], u64, Vec<u8>)]) -> Value {
    let mut bytes = word(32).to_vec();
    bytes.extend(word(items.len() as u64));
    let mut offset = items.len() * 32;
    for (_, _, data) in items {
        bytes.extend(word(offset as u64));
        offset += 128 + data.len().div_ceil(32) * 32;
    }
    for (id, scheme, data) in items {
        bytes.extend(id);
        bytes.extend(word(*scheme));
        bytes.extend(word(96));
        bytes.extend(word(data.len() as u64));
        bytes.extend(data);
        bytes.extend(vec![0; data.len().div_ceil(32) * 32 - data.len()]);
    }
    binary(bytes)
}
fn approve_both(
    host: &mut TransactionalHost<InMemoryState>,
    transaction: Vec<u8>,
    now: u64,
) -> Value {
    let txid = success(
        host,
        request("propose", vec![binary(transaction)], signer(0), now),
    )
    .result
    .return_value;
    for s in [signer(0), signer(1)] {
        success(host, request("approve", vec![txid.clone()], s, now));
    }
    txid
}

fn current_policy(host: &mut TransactionalHost<InMemoryState>, mut tx: Vec<u8>) -> Vec<u8> {
    let policy = success(host, request("policy_hash", vec![], signer(0), 100))
        .result
        .return_value;
    let Value::Word(ValueType::Bytes32, policy) = policy else {
        panic!("wrong policy type")
    };
    tx[224..256].copy_from_slice(&policy);
    tx
}
fn native_payload(
    host: &TransactionalHost<InMemoryState>,
    target: [u8; 32],
    name: &str,
    args: &[Value],
) -> Vec<u8> {
    let function = parse(&host.state().contract(&target).unwrap().bytecode)
        .unwrap()
        .functions
        .into_iter()
        .find(|f| f.name == name)
        .unwrap();
    let mut data = function_selector(&function).to_vec();
    data.extend(values::encode(args).unwrap());
    data
}
fn admin_transaction(
    host: &mut TransactionalHost<InMemoryState>,
    n: u64,
    name: &str,
    args: &[Value],
) -> Vec<u8> {
    let data = native_payload(host, account(), name, args);
    current_policy(host, transaction_calls(n, &[(account(), 0, data)], 0, 0))
}
fn execute_approved(host: &mut TransactionalHost<InMemoryState>, tx: Vec<u8>) {
    approve_both(host, tx.clone(), 100);
    success(
        host,
        request(
            "execute",
            vec![binary(tx), empty_signatures()],
            word(9),
            100,
        ),
    );
}
fn install(host: &mut TransactionalHost<InMemoryState>, at: [u8; 32], source: &str) {
    let code = hex_data(&compile(source).unwrap().bytecode);
    let entries = parse(&code)
        .unwrap()
        .functions
        .iter()
        .map(|f| (function_selector(f), f.name.clone()))
        .collect();
    host.state_mut().deploy(
        at,
        DeployedContract {
            code_hash: code_hash(&code),
            bytecode: code,
            storage: Storage::default(),
            entrypoints: entries,
        },
    );
}

#[test]
fn compiled_account_hashes_and_signer_ids_match_both_fixed_proposed_vectors() {
    for f in fixtures()["vectors"].as_array().unwrap() {
        let n = f["transaction"]["nonce"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let mut host = fixture(n, 100);
        let out = success(
            &mut host,
            request(
                "hash_transaction",
                vec![binary(hex_data(
                    f["canonicalTransaction"].as_str().unwrap(),
                ))],
                signer(0),
                100,
            ),
        );
        assert_eq!(
            out.result.return_value,
            bytes32(f["digest"].as_str().unwrap())
        );
        for signature in f["signatures"].as_array().unwrap() {
            let got = success(
                &mut host,
                request(
                    "signer_id",
                    vec![Value::Word(
                        ValueType::Address,
                        address(signature["address"].as_str().unwrap()),
                    )],
                    signer(0),
                    100,
                ),
            );
            assert_eq!(
                got.result.return_value,
                bytes32(signature["signerId"].as_str().unwrap())
            );
        }
    }
}

#[test]
fn direct_signed_execution_requires_threshold_and_cannot_replay() {
    let f = fixtures();
    let v = &f["vectors"][0];
    let tx = hex_data(v["canonicalTransaction"].as_str().unwrap());
    assert_eq!(
        transaction(
            42,
            &[(address("0x2222222222222222222222222222222222222222"), 1)],
            0,
            0
        ),
        tx
    );
    let mut host = fixture(42, 100);
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![
                binary(tx.clone()),
                signatures(&v["signatures"].as_array().unwrap()[..1]),
            ],
            word(9),
            100,
        ),
    );
    let out = success(
        &mut host,
        request(
            "execute",
            vec![
                binary(tx.clone()),
                binary(hex_data(v["canonicalSignatures"].as_str().unwrap())),
            ],
            word(9),
            100,
        ),
    );
    assert_eq!(
        out.result.return_value,
        Value::Word(ValueType::Bool, word(1))
    );
    assert_eq!(nonce(&mut host), Value::Word(ValueType::U256, word(43)));
    assert_eq!(host.state().balance(&account()), word(99));
    assert_eq!(
        host.state()
            .balance(&address("0x2222222222222222222222222222222222222222")),
        word(1)
    );
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![
                binary(tx),
                binary(hex_data(v["canonicalSignatures"].as_str().unwrap())),
            ],
            word(9),
            100,
        ),
    );
}

#[test]
fn proposals_execute_out_of_order_without_consuming_nonce_twice() {
    let mut host = fixture(0, 100);
    let a = transaction(0, &[(word(3), 5)], 0, 0);
    let b = transaction(1, &[(word(4), 7)], 0, 0);
    approve_both(&mut host, a.clone(), 100);
    approve_both(&mut host, b.clone(), 100);
    assert_eq!(nonce(&mut host), Value::Word(ValueType::U256, word(2)));
    for tx in [b, a] {
        success(
            &mut host,
            request(
                "execute",
                vec![binary(tx), empty_signatures()],
                word(9),
                100,
            ),
        );
    }
    assert_eq!(nonce(&mut host), Value::Word(ValueType::U256, word(2)));
    assert_eq!(host.state().balance(&word(3)), word(5));
    assert_eq!(host.state().balance(&word(4)), word(7));
}

#[test]
fn revoked_signature_stays_revoked_after_explicit_onchain_reapproval() {
    let f = fixtures();
    let v = &f["vectors"][0];
    let tx = hex_data(v["canonicalTransaction"].as_str().unwrap());
    let mut host = fixture(42, 100);
    let id = success(
        &mut host,
        request("propose", vec![binary(tx.clone())], signer(0), 100),
    )
    .result
    .return_value;
    for name in ["approve", "revoke", "approve"] {
        success(&mut host, request(name, vec![id.clone()], signer(0), 100));
    }
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![
                binary(tx.clone()),
                binary(hex_data(v["canonicalSignatures"].as_str().unwrap())),
            ],
            word(9),
            100,
        ),
    );
    // Signer 1 contributes its explicit on-chain reapproval; the second key signs.
    let key2 = &v["signatures"].as_array().unwrap()[..1];
    success(
        &mut host,
        request("execute", vec![binary(tx), signatures(key2)], word(9), 100),
    );
}

#[test]
fn second_transfer_failure_and_oog_roll_back_entire_batch_and_direct_nonce() {
    let mut host = fixture(0, 10);
    let tx = transaction(0, &[(word(3), 7), (word(4), 7)], 0, 0);
    approve_both(&mut host, tx.clone(), 100);
    let reserved = nonce(&mut host);
    let before = host.state().clone();
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(tx.clone()), empty_signatures()],
            word(9),
            100,
        ),
    );
    assert_eq!(nonce(&mut host), reserved);
    assert_eq!(host.state().balance(&word(3)), word(0));
    host.state_mut().set_balance(account(), word(100));
    let success_start = host.state().clone();
    let call = request(
        "execute",
        vec![binary(tx), empty_signatures()],
        word(9),
        100,
    );
    let out = success(&mut host, call.clone());
    for gas in [0, 100, out.gas_used / 2, out.gas_used - 1] {
        let mut limited = TransactionalHost::new(success_start.clone());
        let mut low = call.clone();
        low.gas_limit = gas;
        unchanged_failure(&mut limited, low);
    }
    assert_eq!(before.balance(&account()), word(10));
    let f = fixtures();
    let v = &f["vectors"][0];
    let mut direct = fixture(42, 0);
    unchanged_failure(
        &mut direct,
        request(
            "execute",
            vec![
                binary(hex_data(v["canonicalTransaction"].as_str().unwrap())),
                binary(hex_data(v["canonicalSignatures"].as_str().unwrap())),
            ],
            word(9),
            100,
        ),
    );
    assert_eq!(nonce(&mut direct), Value::Word(ValueType::U256, word(42)));
}

#[test]
fn timelocks_expiry_and_duplicate_signers_fail_closed() {
    for now in [99, 100, 200, 201] {
        let mut host = fixture(0, 100);
        let tx = transaction(0, &[(word(3), 1)], 100, 200);
        approve_both(&mut host, tx.clone(), 99);
        let call = request(
            "execute",
            vec![binary(tx), empty_signatures()],
            word(9),
            now,
        );
        if now == 100 || now == 200 {
            success(&mut host, call);
        } else {
            unchanged_failure(&mut host, call);
        }
    }
    let f = fixtures();
    let v = &f["vectors"][0];
    let mut host = fixture(42, 100);
    for entries in [
        vec![v["signatures"][0].clone(), v["signatures"][0].clone()],
        vec![v["signatures"][1].clone(), v["signatures"][0].clone()],
    ] {
        unchanged_failure(
            &mut host,
            request(
                "execute",
                vec![
                    binary(hex_data(v["canonicalTransaction"].as_str().unwrap())),
                    signatures(&entries),
                ],
                word(9),
                100,
            ),
        );
    }
    unchanged_failure(
        &mut host,
        request(
            "approve",
            vec![Value::Word(ValueType::Bytes32, word(1))],
            word(9),
            100,
        ),
    );
}

#[test]
fn canonical_transaction_and_signature_boundaries_reject_without_effects() {
    let f = fixtures();
    let v = &f["vectors"][0];
    let tx = hex_data(v["canonicalTransaction"].as_str().unwrap());
    let sig = hex_data(v["canonicalSignatures"].as_str().unwrap());
    let mut host = fixture(42, 100);
    for offset in [31, 32, 95, 127, 159, 255, 287, 319, 351, 352, 447, 479, 511] {
        let mut bad = tx.clone();
        bad[offset] ^= 1;
        unchanged_failure(
            &mut host,
            request(
                "execute",
                vec![binary(bad), binary(sig.clone())],
                word(9),
                100,
            ),
        );
    }
    for offset in [31, 63, 95, 127, 191, 223, 255, 351, 352, 575] {
        let mut bad = sig.clone();
        bad[offset] ^= 1;
        unchanged_failure(
            &mut host,
            request(
                "execute",
                vec![binary(tx.clone()), binary(bad)],
                word(9),
                100,
            ),
        );
    }
    for (a, b) in [
        (tx[..tx.len() - 1].to_vec(), sig.clone()),
        ([tx.clone(), vec![0]].concat(), sig.clone()),
        (tx.clone(), [sig.clone(), vec![0]].concat()),
    ] {
        unchanged_failure(
            &mut host,
            request("execute", vec![binary(a), binary(b)], word(9), 100),
        );
    }
    // The second digest vector's opaque bytes are not native selector/LVAL calldata.
    let v = &f["vectors"][1];
    let mut second = fixture(43, 100);
    unchanged_failure(
        &mut second,
        request(
            "execute",
            vec![
                binary(hex_data(v["canonicalTransaction"].as_str().unwrap())),
                binary(hex_data(v["canonicalSignatures"].as_str().unwrap())),
            ],
            word(9),
            100,
        ),
    );
}

#[test]
fn threshold_changes_require_self_authorization_and_invalidate_old_proposals() {
    let mut host = fixture(0, 100);
    unchanged_failure(
        &mut host,
        request(
            "set_threshold",
            vec![Value::Word(ValueType::U64, word(1))],
            signer(0),
            100,
        ),
    );
    let old = transaction(0, &[(word(3), 1)], 0, 0);
    approve_both(&mut host, old.clone(), 100);
    let change = admin_transaction(
        &mut host,
        1,
        "set_threshold",
        &[Value::Word(ValueType::U64, word(1))],
    );
    execute_approved(&mut host, change);
    assert_eq!(
        success(&mut host, request("threshold", vec![], word(9), 100))
            .result
            .return_value,
        Value::Word(ValueType::U64, word(1))
    );
    assert_eq!(
        success(&mut host, request("epoch", vec![], word(9), 100))
            .result
            .return_value,
        Value::Word(ValueType::U256, word(2))
    );
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(old), empty_signatures()],
            word(9),
            100,
        ),
    );
    let fresh = current_policy(&mut host, transaction(2, &[(word(4), 1)], 0, 0));
    let id = success(
        &mut host,
        request("propose", vec![binary(fresh.clone())], signer(0), 100),
    )
    .result
    .return_value;
    success(&mut host, request("approve", vec![id], signer(0), 100));
    success(
        &mut host,
        request(
            "execute",
            vec![binary(fresh), empty_signatures()],
            word(9),
            100,
        ),
    );
    let invalid = admin_transaction(
        &mut host,
        3,
        "set_threshold",
        &[Value::Word(ValueType::U64, word(0))],
    );
    let id = success(
        &mut host,
        request("propose", vec![binary(invalid.clone())], signer(0), 100),
    )
    .result
    .return_value;
    success(&mut host, request("approve", vec![id], signer(0), 100));
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(invalid), empty_signatures()],
            word(9),
            100,
        ),
    );
}

#[test]
fn signer_rotation_preserves_threshold_unique_credentials_and_nonreused_ids() {
    let mut host = fixture(0, 100);
    let removal = admin_transaction(
        &mut host,
        0,
        "remove_signer",
        &[bytes32(
            fixtures()["vectors"][0]["signatures"][1]["signerId"]
                .as_str()
                .unwrap(),
        )],
    );
    approve_both(&mut host, removal.clone(), 100);
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(removal), empty_signatures()],
            word(9),
            100,
        ),
    );
    let add = admin_transaction(
        &mut host,
        1,
        "add_signer",
        &[
            Value::Word(ValueType::Address, word(3)),
            Value::Word(ValueType::U64, word(1)),
        ],
    );
    execute_approved(&mut host, add);
    let first = success(
        &mut host,
        request(
            "signer_id",
            vec![Value::Word(ValueType::Address, word(3))],
            word(9),
            100,
        ),
    )
    .result
    .return_value;
    let removal = admin_transaction(&mut host, 2, "remove_signer", std::slice::from_ref(&first));
    execute_approved(&mut host, removal);
    let add = admin_transaction(
        &mut host,
        3,
        "add_signer",
        &[
            Value::Word(ValueType::Address, word(3)),
            Value::Word(ValueType::U64, word(1)),
        ],
    );
    execute_approved(&mut host, add);
    let second = success(
        &mut host,
        request(
            "signer_id",
            vec![Value::Word(ValueType::Address, word(3))],
            word(9),
            100,
        ),
    )
    .result
    .return_value;
    assert_ne!(first, second);
    let replace = admin_transaction(
        &mut host,
        4,
        "replace_signer",
        &[
            second,
            Value::Word(ValueType::Address, word(4)),
            Value::Word(ValueType::U64, word(2)),
        ],
    );
    execute_approved(&mut host, replace);
    assert_eq!(
        success(
            &mut host,
            request(
                "signer_id",
                vec![Value::Word(ValueType::Address, word(3))],
                word(9),
                100
            )
        )
        .result
        .return_value,
        Value::Word(ValueType::Bytes32, word(0))
    );
    assert_eq!(
        success(&mut host, request("signer_count", vec![], word(9), 100))
            .result
            .return_value,
        Value::Word(ValueType::U64, word(3))
    );
    let duplicate = admin_transaction(
        &mut host,
        5,
        "add_signer",
        &[
            Value::Word(ValueType::Address, signer(0)),
            Value::Word(ValueType::U64, word(1)),
        ],
    );
    approve_both(&mut host, duplicate.clone(), 100);
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(duplicate), empty_signatures()],
            word(9),
            100,
        ),
    );
}

#[test]
fn cancellation_requires_account_threshold_and_never_releases_reserved_nonce() {
    let mut host = fixture(0, 100);
    let victim = transaction(0, &[(word(3), 1)], 0, 0);
    let id = approve_both(&mut host, victim.clone(), 100);
    unchanged_failure(
        &mut host,
        request("cancel", vec![id.clone()], signer(0), 100),
    );
    let cancel = admin_transaction(&mut host, 1, "cancel", std::slice::from_ref(&id));
    execute_approved(&mut host, cancel);
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(victim.clone()), empty_signatures()],
            word(9),
            100,
        ),
    );
    unchanged_failure(&mut host, request("approve", vec![id], signer(1), 100));
    unchanged_failure(
        &mut host,
        request("propose", vec![binary(victim)], signer(0), 100),
    );
    assert_eq!(nonce(&mut host), Value::Word(ValueType::U256, word(2)));
}

#[test]
fn native_contract_calls_are_atomic_and_failed_proposals_can_recover() {
    let mut host = fixture(0, 100);
    install(&mut host,word(7),"contract A { state { n: u64; } event Changed { n: u64 } pub fn run(n: u64) -> bool { self.n = n; emit Changed { n: n }; return true; } }");
    install(&mut host,word(8),"contract B { state { n: u64; ready: bool; } pub fn enable() -> bool { self.ready = true; return true; } pub fn run(n: u64) -> bool { self.n = n; require(self.ready); return true; } }");
    let data_a = native_payload(
        &host,
        word(7),
        "run",
        &[Value::Word(ValueType::U64, word(42))],
    );
    let data_b = native_payload(
        &host,
        word(8),
        "run",
        &[Value::Word(ValueType::U64, word(43))],
    );
    let tx = transaction_calls(0, &[(word(7), 5, data_a), (word(8), 3, data_b)], 0, 0);
    approve_both(&mut host, tx.clone(), 100);
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(tx.clone()), empty_signatures()],
            word(9),
            100,
        ),
    );
    let mut enable = request("enable", vec![], word(9), 100);
    enable.contract = word(8);
    success(&mut host, enable);
    let result = success(
        &mut host,
        request(
            "execute",
            vec![binary(tx), empty_signatures()],
            word(9),
            100,
        ),
    );
    assert_eq!(
        host.state().contract(&word(7)).unwrap().storage.get("n"),
        Some(&word(42))
    );
    assert_eq!(
        host.state().contract(&word(8)).unwrap().storage.get("n"),
        Some(&word(43))
    );
    assert_eq!(host.state().balance(&word(7)), word(5));
    assert_eq!(host.state().balance(&word(8)), word(3));
    assert_eq!(
        result
            .events
            .iter()
            .map(|e| e.event.name.as_str())
            .collect::<Vec<_>>(),
        ["Changed", "Executed"]
    );
    assert_eq!(nonce(&mut host), Value::Word(ValueType::U256, word(1)));
}

#[test]
fn native_contract_signers_are_readonly_rechecked_and_require_canonical_true() {
    let f = fixtures();
    let v = &f["vectors"][0];
    let tx = hex_data(v["canonicalTransaction"].as_str().unwrap());
    for body in [
        "require(msg.sender == ACCOUNT); require(digest == EXPECTED); return bytes_len(signature) == 3;",
        "return false;", "self.changed = true; return true;", "revert();",
    ] {
        let mut host=fixture_with_signers(42,100,&[(word(8),2)],1);
        let source=format!("contract Verifier {{ const ACCOUNT: address = 0x1111111111111111111111111111111111111111; const EXPECTED: bytes32 = {}; state {{ changed: bool; }} pub fn is_valid_signature(digest: bytes32, signature: bytes) -> bool {{ {} }} }}",v["digest"].as_str().unwrap(),body);
        install(&mut host,word(8),&source);
        let id=success(&mut host,request("signer_id",vec![Value::Word(ValueType::Address,word(8))],word(9),100)).result.return_value;
        let Value::Word(ValueType::Bytes32,id)=id else{panic!("signer ID type")};
        let call=request("execute",vec![binary(tx.clone()),signature_bundle(&[(id,2,vec![0,255,128])])],word(9),100);
        let verifier_before=host.state().contract(&word(8)).unwrap().clone();
        if body.starts_with("require(msg.sender") {success(&mut host,call);assert_eq!(host.state().contract(&word(8)).unwrap(),&verifier_before);}else{unchanged_failure(&mut host,call);}
    }
    let mut host = fixture_with_signers(42, 100, &[(word(8), 2)], 1);
    install(&mut host,word(8),"contract Wrong { pub fn is_valid_signature(digest: bytes32, signature: bytes) -> u64 { return 1; } }");
    let id = success(
        &mut host,
        request(
            "signer_id",
            vec![Value::Word(ValueType::Address, word(8))],
            word(9),
            100,
        ),
    )
    .result
    .return_value;
    let Value::Word(ValueType::Bytes32, id) = id else {
        panic!("signer ID type")
    };
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(tx), signature_bundle(&[(id, 2, vec![])])],
            word(9),
            100,
        ),
    );
}

#[test]
fn account_custodies_a_native_token_and_executes_only_the_authorized_transfer() {
    let mut host = fixture(0, 100);
    let token="contract Token { state { ready: bool; balances: map<address, u256>; } event Transfer { from: address, to: address, amount: u256 } pub fn initialize(holder: address, amount: u256) -> bool { require(self.ready == false); self.ready = true; self.balances[holder] = amount; return true; } pub fn transfer(to: address, amount: u256) -> bool { require(self.balances[msg.sender] >= amount); self.balances[msg.sender] = self.balances[msg.sender] - amount; self.balances[to] = self.balances[to] + amount; emit Transfer { from: msg.sender, to: to, amount: amount }; return true; } pub fn balance(holder: address) -> u256 { return self.balances[holder]; } }";
    install(&mut host, word(7), token);
    let mut init = request(
        "initialize",
        vec![
            Value::Word(ValueType::Address, account()),
            Value::Word(ValueType::U256, word(100)),
        ],
        word(9),
        100,
    );
    init.contract = word(7);
    success(&mut host, init);
    let data = native_payload(
        &host,
        word(7),
        "transfer",
        &[
            Value::Word(ValueType::Address, word(3)),
            Value::Word(ValueType::U256, word(40)),
        ],
    );
    let tx = transaction_calls(0, &[(word(7), 0, data)], 0, 0);
    let id = success(
        &mut host,
        request("propose", vec![binary(tx.clone())], signer(0), 100),
    )
    .result
    .return_value;
    success(
        &mut host,
        request("approve", vec![id.clone()], signer(0), 100),
    );
    unchanged_failure(
        &mut host,
        request(
            "execute",
            vec![binary(tx.clone()), empty_signatures()],
            word(9),
            100,
        ),
    );
    success(&mut host, request("approve", vec![id], signer(1), 100));
    let result = success(
        &mut host,
        request(
            "execute",
            vec![binary(tx), empty_signatures()],
            word(9),
            100,
        ),
    );
    for (holder, amount) in [(account(), 60), (word(3), 40)] {
        let mut get = request(
            "balance",
            vec![Value::Word(ValueType::Address, holder)],
            word(9),
            100,
        );
        get.contract = word(7);
        assert_eq!(
            success(&mut host, get).result.return_value,
            Value::Word(ValueType::U256, word(amount))
        );
    }
    assert_eq!(
        result
            .events
            .iter()
            .map(|event| event.event.name.as_str())
            .collect::<Vec<_>>(),
        ["Transfer", "Executed"]
    );
}

#[test]
fn nonce_exhaustion_never_wraps_or_commits_a_partial_reservation() {
    let mut host = fixture(0, 100);
    let mut contract = host.state().contract(&account()).unwrap().clone();
    contract.storage.set_word("next_nonce", [255; 32]);
    host.state_mut().deploy(account(), contract);
    let mut tx = transaction(0, &[(word(3), 1)], 0, 0);
    tx[96..128].fill(255);
    unchanged_failure(
        &mut host,
        request("propose", vec![binary(tx)], signer(0), 100),
    );
    assert_eq!(nonce(&mut host), Value::Word(ValueType::U256, [255; 32]));
}

#[test]
fn deployment_initialization_is_atomic_and_duplicate_credentials_are_rejected() {
    CODE.with(|code| {
        let original = InMemoryState::default();
        for (signers, threshold) in [
            (vec![signer(0), signer(0)], 2),
            (vec![word(0)], 1),
            (vec![signer(0)], 0),
            (vec![signer(0)], 2),
        ] {
            let mut host = TransactionalHost::new(original.clone());
            let result = host.deploy_values(DeployRequest {
                deployer: word(9),
                nonce: 0,
                bytecode: code.clone(),
                initializer: Some(Initializer {
                    function: "initialize".into(),
                    arguments: vec![
                        binary(signer_list(&signers)),
                        Value::Word(ValueType::U64, word(threshold)),
                    ],
                }),
                value: word(0),
                gas_limit: 1_000_000,
                block_height: 1,
                block_timestamp: 100,
                chain_id: 700780,
            });
            assert!(matches!(result, DeployOutcome::Failure(_)), "{result:?}");
            assert_eq!(host.state(), &original);
        }
    });
    let mut host = fixture(0, 100);
    unchanged_failure(
        &mut host,
        request(
            "initialize",
            vec![
                binary(signer_list(&[signer(0)])),
                Value::Word(ValueType::U64, word(1)),
            ],
            word(9),
            100,
        ),
    );
}
