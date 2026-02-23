use anyhow::{anyhow, Result};
use lithovm_bytecode::parse;
use lithovm_receipts::ReceiptV1;
use lithovm_zk_verifier::{StubVerifier, ZkVerifier};

/// Minimal LithoVM execution context (scaffold).
pub struct Vm {
    verifier: Box<dyn ZkVerifier + Send + Sync>,
}

impl Default for Vm {
    fn default() -> Self {
        Self { verifier: Box::new(StubVerifier) }
    }
}

impl Vm {
    pub fn load_and_validate(&self, bytes: &[u8]) -> Result<()> {
        let _bc = parse(bytes)?;
        Ok(())
    }

    /// Validate a receipt signature/zk-proof at a high level (scaffold).
    /// Production: enforce LEP100-2/4/5 rules in consensus + runtime.
    pub fn validate_receipt(&self, _receipt: &ReceiptV1) -> Result<()> {
        Ok(())
    }

    pub fn verify_zk(&self, proof_system: &str, vk_id: &[u8], public_inputs: &[u8], proof: &[u8]) -> Result<bool> {
        self.verifier.verify(proof_system, vk_id, public_inputs, proof)
    }
}
