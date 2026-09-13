//! End-to-end tests of the state-transition function on an in-memory state.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thecoin_core::address::Address;
use thecoin_core::amount::COIN;
use thecoin_core::block::{Block, BlockHeader};
use thecoin_core::contracts::{contract_id, storage_deposit, ContractCall, ContractSpec};
use thecoin_core::crypto::SecretKey;
use thecoin_core::error::{BlockError, TxError};
use thecoin_core::execution::{apply_block, required_fee, BlockReceipt};
use thecoin_core::genesis::{genesis_lthash, genesis_state};
use thecoin_core::governance::{proposal_id, GovParamId, ProposalAction, ProposalSpec, ProposalStatus, VoteChoice};
use thecoin_core::hash::Hash32;
use thecoin_core::lthash::LtHash;
use thecoin_core::params::{ChainParams, REGTEST};
use thecoin_core::programs::{program_address, view};
use thecoin_core::state::*;
use thecoin_core::tccl::Value;
use thecoin_core::tx::{TransferOutput, TxAction, TxBody, FLAG_REPLACEABLE, TX_VERSION};
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

    fn set_global(&mut self, g: &ChainGlobal) {
        let key = global_key();
        let old = self.state.get(&key).cloned().unwrap();
        let new = borsh::to_vec(g).unwrap();
        self.lthash.apply_change(&key, Some(&old), Some(&new));
        self.state.insert(key, new);
    }

    /// balances + native contract balances/deposits + program deposits + locked rewards + voting deposits == emitted - burned
    fn check_supply_invariant(&self) {
        let mut total: u128 = 0;
        for (k, v) in &self.state {
            match k[0] {
                PREFIX_ACCOUNT => total += borsh::from_slice::<Account>(v).unwrap().balance as u128,
                PREFIX_CONTRACT => {
                    let c = borsh::from_slice::<thecoin_core::contracts::Contract>(v).unwrap();
                    total += c.balance as u128 + c.deposit as u128;
                }
                PREFIX_PROGRAM_META => total += borsh::from_slice::<ProgramMeta>(v).unwrap().deposit as u128,
                PREFIX_PENDING_REWARD => total += borsh::from_slice::<PendingReward>(v).unwrap().locked() as u128,
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

    fn tx_with(&mut self, action: TxAction, flags: u8, fee_per_byte: u64) -> Transaction {
        let body = TxBody { version: TX_VERSION, chain_id: REGTEST.chain_id, flags, nonce: self.nonce, fee: 0, expiry_height: 0, action };
        let tx = body.clone().sign(&self.key);
        let fuel_fee = tx.max_fuel() / 10; // regtest: 100 motes per 1000 fuel
        let body = TxBody { fee: 200 + tx.size() as u64 * fee_per_byte + fuel_fee, ..body };
        self.nonce += 1;
        body.sign(&self.key)
    }

    fn tx(&mut self, action: TxAction) -> Transaction {
        self.tx_with(action, 0, 30)
    }

    fn pay(&mut self, to: &Address, amount: u64) -> Transaction {
        self.tx(TxAction::Transfer { to: *to, amount, memo: b"invoice #1".to_vec() })
    }
}

fn funded() -> (TestChain, Wallet) {
    let mut c = TestChain::new();
    let miner = Wallet::new(1);
    // regtest cooldown: 25% after 5 blocks, the rest after 12
    c.mine_empty(miner.addr, 30);
    (c, miner)
}

fn tx_error(r: Result<BlockReceipt, BlockError>) -> TxError {
    match r {
        Err(BlockError::Tx { error, .. }) => error,
        other => panic!("expected tx error, got {other:?}"),
    }
}

#[test]
fn rewards_are_released_gradually_and_transfers_work() {
    let mut c = TestChain::new();
    let mut miner = Wallet::new(1);
    let bob = Wallet::new(2);
    c.mine_empty(miner.addr, 5);
    assert_eq!(c.account(&miner.addr).balance, 0, "rewards are in cooldown");
    c.mine_empty(miner.addr, 1); // height 6: first quarter of block 1
    assert_eq!(c.account(&miner.addr).balance, 10 * COIN);
    c.mine_empty(miner.addr, 7); // height 13: quarters of blocks 1..8, rest of block 1
    assert_eq!(c.account(&miner.addr).balance, 8 * 10 * COIN + 30 * COIN);

    let tx = miner.pay(&bob.addr, 15 * COIN);
    let fee = tx.body.fee;
    let before = c.account(&miner.addr).balance;
    c.mine(miner.addr, vec![tx.clone()]).unwrap(); // height 14: +10 (block 9 quarter) +30 (block 2 rest)
    assert_eq!(c.account(&bob.addr).balance, 15 * COIN);
    assert_eq!(c.account(&miner.addr).balance, before + 40 * COIN - 15 * COIN - fee);

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

    let bad_flags = miner.tx_with(TxAction::Transfer { to: bob.addr, amount: COIN, memo: vec![] }, 0x80, 30);
    assert!(matches!(tx_error(c.mine(miner.addr, vec![bad_flags])), TxError::BadFlags(0x80)));
    miner.nonce -= 1;
    // the replaceable flag is valid
    miner.nonce = c.account(&miner.addr).nonce;
    c.mine(miner.addr, vec![miner.tx_with(TxAction::Transfer { to: bob.addr, amount: COIN, memo: vec![] }, FLAG_REPLACEABLE, 30)]).unwrap();

    // signal bits with no proposal voting are invalid
    assert!(matches!(c.mine_signal(miner.addr, vec![], 1), Err(BlockError::BadSignal(1))));
}

#[test]
fn congestion_surcharge_is_burned_and_tips_go_to_miner() {
    let (mut c, mut alice) = funded();
    let bob = Wallet::new(2);
    let mut g = c.global();
    g.congestion_bp = 30_000; // 3×
    c.set_global(&g);

    // A fee valid at 1× is too low at 3×.
    let tx = alice.tx_with(TxAction::Transfer { to: bob.addr, amount: COIN, memo: vec![] }, 0, 1);
    assert!(matches!(tx_error(c.mine(alice.addr, vec![tx])), TxError::FeeTooLow { .. }));
    alice.nonce -= 1;

    let tx = alice.tx(TxAction::Transfer { to: bob.addr, amount: COIN, memo: vec![] });
    let (required, base) = required_fee(&g.params, 30_000, tx.size(), 0);
    let burned_before = c.global().burned;
    let r = c.mine(Wallet::new(77).addr, vec![tx.clone()]).unwrap();
    assert_eq!(r.burned, required - base);
    assert_eq!(r.fees, tx.body.fee - (required - base), "miner keeps base part and the tip");
    assert_eq!(c.global().burned, burned_before + required - base);
    // almost empty block: the multiplier decreases
    assert!(c.global().congestion_bp < 30_000);
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
fn escrow_with_refundable_deposit() {
    let (mut c, mut rich) = funded();
    let mut buyer = Wallet::new(4);
    let seller = Wallet::new(2);
    let mut arbiter = Wallet::new(3);
    c.mine(rich.addr, vec![rich.pay(&arbiter.addr, COIN), rich.pay(&buyer.addr, 100 * COIN)]).unwrap();
    let miner = Wallet::new(99).addr; // separate miner so rewards don't blur balances

    let id1 = contract_id(&buyer.addr, buyer.nonce);
    let spec = ContractSpec::Escrow { payee: seller.addr, arbiter: Some(arbiter.addr), amount: 10 * COIN, deadline_height: 1000 };
    let create = buyer.tx(TxAction::CreateContract { spec });
    let fee = create.body.fee;
    let before = c.account(&buyer.addr).balance;
    c.mine(miner, vec![create]).unwrap();
    let contract: thecoin_core::contracts::Contract = read_typed(&c.state, &contract_key(&id1)).unwrap().unwrap();
    assert_eq!(contract.balance, 10 * COIN);
    let without_deposit = thecoin_core::contracts::Contract { deposit: 0, ..contract.clone() };
    assert_eq!(contract.deposit, storage_deposit(borsh::to_vec(&without_deposit).unwrap().len() as u64 + 33, REGTEST.gov_defaults.storage_deposit_per_kb));
    assert!(contract.deposit > 0);
    assert_eq!(c.account(&buyer.addr).balance, before - 10 * COIN - fee - contract.deposit);

    let release = buyer.tx(TxAction::CallContract { contract: id1, call: ContractCall::EscrowRelease });
    let fee2 = release.body.fee;
    let before = c.account(&buyer.addr).balance;
    c.mine(miner, vec![release]).unwrap();
    assert_eq!(c.account(&seller.addr).balance, 10 * COIN);
    assert!(!c.state.contains_key(&contract_key(&id1)), "finished contract removed");
    assert_eq!(c.account(&buyer.addr).balance, before - fee2 + contract.deposit, "deposit refunded");

    let id2 = contract_id(&buyer.addr, buyer.nonce);
    let spec = ContractSpec::Escrow { payee: seller.addr, arbiter: Some(arbiter.addr), amount: 5 * COIN, deadline_height: 1000 };
    c.mine(miner, vec![buyer.tx(TxAction::CreateContract { spec })]).unwrap();
    c.mine(miner, vec![arbiter.tx(TxAction::CallContract { contract: id2, call: ContractCall::EscrowRelease })]).unwrap();
    assert_eq!(c.account(&seller.addr).balance, 15 * COIN);
}

#[test]
fn vesting_subscription_htlc_multisig() {
    let (mut c, mut alice) = funded();
    let mut bob = Wallet::new(2);
    let mut carol = Wallet::new(3);
    c.mine(alice.addr, vec![alice.pay(&bob.addr, COIN), alice.pay(&carol.addr, COIN)]).unwrap();
    let miner = Wallet::new(99).addr;

    // Vesting: 100 TCN from h+0 to h+10, cliff h+5
    let h = c.height + 1;
    let vid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Vesting { beneficiary: bob.addr, amount: 100 * COIN, start_height: h, cliff_height: h + 5, end_height: h + 10, revocable: true };
    c.mine(miner, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let e = tx_error(c.mine(miner, vec![bob.tx(TxAction::CallContract { contract: vid, call: ContractCall::VestingClaim })]));
    assert!(matches!(e, TxError::Contract(_)));
    bob.nonce -= 1;
    c.mine_empty(miner, 4);
    c.mine(miner, vec![bob.tx(TxAction::CallContract { contract: vid, call: ContractCall::VestingClaim })]).unwrap();
    let bob_bal = c.account(&bob.addr).balance;
    assert!(bob_bal > 50 * COIN - COIN && bob_bal < 52 * COIN, "{bob_bal}");
    c.mine(miner, vec![alice.tx(TxAction::CallContract { contract: vid, call: ContractCall::VestingRevoke })]).unwrap();
    assert!(!c.state.contains_key(&contract_key(&vid)));

    // Subscription: 2 TCN per 3 blocks, 4 periods
    let sid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Subscription { payee: carol.addr, amount_per_period: 2 * COIN, period_blocks: 3, max_periods: 4 };
    c.mine(miner, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let carol0 = c.account(&carol.addr).balance;
    let claim = carol.tx(TxAction::CallContract { contract: sid, call: ContractCall::SubscriptionClaim });
    let fee = claim.body.fee;
    c.mine(miner, vec![claim]).unwrap();
    assert_eq!(c.account(&carol.addr).balance, carol0 + 2 * COIN - fee);
    c.mine_empty(miner, 3);
    c.mine(miner, vec![alice.tx(TxAction::CallContract { contract: sid, call: ContractCall::SubscriptionCancel })]).unwrap();
    assert_eq!(c.account(&carol.addr).balance, carol0 + 4 * COIN - fee);
    assert!(!c.state.contains_key(&contract_key(&sid)));

    // HTLC
    let preimage = b"super secret swap preimage".to_vec();
    let lock = Hash32(Sha256::digest(&preimage).into());
    let hid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Htlc { recipient: bob.addr, amount: 7 * COIN, hash_lock: lock, timeout_height: c.height + 50 };
    c.mine(miner, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let wrong = bob.tx(TxAction::CallContract { contract: hid, call: ContractCall::HtlcRedeem { preimage: b"nope".to_vec() } });
    assert!(matches!(tx_error(c.mine(miner, vec![wrong])), TxError::Contract(_)));
    bob.nonce -= 1;
    let b0 = c.account(&bob.addr).balance;
    let redeem = bob.tx(TxAction::CallContract { contract: hid, call: ContractCall::HtlcRedeem { preimage } });
    let fee = redeem.body.fee;
    c.mine(miner, vec![redeem]).unwrap();
    assert_eq!(c.account(&bob.addr).balance, b0 + 7 * COIN - fee);

    // Multisig 2-of-3, then close it and recover the deposit
    let mid = contract_id(&alice.addr, alice.nonce);
    let spec = ContractSpec::Multisig { signers: vec![alice.addr, bob.addr, carol.addr], threshold: 2, initial_deposit: 5 * COIN };
    c.mine(miner, vec![alice.tx(TxAction::CreateContract { spec })]).unwrap();
    let dest = Wallet::new(9).addr;
    c.mine(miner, vec![alice.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigPropose { to: dest, amount: 5 * COIN, memo: vec![] } })])
        .unwrap();
    let dup = alice.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigApprove { spend_id: 0 } });
    assert!(matches!(tx_error(c.mine(miner, vec![dup])), TxError::Contract(_)));
    alice.nonce -= 1;
    c.mine(miner, vec![carol.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigApprove { spend_id: 0 } })]).unwrap();
    assert_eq!(c.account(&dest).balance, 5 * COIN);
    let deposit = read_typed::<thecoin_core::contracts::Contract, _>(&c.state, &contract_key(&mid)).unwrap().unwrap().deposit;
    let mut outsider = Wallet::new(9);
    let outsider_close = outsider.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigClose });
    assert!(matches!(tx_error(c.mine(miner, vec![outsider_close])), TxError::Contract(_)));
    let alice_before = c.account(&alice.addr).balance;
    let close = bob.tx(TxAction::CallContract { contract: mid, call: ContractCall::MultisigClose });
    c.mine(miner, vec![close]).unwrap();
    assert!(!c.state.contains_key(&contract_key(&mid)));
    assert_eq!(c.account(&alice.addr).balance, alice_before + deposit, "creator recovers the deposit");
}

