//! Ed25519 signatures (RFC 8032) with strict verification.
//!
//! Strict verification rejects non-canonical signatures and small-order public
//! keys, which removes signature malleability: a transaction id can only change
//! if the signer produces a new signature.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

pub const PUBLIC_KEY_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;
pub const SECRET_KEY_LEN: usize = 32;

/// A private key. Wraps `ed25519_dalek::SigningKey` (zeroized on drop).
#[derive(Clone)]
pub struct SecretKey(SigningKey);

impl SecretKey {
    pub fn from_bytes(bytes: &[u8; SECRET_KEY_LEN]) -> Self {
        SecretKey(SigningKey::from_bytes(bytes))
    }

    pub fn to_bytes(&self) -> [u8; SECRET_KEY_LEN] {
        self.0.to_bytes()
    }

    pub fn public_key(&self) -> [u8; PUBLIC_KEY_LEN] {
        self.0.verifying_key().to_bytes()
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.0.sign(msg).to_bytes()
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

/// Verifies `signature` over `msg` using strict RFC 8032 rules.
pub fn verify(public_key: &[u8; PUBLIC_KEY_LEN], msg: &[u8], signature: &[u8; SIGNATURE_LEN]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    if vk.is_weak() {
        return false;
    }
    let sig = Signature::from_bytes(signature);
    vk.verify_strict(msg, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify() {
        let sk = SecretKey::from_bytes(&[7u8; 32]);
        let pk = sk.public_key();
        let sig = sk.sign(b"hello");
        assert!(verify(&pk, b"hello", &sig));
        assert!(!verify(&pk, b"hellO", &sig));
        let mut bad = sig;
        bad[0] ^= 1;
        assert!(!verify(&pk, b"hello", &bad));
    }
}
