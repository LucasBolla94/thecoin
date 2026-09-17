//! Finality signed by the recent miners.
//!
//! Proof of work alone never says "this payment is final", only "it gets more
//! expensive to undo". For games and shops that is awkward: they would have to
//! wait several blocks. This module adds a second, cheap signal on top of the
//! proof of work:
//!
//! 1. Every mining node has a **signing key** and puts its public key in the
//!    blocks it mines (`BlockHeader::signer`).
//! 2. The **voters** of a block are the nodes that mined the previous
//!    [`FINALITY_WINDOW`] blocks; each one weighs as many votes as blocks it
//!    mined in that window.
//! 3. A node that sees a valid block signs `vote_message(...)` and relays the
//!    signature.
//! 4. When votes representing **two thirds of the window** agree on the same
//!    block, that block is **final**: nodes refuse any chain that drops it.
//!
//! Properties:
//!
//! * Reversing a final block needs two thirds of the recent mining power —
//!   above the 51 % that would already break the chain.
//! * It cannot stall the network: without votes the chain keeps going by work
//!   alone, only without the "final" mark.
//! * It does not depend on who holds coins, so it does not push the network
//!   towards the richest (unlike proof of stake).
//! * Signing two different blocks at the same height is public proof of
//!   misbehaviour; such a key is ignored by every node.

use crate::crypto::{self, SecretKey};
use crate::hash::{tagged_hash, Hash32};
use borsh::{BorshDeserialize, BorshSerialize};

/// Blocks whose miners may vote.
pub const FINALITY_WINDOW: u64 = 200;
/// Fraction of the window needed to finalise, in parts per 10 000 (2/3).
pub const FINALITY_THRESHOLD_BP: u64 = 6_667;
/// A vote is only useful around the tip.
pub const MAX_VOTE_DISTANCE: u64 = 64;
/// Different miners needed in the window before finality means anything.
/// Without this, a network with one or two miners would "finalise" its own
/// blocks and refuse everyone else's.
pub const MIN_FINALITY_SIGNERS: usize = 4;

/// What a voter signs.
pub fn vote_message(chain_id: u32, height: u64, block: &Hash32) -> Hash32 {
    tagged_hash("finality-vote", &[&chain_id.to_le_bytes(), &height.to_le_bytes(), &block.0])
}

/// A signature saying "I saw this block and I consider it the chain".
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FinalityVote {
    pub height: u64,
    pub block: Hash32,
    /// Public key announced by this voter in the blocks it mined.
    pub signer: [u8; 32],
    pub signature: [u8; 64],
}

impl FinalityVote {
    pub fn sign(chain_id: u32, height: u64, block: Hash32, key: &SecretKey) -> FinalityVote {
        let msg = vote_message(chain_id, height, &block);
        FinalityVote { height, block, signer: key.public_key(), signature: key.sign(&msg.0) }
    }

    pub fn verify(&self, chain_id: u32) -> bool {
        let msg = vote_message(chain_id, self.height, &self.block);
        crypto::verify(&self.signer, &msg.0, &self.signature)
    }

    /// Identifier used to avoid relaying the same vote twice.
    pub fn id(&self) -> Hash32 {
        tagged_hash("finality-vote-id", &[&self.signer, &self.height.to_le_bytes(), &self.block.0])
    }
}

/// Whether the window is diverse enough for finality to be meaningful:
/// a full window mined by at least [`MIN_FINALITY_SIGNERS`] different nodes.
pub fn can_finalize(window: &[[u8; 32]], full_window: u64) -> bool {
    if (window.len() as u64) < full_window {
        return false;
    }
    let mut distinct: Vec<&[u8; 32]> = Vec::new();
    for k in window {
        if !distinct.contains(&k) {
            distinct.push(k);
            if distinct.len() >= MIN_FINALITY_SIGNERS {
                return true;
            }
        }
    }
    false
}

/// Votes needed to finalise, given the weights of the window.
pub fn votes_needed(window_blocks: u64) -> u64 {
    (window_blocks * FINALITY_THRESHOLD_BP).div_ceil(10_000)
}

/// Counts the weight of a set of voters over the miners of the window.
///
/// `window` lists the signer key of each of the last blocks (one entry per
/// block, repetitions included). Returns how many blocks the voters mined.
pub fn weight_of(window: &[[u8; 32]], voters: &[[u8; 32]]) -> u64 {
    window.iter().filter(|k| voters.contains(k)).count() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn votes_are_signed_and_bound_to_the_network() {
        let key = SecretKey::from_bytes(&[3; 32]);
        let block = Hash32([9; 32]);
        let v = FinalityVote::sign(7, 12, block, &key);
        assert!(v.verify(7));
        assert!(!v.verify(8), "another network");
        let mut tampered = v.clone();
        tampered.block = Hash32([8; 32]);
        assert!(!tampered.verify(7));
        let other = FinalityVote::sign(7, 13, block, &key);
        assert_ne!(v.id(), other.id());
    }

    #[test]
    fn finality_needs_several_miners() {
        let full: Vec<[u8; 32]> = (0..FINALITY_WINDOW).map(|i| [(i % 4) as u8; 32]).collect();
        assert!(can_finalize(&full, FINALITY_WINDOW), "four miners over a full window");
        let three: Vec<[u8; 32]> = (0..FINALITY_WINDOW).map(|i| [(i % 3) as u8; 32]).collect();
        assert!(!can_finalize(&three, FINALITY_WINDOW), "three miners are not enough");
        let short: Vec<[u8; 32]> = (0..10).map(|i| [i as u8; 32]).collect();
        assert!(!can_finalize(&short, FINALITY_WINDOW), "a young chain does not finalise");
    }

    #[test]
    fn two_thirds_of_the_window() {
        assert_eq!(votes_needed(200), 134);
        assert_eq!(votes_needed(3), 3);
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];
        let window = [a, a, b, c, a];
        assert_eq!(weight_of(&window, &[a]), 3);
        assert_eq!(weight_of(&window, &[a, c]), 4);
        assert_eq!(weight_of(&window, &[[9u8; 32]]), 0);
    }
}
