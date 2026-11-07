#![no_main]
//! Actual custody ABI: prefunded balances, conservation and failed-call isolation.
use libfuzzer_sys::fuzz_target;
use lithovm_bytecode::{
    values::{encode, Value},
    ValueType,
};
use lithovm_ffi::{lithovm_execute_v3, lithovm_free_v1, Buffer};
use lithovm_host::{code_hash, contract_address};
use serde_json::{json, Value as Json};
use std::{collections::BTreeMap, sync::OnceLock};

const SOURCE: &str = "contract Vault { pub fn pay(to: address, amount: u256, accept: bool) -> bool { transfer_native(to, amount); require(accept); return true; } }";
struct State {
    records: BTreeMap<String, Vec<u8>>,
    bank: BTreeMap<[u8; 20], [u8; 32]>,
}
fn word(n: u128) -> [u8; 32] {
    let mut w = [0; 32];
    w[16..].copy_from_slice(&n.to_be_bytes());
    w
}
fn addr(n: u8) -> [u8; 32] {
    let mut a = [0; 32];
    a[31] = n;
    a
}
fn hex(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}
unsafe extern "C" fn read(
    ctx: usize,
    key: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> isize {
    let s = &*(ctx as *const State);
    let k = std::str::from_utf8(std::slice::from_raw_parts(key, len)).unwrap();
    let Some(v) = s.records.get(k) else { return -1 };
    if cap > 0 {
        assert_eq!(cap, v.len());
        std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
    }
    v.len() as isize
}
unsafe extern "C" fn balance(ctx: usize, address: *const u8, out: *mut u8) -> i32 {
    let s = &*(ctx as *const State);
    let a: [u8; 20] = std::slice::from_raw_parts(address, 20).try_into().unwrap();
    let v = s.bank.get(&a).copied().unwrap_or([0; 32]);
    std::ptr::copy_nonoverlapping(v.as_ptr(), out, 32);
    1
}
unsafe extern "C" fn charge(_: usize, _: u64) -> i32 {
    1
}
fn invoke(s: &State, request: &Json) -> Json {
    let raw = serde_json::to_vec(request).unwrap();
    let mut output = Buffer {
        data: std::ptr::null_mut(),
        len: 0,
    };
    assert_eq!(
        unsafe {
            lithovm_execute_v3(
                raw.as_ptr(),
                raw.len(),
                s as *const _ as usize,
                Some(read),
                Some(balance),
                Some(charge),
                &mut output,
            )
        },
        0
    );
    let result =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(output.data, output.len) })
            .unwrap();
    unsafe { lithovm_free_v1(output) };
    result
}
fuzz_target!(|data: &[u8]| {
    if data.len() < 25 {
        return;
    }
    let funding = u64::from_le_bytes(data[0..8].try_into().unwrap()) as u128;
    let recipient = u64::from_le_bytes(data[8..16].try_into().unwrap()) as u128;
    let amount = u64::from_le_bytes(data[16..24].try_into().unwrap()) as u128;
    let accept = data[24] & 1 != 0;
    static CODE: OnceLock<Vec<u8>> = OnceLock::new();
    let code = CODE.get_or_init(|| {
        hex::decode(
            lithic_lithovm::compile(SOURCE)
                .unwrap()
                .bytecode
                .trim_start_matches("0x"),
        )
        .unwrap()
    });
    let contract = contract_address(addr(9), 1, code_hash(code), 700777);
    let mut s = State {
        records: BTreeMap::new(),
        bank: BTreeMap::from([
            (contract[12..].try_into().unwrap(), word(funding)),
            (addr(10)[12..].try_into().unwrap(), word(recipient)),
        ]),
    };
    let mut request = json!({"version":2,"operation":"deploy","caller":hex(&addr(9)[12..]),"bytecode":hex(code),"arguments":hex(&encode(&[]).unwrap()),"nonce":1,"gas_limit":1000000,"chain_id":700777,"block_height":1,"block_timestamp":1});
    let deployed = invoke(&s, &request);
    assert_eq!(deployed["success"], true);
    assert_eq!(deployed["balances"], json!([]));
    for w in deployed["writes"].as_array().unwrap() {
        s.records.insert(
            w["key"].as_str().unwrap().into(),
            hex::decode(w["value"].as_str().unwrap().trim_start_matches("0x")).unwrap(),
        );
    }
    request.as_object_mut().unwrap().remove("bytecode");
    request["operation"] = json!("call");
    request["contract"] = json!(hex(&contract[12..]));
    request["function"] = json!("pay");
    request["arguments"] = json!(hex(&encode(&[
        Value::Word(ValueType::Address, addr(10)),
        Value::Word(ValueType::U256, word(amount)),
        Value::Word(ValueType::Bool, word(u128::from(accept)))
    ])
    .unwrap()));
    let result = invoke(&s, &request);
    assert_eq!(result["success"], accept && amount <= funding);
    if result["success"] == false {
        for field in ["writes", "events", "deployments", "balances"] {
            assert_eq!(result[field], json!([]));
        }
    } else {
        let mut after = s.bank.clone();
        for update in result["balances"].as_array().unwrap() {
            let address: [u8; 20] =
                hex::decode(update["address"].as_str().unwrap().trim_start_matches("0x"))
                    .unwrap()
                    .try_into()
                    .unwrap();
            assert_eq!(
                update["before"],
                hex(&s.bank.get(&address).copied().unwrap_or([0; 32]))
            );
            after.insert(
                address,
                hex::decode(update["after"].as_str().unwrap().trim_start_matches("0x"))
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );
        }
        assert_eq!(after[&contract[12..]], word(funding - amount));
        assert_eq!(after[&addr(10)[12..]], word(recipient + amount));
    }
});
