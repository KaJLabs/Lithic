use anyhow::Result;

/// Verifier interface for LEP100-5.
/// This crate is intentionally small and pluggable; real implementations can live behind feature flags.
pub trait ZkVerifier {
    fn verify(&self, proof_system: &str, vk_id: &[u8], public_inputs: &[u8], proof: &[u8]) -> Result<bool>;
}

/// A stub verifier that always returns true (dev-only).
pub struct StubVerifier;

impl ZkVerifier for StubVerifier {
    fn verify(&self, _proof_system: &str, _vk_id: &[u8], _public_inputs: &[u8], _proof: &[u8]) -> Result<bool> {
        Ok(true)
    }
}
