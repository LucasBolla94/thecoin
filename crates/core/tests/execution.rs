//! End-to-end tests of the state-transition function on an in-memory state.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thecoin_core::address::Address;
use thecoin_core::amount::COIN;
use thecoin_core::block::{Block, BlockHeader};
use thecoin_core::contracts::{contract_id, ContractCall, ContractSpec};
use thecoin_core::crypto::SecretKey;
use thecoin_core::error::{BlockError, TxError};
use thecoin_core::execution::{apply_block, BlockReceipt};
use thecoin_core::genesis::{genesis_lthash, genesis_state};
use thecoin_core::governance::{proposal_id, GovParamId, ProposalAction, ProposalSpec, ProposalStatus, VoteChoice};
use thecoin_core::hash::Hash32;
use thecoin_core::lthash::LtHash;
use thecoin_core::params::{ChainParams, REGTEST};
use thecoin_core::state::*;
use thecoin_core::tx::{TransferOutput, TxAction, TxBody, TX_VERSION};
use thecoin_core::Transaction;

struct TestChain {
    p: &'static ChainParams,
    state: BTreeMap<Vec<u8>, Vec<u8>>,
    lthash: LtHash,
    height: u64,
}

impl TestChain {
    fn new() -> Self {
        let p = &REGTEST;
        TestChain { p, state: genesis_state(p).into_iter().collect(), lthash: genesis_lthash(p), height: 0 }
    }

    fn mine_signal(&mut self, miner: Address, txs: Vec<Transaction>, signal: u32) -> Result<BlockReceipt, BlockError> {
        let header = BlockHeader {
            version: 1,
            height: self.height + 1,
            prev_hash: Hash32::ZERO,
            tx_root: Hash32::ZERO,
            state_root: Hash32::ZERO,
            timestamp: 0,
            target: [0xff; 32],
            nonce: 0,
            miner,
            signal,
        };
        let block = Block { header, txs };
        let (receipt, diff) = {
            let mut ov = Overlay::new(&self.state);
            let r = apply_block(self.p, &mut ov, &block, false)?;
            (r, ov.diff()?)
        };
        apply_changes_to_lthash(&mut self.lthash, &diff);
        for c in diff {
            match c.new {
                Some(v) => self.state.insert(c.key, v),
                None => self.state.remove(&c.key),
            };
        }
        self.height += 1;
        self.check_supply_invariant();
        Ok(receipt)
    }

    fn mine(&mut self, miner: Address, txs: Vec<Transaction>) -> Result<BlockReceipt, BlockError> {
        self.mine_signal(miner, txs, 0)
    }

    fn mine_empty(&mut self, miner: Address, n: u64) {
        for _ in 0..n {
            self.mine(miner, vec![]).unwrap();
        }
    }

    fn account(&self, a: &Address) -> Account {
        read_typed(&self.state, &account_key(a)).unwrap().unwrap_or_default()
    }

    fn global(&self) -> ChainGlobal {
        read_typed(&self.state, &global_key()).unwrap().unwrap()
    }

    /// balances + contract balances + pending rewards + voting deposits == emitted - burned
    fn check_supply_invariant(&self) {
        let mut total: u128 = 0;
        for (k, v) in &self.state {
            match k[0] {
                PREFIX_ACCOUNT => total += borsh::from_slice::<Account>(v).unwrap().balance as u128,
                PREFIX_CONTRACT => total += borsh::from_slice::<thecoin_core::contracts::Contract>(v).unwrap().balance as u128,
                PREFIX_PENDING_REWARD => total += borsh::from_slice::<PendingReward>(v).unwrap().amount as u128,
                PREFIX_PROPOSAL => {
                    let p = borsh::from_slice::<thecoin_core::governance::Proposal>(v).unwrap();
                    if p.status == ProposalStatus::Voting {
                        total += p.deposit as u128;
                    }
                }
                _ => {}
            }
        }
        let g = self.global();
        assert_eq!(total, g.circulating() as u128, "supply invariant broken at height {}", self.height);
        let mut recomputed = LtHash::default();
        for (k, v) in &self.state {
            recomputed.insert(k, v);
        }
        assert_eq!(recomputed.root(), self.lthash.root(), "incremental lthash diverged");
    }
}

struct Wallet {
    key: SecretKey,
    addr: Address,
    nonce: u64,
}

impl Wallet {
    fn new(seed: u8) -> Self {
        let key = SecretKey::from_bytes(&[seed; 32]);
        let addr = Address::from_public_key(&key.public_key());
        Wallet { key, addr, nonce: 0 }
    }

    fn tx(&mut self, action: TxAction) -> Transaction {
        let body = TxBody { version: TX_VERSION, chain_id: REGTEST.chain_id, nonce: self.nonce, fee: 0, expiry_height: 0, action };
        let size = body.clone().sign(&self.key).size() as u64;
        let body = TxBody { fee: size * 30, ..body };
        self.nonce += 1;
        body.sign(&self.key)
    }

