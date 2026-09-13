//! Hierarchical deterministic keys.
//!
//! * Mnemonic: **BIP-39** (English, 12 or 24 words) → 64-byte seed.
//! * Derivation: **SLIP-0010** for Ed25519 (hardened only).
//! * Path: `m/44'/7333'/account'/0'/index'` (7333 = The Coin coin type).
//!
//! Any wallet implementing these standards derives the same addresses from the
//! same 24 words.

use anyhow::{anyhow, Result};
use bip39::{Language, Mnemonic};
use hmac::{Hmac, Mac};
use sha2::Sha512;
use thecoin_core::crypto::SecretKey;
use thecoin_core::{Address, Network};
use zeroize::Zeroize;

pub const COIN_TYPE: u32 = 7333;
const HARDENED: u32 = 0x8000_0000;

type HmacSha512 = Hmac<Sha512>;

/// Generates a new mnemonic from OS randomness.
pub fn generate_mnemonic(words: usize) -> Result<Mnemonic> {
    let bytes = match words {
        12 => 16,
        24 => 32,
        _ => return Err(anyhow!("mnemonic must have 12 or 24 words")),
    };
    let mut entropy = vec![0u8; bytes];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut entropy);
    let m = Mnemonic::from_entropy_in(Language::English, &entropy).map_err(|e| anyhow!("{e}"))?;
    entropy.zeroize();
    Ok(m)
}

pub fn parse_mnemonic(phrase: &str) -> Result<Mnemonic> {
    let normalized = phrase.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    Mnemonic::parse_in_normalized(Language::English, &normalized).map_err(|e| anyhow!("invalid mnemonic: {e}"))
}

fn hmac512(key: &[u8], data: &[&[u8]]) -> [u8; 64] {
    let mut mac = HmacSha512::new_from_slice(key).expect("hmac accepts any key length");
    for d in data {
        mac.update(d);
    }
    mac.finalize().into_bytes().into()
}

/// SLIP-0010 Ed25519 derivation. All indices are hardened automatically.
pub fn derive_ed25519(seed: &[u8], path: &[u32]) -> [u8; 32] {
    let mut i = hmac512(b"ed25519 seed", &[seed]);
    let mut key = [0u8; 32];
    let mut chain = [0u8; 32];
    key.copy_from_slice(&i[..32]);
    chain.copy_from_slice(&i[32..]);
    for index in path {
        let hardened = index | HARDENED;
        i = hmac512(&chain, &[&[0u8], &key, &hardened.to_be_bytes()]);
        key.copy_from_slice(&i[..32]);
        chain.copy_from_slice(&i[32..]);
    }
    i.zeroize();
    chain.zeroize();
    key
}

/// Keys of one wallet seed.
pub struct HdKeys {
    seed: [u8; 64],
}

impl Drop for HdKeys {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

impl HdKeys {
    pub fn from_mnemonic(m: &Mnemonic, passphrase: &str) -> HdKeys {
        HdKeys { seed: m.to_seed(passphrase) }
    }

    pub fn path(account: u32, index: u32) -> [u32; 5] {
        [44, COIN_TYPE, account, 0, index]
    }

    pub fn secret_key(&self, account: u32, index: u32) -> SecretKey {
        let mut k = derive_ed25519(&self.seed, &Self::path(account, index));
        let sk = SecretKey::from_bytes(&k);
        k.zeroize();
        sk
    }

    pub fn address(&self, account: u32, index: u32) -> Address {
        Address::from_public_key(&self.secret_key(account, index).public_key())
    }

    pub fn address_string(&self, account: u32, index: u32, network: Network) -> String {
        self.address(account, index).encode(network)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SLIP-0010 official test vector 1 for ed25519 (seed 000102...0f).
    #[test]
    fn slip10_vector() {
        let seed = hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
        let master = derive_ed25519(&seed, &[]);
        assert_eq!(hex::encode(master), "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7");
        let k = derive_ed25519(&seed, &[0]);
        assert_eq!(hex::encode(k), "68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3");
        let k = derive_ed25519(&seed, &[0, 1, 2, 2, 1_000_000_000]);
        assert_eq!(hex::encode(k), "8f94d394a8e8fd6b1bc2f3f49f5c47e385281d5c17e65324b0f62483e37e8793");
    }

    #[test]
    fn mnemonic_roundtrip() {
        let m = generate_mnemonic(24).unwrap();
        let phrase = m.to_string();
        let m2 = parse_mnemonic(&format!("  {}  ", phrase.to_uppercase())).unwrap();
        let a = HdKeys::from_mnemonic(&m, "").address(0, 0);
        let b = HdKeys::from_mnemonic(&m2, "").address(0, 0);
        assert_eq!(a, b);
        assert_ne!(a, HdKeys::from_mnemonic(&m, "").address(0, 1));
        assert!(parse_mnemonic("abandon abandon").is_err());
    }
}
