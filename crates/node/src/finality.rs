//! Collects the finality votes of the recent miners (see
//! [`thecoin_core::finality`] for the design).
//!
//! The node keeps one signing key in `<data-dir>/finality.key`, announces its
//! public key in the blocks it mines, votes for every new tip it accepts, and
//! relays the votes of others. When the votes for a block reach two thirds of
//! the mining window, the block is recorded as final and the chain manager
//! refuses any branch that drops it.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use thecoin_core::crypto::SecretKey;
use thecoin_core::finality::{can_finalize, votes_needed, weight_of, FinalityVote, FINALITY_WINDOW, MAX_VOTE_DISTANCE};
use thecoin_core::hash::Hash32;

/// Why a vote was not counted.
#[derive(Debug, PartialEq, Eq)]
pub enum VoteError {
    /// Already counted (do not relay again).
    Known,
    BadSignature,
    /// Too far from the tip to matter.
    TooFar,
    /// The signer did not mine in the window, so it has no weight.
    NotAVoter,
    /// The signer already voted for another block at this height.
    Equivocation,
}

pub struct Tally {
    /// Votes for each (height, block), by signer.
    votes: HashMap<(u64, Hash32), HashMap<[u8; 32], FinalityVote>>,
    /// What each signer voted at each height, to catch double votes.
    voted: HashMap<(u64, [u8; 32]), Hash32>,
    /// Keys caught signing two blocks at the same height: ignored forever.
    banned: Vec<[u8; 32]>,
    /// Votes for blocks this node has not seen yet (they often arrive first).
    parked: Vec<FinalityVote>,
    /// Last block known to be final.
    final_block: Option<(u64, Hash32)>,
}

impl Default for Tally {
    fn default() -> Self {
        Tally::new()
    }
}

impl Tally {
    pub fn new() -> Tally {
        Tally { votes: HashMap::new(), voted: HashMap::new(), banned: Vec::new(), parked: Vec::new(), final_block: None }
    }

    pub fn finalized(&self) -> Option<(u64, Hash32)> {
        self.final_block
    }

    pub fn set_finalized(&mut self, height: u64, block: Hash32) {
        if self.final_block.is_none_or(|(h, _)| height > h) {
            self.final_block = Some((height, block));
        }
    }

    /// Votes counted so far for a block.
    pub fn weight(&self, height: u64, block: &Hash32, window: &[[u8; 32]]) -> u64 {
        match self.votes.get(&(height, *block)) {
            Some(v) => {
                let voters: Vec<[u8; 32]> = v.keys().copied().collect();
                weight_of(window, &voters)
            }
            None => 0,
        }
    }

    /// Records a vote. `window` are the signer keys of the blocks before the
    /// voted one (newest first or oldest first, it only counts repetitions).
    /// Returns `Ok(true)` when this vote makes the block final.
    pub fn add(
        &mut self,
        vote: FinalityVote,
        chain_id: u32,
        tip_height: u64,
        window: &[[u8; 32]],
        full_window: u64,
    ) -> Result<bool, VoteError> {
        if self.banned.contains(&vote.signer) {
            return Err(VoteError::Known);
        }
        if vote.height + MAX_VOTE_DISTANCE < tip_height || vote.height > tip_height + 1 {
            return Err(VoteError::TooFar);
        }
        if let Some((h, _)) = self.final_block {
            if vote.height <= h {
                return Err(VoteError::Known);
            }
        }
        if !window.contains(&vote.signer) {
            return Err(VoteError::NotAVoter);
        }
        match self.voted.get(&(vote.height, vote.signer)) {
            Some(b) if *b == vote.block => return Err(VoteError::Known),
            Some(_) => {
                // Two different blocks at the same height: drop this key.
                self.banned.push(vote.signer);
                for entry in self.votes.values_mut() {
                    entry.remove(&vote.signer);
                }
                return Err(VoteError::Equivocation);
            }
            None => {}
        }
        if !vote.verify(chain_id) {
            return Err(VoteError::BadSignature);
        }
        self.voted.insert((vote.height, vote.signer), vote.block);
        let (height, block) = (vote.height, vote.block);
        self.votes.entry((height, block)).or_default().insert(vote.signer, vote);
        let weight = self.weight(height, &block, window);
        let needed = votes_needed(window.len() as u64);
        if weight >= needed && needed > 0 && can_finalize(window, full_window) {
            self.set_finalized(height, block);
            self.prune(height);
            return Ok(true);
        }
        Ok(false)
    }

    /// Keeps a vote for a block that is not known yet.
    pub fn park(&mut self, vote: FinalityVote) {
        if self.parked.iter().any(|v| v.id() == vote.id()) {
            return;
        }
        self.parked.push(vote);
        while self.parked.len() > 512 {
            self.parked.remove(0);
        }
    }

    /// Takes back the parked votes for a block that just arrived.
    pub fn take_parked(&mut self, block: &Hash32) -> Vec<FinalityVote> {
        let mut out = Vec::new();
        self.parked.retain(|v| {
            if v.block == *block {
                out.push(v.clone());
                false
            } else {
                true
            }
        });
        out
    }