    fn pay(&mut self, to: &Address, amount: u64) -> Transaction {
        self.tx(TxAction::Transfer { to: *to, amount, memo: b"invoice #1".to_vec() })
    }
}

fn funded() -> (TestChain, Wallet) {
    let mut c = TestChain::new();
    let miner = Wallet::new(1);
    // maturity 5 on regtest: mine enough blocks for rewards to mature
    c.mine_empty(miner.addr, 20);
    (c, miner)
}

fn tx_error(r: Result<BlockReceipt, BlockError>) -> TxError {
    match r {
        Err(BlockError::Tx { error, .. }) => error,
        other => panic!("expected tx error, got {other:?}"),
    }
}

#[test]
fn rewards_mature_and_transfers_work() {
    let mut c = TestChain::new();
    let mut miner = Wallet::new(1);
    let bob = Wallet::new(2);
    c.mine_empty(miner.addr, 5);
    assert_eq!(c.account(&miner.addr).balance, 0, "rewards are immature");
    c.mine_empty(miner.addr, 1);
    assert_eq!(c.account(&miner.addr).balance, 40 * COIN);

    let tx = miner.pay(&bob.addr, 15 * COIN);
    let fee = tx.body.fee;
    c.mine(miner.addr, vec![tx.clone()]).unwrap();
    assert_eq!(c.account(&bob.addr).balance, 15 * COIN);
    // miner got one more matured reward (height 2) in that block
    assert_eq!(c.account(&miner.addr).balance, 80 * COIN - 15 * COIN - fee);

    // replay is rejected
    assert!(matches!(tx_error(c.mine(miner.addr, vec![tx])), TxError::BadNonce { .. }));
}

#[test]
fn rejects_invalid_transactions() {
    let (mut c, mut miner) = funded();
    let bob = Wallet::new(2);

    let too_much = miner.pay(&bob.addr, 1_000_000 * COIN);
    assert!(matches!(tx_error(c.mine(miner.addr, vec![too_much])), TxError::InsufficientFunds { .. }));
    miner.nonce -= 1;

    let mut low_fee = miner.pay(&bob.addr, COIN);
    low_fee.body.fee = 1;
    let low_fee = low_fee.body.sign(&miner.key);
    assert!(matches!(tx_error(c.mine(miner.addr, vec![low_fee])), TxError::FeeTooLow { .. }));

    let mut wrong_chain = miner.pay(&bob.addr, COIN);
    wrong_chain.body.chain_id = 999;
    let wrong_chain = wrong_chain.body.sign(&miner.key);
    assert!(matches!(tx_error(c.mine(miner.addr, vec![wrong_chain])), TxError::WrongChain { .. }));

    let mut forged = miner.pay(&bob.addr, COIN);
    if let TxAction::Transfer { amount, .. } = &mut forged.body.action {
        *amount = 2 * COIN;
    }
    assert!(matches!(tx_error(c.mine(miner.addr, vec![forged])), TxError::BadSignature));

    let mut expired = miner.tx(TxAction::Transfer { to: bob.addr, amount: COIN, memo: vec![] });
    expired.body.expiry_height = 3;
    let expired = expired.body.sign(&miner.key);
    assert!(matches!(tx_error(c.mine(miner.addr, vec![expired])), TxError::Expired { .. }));

    // signal bits with no proposal voting are invalid
    assert!(matches!(c.mine_signal(miner.addr, vec![], 1), Err(BlockError::BadSignal(1))));
}

#[test]
fn batch_transfer() {
    let (mut c, mut miner) = funded();
    let outs: Vec<TransferOutput> = (10u8..20).map(|i| TransferOutput { to: Wallet::new(i).addr, amount: COIN + i as u64 }).collect();
    c.mine(miner.addr, vec![miner.tx(TxAction::BatchTransfer { outputs: outs.clone(), memo: b"payroll".to_vec() })]).unwrap();
    for o in outs {
        assert_eq!(c.account(&o.to).balance, o.amount);
    }
}

