//! Binary Merkle tree over transaction ids.
//!
//! * leaf  = `tagged_hash("merkle-leaf", txid)`
//! * node  = `tagged_hash("merkle-node", left || right)`
//! * An odd node at the end of a level is promoted unchanged to the next level
//!   (no duplication, which avoids Bitcoin's CVE-2012-2459 ambiguity).
//! * Empty tree = `tagged_hash("merkle-empty")`.

use crate::hash::{tagged_hash, tags, Hash32};

pub fn merkle_root(leaves: &[Hash32]) -> Hash32 {
    if leaves.is_empty() {
        return tagged_hash(tags::MERKLE_EMPTY, &[]);
    }
    let mut level: Vec<Hash32> = leaves.iter().map(|l| tagged_hash(tags::MERKLE_LEAF, &[&l.0])).collect();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [a, b] => tagged_hash(tags::MERKLE_NODE, &[&a.0, &b.0]),
                [a] => *a,
                _ => unreachable!(),
            })
            .collect();
    }
    level[0]
}

/// Proof that a leaf is included: sibling hashes from the leaf level upward.
/// `None` entries mean the node was promoted without a sibling.
pub fn merkle_proof(leaves: &[Hash32], index: usize) -> Option<Vec<(bool, Hash32)>> {
    if index >= leaves.len() {
        return None;
    }
    let mut proof = Vec::new();
    let mut level: Vec<Hash32> = leaves.iter().map(|l| tagged_hash(tags::MERKLE_LEAF, &[&l.0])).collect();
    let mut idx = index;
    while level.len() > 1 {
        let sibling = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
        if sibling < level.len() {
            // (sibling_is_left, hash)
            proof.push((sibling < idx, level[sibling]));
        }
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [a, b] => tagged_hash(tags::MERKLE_NODE, &[&a.0, &b.0]),
                [a] => *a,
                _ => unreachable!(),
            })
            .collect();
        idx /= 2;
    }
    Some(proof)
}

pub fn verify_merkle_proof(leaf: &Hash32, proof: &[(bool, Hash32)], root: &Hash32) -> bool {
    let mut acc = tagged_hash(tags::MERKLE_LEAF, &[&leaf.0]);
    for (sibling_is_left, sib) in proof {
        acc = if *sibling_is_left {
            tagged_hash(tags::MERKLE_NODE, &[&sib.0, &acc.0])
        } else {
            tagged_hash(tags::MERKLE_NODE, &[&acc.0, &sib.0])
        };
    }
    &acc == root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_and_proofs() {
        let leaves: Vec<Hash32> = (0u8..7).map(|i| tagged_hash("t", &[&[i]])).collect();
        for n in 1..=leaves.len() {
            let root = merkle_root(&leaves[..n]);
            for i in 0..n {
                let proof = merkle_proof(&leaves[..n], i).unwrap();
                assert!(verify_merkle_proof(&leaves[i], &proof, &root));
                assert!(!verify_merkle_proof(&leaves[(i + 1) % 7], &proof, &root));
            }
        }
        assert_ne!(merkle_root(&leaves[..2]), merkle_root(&leaves[..3]));
        assert_ne!(merkle_root(&[]), merkle_root(&leaves[..1]));
    }
}
