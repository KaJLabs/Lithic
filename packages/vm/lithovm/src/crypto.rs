//! Strict, public-input secp256k1 verification. No signing or private keys.
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use sha3::{Digest, Keccak256};

/// Verify an already computed 32-byte digest, without hashing it again.
/// Accept only r[32] || s[32] || v[1], v=27/28 and low S. Malformed or
/// noncanonical signatures return false; they are never normalized here.
pub(super) fn verify_secp256k1(digest: &[u8; 32], bytes: &[u8], signer: &[u8; 32]) -> bool {
    if bytes.len() != 65
        || !matches!(bytes[64], 27 | 28)
        || signer[..12] != [0; 12]
        || *signer == [0; 32]
    {
        return false;
    }
    let Ok(signature) = Signature::from_slice(&bytes[..64]) else {
        return false;
    };
    if signature.normalize_s() != signature {
        return false;
    }
    let Some(recovery) = RecoveryId::from_byte(bytes[64] - 27) else {
        return false;
    };
    let Ok(key) = VerifyingKey::recover_from_prehash(digest, &signature, recovery) else {
        return false;
    };
    let public = key.to_sec1_point(false);
    let address = Keccak256::digest(&public.as_bytes()[1..]);
    address[12..] == signer[12..]
}
