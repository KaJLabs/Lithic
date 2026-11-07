//! Deterministic rebuild boundary for an explorer worker. Chain evidence must
//! be supplied independently by the indexer, never copied from the submission.
use crate::{compile, keccak_hex, Artifact};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub path: String,
    pub content: String,
    pub keccak256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub schema_version: String,
    pub chain_id: u64,
    pub contract_address: String,
    pub code_hash: String,
    pub artifact: Artifact,
    pub sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_hash: Option<String>,
}

/// Trusted bytes fetched by the caller at a pinned block on its configured chain.
pub struct ChainCode<'a> {
    pub chain_id: u64,
    pub contract_address: &'a str,
    pub bytecode: &'a [u8],
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedArtifact {
    pub source_hash: String,
    pub rebuilt_code_hash: String,
    pub compiler_version: String,
}

/// Rebuilds a single source and checks every artifact field against compilation
/// and independently observed chain code. Does not assert chain finality.
pub fn verify(request: &Request, chain: ChainCode<'_>) -> Result<VerifiedArtifact, String> {
    if request.schema_version != "lithovm-verification-v1" {
        return Err("unsupported verification schema".into());
    }
    if request.chain_id == 0
        || request.chain_id != chain.chain_id
        || !canonical_hex(&request.contract_address, 20)
        || request.contract_address != chain.contract_address
    {
        return Err("chain or contract identity mismatch".into());
    }
    if request
        .transaction_hash
        .as_ref()
        .is_some_and(|hash| !canonical_hex(hash, 32))
    {
        return Err("noncanonical transaction hash".into());
    }
    if request.sources.len() != 1 {
        return Err("native compilation requires exactly one source".into());
    }
    let source = &request.sources[0];
    // Paths are metadata only; never open a submitted path or download a URL.
    if source.path.is_empty()
        || source.path.contains(['\\', ':', '\0'])
        || source
            .path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("source path must be a normalized relative path".into());
    }
    if keccak_hex(source.content.as_bytes()) != source.keccak256 {
        return Err("source hash mismatch".into());
    }
    if request.artifact.compiler_version != env!("CARGO_PKG_VERSION") {
        return Err("request requires a different compiler version".into());
    }
    let rebuilt = compile(&source.content).map_err(|error| error.to_string())?;
    if rebuilt != request.artifact {
        return Err("submitted artifact does not match deterministic rebuild".into());
    }
    if rebuilt.code_hash != request.code_hash
        || rebuilt.code_hash != keccak_hex(chain.bytecode)
        || rebuilt.bytecode != format!("0x{}", hex::encode(chain.bytecode))
    {
        return Err("rebuilt bytecode does not match observed chain code".into());
    }
    Ok(VerifiedArtifact {
        source_hash: rebuilt.source_hash,
        rebuilt_code_hash: rebuilt.code_hash,
        compiler_version: rebuilt.compiler_version,
    })
}

fn canonical_hex(value: &str, bytes: usize) -> bool {
    value.len() == 2 + bytes * 2
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}
