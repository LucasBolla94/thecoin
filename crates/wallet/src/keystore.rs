//! Encrypted wallet file (JSON, version 1).
//!
//! ```json
//! {
//!   "version": 1,
//!   "network": "mainnet",
//!   "kdf":    { "name": "argon2id", "m_kib": 65536, "t": 3, "p": 1, "salt": "<hex 16>" },
//!   "cipher": { "name": "chacha20poly1305", "nonce": "<hex 12>", "ciphertext": "<hex>" },
//!   "accounts": [ { "index": 0, "label": "default" } ],
//!   "node_url": "http://127.0.0.1:7334"
//! }
//! ```
//!
//! The plaintext is `{"mnemonic": "...", "passphrase": "..."}` encrypted with
//! ChaCha20-Poly1305 (AAD `"thecoin-wallet-v1"`) under a key derived from the
//! password with Argon2id. A wrong password fails authentication.

use crate::keys::{parse_mnemonic, HdKeys};
use anyhow::{anyhow, bail, Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thecoin_core::Network;
use zeroize::{Zeroize, Zeroizing};

const AAD: &[u8] = b"thecoin-wallet-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KdfParams {
    pub name: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CipherParams {
    pub name: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountEntry {
    pub index: u32,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletFile {
    pub version: u32,
    pub network: Network,
    pub kdf: KdfParams,
    pub cipher: CipherParams,
    pub accounts: Vec<AccountEntry>,
    #[serde(default)]
    pub node_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Secret {
    mnemonic: String,
    passphrase: String,
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.mnemonic.zeroize();
        self.passphrase.zeroize();
    }
}

/// Default KDF cost: 64 MiB, 3 passes (≈0.5 s on a small VPS).
pub fn default_kdf_cost() -> (u32, u32) {
    match std::env::var("THECOIN_WALLET_FAST_KDF") {
        Ok(v) if v == "1" => (1024, 1), // tests only
        _ => (65_536, 3),
    }
}

fn derive_key(password: &str, kdf: &KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    if kdf.name != "argon2id" {
        bail!("unsupported kdf {}", kdf.name);
    }
    let salt = hex::decode(&kdf.salt).context("bad salt")?;
    let params = Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|e| anyhow!("bad kdf params: {e}"))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &salt, key.as_mut())
        .map_err(|e| anyhow!("kdf failed: {e}"))?;
    Ok(key)
}

impl WalletFile {
    /// Encrypts `mnemonic` with `password`.
    pub fn create(network: Network, mnemonic: &str, passphrase: &str, password: &str) -> Result<WalletFile> {
        parse_mnemonic(mnemonic)?;
        let (m_kib, t) = default_kdf_cost();
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
        let kdf = KdfParams { name: "argon2id".into(), m_kib, t, p: 1, salt: hex::encode(salt) };
        let key = derive_key(password, &kdf)?;
        let secret = Secret { mnemonic: mnemonic.to_string(), passphrase: passphrase.to_string() };
        let plaintext = Zeroizing::new(serde_json::to_vec(&secret)?);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
        let mut ciphertext = plaintext.to_vec();
        cipher.encrypt_in_place(Nonce::from_slice(&nonce), AAD, &mut ciphertext).map_err(|_| anyhow!("encryption failed"))?;
        Ok(WalletFile {
            version: 1,
            network,
            kdf,
            cipher: CipherParams { name: "chacha20poly1305".into(), nonce: hex::encode(nonce), ciphertext: hex::encode(ciphertext) },
            accounts: vec![AccountEntry { index: 0, label: "default".into() }],
            node_url: None,
        })
    }

    /// Decrypts the wallet and returns its keys.
    pub fn unlock(&self, password: &str) -> Result<HdKeys> {
        let (mnemonic, passphrase) = self.reveal(password)?;
        let m = parse_mnemonic(&mnemonic)?;
        Ok(HdKeys::from_mnemonic(&m, &passphrase))
    }

    /// Decrypts and returns `(mnemonic, passphrase)` (for backups).
    pub fn reveal(&self, password: &str) -> Result<(Zeroizing<String>, Zeroizing<String>)> {
        if self.version != 1 {
            bail!("unsupported wallet version {}", self.version);
        }
        if self.cipher.name != "chacha20poly1305" {
            bail!("unsupported cipher {}", self.cipher.name);
        }
        let key = derive_key(password, &self.kdf)?;
        let nonce = hex::decode(&self.cipher.nonce).context("bad nonce")?;
        let ct: Vec<u8> = hex::decode(&self.cipher.ciphertext).context("bad ciphertext")?;
        if nonce.len() != 12 {
            bail!("bad nonce length");
        }
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
        let mut pt: Zeroizing<Vec<u8>> = Zeroizing::new(ct);
        cipher.decrypt_in_place(Nonce::from_slice(&nonce), AAD, &mut *pt).map_err(|_| anyhow!("wrong password or corrupted wallet file"))?;
        let secret: Secret = serde_json::from_slice(&pt)?;
        Ok((Zeroizing::new(secret.mnemonic.clone()), Zeroizing::new(secret.passphrase.clone())))
    }

    pub fn load(path: &Path) -> Result<WalletFile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("cannot read wallet {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("invalid wallet file {}", path.display()))
    }

    /// Writes atomically with owner-only permissions.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn next_index(&self) -> u32 {
        self.accounts.iter().map(|a| a.index + 1).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt() {
        std::env::set_var("THECOIN_WALLET_FAST_KDF", "1");
        let m = crate::keys::generate_mnemonic(12).unwrap().to_string();
        let w = WalletFile::create(Network::Testnet, &m, "", "correct horse").unwrap();
        let keys = w.unlock("correct horse").unwrap();
        let expected = HdKeys::from_mnemonic(&parse_mnemonic(&m).unwrap(), "").address(0, 0);
        assert_eq!(keys.address(0, 0), expected);
        assert!(w.unlock("wrong").is_err());
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("w.json");
        w.save(&p).unwrap();
        assert!(WalletFile::load(&p).unwrap().unlock("correct horse").is_ok());
    }
}
