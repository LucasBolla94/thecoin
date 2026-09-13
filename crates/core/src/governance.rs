//! **On-chain governance** — bicameral voting.
//!
//! A change to the network passes only if *both* chambers approve it:
//!
//! 1. **Holders** vote with coins. `Vote { weight }` locks `weight` coins of the
//!    voter until the end of the vote (they cannot be moved, so they cannot be
//!    reused by another address). Needs quorum (share of circulating supply)
//!    and a super-majority of `yes / (yes + no)`.
//! 2. **Miners** signal in block headers (`signal` bit assigned to the
//!    proposal). Needs a minimum share of the blocks mined during the vote.
//!
//! Neither large holders nor large miners can change the rules alone.
//!
//! Lifecycle: `Voting` → (at `end_height`) `Approved` or `Rejected` →
//! (at `activation_height`) `Activated`. The proposal deposit is refunded when
//! quorum is reached and burned otherwise (anti-spam).
//!
//! What governance can change: the parameters in [`GovParams`], always inside
//! the hard bounds of [`GovBounds`]. It can also approve text proposals and
//! software upgrades (recorded on-chain and shown by nodes/wallets/site).
//! **It can never change the 50M supply cap, the emission schedule or the PoW
//! algorithm** — those are hard-coded.

use crate::address::Address;
use crate::error::{BlockError, TxError};
use crate::hash::{tagged_hash, tags, Hash32};
use crate::state::{proposal_key, vote_key, Overlay, StateReader};
use borsh::{BorshDeserialize, BorshSerialize};

pub const MAX_TITLE_BYTES: usize = 120;
pub const MAX_URL_BYTES: usize = 256;
pub const MAX_VERSION_BYTES: usize = 32;
/// One signalling bit per concurrently voting proposal.
pub const MAX_CONCURRENT_PROPOSALS: usize = 32;

