//! **LtHash16** — a homomorphic multiset hash used as the state commitment.
//!
//! The state is a set of `(key, value)` records. Its commitment is the
//! element-wise sum (mod 2^16) of `XOF(key, value)` vectors of 2048 × u16.
//! Adding or removing a record is a vector add/subtract, so updating the
//! commitment after a block costs O(records touched) instead of rehashing the
//! whole state — ideal for low-end machines. Security relies on the lattice
//! Short Integer Solution problem (Bellare–Micciancio; used by Facebook for
//! state integrity at scale).
//!
//! Every block header carries `state_root = tagged_hash("state-root", lthash)`,
//! so any node that computes a different state (bug or tampering) immediately
//! rejects the block.

use crate::hash::{tagged_hash, tags, Hash32};

pub const LTHASH_LEN: usize = 2048;

#[derive(Clone, PartialEq, Eq)]
pub struct LtHash(pub Box<[u16; LTHASH_LEN]>);

impl Default for LtHash {
    fn default() -> Self {
        LtHash(Box::new([0u16; LTHASH_LEN]))
    }
}

impl std::fmt::Debug for LtHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LtHash({})", self.root())
    }
}

fn element(key: &[u8], value: &[u8]) -> [u16; LTHASH_LEN] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"TheCoin:lthash16\x00");
    hasher.update(&(key.len() as u32).to_le_bytes());
    hasher.update(key);
    hasher.update(&(value.len() as u32).to_le_bytes());
    hasher.update(value);
    let mut reader = hasher.finalize_xof();
    let mut bytes = [0u8; LTHASH_LEN * 2];
    reader.fill(&mut bytes);
    let mut out = [0u16; LTHASH_LEN];
    for (i, chunk) in bytes.as_chunks::<2>().0.iter().enumerate() {
        out[i] = u16::from_le_bytes([chunk[0], chunk[1]]);
    }
    out
}

impl LtHash {
    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        let e = element(key, value);
        for (a, b) in self.0.iter_mut().zip(e.iter()) {
            *a = a.wrapping_add(*b);
        }
    }

    pub fn remove(&mut self, key: &[u8], value: &[u8]) {
        let e = element(key, value);
        for (a, b) in self.0.iter_mut().zip(e.iter()) {
            *a = a.wrapping_sub(*b);
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(LTHASH_LEN * 2);
        for x in self.0.iter() {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<LtHash> {
        if b.len() != LTHASH_LEN * 2 {
            return None;
        }
        let mut out = LtHash::default();
        for (i, chunk) in b.as_chunks::<2>().0.iter().enumerate() {
            out.0[i] = u16::from_le_bytes([chunk[0], chunk[1]]);
        }
        Some(out)
    }

    /// The 32-byte state root committed in block headers.
    pub fn root(&self) -> Hash32 {
        tagged_hash(tags::STATE_ROOT, &[&self.to_bytes()])
    }

    /// Applies a change `old -> new` for `key` (either side may be absent).
    pub fn apply_change(&mut self, key: &[u8], old: Option<&[u8]>, new: Option<&[u8]>) {
        if let Some(o) = old {
            self.remove(key, o);
        }
        if let Some(n) = new {
            self.insert(key, n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_independent_and_reversible() {
        let mut a = LtHash::default();
        a.insert(b"k1", b"v1");
        a.insert(b"k2", b"v2");
        let mut b = LtHash::default();
        b.insert(b"k2", b"v2");
        b.insert(b"k1", b"v1");
        assert_eq!(a.root(), b.root());
        a.apply_change(b"k1", Some(b"v1"), Some(b"v9"));
        assert_ne!(a.root(), b.root());
        a.apply_change(b"k1", Some(b"v9"), Some(b"v1"));
        assert_eq!(a.root(), b.root());
        a.remove(b"k1", b"v1");
        a.remove(b"k2", b"v2");
        assert_eq!(a, LtHash::default());
        assert_eq!(LtHash::from_bytes(&b.to_bytes()).unwrap(), b);
    }

    #[test]
    fn key_value_boundary() {
        let mut a = LtHash::default();
        a.insert(b"ab", b"c");
        let mut b = LtHash::default();
        b.insert(b"a", b"bc");
        assert_ne!(a.root(), b.root());
    }
}
