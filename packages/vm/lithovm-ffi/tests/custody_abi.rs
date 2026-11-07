//! The actual exported custody ABI, with external state owned by the caller.
use lithovm_bytecode::{
    values::{encode, Value},
    ValueType,
};
use lithovm_ffi::{lithovm_execute_v2, lithovm_execute_v3, lithovm_free_v1, Buffer};
use lithovm_host::{code_hash, contract_address};
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

const SOURCE: &str = "contract Vault { state { initialized: bool; owner: address; sent: u256; } event Paid { to: address, amount: u256 } pub fn initialize(owner: address) -> bool { require(self.initialized == false); self.initialized = true; self.owner = owner; return true; } pub fn pay(to: address, amount: u256, accept: bool) -> u256 { require(msg.sender == self.owner); transfer_native(to, amount); self.sent = self.sent + amount; emit Paid { to: to, amount: amount }; require(accept); return self.sent; } }";
#[derive(Default)]
struct State {
    records: BTreeMap<String, Vec<u8>>,
    balances: BTreeMap<[u8; 20], [u8; 32]>,
    bank_reads: usize,
    bank_fails: bool,
    fuel: u64,
    fuel_limit: u64,
}
fn word(n: u64) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}
fn addr(n: u8) -> [u8; 32] {
    let mut w = [0; 32];
    w[31] = n;
    w
}
fn hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}
fn arguments(values: Vec<Value>) -> String {
    hex(&encode(&values).unwrap())
}
unsafe extern "C" fn read(
    context: usize,
    key: *const u8,
    len: usize,
    out: *mut u8,
    capacity: usize,
) -> isize {
    let s = &*(context as *const State);
    let k = std::str::from_utf8(std::slice::from_raw_parts(key, len)).unwrap();
    let Some(value) = s.records.get(k) else {
        return -1;
    };
    if capacity > 0 {
        if capacity < value.len() {
            return -2;
        }
        std::ptr::copy_nonoverlapping(value.as_ptr(), out, value.len());
    }
    value.len() as isize
}
unsafe extern "C" fn balance(context: usize, address: *const u8, out: *mut u8) -> i32 {
    let s = &mut *(context as *mut State);
    s.bank_reads += 1;
    if s.bank_fails {
        return 0;
    }
    let key: [u8; 20] = std::slice::from_raw_parts(address, 20).try_into().unwrap();
    let value = s.balances.get(&key).copied().unwrap_or([0; 32]);
    std::ptr::copy_nonoverlapping(value.as_ptr(), out, 32);
    1
}
unsafe extern "C" fn charge(context: usize, amount: u64) -> i32 {
    let s = &mut *(context as *mut State);
    s.fuel = s.fuel.saturating_add(amount);
    i32::from(s.fuel <= s.fuel_limit)
}
fn request() -> Json {
    json!({"version":2,"operation":"deploy","caller":hex(&addr(9)[12..]),"bytecode":lithic_lithovm::compile(SOURCE).unwrap().bytecode,"function":"initialize","arguments":arguments(vec![Value::Word(ValueType::Address,addr(9))]),"nonce":1,"gas_limit":1000000,"chain_id":700777,"block_height":1,"block_timestamp":1})
}
fn invoke(s: &mut State, r: &Json, legacy: bool) -> (i32, Buffer) {
    let raw = serde_json::to_vec(r).unwrap();
    s.fuel = 0;
    let mut out = Buffer {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let code = unsafe {
        if legacy {
            lithovm_execute_v2(
                raw.as_ptr(),
                raw.len(),
                s as *mut _ as usize,
                Some(read),
                Some(charge),
                &mut out,
            )
        } else {
            lithovm_execute_v3(
                raw.as_ptr(),
                raw.len(),
                s as *mut _ as usize,
                Some(read),
                Some(balance),
                Some(charge),
                &mut out,
            )
        }
    };
    (code, out)
}
fn call(s: &mut State, r: &Json, legacy: bool) -> Json {
    let (code, out) = invoke(s, r, legacy);
    assert_eq!(code, 0);
    let parsed =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(out.data, out.len) }).unwrap();
    unsafe { lithovm_free_v1(out) };
    parsed
}
fn apply(s: &mut State, r: &Json) {
    assert_eq!(r["success"], true);
    if let Some(changes) = r["balances"].as_array() {
        // Validate the complete external bank batch before the first mutation.
        for change in changes {
            let address: [u8; 20] =
                hex::decode(change["address"].as_str().unwrap().trim_start_matches("0x"))
                    .unwrap()
                    .try_into()
                    .unwrap();
            assert_eq!(
                hex(&s.balances.get(&address).copied().unwrap_or([0; 32])),
                change["before"]
            );
        }
        for change in changes {
            s.balances.insert(
                hex::decode(change["address"].as_str().unwrap().trim_start_matches("0x"))
                    .unwrap()
                    .try_into()
                    .unwrap(),
                hex::decode(change["after"].as_str().unwrap().trim_start_matches("0x"))
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );
        }
    }
    for write in r["writes"].as_array().unwrap() {
        let key = write["key"].as_str().unwrap();
        if write["delete"] == true {
            s.records.remove(key);
        } else {
            s.records.insert(
                key.into(),
                hex::decode(write["value"].as_str().unwrap().trim_start_matches("0x")).unwrap(),
            );
        }
    }
}
fn fixture() -> (State, Json, [u8; 20]) {
    let mut s = State {
        fuel_limit: 1_000_000,
        ..State::default()
    };
    let r = request();
    let code = hex::decode(r["bytecode"].as_str().unwrap().trim_start_matches("0x")).unwrap();
    let contract = contract_address(addr(9), 1, code_hash(&code), 700777);
    let key: [u8; 20] = contract[12..].try_into().unwrap();
    s.balances.insert(key, word(100));
    s.balances
        .insert(addr(9)[12..].try_into().unwrap(), word(1000));
    let result = call(&mut s, &r, false);
    assert_eq!(result["balances"], json!([]));
    apply(&mut s, &result);
    let mut r = r;
    r["operation"] = json!("call");
    r.as_object_mut().unwrap().remove("bytecode");
    r["contract"] = json!(hex(&key));
    r["function"] = json!("pay");
    r["arguments"] = json!(arguments(vec![
        Value::Word(ValueType::Address, addr(10)),
        Value::Word(ValueType::U256, word(7)),
        Value::Word(ValueType::Bool, word(1))
    ]));
    (s, r, key)
}
#[test]
fn prefunding_is_preserved_and_success_only_returns_conservative_bank_changes() {
    let (mut s, r, contract) = fixture();
    let before = s.balances.clone();
    s.bank_reads = 0;
    let result = call(&mut s, &r, false);
    assert_eq!(result["bankVersion"], 1);
    assert_eq!(result["success"], true);
    assert_eq!(
        s.balances, before,
        "the ABI must never apply external bank state"
    );
    assert_eq!(result["balances"].as_array().unwrap().len(), 2);
    assert!(s.bank_reads <= 3, "original bank reads must be cached");
    apply(&mut s, &result);
    assert_eq!(s.balances[&contract], word(93));
    assert_eq!(s.balances[&addr(10)[12..]], word(7));
    assert_eq!(s.balances[&addr(9)[12..]], word(1000));
}
#[test]
fn revert_unauthorized_overspend_overflow_and_callback_failure_leave_no_effects() {
    for fault in [
        "revert",
        "unauthorized",
        "overspend",
        "overflow",
        "callback",
        "fuel",
    ] {
        let (mut s, mut r, _) = fixture();
        let mut amount = 7;
        let mut accept = 1;
        if fault == "revert" {
            accept = 0
        }
        if fault == "unauthorized" {
            r["caller"] = json!(hex(&addr(11)[12..]));
        }
        if fault == "overspend" {
            amount = 101
        }
        if fault == "overflow" {
            s.balances
                .insert(addr(10)[12..].try_into().unwrap(), [255; 32]);
        }
        if fault == "callback" {
            s.bank_fails = true
        }
        if fault == "fuel" {
            s.fuel_limit = 1
        }
        r["arguments"] = json!(arguments(vec![
            Value::Word(ValueType::Address, addr(10)),
            Value::Word(ValueType::U256, word(amount)),
            Value::Word(ValueType::Bool, word(accept))
        ]));
        let records = s.records.clone();
        let bank = s.balances.clone();
        let result = call(&mut s, &r, false);
        assert_eq!(result["success"], false, "{fault}: {result}");
        for name in ["writes", "events", "deployments", "balances"] {
            assert_eq!(result[name], json!([]), "{fault}: {name}");
        }
        assert_eq!(s.records, records);
        assert_eq!(s.balances, bank);
    }
}
#[test]
fn first_profile_stays_nonpayable_and_versions_cannot_be_silently_mixed() {
    let (mut s, mut r, _) = fixture();
    r["version"] = json!(1);
    s.bank_reads = 0;
    let result = call(&mut s, &r, true);
    assert_eq!(result["success"], false);
    assert!(result.get("bankVersion").is_none());
    assert!(result.get("balances").is_none());
    assert_eq!(s.bank_reads, 0);
    let (code, out) = invoke(&mut s, &r, false);
    assert_eq!(code, 2);
    assert!(out.data.is_null());
    assert_eq!(out.len, 0);
    r["version"] = json!(2);
    let (code, out) = invoke(&mut s, &r, true);
    assert_eq!(code, 2);
    assert!(out.data.is_null());
}