#[test]
fn governance_changes_parameter_with_both_chambers() {
    let (mut c, mut miner) = funded();
    let mut voter = Wallet::new(2);
    c.mine(miner.addr, vec![miner.pay(&voter.addr, 400 * COIN)]).unwrap();

    let spec = ProposalSpec {
        title: "Raise fee per kB".into(),
        url: "https://the-coin.cloud/governance/1".into(),
        content_hash: Hash32::ZERO,
        action: ProposalAction::SetParam { param: GovParamId::FeePerKb, value: 20_000 },
    };
    let pid = proposal_id(&voter.addr, voter.nonce);
    c.mine(miner.addr, vec![voter.tx(TxAction::Propose { proposal: spec })]).unwrap();
    let g = c.global();
    assert_eq!(g.voting.len(), 1);
    let bit = g.voting[0].0;

    c.mine_signal(miner.addr, vec![voter.tx(TxAction::Vote { proposal: pid, choice: VoteChoice::Yes, weight: 390 * COIN })], 1 << bit).unwrap();
    assert_eq!(c.account(&voter.addr).locked, 390 * COIN);
    let spend = voter.pay(&miner.addr, 100 * COIN);
    assert!(matches!(tx_error(c.mine_signal(miner.addr, vec![spend], 1 << bit)), TxError::InsufficientFunds { .. }));
    voter.nonce -= 1;
    let again = voter.tx(TxAction::Vote { proposal: pid, choice: VoteChoice::Yes, weight: COIN });
    assert!(matches!(tx_error(c.mine_signal(miner.addr, vec![again], 1 << bit)), TxError::Governance(_)));
    voter.nonce -= 1;

    let end = read_typed::<thecoin_core::governance::Proposal, _>(&c.state, &proposal_key(&pid)).unwrap().unwrap().end_height;
    while c.height < end {
        c.mine_signal(miner.addr, vec![], 1 << bit).unwrap();
    }
    let p: thecoin_core::governance::Proposal = read_typed(&c.state, &proposal_key(&pid)).unwrap().unwrap();
    let ProposalStatus::Approved { activation_height } = p.status else { panic!("not approved: {p:?}") };
    assert!(p.outcome.unwrap().deposit_refunded);
    assert_eq!(c.global().params.fee_per_kb, 1_000);
    while c.height < activation_height {
        c.mine(miner.addr, vec![]).unwrap();
    }
    assert_eq!(c.global().params.fee_per_kb, 20_000);
    c.mine(miner.addr, vec![voter.pay(&miner.addr, 100 * COIN)]).unwrap();
}