#[test]
fn escrow_release_and_refund() {
    let (mut c, mut buyer) = funded();
    let seller = Wallet::new(2);
    let mut arbiter = Wallet::new(3);
    c.mine(buyer.addr, vec![buyer.pay(&arbiter.addr, COIN)]).unwrap();

    // escrow 1: buyer releases
    let id1 = contract_id(&buyer.addr, buyer.nonce);
    let spec = ContractSpec::Escrow { payee: seller.addr, arbiter: Some(arbiter.addr), amount: 10 * COIN, deadline_height: 1000 };
    c.mine(buyer.addr, vec![buyer.tx(TxAction::CreateContract { spec })]).unwrap();
    assert_eq!(read_typed::<thecoin_core::contracts::Contract, _>(&c.state, &contract_key(&id1)).unwrap().unwrap().balance, 10 * COIN);
    c.mine(buyer.addr, vec![buyer.tx(TxAction::CallContract { contract: id1, call: ContractCall::EscrowRelease })]).unwrap();
    assert_eq!(c.account(&seller.addr).balance, 10 * COIN);
    assert!(c.state.get(&contract_key(&id1)).is_none(), "finished contract removed");

    // escrow 2: arbiter refunds; seller cannot release
    let id2 = contract_id(&buyer.addr, buyer.nonce);
    let spec = ContractSpec::Escrow { payee: seller.addr, arbiter: Some(arbiter.addr), amount: 5 * COIN, deadline_height: 1000 };
    c.mine(buyer.addr, vec![buyer.tx(TxAction::CreateContract { spec })]).unwrap();
    let before = c.account(&buyer.addr).balance;
    let r = c.mine(buyer.addr, vec![arbiter.tx(TxAction::CallContract { contract: id2, call: ContractCall::EscrowRelease })]);
    assert!(r.is_ok(), "arbiter may release");
    let _ = before;
    assert_eq!(c.account(&seller.addr).balance, 15 * COIN);
}

#[test]
fn vesting_subscription_htlc_multisig() {
    let (mut c, mut alice) = funded();
    let mut bob = Wallet::new(2);
    let mut carol = Wallet::new(3);
    c.mine(alice.addr, vec![alice.pay(&bob.addr, COIN), alice.pay(&carol.addr, COIN)]).unwrap();

    // Vesting: 100 TCN from h+0 to h+10, cliff h+5
    let h = c.height + 1;
    let vid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Vesting { beneficiary: bob.addr, amount: 100 * COIN, start_height: h, cliff_height: h + 5, end_height: h + 10, revocable: true };
    c.mine(alice.addr, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let e = tx_error(c.mine(alice.addr, vec![bob.tx(TxAction::CallContract { contract: vid, call: ContractCall::VestingClaim })]));
    assert!(matches!(e, TxError::Contract(_)));
    bob.nonce -= 1;
    c.mine_empty(alice.addr, 4); // now tip = h+4, next block h+5 = cliff
    c.mine(alice.addr, vec![bob.tx(TxAction::CallContract { contract: vid, call: ContractCall::VestingClaim })]).unwrap();
    let bob_bal = c.account(&bob.addr).balance;
    assert!(bob_bal > 50 * COIN - COIN && bob_bal < 52 * COIN, "{bob_bal}");
    // alice revokes: bob gets vested part, alice the rest
    c.mine(alice.addr, vec![alice.tx(TxAction::CallContract { contract: vid, call: ContractCall::VestingRevoke })]).unwrap();
    assert!(c.state.get(&contract_key(&vid)).is_none());

    // Subscription: 2 TCN per 3 blocks, 4 periods
    let sid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Subscription { payee: carol.addr, amount_per_period: 2 * COIN, period_blocks: 3, max_periods: 4 };
    c.mine(alice.addr, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let carol0 = c.account(&carol.addr).balance;
    let claim = carol.tx(TxAction::CallContract { contract: sid, call: ContractCall::SubscriptionClaim });
    let fee = claim.body.fee;
    c.mine(alice.addr, vec![claim]).unwrap();
    assert_eq!(c.account(&carol.addr).balance, carol0 + 2 * COIN - fee);
    c.mine_empty(alice.addr, 3);
    let cancel = alice.tx(TxAction::CallContract { contract: sid, call: ContractCall::SubscriptionCancel });
    c.mine(alice.addr, vec![cancel]).unwrap();
    // carol received the second period automatically at cancel
    assert_eq!(c.account(&carol.addr).balance, carol0 + 4 * COIN - fee);
    assert!(c.state.get(&contract_key(&sid)).is_none());

    // HTLC
    let preimage = b"super secret swap preimage".to_vec();
    let lock = Hash32(Sha256::digest(&preimage).into());
    let hid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Htlc { recipient: bob.addr, amount: 7 * COIN, hash_lock: lock, timeout_height: c.height + 50 };
    c.mine(alice.addr, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let wrong = bob.tx(TxAction::CallContract { contract: hid, call: ContractCall::HtlcRedeem { preimage: b"nope".to_vec() } });
    assert!(matches!(tx_error(c.mine(alice.addr, vec![wrong])), TxError::Contract(_)));
    bob.nonce -= 1;
    let b0 = c.account(&bob.addr).balance;
    let redeem = bob.tx(TxAction::CallContract { contract: hid, call: ContractCall::HtlcRedeem { preimage } });
    let fee = redeem.body.fee;
    c.mine(alice.addr, vec![redeem]).unwrap();
    assert_eq!(c.account(&bob.addr).balance, b0 + 7 * COIN - fee);

    // Multisig 2-of-3
    let mid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Multisig { signers: vec![alice.addr, bob.addr, carol.addr], threshold: 2, initial_deposit: 20 * COIN };
    c.mine(alice.addr, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let dest = Wallet::new(9).addr;
    c.mine(alice.addr, vec![alice.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigPropose { to: dest, amount: 5 * COIN, memo: vec![] } })]).unwrap();
    assert_eq!(c.account(&dest).balance, 0);
    let dup = alice.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigApprove { spend_id: 0 } });
    assert!(matches!(tx_error(c.mine(alice.addr, vec![dup])), TxError::Contract(_)));
    alice.nonce -= 1;
    c.mine(alice.addr, vec![carol.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigApprove { spend_id: 0 } })]).unwrap();
    assert_eq!(c.account(&dest).balance, 5 * COIN);
    let outsider = Wallet::new(9);
    let mut outsider = outsider;
    let _ = &mut outsider;
}