    /// Forgets votes for heights that no longer matter.
    pub fn prune(&mut self, height: u64) {
        let floor = height.saturating_sub(MAX_VOTE_DISTANCE);
        self.votes.retain(|(h, _), _| *h >= floor);
        self.voted.retain(|(h, _), _| *h >= floor);
        self.parked.retain(|v| v.height >= floor);
        if self.banned.len() > 1_000 {
            self.banned.drain(..500);
        }
    }

    /// (blocks with votes, parked votes, banned keys) — for diagnostics.
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.votes.len(), self.parked.len(), self.banned.len())
    }

    /// Votes already collected for a block, to send to a peer that asks.
    pub fn votes_for(&self, height: u64, block: &Hash32) -> Vec<FinalityVote> {
        self.votes.get(&(height, *block)).map(|m| m.values().cloned().collect()).unwrap_or_default()
    }
}

/// Loads the node's voting key, creating it on first use.
pub fn load_key(data_dir: &Path) -> Result<SecretKey> {
    let path = data_dir.join("finality.key");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let bytes = hex::decode(text.trim()).context("finality.key is not hexadecimal")?;
        let seed: [u8; 32] = bytes.as_slice().try_into().context("finality.key must be 32 bytes")?;
        return Ok(SecretKey::from_bytes(&seed));
    }
    let mut seed = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut seed);
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(&path, hex::encode(seed))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(SecretKey::from_bytes(&seed))
}

/// How many of the window's blocks this key mined.
pub fn own_weight(window: &[[u8; 32]], key: &SecretKey) -> u64 {
    weight_of(window, &[key.public_key()])
}

/// Blocks of the window (for `votes_needed`), capped at [`FINALITY_WINDOW`].
pub fn window_len(height: u64) -> u64 {
    height.min(FINALITY_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> SecretKey {
        SecretKey::from_bytes(&[n; 32])
    }

    #[test]
    fn finalises_with_two_thirds_of_a_diverse_window() {
        let chain_id = 5;
        let block = Hash32([1; 32]);
        let keys: Vec<SecretKey> = (1..=4).map(key).collect();
        // A full window mined by four nodes, one quarter each.
        let window: Vec<[u8; 32]> = (0..FINALITY_WINDOW).map(|i| keys[(i % 4) as usize].public_key()).collect();
        let mut t = Tally::new();
        assert_eq!(t.add(FinalityVote::sign(chain_id, 300, block, &keys[0]), chain_id, 300, &window, FINALITY_WINDOW), Ok(false));
        assert_eq!(t.add(FinalityVote::sign(chain_id, 300, block, &keys[1]), chain_id, 300, &window, FINALITY_WINDOW), Ok(false));
        assert!(t.finalized().is_none(), "half the window is not enough");
        assert_eq!(t.add(FinalityVote::sign(chain_id, 300, block, &keys[2]), chain_id, 300, &window, FINALITY_WINDOW), Ok(true));
        assert_eq!(t.finalized(), Some((300, block)));

        // Two different blocks signed by the same key: that key is dropped.
        let mut t2 = Tally::new();
        assert_eq!(t2.add(FinalityVote::sign(chain_id, 301, block, &keys[0]), chain_id, 301, &window, FINALITY_WINDOW), Ok(false));
        let other = Hash32([2; 32]);
        assert_eq!(
            t2.add(FinalityVote::sign(chain_id, 301, other, &keys[0]), chain_id, 301, &window, FINALITY_WINDOW),
            Err(VoteError::Equivocation)
        );
        assert_eq!(t2.weight(301, &block, &window), 0, "the banned key stops counting");

        // Outsiders, bad signatures and old votes are refused.
        let outsider = key(9);
        assert_eq!(
            t2.add(FinalityVote::sign(chain_id, 301, block, &outsider), chain_id, 301, &window, FINALITY_WINDOW),
            Err(VoteError::NotAVoter)
        );
        let mut bad = FinalityVote::sign(chain_id, 301, block, &keys[1]);
        bad.signature[0] ^= 1;
        assert_eq!(t2.add(bad, chain_id, 301, &window, FINALITY_WINDOW), Err(VoteError::BadSignature));
        assert_eq!(
            t2.add(FinalityVote::sign(chain_id, 1, block, &keys[1]), chain_id, 500, &window, FINALITY_WINDOW),
            Err(VoteError::TooFar)
        );
    }

    #[test]
    fn a_single_miner_never_finalises() {
        let chain_id = 5;
        let solo = key(1);
        let window: Vec<[u8; 32]> = (0..FINALITY_WINDOW).map(|_| solo.public_key()).collect();
        let mut t = Tally::new();
        let block = Hash32([7; 32]);
        assert_eq!(t.add(FinalityVote::sign(chain_id, 300, block, &solo), chain_id, 300, &window, FINALITY_WINDOW), Ok(false));
        assert!(t.finalized().is_none(), "one miner must not finalise its own chain");
    }
}
