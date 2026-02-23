use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// LEP100-4 domain separator (v1)
pub const DOMAIN_SEPARATOR: &str = "LITHOSPHERE_AI_RECEIPT_V1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptV1 {
    pub request_id: String,       // bytes32 hex
    pub contract_address: String, // address
    pub provider_id: String,      // string
    pub model_hash: String,       // bytes32 hex
    pub input_hash: String,       // bytes32 hex
    pub output_hash: String,      // bytes32 hex
    pub cost_used: u128,          // uint256 subset
    pub timestamp: u64,           // unix seconds
    pub provider_signature: String, // hex or base64 (provider-defined), verified at higher layer
}

pub fn digest_v1(r: &ReceiptV1) -> [u8; 32] {
    // Canonical digest (scaffold): SHA256 of concatenated UTF-8 fields + separator.
    // Production version should be binary-encoded canonical serialization.
    let mut h = Sha256::new();
    h.update(DOMAIN_SEPARATOR.as_bytes());
    h.update(b"|");
    h.update(r.request_id.as_bytes());
    h.update(b"|");
    h.update(r.contract_address.as_bytes());
    h.update(b"|");
    h.update(r.provider_id.as_bytes());
    h.update(b"|");
    h.update(r.model_hash.as_bytes());
    h.update(b"|");
    h.update(r.input_hash.as_bytes());
    h.update(b"|");
    h.update(r.output_hash.as_bytes());
    h.update(b"|");
    h.update(r.cost_used.to_string().as_bytes());
    h.update(b"|");
    h.update(r.timestamp.to_string().as_bytes());
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out[..32]);
    arr
}