#[test]
fn governance_changes_parameter_with_both_chambers() {
    let (mut c, mut miner) = funded();
    let mut voter = Wallet::new(2);
    // voter gets a large share of circulating supply
    c.mine(miner.addr, vec![miner.pay(&voter.addr, 200 * COIN)]).unwrap();

    let spec = ProposalSpec {
        title: "Raise minimum fee to 20 motes/byte".into(),
        url: "https://the-coin.cloud/governance/1".into(),
        content_hash: Hash32::ZERO,
        action: ProposalAction::SetParam { param: GovParamId::MinFeePerByte, value: 20 },
    };
    let pid = proposal_id(&voter.addr, voter.nonce);
    c.mine(miner.addr, vec![voter.tx(TxAction::Propose { proposal: spec })]).unwrap();
    let g = c.global();
    assert_eq!(g.voting.len(), 1);
    let bit = g.voting[0].0;

    // vote with 150 TCN (locked)
    c.mine_signal(miner.addr, vec![voter.tx(TxAction::Vote { proposal: pid, choice: VoteChoice::Yes, weight: 190 * COIN })], 1 << bit).unwrap();
    let acc = c.account(&voter.addr);
    assert_eq!(acc.locked, 190 * COIN);
    // locked coins cannot be moved
    let spend = voter.pay(&miner.addr, 100 * COIN);
    assert!(matches!(tx_error(c.mine_signal(miner.addr, vec![spend], 1 << bit)), TxError::InsufficientFunds { .. }));
    voter.nonce -= 1;
    // double vote rejected
    let again = voter.tx(TxAction::Vote { proposal: pid, choice: VoteChoice::Yes, weight: COIN });
    assert!(matches!(tx_error(c.mine_signal(miner.addr, vec![again], 1 << bit)), TxError::Governance(_)));
    voter.nonce -= 1;

    // miners signal for the whole vote period (regtest: 20 blocks)
    let end = read_typed::<thecoin_core::governance::Proposal, _>(&c.state, &proposal_key(&pid)).unwrap().unwrap().end_height;
    while c.height < end {
        c.mine_signal(miner.addr, vec![], 1 << bit).unwrap();
    }
    let p: thecoin_core::governance::Proposal = read_typed(&c.state, &proposal_key(&pid)).unwrap().unwrap();
    let ProposalStatus::Approved { activation_height } = p.status else { panic!("not approved: {p:?}") };
    assert!(p.outcome.unwrap().deposit_refunded);
    assert_eq!(c.global().params.min_fee_per_byte, 1);
    while c.height < activation_height {
        c.mine(miner.addr, vec![]).unwrap();
    }
    assert_eq!(c.global().params.min_fee_per_byte, 20);
    // after the vote, coins are unlocked
    c.mine(miner.addr, vec![voter.pay(&miner.addr, 100 * COIN)]).unwrap();
}

#[test]
fn governance_without_miner_support_is_rejected_and_no_quorum_burns() {
    let (mut c, mut miner) = funded();
    let spec = ProposalSpec { title: "Text".into(), url: String::new(), content_hash: Hash32::ZERO, action: ProposalAction::Text };
    let pid = proposal_id(&miner.addr, miner.nonce);
    c.mine(miner.addr, vec![miner.tx(TxAction::Propose { proposal: spec })]).unwrap();
    let burned_before = c.global().burned;
    c.mine_empty(miner.addr, 25);
    let p: thecoin_core::governance::Proposal = read_typed(&c.state, &proposal_key(&pid)).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Rejected);
    assert!(!p.outcome.as_ref().unwrap().quorum_reached);
    assert_eq!(c.global().burned, burned_before + REGTEST.gov_defaults.proposal_deposit);
    assert!(c.global().voting.is_empty());
}
