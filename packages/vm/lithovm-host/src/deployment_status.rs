//! Indexer handoff, not an RPC proof verifier or a finality oracle.
use crate::{child_contract_address, contract_address, CommittedDeployment, DeploymentOrigin};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Supply only from the embedding chain after the entire enclosing transaction
/// succeeds. A native host commit inside an EVM frame is not chain inclusion.
pub struct Inclusion {
    pub chain_id: u64,
    pub transaction_hash: [u8; 32],
    pub block_hash: [u8; 32],
    pub block_height: u64,
    pub transaction_gas_used: u64,
}

fn hash(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}
fn address(word: &[u8; 32]) -> Result<String, String> {
    if word[..12] != [0; 12] {
        return Err("noncanonical address".into());
    }
    Ok(hash(&word[12..]))
}

/// Validate and map the whole committed journal; no partial batch is returned.
/// The input is host output, never a contract event or client-supplied receipt.
pub fn included(
    records: &[CommittedDeployment],
    inclusion: &Inclusion,
) -> Result<Vec<Value>, String> {
    if records.len() > 64
        || inclusion.chain_id == 0
        || inclusion.transaction_hash == [0; 32]
        || inclusion.block_hash == [0; 32]
    {
        return Err("invalid inclusion context or deployment count".into());
    }
    let mut seen = BTreeSet::new();
    records.iter().enumerate().map(|(index, record)| {
        if !seen.insert(record.contract) { return Err("duplicate deployment address".into()); }
        let (expected, origin) = match record.origin {
            DeploymentOrigin::Transaction { nonce } => (
                contract_address(record.creator, nonce, record.code_hash, inclusion.chain_id),
                json!({"kind": "transaction", "nonce": nonce.to_string()}),
            ),
            DeploymentOrigin::Child { template, salt } => (
                child_contract_address(record.creator, salt, record.code_hash, inclusion.chain_id),
                json!({"kind": "child", "template": address(&template)?, "salt": hash(&salt)}),
            ),
        };
        if record.contract != expected { return Err("deployment address derivation mismatch".into()); }
        let transaction_hash = hash(&inclusion.transaction_hash);
        Ok(json!({
            "schemaVersion": "lithovm-included-deployment-v1",
            "deploymentId": format!("{}:{}:{}:{}", inclusion.chain_id, hash(&inclusion.block_hash), transaction_hash, index),
            "state": "included", "verificationState": "unverified",
            "chainId": inclusion.chain_id.to_string(), "transactionHash": transaction_hash,
            "blockHash": hash(&inclusion.block_hash), "blockHeight": inclusion.block_height.to_string(),
            "transactionGasUsed": inclusion.transaction_gas_used.to_string(), "creationIndex": index,
            "contractAddress": address(&record.contract)?, "creator": address(&record.creator)?,
            "codeHash": hash(&record.code_hash), "origin": origin,
        }))
    }).collect()
}