#[test]
fn governance_without_quorum_burns_deposit() {
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

#[test]
fn vote_in_same_block_as_proposal() {
    let (mut c, mut miner) = funded();
    let spec = ProposalSpec { title: "Same-block vote".into(), url: String::new(), content_hash: Hash32::ZERO, action: ProposalAction::Text };
    let pid = proposal_id(&miner.addr, miner.nonce);
    let propose = miner.tx(TxAction::Propose { proposal: spec });
    let vote = miner.tx(TxAction::Vote { proposal: pid, choice: VoteChoice::Yes, weight: COIN });
    c.mine(miner.addr, vec![propose, vote]).unwrap();
    let p: thecoin_core::governance::Proposal = read_typed(&c.state, &proposal_key(&pid)).unwrap().unwrap();
    assert_eq!(p.tally.yes, COIN);
}

const TIP_JAR: &str = r#"
contract TipJar
state owner: address
state tips: map[address, int]
event Tip(from: address, amount: int)
init():
    owner = caller
action tip(note: text) payable:
    require value >= 1000, "tip too small"
    tips[caller] += value
    emit Tip(caller, value)
action clear():
    tips.remove(caller)
action withdraw(amount: int):
    require caller == owner, "only owner"
    send(owner, amount)
action close():
    require caller == owner, "only owner"
    destroy(owner)
view tipped(who: address) -> int:
    return tips[who]
"#;

fn deploy_tx(w: &mut Wallet, source: &str) -> Transaction {
    w.tx(TxAction::Deploy { source: source.into(), init_args: vec![], value: 0, max_fuel: 1_000_000, max_deposit: COIN })
}

fn invoke_tx(w: &mut Wallet, contract: Address, function: &str, args: Vec<Value>, value: u64) -> Transaction {
    w.tx(TxAction::Invoke { contract, function: function.into(), args, value, max_fuel: 200_000, max_deposit: COIN })
}

#[test]
fn tccl_contract_lifecycle_fees_deposits_and_reverts() {
    let (mut c, mut rich) = funded();
    let mut owner = Wallet::new(6);
    let mut fan = Wallet::new(5);
    let miner = Wallet::new(99).addr;
    c.mine(rich.addr, vec![rich.pay(&fan.addr, 100 * COIN), rich.pay(&owner.addr, 100 * COIN)]).unwrap();

    // Deploy.
    let addr = program_address(&owner.addr, owner.nonce);
    let r = c.mine(miner, vec![deploy_tx(&mut owner, TIP_JAR)]).unwrap();
    let rec = &r.txs[0];
    assert!(rec.success, "{:?}", rec.error);
    assert_eq!(rec.program, Some(addr));
    let meta: ProgramMeta = read_typed(&c.state, &program_meta_key(&addr)).unwrap().unwrap();
    assert_eq!(meta.name, "TipJar");
    assert_eq!(meta.deposit, storage_deposit(meta.state_bytes, REGTEST.gov_defaults.storage_deposit_per_kb));
    assert!(meta.deposit > 0);

    // Payable call: value moves to the contract, deposit grows with storage.
    let fan_before = c.account(&fan.addr).balance;
    let call = invoke_tx(&mut fan, addr, "tip", vec![Value::Text("thanks".into())], 5 * COIN);
    let fee = call.body.fee;
    let r = c.mine(miner, vec![call]).unwrap();
    let rec = &r.txs[0];
    assert!(rec.success, "{:?}", rec.error);
    assert_eq!(rec.logs.len(), 1);
    assert_eq!(c.account(&addr).balance, 5 * COIN);
    let meta2: ProgramMeta = read_typed(&c.state, &program_meta_key(&addr)).unwrap().unwrap();
    let deposit_paid = meta2.deposit - meta.deposit;
    assert_eq!(c.account(&fan.addr).balance, fan_before - 5 * COIN - fee - deposit_paid);
    let (v, _) = view(&c.state, c.height, &addr, "tipped", vec![Value::Address(fan.addr.0)], 100_000).unwrap();
    assert_eq!(v.unwrap(), Value::Int(5 * COIN as i128));

    // Failing call: included, fee charged, nonce used, value and state reverted.
    let fan_before = c.account(&fan.addr).balance;
    let nonce_before = c.account(&fan.addr).nonce;
    let bad = invoke_tx(&mut fan, addr, "tip", vec![Value::Text("x".into())], 10);
    let fee = bad.body.fee;
    let r = c.mine(miner, vec![bad]).unwrap();
    assert!(!r.txs[0].success);
    assert_eq!(r.txs[0].error.as_deref(), Some("requirement failed: tip too small"));
    assert_eq!(c.account(&fan.addr).balance, fan_before - fee);
    assert_eq!(c.account(&fan.addr).nonce, nonce_before + 1);
    assert_eq!(c.account(&addr).balance, 5 * COIN);

    // Permission failure.
    let r = c.mine(miner, vec![invoke_tx(&mut fan, addr, "withdraw", vec![Value::Int(COIN as i128)], 0)]).unwrap();
    assert!(!r.txs[0].success);

    // Freeing storage refunds deposit to the caller.
    let before = c.account(&fan.addr).balance;
    let clear = invoke_tx(&mut fan, addr, "clear", vec![], 0);
    let fee = clear.body.fee;
    c.mine(miner, vec![clear]).unwrap();
    let meta3: ProgramMeta = read_typed(&c.state, &program_meta_key(&addr)).unwrap().unwrap();
    assert!(meta3.deposit < meta2.deposit);
    assert_eq!(c.account(&fan.addr).balance, before - fee + (meta2.deposit - meta3.deposit));

    // Owner withdraws, then destroys: balance + remaining deposit returned, records deleted.
    let r = c.mine(miner, vec![invoke_tx(&mut owner, addr, "withdraw", vec![Value::Int(2 * COIN as i128)], 0)]).unwrap();
    assert!(r.txs[0].success);
    assert_eq!(c.account(&addr).balance, 3 * COIN);
    let before = c.account(&owner.addr).balance;
    let close = invoke_tx(&mut owner, addr, "close", vec![], 0);
    let fee = close.body.fee;
    let r = c.mine(miner, vec![close]).unwrap();
    assert!(r.txs[0].success, "{:?}", r.txs[0].error);
    assert!(!c.state.contains_key(&program_meta_key(&addr)));
    assert!(!c.state.contains_key(&program_code_key(&addr)));
    assert_eq!(c.account(&owner.addr).balance, before - fee + 3 * COIN + meta3.deposit);

    // Calling a destroyed contract fails but pays the fee.
    let r = c.mine(miner, vec![invoke_tx(&mut fan, addr, "tipped", vec![], 0)]).unwrap();
    assert!(!r.txs[0].success);
}

#[test]
fn tccl_compile_errors_and_limits() {
    let (mut c, mut rich) = funded();
    let mut dev = Wallet::new(7);
    let miner = Wallet::new(99).addr;
    c.mine(rich.addr, vec![rich.pay(&dev.addr, 100 * COIN)]).unwrap();
    let before = c.account(&dev.addr).balance;
    let bad = deploy_tx(&mut dev, "contract Bad\naction f():\n    let x: int = \"text\"\n");
    let fee = bad.body.fee;
    let r = c.mine(miner, vec![bad]).unwrap();
    assert!(!r.txs[0].success);
    assert!(r.txs[0].error.as_ref().unwrap().contains("compile error"));
    assert_eq!(c.account(&dev.addr).balance, before - fee);

    // Out of fuel.
    let spin = "contract Spin\naction go():\n    while true:\n        pass\n";
    let addr = program_address(&dev.addr, dev.nonce);
    c.mine(miner, vec![deploy_tx(&mut dev, spin)]).unwrap();
    let r = c.mine(miner, vec![invoke_tx(&mut dev, addr, "go", vec![], 0)]).unwrap();
    assert_eq!(r.txs[0].error.as_deref(), Some("out of fuel"));
    assert_eq!(r.txs[0].fuel_used, 200_000);

    // Storage deposit above max_deposit fails.
    let hog = "contract Hog\nstate data: list[bytes]\naction fill():\n    for i in range(0, 50):\n        data.push(0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff)\n";
    let addr = program_address(&dev.addr, dev.nonce);
    c.mine(miner, vec![deploy_tx(&mut dev, hog)]).unwrap();
    let tight = dev.tx(TxAction::Invoke { contract: addr, function: "fill".into(), args: vec![], value: 0, max_fuel: 500_000, max_deposit: 1 });
    let r = c.mine(miner, vec![tight]).unwrap();
    assert!(r.txs[0].error.as_ref().unwrap().contains("exceeds max_deposit"), "{:?}", r.txs[0].error);

    // Invalid max_fuel is rejected before execution.
    let zero = dev.tx(TxAction::Invoke { contract: addr, function: "fill".into(), args: vec![], value: 0, max_fuel: 0, max_deposit: 0 });
    assert!(matches!(tx_error(c.mine(miner, vec![zero])), TxError::BadContractTx(_)));
}
