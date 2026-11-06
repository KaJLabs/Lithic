use lithovm_bytecode::{
    values::{self, Value as NativeValue},
    ValueType,
};
use lithovm_host::{
    child_contract_address, contract_address,
    gateway_receipts::{decode, gateway_topic, Receipt},
};
use serde_json::{json, Value};

const GATEWAY: &str = "0x000000000000000000000000000000000000f000";
fn word(n: u8) -> [u8; 32] {
    let mut w = [0; 32];
    w[31] = n;
    w
}
fn hex(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}
fn addr(w: [u8; 32]) -> String {
    hex(&w[12..])
}
fn registration(salt: u8) -> Value {
    let creator = contract_address(word(9), 0, word(3), 700777);
    json!({"contract":addr(child_contract_address(creator,word(salt),word(5),700777)),"creator":addr(creator),"codeHash":hex(&word(5)),"origin":{"kind":"child","template":addr(word(6)),"salt":hex(&word(salt))}})
}
fn log(payload: Value, index: u8) -> Value {
    json!({"address":GATEWAY,"topics":[gateway_topic()],"data":hex(&serde_json::to_vec(&payload).unwrap()),"logIndex":index.to_string()})
}
fn receipt(logs: Vec<Value>) -> Value {
    json!({"schemaVersion":"lithovm-evm-receipt-v1","chainId":"700777","transactionHash":hex(&word(1)),"from":addr(word(9)),"nonce":"1","blockHash":hex(&word(2)),"blockHeight":u64::MAX.to_string(),"transactionGasUsed":u64::MAX.to_string(),"status":"success","logs":logs})
}
fn check(v: Value) -> Result<Value, String> {
    decode(
        &serde_json::from_value::<Receipt>(v).map_err(|e| e.to_string())?,
        700777,
        GATEWAY,
    )
}

#[test]
fn multiple_gateway_calls_use_unique_transaction_wide_creation_indexes() {
    let packet = receipt(vec![
        log(json!({"Deployments":[registration(1)],"Events":[]}), 4),
        log(json!({"Deployments":[registration(2)],"Events":[]}), 8),
    ]);
    let result = check(packet.clone()).unwrap();
    assert_eq!(result["deployments"][0]["creationIndex"], 0);
    assert_eq!(result["deployments"][1]["creationIndex"], 1);
    assert_ne!(
        result["deployments"][0]["deploymentId"],
        result["deployments"][1]["deploymentId"]
    );
    assert_eq!(
        result["deployments"][1]["transactionGasUsed"],
        u64::MAX.to_string()
    );
    assert_eq!(result["canonicalProofVerified"], false);
    let mut reorganized = packet;
    reorganized["blockHash"] = json!(hex(&word(8)));
    assert_ne!(
        result["deployments"][0]["deploymentId"],
        check(reorganized).unwrap()["deployments"][0]["deploymentId"]
    );
}

#[test]
fn raw_gateway_json_and_dynamic_metadata_decode_without_abi_reinterpretation() {
    let encoded = values::encode(&[
        NativeValue::Word(ValueType::Address, word(7)),
        NativeValue::String("Token 🪨\0".into()),
        NativeValue::Word(ValueType::U256, [255; 32]),
    ])
    .unwrap();
    let packet = receipt(vec![log(
        json!({"Deployments":[],"Events":[{"contract":addr(word(6)),"name":"TokenCreated","values":hex(&encoded)}]}),
        0,
    )]);
    let out = check(packet).unwrap();
    assert_eq!(
        out["events"][0]["values"][1],
        json!({"type":"string","value":"Token 🪨\0"})
    );
    assert_eq!(
        out["events"][0]["values"][2]["value"],
        "115792089237316195423570985008687907853269984665640564039457584007913129639935"
    );
    assert!(
        out["deployments"].as_array().unwrap().is_empty(),
        "contract events cannot register code"
    );
}

#[test]
fn reverted_transactions_and_other_emitters_never_manufacture_deployments() {
    let mut foreign = log(json!({"Deployments":[registration(1)],"Events":[]}), 1);
    foreign["address"] = json!(addr(word(8)));
    foreign["data"] = json!("untrusted/non-hex");
    assert!(check(receipt(vec![foreign])).unwrap()["deployments"]
        .as_array()
        .unwrap()
        .is_empty());
    let mut reverted = receipt(vec![log(
        json!({"Deployments":[registration(1)],"Events":[]}),
        0,
    )]);
    reverted["status"] = json!("reverted");
    let result = check(reverted).unwrap();
    assert_eq!(result["state"], "failed");
    assert_eq!(result["deployments"], json!([]));
    assert_eq!(result["events"], json!([]));
}

#[test]
fn bad_batches_and_ambiguous_or_malformed_logs_reject_atomically() {
    let good = json!({"Deployments":[registration(1)],"Events":[]});
    for bad in [
        json!({"Deployments":[registration(1),registration(1)],"Events":[]}),
        json!({"Deployments":vec![registration(1);65],"Events":[]}),
        json!({"Deployments":[],"Events":[],"extra":true}),
        json!({"Deployments":[],"Events":[{"contract":addr(word(6)),"name":"Bad","values":"0x00"}]}),
    ] {
        assert!(check(receipt(vec![log(good.clone(), 0), log(bad, 1)])).is_err());
    }
    let mut forged = registration(2);
    forged["codeHash"] = json!(hex(&word(9)));
    assert!(check(receipt(vec![
        log(good.clone(), 0),
        log(json!({"Deployments":[forged],"Events":[]}), 1)
    ]))
    .is_err());
    let mut bad = log(good.clone(), 0);
    bad["topics"] = json!([gateway_topic(), hex(&word(1))]);
    assert!(check(receipt(vec![bad])).is_err());
    assert!(check(receipt(vec![log(good.clone(), 0), log(good, 0)])).is_err());
    let mut duplicate = log(json!({"Deployments":[],"Events":[]}), 0);
    duplicate["data"] = json!(hex(br#"{"Deployments":[],"Deployments":[],"Events":[]}"#));
    assert!(check(receipt(vec![duplicate])).is_err());
}

#[test]
fn top_level_registration_binds_outer_creator_nonce_and_chain() {
    let registration = json!({"contract":addr(contract_address(word(9),1,word(3),700777)),"creator":addr(word(9)),"codeHash":hex(&word(3)),"origin":{"kind":"transaction","nonce":"1"}});
    let packet = receipt(vec![log(
        json!({"Deployments":[registration],"Events":[]}),
        0,
    )]);
    assert_eq!(
        check(packet.clone()).unwrap()["deployments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for (field, value) in [
        ("nonce", json!("2")),
        ("from", json!(addr(word(8)))),
        ("chainId", json!("9005")),
        ("blockHeight", json!("0")),
        ("transactionGasUsed", json!("01")),
        ("blockHash", json!(hex(&[0; 32]))),
    ] {
        let mut bad = packet.clone();
        bad[field] = value;
        assert!(check(bad).is_err(), "accepted {field}");
    }
}
