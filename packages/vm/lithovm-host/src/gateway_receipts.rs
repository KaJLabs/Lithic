//! Bounded offline mapping of a receipt supplied by a trusted chain adapter.
//! The caller authenticates the RPC/chain/transaction first. This module never
//! establishes finality, verifies a consensus proof or accepts contract events
//! as deployment registrations.
use crate::{
    deployment_status::{included_transaction, Inclusion},
    Address, CommittedDeployment, DeploymentOrigin,
};
use lithovm_bytecode::{
    values::{self, Value as NativeValue},
    ValueType,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};

pub const MAX_LOGS: usize = 1024;
pub const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_REGISTRATIONS: usize = 4096;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: String,
    pub chain_id: String,
    pub transaction_hash: String,
    pub from: String,
    pub nonce: String,
    pub block_hash: String,
    pub block_height: String,
    pub transaction_gas_used: String,
    pub status: String,
    pub logs: Vec<Log>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Log {
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
    pub log_index: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    #[serde(rename = "Deployments")]
    deployments: Vec<Registration>,
    #[serde(rename = "Events")]
    events: Vec<Event>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registration {
    contract: String,
    creator: String,
    code_hash: String,
    origin: Origin,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Origin {
    Transaction { nonce: String },
    Child { template: String, salt: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    contract: String,
    name: String,
    values: String,
}

pub fn gateway_topic() -> String {
    format!(
        "0x{}",
        hex::encode(Keccak256::digest(b"LithoNativeLab(bytes)"))
    )
}

fn bytes(s: &str, len: usize) -> Result<Vec<u8>, String> {
    let raw = s.strip_prefix("0x").ok_or("hex prefix required")?;
    if raw.len() != 2 * len
        || raw
            .bytes()
            .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
    {
        return Err("noncanonical receipt hex".into());
    }
    hex::decode(raw).map_err(|e| e.to_string())
}
fn hash(s: &str) -> Result<[u8; 32], String> {
    Ok(bytes(s, 32)?.try_into().unwrap())
}
fn address(s: &str) -> Result<Address, String> {
    let mut word = [0; 32];
    word[12..].copy_from_slice(&bytes(s, 20)?);
    if word == [0; 32] {
        return Err("zero receipt address".into());
    }
    Ok(word)
}
fn number(s: &str) -> Result<u64, String> {
    let n = s.parse::<u64>().map_err(|_| "invalid receipt integer")?;
    if n.to_string() != s {
        return Err("noncanonical receipt integer".into());
    }
    Ok(n)
}
fn decimal(mut word: [u8; 32]) -> String {
    if word == [0; 32] {
        return "0".into();
    }
    let mut digits = Vec::new();
    while word != [0; 32] {
        let mut rem = 0u16;
        for byte in &mut word {
            let n = rem * 256 + *byte as u16;
            *byte = (n / 10) as u8;
            rem = n % 10;
        }
        digits.push((b'0' + rem as u8) as char);
    }
    digits.into_iter().rev().collect()
}
fn value(v: NativeValue) -> Value {
    match v {
        NativeValue::String(s) => json!({"type":"string","value":s}),
        NativeValue::Bytes(b) => json!({"type":"bytes","value":format!("0x{}",hex::encode(b))}),
        NativeValue::Word(ty, w) => match ty {
            ValueType::U64 | ValueType::U256 => json!({"type":ty.name(),"value":decimal(w)}),
            ValueType::Bool => json!({"type":"bool","value":w[31]==1}),
            ValueType::Address => {
                json!({"type":"address","value":format!("0x{}",hex::encode(&w[12..]))})
            }
            _ => json!({"type":ty.name(),"value":format!("0x{}",hex::encode(w))}),
        },
    }
}

/// Supply expected chain/gateway from operator configuration, not the submitted
/// source bundle. Output is always an observation with no finality assertion.
pub fn decode(receipt: &Receipt, expected_chain: u64, gateway: &str) -> Result<Value, String> {
    let gateway = address(gateway)?;
    let chain_id = number(&receipt.chain_id)?;
    let from = address(&receipt.from)?;
    let nonce = number(&receipt.nonce)?;
    let inclusion = Inclusion {
        chain_id,
        transaction_hash: hash(&receipt.transaction_hash)?,
        block_hash: hash(&receipt.block_hash)?,
        block_height: number(&receipt.block_height)?,
        transaction_gas_used: number(&receipt.transaction_gas_used)?,
    };
    if receipt.schema_version != "lithovm-evm-receipt-v1"
        || chain_id == 0
        || chain_id != expected_chain
        || inclusion.block_height == 0
        || inclusion.transaction_hash == [0; 32]
        || inclusion.block_hash == [0; 32]
        || receipt.logs.len() > MAX_LOGS
    {
        return Err("invalid receipt identity or limits".into());
    }
    if receipt.status != "success" && receipt.status != "reverted" {
        return Err("unknown receipt status".into());
    }
    if receipt.status == "reverted" {
        return Ok(
            json!({"schemaVersion":"lithovm-receipt-observation-v1","chainId":receipt.chain_id,"transactionHash":receipt.transaction_hash,"blockHash":receipt.block_hash,"blockHeight":receipt.block_height,"state":"failed","canonicalProofVerified":false,"deployments":[],"events":[]}),
        );
    }
    let topic = gateway_topic();
    let (mut records, mut events) = (Vec::new(), Vec::new());
    let (mut previous, mut size, mut roots) = (None, 0usize, 0usize);
    for log in &receipt.logs {
        let index = number(&log.log_index)?;
        if previous.is_some_and(|p| p >= index) {
            return Err("duplicate or out-of-order receipt log index".into());
        }
        previous = Some(index);
        if address(&log.address)? != gateway {
            continue;
        }
        // Other events emitted by the gateway are not native registration data.
        if log.topics.first() != Some(&topic) {
            continue;
        }
        if log.topics.len() != 1 {
            return Err("native receipt has unexpected topics".into());
        }
        let raw = log
            .data
            .strip_prefix("0x")
            .ok_or("native log hex prefix required")?;
        if raw.len() % 2 != 0
            || raw.len() / 2 > MAX_LOG_BYTES
            || raw
                .bytes()
                .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
        {
            return Err("native log encoding/size exceeds bounds".into());
        }
        size = size
            .checked_add(raw.len() / 2)
            .ok_or("native receipt size overflow")?;
        if size > 4 * MAX_LOG_BYTES {
            return Err("aggregate native receipt data exceeds bounds".into());
        }
        let payload: Payload =
            serde_json::from_slice(&hex::decode(raw).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if payload.deployments.len() > 64
            || records.len() + payload.deployments.len() > MAX_REGISTRATIONS
            || events.len() + payload.events.len() > 4096
        {
            return Err("native receipt journal exceeds bounds".into());
        }
        for record in payload.deployments {
            let creator = address(&record.creator)?;
            let origin = match record.origin {
                Origin::Transaction {
                    nonce: record_nonce,
                } => {
                    let record_nonce = number(&record_nonce)?;
                    roots += 1;
                    if roots > 1 || creator != from || record_nonce != nonce {
                        return Err(
                            "native transaction origin does not match outer transaction".into()
                        );
                    }
                    DeploymentOrigin::Transaction {
                        nonce: record_nonce,
                    }
                }
                Origin::Child { template, salt } => DeploymentOrigin::Child {
                    template: address(&template)?,
                    salt: hash(&salt)?,
                },
            };
            records.push(CommittedDeployment {
                contract: address(&record.contract)?,
                creator,
                code_hash: hash(&record.code_hash)?,
                origin,
            });
        }
        for event in payload.events {
            address(&event.contract)?;
            if event.name.is_empty()
                || event.name.len() > 255
                || !event.name.bytes().enumerate().all(|(i, b)| {
                    b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit())
                })
            {
                return Err("invalid native event name".into());
            }
            let raw = event
                .values
                .strip_prefix("0x")
                .ok_or("native values hex prefix required")?;
            if raw.len() > 2 * values::MAX_ENVELOPE_BYTES
                || raw.len() % 2 != 0
                || raw
                    .bytes()
                    .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
            {
                return Err("invalid native event value encoding".into());
            }
            let decoded = values::decode(&hex::decode(raw).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            events.push(json!({"contract":event.contract,"name":event.name,"values":decoded.into_iter().map(value).collect::<Vec<_>>(),"gatewayLogIndex":index.to_string()}));
        }
    }
    let deployments = included_transaction(&records, &inclusion)?;
    Ok(
        json!({"schemaVersion":"lithovm-receipt-observation-v1","chainId":receipt.chain_id,"transactionHash":receipt.transaction_hash,"blockHash":receipt.block_hash,"blockHeight":receipt.block_height,"state":"included","canonicalProofVerified":false,"deployments":deployments,"events":events}),
    )
}