macro_rules! gov_params {
    ($( $(#[doc = $doc:literal])* $field:ident => $variant:ident ),+ $(,)?) => {
        /// Parameters adjustable by governance.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, serde::Serialize, serde::Deserialize)]
        pub struct GovParams {
            $( $(#[doc = $doc])* pub $field: u64, )+
        }

        /// Inclusive `(min, max)` bounds for each [`GovParams`] field.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
        pub struct GovBounds {
            $( pub $field: (u64, u64), )+
        }

        /// Identifies a governable parameter. **Variant order is consensus-critical.**
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum GovParamId {
            $( $variant, )+
        }

        impl GovParamId {
            pub const ALL: &'static [GovParamId] = &[ $( GovParamId::$variant, )+ ];

            pub fn name(self) -> &'static str {
                match self {
                    $( GovParamId::$variant => stringify!($field), )+
                }
            }
        }

        impl GovParams {
            pub fn get(&self, id: GovParamId) -> u64 {
                match id {
                    $( GovParamId::$variant => self.$field, )+
                }
            }

            pub fn set(&mut self, id: GovParamId, v: u64) {
                match id {
                    $( GovParamId::$variant => self.$field = v, )+
                }
            }
        }

        impl GovBounds {
            pub fn get(&self, id: GovParamId) -> (u64, u64) {
                match id {
                    $( GovParamId::$variant => self.$field, )+
                }
            }
        }
    };
}

gov_params! {
    /// Maximum serialized block size in bytes.
    max_block_bytes => MaxBlockBytes,
    /// Maximum total `max_fuel` of the contract transactions in a block.
    max_block_fuel => MaxBlockFuel,
    /// Base fee of every transaction (motes).
    base_fee => BaseFee,
    /// Fee per 1 000 bytes of serialized transaction (motes).
    fee_per_kb => FeePerKb,
    /// Fee per 1 000 units of contract fuel reserved by `max_fuel` (motes).
    fee_per_kfuel => FeePerKfuel,
    /// Refundable deposit per 1 000 bytes of contract state (motes).
    storage_deposit_per_kb => StorageDepositPerKb,
    /// Deposit required to open a proposal (motes).
    proposal_deposit => ProposalDeposit,
    /// Voting duration in blocks.
    vote_period => VotePeriod,
    /// Quorum: votes cast / circulating supply, in basis points (1/100 %).
    quorum_bp => QuorumBp,
    /// Holder approval: yes / (yes + no), in basis points.
    approval_bp => ApprovalBp,
    /// Miner approval: signalling blocks / blocks in the vote, in basis points.
    miner_approval_bp => MinerApprovalBp,
    /// Blocks between approval and activation (time for nodes to upgrade).
    activation_delay => ActivationDelay,
}

impl GovParamId {
    pub fn from_name(s: &str) -> Option<GovParamId> {
        GovParamId::ALL.iter().copied().find(|p| p.name() == s)
    }
}

impl GovParams {
    pub fn validate(&self, b: &GovBounds) -> Result<(), String> {
        for id in GovParamId::ALL {
            let (lo, hi) = b.get(*id);
            let v = self.get(*id);
            if v < lo || v > hi {
                return Err(format!("{} = {v} outside bounds [{lo}, {hi}]", id.name()));
            }
        }
        Ok(())
    }
}

/// What an approved proposal does. **Variant order is consensus-critical.**
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ProposalAction {
    /// Non-binding decision recorded on-chain (roadmap, community decisions).
    Text,
    /// Changes one governance parameter at activation.
    SetParam { param: GovParamId, value: u64 },
    /// Approves a software release (hard/soft fork coordination). Nodes show a
    /// warning when running an older version after activation.
    SoftwareUpgrade { version: String, release_hash: Hash32 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProposalSpec {
    pub title: String,
    /// Link to the full text, e.g. `https://the-coin.cloud/governance/...`.
    pub url: String,
    /// BLAKE3/SHA-256 hash of the full text so it cannot be edited later.
    pub content_hash: Hash32,
    pub action: ProposalAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoteChoice {
    Yes,
    No,
    Abstain,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ProposalStatus {
    Voting,
    Approved { activation_height: u64 },
    Rejected,
    Activated { height: u64 },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, serde::Serialize, serde::Deserialize)]
pub struct Tally {
    pub yes: u64,
    pub no: u64,
    pub abstain: u64,
    pub voters: u64,
    pub miner_yes_blocks: u64,
    pub miner_total_blocks: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Proposal {
    pub id: Hash32,
    pub proposer: Address,
    pub spec: ProposalSpec,
    pub deposit: u64,
    pub created_height: u64,
    /// Last block in which votes and signals count.
    pub end_height: u64,
    pub signal_bit: u8,
    pub tally: Tally,
    pub status: ProposalStatus,
    /// Result details, filled at `end_height`.
    pub outcome: Option<Outcome>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, serde::Serialize, serde::Deserialize)]
pub struct Outcome {
    pub quorum_reached: bool,
    pub holders_approved: bool,
    pub miners_approved: bool,
    pub circulating_at_end: u64,
    pub deposit_refunded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct VoteRecord {
    pub choice: VoteChoice,
    pub weight: u64,
    pub height: u64,
}

pub fn proposal_id(proposer: &Address, nonce: u64) -> Hash32 {
    tagged_hash(tags::PROPOSAL_ID, &[&proposer.0, &nonce.to_le_bytes()])
}

impl ProposalSpec {
    pub fn validate_static(&self) -> Result<(), TxError> {
        let err = |m: String| Err(TxError::Governance(m));
        if self.title.trim().is_empty() || self.title.len() > MAX_TITLE_BYTES {
            return err(format!("title must be 1..={MAX_TITLE_BYTES} bytes"));
        }
        if self.url.len() > MAX_URL_BYTES {
            return err(format!("url must be <= {MAX_URL_BYTES} bytes"));
        }
        if let ProposalAction::SoftwareUpgrade { version, .. } = &self.action {
            if version.is_empty() || version.len() > MAX_VERSION_BYTES {
                return err("version must be 1..=32 bytes".into());
            }
        }
        Ok(())
    }
}

/// Bits currently assigned to voting proposals.
pub fn assigned_signal_mask(voting: &[(u8, Hash32)]) -> u32 {
    voting.iter().fold(0u32, |m, (bit, _)| m | (1u32 << bit))
}

/// Creates a proposal. Deposit and fee have already been debited.
pub(crate) fn propose<R: StateReader + ?Sized>(
    state: &mut Overlay<'_, R>,
    bounds: &GovBounds,
    height: u64,
    proposer: Address,
    nonce: u64,
    spec: &ProposalSpec,
    deposit: u64,
) -> Result<Hash32, TxError> {
    let mut g = state.global()?;
    if let ProposalAction::SetParam { param, value } = &spec.action {
        let (lo, hi) = bounds.get(*param);
        if *value < lo || *value > hi {
            return Err(TxError::Governance(format!("{} must be within [{lo}, {hi}]", param.name())));
        }
    }
    if g.voting.len() >= MAX_CONCURRENT_PROPOSALS {
        return Err(TxError::Governance("too many proposals in voting; wait for one to finish".into()));
    }
    let used = assigned_signal_mask(&g.voting);
    let bit = (0u8..32).find(|b| used & (1 << b) == 0).expect("fewer than 32 proposals voting");
    let id = proposal_id(&proposer, nonce);
    if state.proposal(&id)?.is_some() {
        return Err(TxError::Governance("proposal id collision".into()));
    }
    let end_height = height.checked_add(g.params.vote_period).ok_or(TxError::Overflow)?;
    let p = Proposal {
        id,
        proposer,
        spec: spec.clone(),
        deposit,
        created_height: height,
        end_height,
        signal_bit: bit,
        tally: Tally::default(),
        status: ProposalStatus::Voting,
        outcome: None,
    };
    state.put(proposal_key(&id), &p);
    g.voting.push((bit, id));
    g.proposal_count += 1;
    state.put_global(&g);
    Ok(id)
}

/// Records a vote and locks the voter's coins. Fee has already been debited.
pub(crate) fn vote<R: StateReader + ?Sized>(
    state: &mut Overlay<'_, R>,
    height: u64,
    voter: Address,
    id: &Hash32,
    choice: VoteChoice,
    weight: u64,
) -> Result<(), TxError> {
    if weight == 0 {
        return Err(TxError::ZeroAmount);
    }
    let mut p = state.proposal(id)?.ok_or_else(|| TxError::Governance("unknown proposal".into()))?;
    if p.status != ProposalStatus::Voting || height > p.end_height {
        return Err(TxError::Governance("proposal is not open for voting".into()));
    }
    if state.vote(id, &voter)?.is_some() {
        return Err(TxError::Governance("address already voted on this proposal".into()));
    }
    let mut acc = state.account(&voter)?;
    // Coins already locked by other votes can vote again (different proposal);
    // the lock becomes max(previous lock, weight) until the latest end.
    if weight > acc.balance {
        return Err(TxError::InsufficientFunds { needed: weight, available: acc.balance });
    }
    let active_lock = if height <= acc.locked_until { acc.locked } else { 0 };
    acc.locked = active_lock.max(weight);
    acc.locked_until = if active_lock > 0 { acc.locked_until.max(p.end_height) } else { p.end_height };
    state.put_account(&voter, &acc);

    match choice {
        VoteChoice::Yes => p.tally.yes = p.tally.yes.checked_add(weight).ok_or(TxError::Overflow)?,
        VoteChoice::No => p.tally.no = p.tally.no.checked_add(weight).ok_or(TxError::Overflow)?,
        VoteChoice::Abstain => p.tally.abstain = p.tally.abstain.checked_add(weight).ok_or(TxError::Overflow)?,
    }
    p.tally.voters += 1;
    state.put(proposal_key(id), &p);
    state.put(vote_key(id, &voter), &VoteRecord { choice, weight, height });
    Ok(())
}

/// End-of-block governance processing: counts miner signals, closes votes that
/// end at this height and activates approved proposals.
pub(crate) fn end_block<R: StateReader + ?Sized>(
    state: &mut Overlay<'_, R>,
    bounds: &GovBounds,
    height: u64,
    signal: u32,
) -> Result<(), BlockError> {
    let mut g = state.global()?;
    let gerr = |m: &str| BlockError::Governance(m.to_string());

    // 1. Miner signals and vote closing.
    let mut still_voting = Vec::with_capacity(g.voting.len());
    for (bit, id) in g.voting.clone() {
        let mut p = state.proposal(&id)?.ok_or_else(|| gerr("voting proposal missing"))?;
        if height > p.created_height {
            p.tally.miner_total_blocks += 1;
            if signal & (1u32 << bit) != 0 {
                p.tally.miner_yes_blocks += 1;
            }
        }
        if height >= p.end_height {
            let votes = p.tally.yes as u128 + p.tally.no as u128 + p.tally.abstain as u128;
            let circulating = g.circulating();
            let quorum_reached = votes * 10_000 >= g.params.quorum_bp as u128 * circulating as u128 && votes > 0;
            let holders_approved = p.tally.yes > 0
                && p.tally.yes as u128 * 10_000 >= g.params.approval_bp as u128 * (p.tally.yes as u128 + p.tally.no as u128);
            let miners_approved =
                p.tally.miner_yes_blocks as u128 * 10_000 >= g.params.miner_approval_bp as u128 * p.tally.miner_total_blocks as u128;
            if quorum_reached {
                let mut acc = state.account(&p.proposer)?;
                acc.balance = acc.balance.checked_add(p.deposit).ok_or_else(|| gerr("overflow"))?;
                state.put_account(&p.proposer, &acc);
            } else {
                g.burned = g.burned.checked_add(p.deposit).ok_or_else(|| gerr("overflow"))?;
            }
            p.outcome = Some(Outcome {
                quorum_reached,
                holders_approved,
                miners_approved,
                circulating_at_end: circulating,
                deposit_refunded: quorum_reached,
            });
            if quorum_reached && holders_approved && miners_approved {
                let activation_height = height + g.params.activation_delay;
                p.status = ProposalStatus::Approved { activation_height };
                g.pending_activations.push((activation_height, id));
            } else {
                p.status = ProposalStatus::Rejected;
            }
        } else {
            still_voting.push((bit, id));
        }
        state.put(proposal_key(&id), &p);
    }
    g.voting = still_voting;

    // 2. Activations.
    g.pending_activations.sort();
    let mut remaining = Vec::new();
    for (act_height, id) in g.pending_activations.clone() {
        if act_height > height {
            remaining.push((act_height, id));
            continue;
        }
        let mut p = state.proposal(&id)?.ok_or_else(|| gerr("approved proposal missing"))?;
        if let ProposalAction::SetParam { param, value } = &p.spec.action {
            let (lo, hi) = bounds.get(*param);
            if *value >= lo && *value <= hi {
                g.params.set(*param, *value);
            }
        }
        p.status = ProposalStatus::Activated { height };
        state.put(proposal_key(&id), &p);
    }
    g.pending_activations = remaining;
    state.put_global(&g);
    Ok(())
}
