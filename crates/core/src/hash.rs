//! 32-byte hashes and domain-separated BLAKE3.
//!
//! Every hash used by the protocol is computed as
//! `BLAKE3("TheCoin:" || tag || 0x00 || data...)` so that a value hashed for one
//! purpose (e.g. a transaction id) can never collide with a value hashed for
//! another purpose (e.g. a Merkle node).

use borsh::{BorshDeserialize, BorshSerialize};
use std::fmt;
use std::str::FromStr;

/// A 32-byte hash (block hash, transaction id, contract id, ...).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, BorshSerialize, BorshDeserialize)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    pub const ZERO: Hash32 = Hash32([0u8; 32]);

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let mut out = [0u8; 32];
        hex::decode_to_slice(s.trim(), &mut out)?;
        Ok(Hash32(out))
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash32({})", self.to_hex())
    }
}

impl FromStr for Hash32 {
    type Err = hex::FromHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hash32::from_hex(s)
    }
}

impl serde::Serialize for Hash32 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> serde::Deserialize<'de> for Hash32 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        Hash32::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// Domain tags. Adding a tag is fine; changing an existing one is a hard fork.
pub mod tags {
    pub const TX_SIGN: &str = "tx-sign";
    pub const TXID: &str = "txid";
    pub const BLOCK: &str = "block";
    pub const MERKLE_LEAF: &str = "merkle-leaf";
    pub const MERKLE_NODE: &str = "merkle-node";
    pub const MERKLE_EMPTY: &str = "merkle-empty";
    pub const ADDRESS: &str = "address";
    pub const CONTRACT_ID: &str = "contract-id";
    pub const PROPOSAL_ID: &str = "proposal-id";
    pub const PROGRAM_ADDRESS: &str = "program-address";
    pub const STATE_ROOT: &str = "state-root";
    pub const GENESIS: &str = "genesis";
    pub const P2P_CHECKSUM: &str = "p2p";
}

/// `BLAKE3("TheCoin:" || tag || 0x00 || parts[0] || parts[1] || ...)`.
pub fn tagged_hash(tag: &str, parts: &[&[u8]]) -> Hash32 {
    let mut h = blake3::Hasher::new();
    h.update(b"TheCoin:");
    h.update(tag.as_bytes());
    h.update(&[0u8]);
    for p in parts {
        h.update(p);
    }
    Hash32(*h.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let h = tagged_hash("test", &[b"abc"]);
        assert_eq!(Hash32::from_hex(&h.to_hex()).unwrap(), h);
    }

    #[test]
    fn domain_separation() {
        assert_ne!(tagged_hash("a", &[b"b"]), tagged_hash("ab", &[]));
        assert_ne!(tagged_hash(tags::TXID, &[b"x"]), tagged_hash(tags::BLOCK, &[b"x"]));
    }
}