#[test]
fn custody_pointer_callback_and_untrusted_balance_inputs_fail_closed() {
    let mut s = State::default();
    let mut request = request();
    for extra in ["balances", "balance", "value"] {
        request[extra] = json!(1000000);
        let (status, output) = invoke(&mut s, &request, false);
        assert_eq!(status, 2, "untrusted {extra} accepted");
        assert!(output.data.is_null());
        request.as_object_mut().unwrap().remove(extra);
    }
    let raw = serde_json::to_vec(&request).unwrap();
    for missing in ["input", "read", "balance", "charge", "output"] {
        let mut output = Buffer {
            data: std::ptr::dangling_mut(),
            len: 999,
        };
        let status = unsafe {
            lithovm_execute_v3(
                if missing == "input" {
                    std::ptr::null()
                } else {
                    raw.as_ptr()
                },
                raw.len(),
                &mut s as *mut _ as usize,
                if missing == "read" { None } else { Some(read) },
                if missing == "balance" {
                    None
                } else {
                    Some(balance)
                },
                if missing == "charge" {
                    None
                } else {
                    Some(charge)
                },
                if missing == "output" {
                    std::ptr::null_mut()
                } else {
                    &mut output
                },
            )
        };
        assert_eq!(status, 1, "missing {missing}");
        if missing != "output" {
            assert!(output.data.is_null());
            assert_eq!(output.len, 0);
        }
    }
}

#[test]
fn proposed_multisig_account_executes_custody_only_after_quorum_and_rolls_back_failed_batch() {
    const ACCOUNT: &str = include_str!(
        "../../../../sdk/contracts/standards/lep10015_native_account_candidate.lithic"
    );
    let mut s = State {
        fuel_limit: 10_000_000,
        ..State::default()
    };
    let mut req = request();
    req["gas_limit"] = json!(10_000_000);
    req["bytecode"] = json!(lithic_lithovm::compile(ACCOUNT).unwrap().bytecode);
    let descriptors = [word(32), word(2), addr(9), word(1), addr(11), word(1)].concat();
    req["arguments"] = json!(arguments(vec![
        Value::Bytes(descriptors),
        Value::Word(ValueType::U64, word(2))
    ]));
    let deployed = call(&mut s, &req, false);
    apply(&mut s, &deployed);
    let result_bytes = hex::decode(
        deployed["result"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
    )
    .unwrap();
    let account: [u8; 32] = result_bytes[8..40].try_into().unwrap();
    s.balances
        .insert(account[12..].try_into().unwrap(), word(100));
    req.as_object_mut().unwrap().remove("bytecode");
    req["operation"] = json!("call");
    req["contract"] = json!(hex(&account[12..]));
    req["function"] = json!("policy_hash");
    req["arguments"] = json!(arguments(vec![]));
    let policy = call(&mut s, &req, false);
    assert_eq!(policy["success"], true);
    let encoded = hex::decode(policy["result"].as_str().unwrap().trim_start_matches("0x")).unwrap();
    let policy: [u8; 32] = encoded[8..40].try_into().unwrap();
    for (nonce, second_amount) in [(0, 6), (1, 101)] {
        // Canonical ABI MultiSigTransaction with two value-only CALL entries.
        let tx = [
            word(32),
            account,
            word(700777),
            word(nonce),
            word(256),
            word(0),
            word(0),
            policy,
            word(0),
            word(2),
            word(64),
            word(224),
            addr(10),
            word(7),
            word(128),
            word(0),
            word(0),
            addr(12),
            word(second_amount),
            word(128),
            word(0),
            word(0),
        ]
        .concat();
        req["caller"] = json!(hex(&addr(9)[12..]));
        req["function"] = json!("propose");
        req["arguments"] = json!(arguments(vec![Value::Bytes(tx.clone())]));
        let proposed = call(&mut s, &req, false);
        apply(&mut s, &proposed);
        let id = hex::decode(
            proposed["result"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
        )
        .unwrap();
        let id: [u8; 32] = id[8..40].try_into().unwrap();
        req["function"] = json!("execute");
        req["arguments"] = json!(arguments(vec![
            Value::Bytes(tx.clone()),
            Value::Bytes([word(32), word(0)].concat())
        ]));
        let no_quorum = call(&mut s, &req, false);
        assert_eq!(no_quorum["success"], false);
        for signer in [9, 11] {
            req["caller"] = json!(hex(&addr(signer)[12..]));
            req["function"] = json!("approve");
            req["arguments"] = json!(arguments(vec![Value::Word(ValueType::Bytes32, id)]));
            let approval = call(&mut s, &req, false);
            apply(&mut s, &approval);
        }
        req["caller"] = json!(hex(&addr(13)[12..]));
        req["function"] = json!("execute");
        req["arguments"] = json!(arguments(vec![
            Value::Bytes(tx),
            Value::Bytes([word(32), word(0)].concat())
        ]));
        let before_bank = s.balances.clone();
        let before_records = s.records.clone();
        let executed = call(&mut s, &req, false);
        if second_amount == 6 {
            apply(&mut s, &executed);
            assert_eq!(s.balances[&account[12..]], word(87));
            assert_eq!(s.balances[&addr(10)[12..]], word(7));
            assert_eq!(s.balances[&addr(12)[12..]], word(6));
            let replay = call(&mut s, &req, false);
            assert_eq!(replay["success"], false);
        } else {
            assert_eq!(executed["success"], false);
            for field in ["writes", "events", "deployments", "balances"] {
                assert_eq!(executed[field], json!([]));
            }
            assert_eq!(s.balances, before_bank);
            assert_eq!(s.records, before_records);
        }
    }
}
