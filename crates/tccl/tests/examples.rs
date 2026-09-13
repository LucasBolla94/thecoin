//! Runs every example contract in the simulator.

use tccl::program::Value;
use tccl::sim::{account, Simulator};
use tccl::VmError;

const TCN: u64 = 100_000_000;

fn src(name: &str) -> String {
    std::fs::read_to_string(format!("{}/examples/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn int(v: i128) -> Value {
    Value::Int(v)
}

fn addr(name: &str) -> Value {
    Value::Address(account(name))
}

#[test]
fn all_examples_compile() {
    for f in std::fs::read_dir(format!("{}/examples", env!("CARGO_MANIFEST_DIR"))).unwrap() {
        let path = f.unwrap().path();
        let s = std::fs::read_to_string(&path).unwrap();
        tccl::compile(&s, &Default::default()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

#[test]
fn counter() {
    let mut sim = Simulator::new();
    let (c, r) = sim.deploy(&src("counter.tccl"), account("alice"), vec![], 0).unwrap();
    r.result.unwrap();
    sim.call(&c, account("bob"), "increment", vec![int(5)], 0).unwrap().result.unwrap();
    let r = sim.call(&c, account("bob"), "increment", vec![int(7)], 0).unwrap();
    assert_eq!(r.events.len(), 1);
    assert_eq!(sim.view(&c, "get", vec![]).unwrap().result.unwrap(), int(12));
    assert_eq!(sim.view(&c, "last", vec![]).unwrap().result.unwrap(), addr("bob"));
    let bad = sim.call(&c, account("bob"), "increment", vec![int(0)], 0).unwrap();
    assert_eq!(bad.result, Err(VmError::Require("amount must be positive".into())));
    assert_eq!(sim.view(&c, "get", vec![]).unwrap().result.unwrap(), int(12), "failed call changed nothing");
    // views cannot be called as actions and vice versa
    assert!(matches!(sim.call(&c, account("bob"), "get", vec![], 0).unwrap().result, Err(VmError::NotCallable(_))));
    assert!(matches!(sim.view(&c, "increment", vec![int(1)]).unwrap().result, Err(VmError::NotCallable(_))));
    // non-payable
    assert_eq!(sim.call(&c, account("bob"), "increment", vec![int(1)], 5).unwrap().result, Err(VmError::NotPayable));
    // wrong argument type
    assert!(matches!(sim.call(&c, account("bob"), "increment", vec![Value::Bool(true)], 0).unwrap().result, Err(VmError::BadArguments(_))));
}

#[test]
fn tip_jar_payments_and_permissions() {
    let mut sim = Simulator::new();
    let (c, _) = sim.deploy(&src("tip_jar.tccl"), account("owner"), vec![], 0).unwrap();
    let start = sim.balance_of(&account("fan"));
    sim.call(&c, account("fan"), "tip", vec![Value::Text("great work".into())], 3 * TCN).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("fan")), start - 3 * TCN);
    let too_small = sim.call(&c, account("fan"), "tip", vec![Value::Text("x".into())], 1000).unwrap();
    assert!(too_small.result.is_err());
    assert_eq!(sim.balance_of(&account("fan")), start - 3 * TCN, "value refunded on failure");
    assert!(sim.call(&c, account("fan"), "withdraw", vec![int(TCN as i128)], 0).unwrap().result.is_err());
    let owner_before = sim.balance_of(&account("owner"));
    sim.call(&c, account("owner"), "withdraw", vec![int(2 * TCN as i128)], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("owner")), owner_before + 2 * TCN);
    assert_eq!(sim.view(&c, "stats", vec![]).unwrap().result.unwrap(), Value::List(vec![int(3 * TCN as i128), int(1), int(TCN as i128)]));
}

#[test]
fn token_transfers_and_allowances() {
    let mut sim = Simulator::new();
    let (c, _) = sim.deploy(&src("token.tccl"), account("issuer"), vec![], 0).unwrap();
    sim.call(&c, account("issuer"), "mint", vec![addr("alice"), int(1000)], 0).unwrap().result.unwrap();
    assert!(sim.call(&c, account("alice"), "mint", vec![addr("alice"), int(1)], 0).unwrap().result.is_err());
    sim.call(&c, account("alice"), "transfer", vec![addr("bob"), int(300)], 0).unwrap().result.unwrap();
    assert!(sim.call(&c, account("bob"), "transfer", vec![addr("carol"), int(301)], 0).unwrap().result.is_err());
    sim.call(&c, account("alice"), "approve", vec![addr("dex"), int(100)], 0).unwrap().result.unwrap();
    sim.call(&c, account("dex"), "transfer_from", vec![addr("alice"), addr("carol"), int(60)], 0).unwrap().result.unwrap();
    assert!(sim.call(&c, account("dex"), "transfer_from", vec![addr("alice"), addr("carol"), int(41)], 0).unwrap().result.is_err());
    let bal = |sim: &mut Simulator, who: &str| sim.view(&c, "balance_of", vec![addr(who)]).unwrap().result.unwrap();
    assert_eq!(bal(&mut sim, "alice"), int(640));
    assert_eq!(bal(&mut sim, "bob"), int(300));
    assert_eq!(bal(&mut sim, "carol"), int(60));
    assert_eq!(sim.view(&c, "allowance", vec![addr("alice"), addr("dex")]).unwrap().result.unwrap(), int(40));
    let over = sim.call(&c, account("issuer"), "mint", vec![addr("bob"), int(21_000_000)], 0).unwrap();
    assert_eq!(over.result, Err(VmError::Require("max supply reached".into())));
}

#[test]
fn crowdfund_success_and_refunds() {
    let mut sim = Simulator::new();
    let (ok, _) = sim.deploy(&src("crowdfund.tccl"), account("maker"), vec![int(5), int(10)], 0).unwrap();
    sim.call(&ok, account("a"), "pledge", vec![], 3 * TCN).unwrap().result.unwrap();
    sim.call(&ok, account("b"), "pledge", vec![], 2 * TCN).unwrap().result.unwrap();
    assert!(sim.call(&ok, account("maker"), "collect", vec![], 0).unwrap().result.is_err(), "still running");
    sim.height += 20;
    assert_eq!(sim.view(&ok, "status", vec![]).unwrap().result.unwrap(), Value::Text("successful".into()));
    let before = sim.balance_of(&account("maker"));
    sim.call(&ok, account("maker"), "collect", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("maker")), before + 5 * TCN);
    assert!(sim.call(&ok, account("maker"), "collect", vec![], 0).unwrap().result.is_err());

    let (fail, _) = sim.deploy(&src("crowdfund.tccl"), account("maker"), vec![int(100), int(10)], 0).unwrap();
    sim.call(&fail, account("a"), "pledge", vec![], 4 * TCN).unwrap().result.unwrap();
    sim.height += 20;
    let a_before = sim.balance_of(&account("a"));
    sim.call(&fail, account("a"), "refund", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("a")), a_before + 4 * TCN);
    assert!(sim.call(&fail, account("a"), "refund", vec![], 0).unwrap().result.is_err(), "no double refund");
    assert!(sim.deploy(&src("crowdfund.tccl"), account("maker"), vec![int(0), int(10)], 0).unwrap().1.result.is_err());
}

#[test]
fn private_pool_hides_depositor_and_blocks_double_withdraw() {
    let mut sim = Simulator::new();
    let (pool, _) = sim.deploy(&src("private_pool.tccl"), account("anyone"), vec![], 0).unwrap();
    // Five people deposit with fresh ring keys.
    let keys: Vec<([u8; 32], [u8; 32])> = (0..5u8).map(|i| tccl::ring::keypair_from_seed(&[i + 10; 32])).collect();
    for (i, (_, pk)) in keys.iter().enumerate() {
        let who = format!("depositor{i}");
        sim.call(&pool, account(&who), "deposit", vec![Value::Bytes(pk.to_vec())], 10 * TCN).unwrap().result.unwrap();
    }
    assert!(sim.call(&pool, account("x"), "deposit", vec![Value::Bytes(vec![1; 32])], 5 * TCN).unwrap().result.is_err());
    assert_eq!(sim.view(&pool, "deposits", vec![]).unwrap().result.unwrap(), int(5));

    // Depositor 3 withdraws to a brand-new address through a relayer.
    let dest = account("fresh-destination");
    let relayer = account("relayer");
    let fee: i128 = TCN as i128 / 10;
    let Value::Bytes(msg) =
        sim.view(&pool, "message_for", vec![Value::Address(dest), Value::Address(relayer), int(fee)]).unwrap().result.unwrap()
    else {
        panic!()
    };
    let members: Vec<usize> = vec![0, 1, 2, 3, 4];
    let ring: Vec<[u8; 32]> = members.iter().map(|i| keys[*i].1).collect();
    let (sig, ki) = tccl::ring::sign(&msg, &ring, 3, &keys[3].0, &mut rand::thread_rng()).unwrap();
    let args = |sig: &[u8], ki: &[u8], to: [u8; 20]| {
        vec![
            Value::Address(to),
            Value::Address(relayer),
            int(fee),
            Value::List(members.iter().map(|i| int(*i as i128)).collect()),
            Value::Bytes(sig.to_vec()),
            Value::Bytes(ki.to_vec()),
        ]
    };
    let relayer_before = sim.balance_of(&relayer);
    let dest_before = sim.balance_of(&dest);
    let r = sim.call(&pool, relayer, "withdraw", args(&sig, &ki, dest), 0).unwrap();
    r.result.unwrap();
    assert_eq!(sim.balance_of(&dest), dest_before + 10 * TCN - fee as u64);
    assert_eq!(sim.balance_of(&relayer), relayer_before + fee as u64);

    // Same deposit cannot be withdrawn twice (same key image), even with a new signature.
    let (sig2, ki2) = tccl::ring::sign(&msg, &ring, 3, &keys[3].0, &mut rand::thread_rng()).unwrap();
    assert_eq!(ki, ki2);
    let again = sim.call(&pool, relayer, "withdraw", args(&sig2, &ki2, dest), 0).unwrap();
    assert_eq!(again.result, Err(VmError::Require("this deposit was already withdrawn".into())));

    // A signature for another destination cannot be redirected (front-running protection).
    let (sig4, ki4) = tccl::ring::sign(&msg, &ring, 4, &keys[4].0, &mut rand::thread_rng()).unwrap();
    let stolen = sim.call(&pool, relayer, "withdraw", args(&sig4, &ki4, account("thief")), 0).unwrap();
    assert_eq!(stolen.result, Err(VmError::Require("invalid ring signature".into())));

    // Someone who never deposited cannot withdraw.
    let (outsider_sk, outsider_pk) = tccl::ring::keypair_from_seed(&[99; 32]);
    let mut fake_ring = ring.clone();
    fake_ring[0] = outsider_pk;
    let (fsig, fki) = tccl::ring::sign(&msg, &fake_ring, 0, &outsider_sk, &mut rand::thread_rng()).unwrap();
    let fake = sim.call(&pool, relayer, "withdraw", args(&fsig, &fki, dest), 0).unwrap();
    assert_eq!(fake.result, Err(VmError::Require("invalid ring signature".into())));
}

#[test]
fn fuel_and_limits_protect_the_network() {
    let mut sim = Simulator::new();
    let looping = "contract Loop\naction spin():\n    while true:\n        pass\n";
    let (c, _) = sim.deploy(looping, account("a"), vec![], 0).unwrap();
    let r = sim.call(&c, account("a"), "spin", vec![], 0).unwrap();
    assert_eq!(r.result, Err(VmError::OutOfFuel));
    assert_eq!(r.fuel_used, tccl::sim::DEFAULT_FUEL);

    let recursion = "contract Rec\nview go(n: int) -> int:\n    return down(n)\nfn down(n: int) -> int:\n    if n == 0:\n        return 0\n    return down(n - 1)\n";
    let (c, _) = sim.deploy(recursion, account("a"), vec![], 0).unwrap();
    assert_eq!(sim.view(&c, "go", vec![int(5)]).unwrap().result.unwrap(), int(0));
    assert_eq!(sim.view(&c, "go", vec![int(100)]).unwrap().result, Err(VmError::CallDepth));

    let overflow = "contract Of\nview big() -> int:\n    let x: int = 170141183460469231731687303715884105727\n    return x + 1\n";
    let (c, _) = sim.deploy(overflow, account("a"), vec![], 0).unwrap();
    assert_eq!(sim.view(&c, "big", vec![]).unwrap().result, Err(VmError::Overflow));

    let growth = "contract G\nview grow() -> int:\n    let b: bytes = 0x00\n    while true:\n        b = b + b\n    return 0\n";
    let (c, _) = sim.deploy(growth, account("a"), vec![], 0).unwrap();
    assert_eq!(sim.view(&c, "grow", vec![]).unwrap().result, Err(VmError::TooLarge));

    let destroy = "contract D\nstate owner: address\nstate m: map[int, int]\ninit():\n    owner = caller\naction set():\n    m[1] = 5\naction clear():\n    m.remove(1)\naction close():\n    destroy(caller)\n";
    let (c, _) = sim.deploy(destroy, account("a"), vec![], 0).unwrap();
    sim.call(&c, account("a"), "set", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.call(&c, account("a"), "close", vec![], 0).unwrap().result, Err(VmError::StorageNotEmpty(1)));
    sim.call(&c, account("a"), "clear", vec![], 0).unwrap().result.unwrap();
    sim.call(&c, account("a"), "close", vec![], 0).unwrap().result.unwrap();
    assert!(sim.program(&c).is_none(), "contract removed");
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

fn fails_with(r: tccl::sim::CallResult, message: &str) {
    assert_eq!(r.result, Err(VmError::Require(message.into())));
}

#[test]
fn escrow_release_dispute_and_reclaim() {
    let mut sim = Simulator::new();
    let args = || vec![addr("seller"), addr("arbiter"), int(1_440)];
    // Happy path: buyer releases, then closes the contract.
    let (c, r) = sim.deploy(&src("escrow.tccl"), account("buyer"), args(), 25 * TCN).unwrap();
    r.result.unwrap();
    fails_with(sim.call(&c, account("seller"), "release", vec![], 0).unwrap(), "only the buyer can release");
    fails_with(sim.call(&c, account("buyer"), "close", vec![], 0).unwrap(), "settle the escrow first");
    let seller_before = sim.balance_of(&account("seller"));
    sim.call(&c, account("buyer"), "release", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("seller")), seller_before + 25 * TCN);
    fails_with(sim.call(&c, account("buyer"), "release", vec![], 0).unwrap(), "already settled");
    assert_eq!(sim.view(&c, "status", vec![]).unwrap().result.unwrap(), text("settled"));
    sim.call(&c, account("buyer"), "close", vec![], 0).unwrap().result.unwrap();
    assert!(sim.program(&c).is_none(), "contract destroyed");

    // Dispute: the arbiter decides, the buyer cannot reclaim after the deadline.
    let (d, _) = sim.deploy(&src("escrow.tccl"), account("buyer"), args(), 10 * TCN).unwrap();
    sim.call(&d, account("seller"), "dispute", vec![], 0).unwrap().result.unwrap();
    fails_with(sim.call(&d, account("seller"), "resolve", vec![Value::Bool(true)], 0).unwrap(), "only the arbiter");
    sim.height += 2_000;
    fails_with(sim.call(&d, account("buyer"), "reclaim", vec![], 0).unwrap(), "a dispute is open: the arbiter decides");
    let buyer_before = sim.balance_of(&account("buyer"));
    sim.call(&d, account("arbiter"), "resolve", vec![Value::Bool(false)], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("buyer")), buyer_before + 10 * TCN);

    // No dispute: the buyer reclaims after the deadline.
    let (e, _) = sim.deploy(&src("escrow.tccl"), account("buyer"), args(), 5 * TCN).unwrap();
    fails_with(sim.call(&e, account("buyer"), "reclaim", vec![], 0).unwrap(), "the deadline has not passed");
    sim.height += 2_000;
    assert_eq!(sim.view(&e, "status", vec![]).unwrap().result.unwrap(), text("expired"));
    sim.call(&e, account("buyer"), "reclaim", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.view(&e, "locked", vec![]).unwrap().result.unwrap(), int(0));

    // Invalid deployments are rejected and nothing is kept.
    let no_value = sim.deploy(&src("escrow.tccl"), account("buyer"), args(), 0).unwrap().1;
    assert_eq!(no_value.result, Err(VmError::Require("attach the payment with --value".into())));
    let self_deal = sim.deploy(&src("escrow.tccl"), account("buyer"), vec![addr("buyer"), addr("arbiter"), int(1_440)], TCN).unwrap().1;
    assert!(self_deal.result.is_err());
}

#[test]
fn poll_registered_voters_and_winner() {
    let mut sim = Simulator::new();
    let choices = Value::List(vec![text("red"), text("green"), text("blue")]);
    let (c, r) = sim.deploy(&src("poll.tccl"), account("chair"), vec![text("Favourite colour?"), choices, int(100)], 0).unwrap();
    r.result.unwrap();
    fails_with(
        sim.call(&c, account("v1"), "add_voters", vec![Value::List(vec![addr("v1")])], 0).unwrap(),
        "only the creator registers voters",
    );
    let voters = Value::List(vec![addr("v1"), addr("v2"), addr("v3"), addr("v1")]);
    let r = sim.call(&c, account("chair"), "add_voters", vec![voters], 0).unwrap();
    r.result.unwrap();
    assert_eq!(r.events.len(), 3, "duplicates are ignored");
    fails_with(sim.call(&c, account("stranger"), "vote", vec![int(0)], 0).unwrap(), "you are not a registered voter");
    sim.call(&c, account("v1"), "vote", vec![int(1)], 0).unwrap().result.unwrap();
    fails_with(sim.call(&c, account("v1"), "vote", vec![int(2)], 0).unwrap(), "you already voted");
    fails_with(sim.call(&c, account("v2"), "vote", vec![int(3)], 0).unwrap(), "unknown option");
    sim.call(&c, account("v2"), "vote", vec![int(1)], 0).unwrap().result.unwrap();
    sim.call(&c, account("v3"), "vote", vec![int(2)], 0).unwrap().result.unwrap();
    assert_eq!(sim.view(&c, "results", vec![]).unwrap().result.unwrap(), Value::List(vec![int(0), int(2), int(1)]));
    assert_eq!(sim.view(&c, "turnout", vec![]).unwrap().result.unwrap(), Value::List(vec![int(3), int(3)]));
    assert_eq!(sim.view(&c, "winner", vec![]).unwrap().result, Err(VmError::Require("voting is still open".into())));
    sim.height += 200;
    fails_with(sim.call(&c, account("v3"), "vote", vec![int(0)], 0).unwrap(), "voting is closed");
    assert_eq!(sim.view(&c, "winner", vec![]).unwrap().result.unwrap(), text("green"));
    assert_eq!(sim.view(&c, "option_name", vec![int(2)]).unwrap().result.unwrap(), text("blue"));
    let one_option = Value::List(vec![text("only")]);
    assert!(sim.deploy(&src("poll.tccl"), account("chair"), vec![text("?"), one_option, int(10)], 0).unwrap().1.result.is_err());
}

#[test]
fn savings_are_locked_until_the_chosen_height() {
    let mut sim = Simulator::new();
    let (c, _) = sim.deploy(&src("savings.tccl"), account("anyone"), vec![], 0).unwrap();
    let unlock = sim.height as i128 + 1_000;
    sim.call(&c, account("saver"), "deposit", vec![int(unlock)], 7 * TCN).unwrap().result.unwrap();
    sim.call(&c, account("saver"), "deposit", vec![int(unlock + 10)], 3 * TCN).unwrap().result.unwrap();
    fails_with(sim.call(&c, account("saver"), "deposit", vec![int(unlock)], TCN).unwrap(), "you cannot shorten an existing lock");
    fails_with(sim.call(&c, account("saver"), "deposit", vec![int(1)], TCN).unwrap(), "the unlock height must be in the future");
    fails_with(sim.call(&c, account("saver"), "withdraw", vec![], 0).unwrap(), "your savings are still locked");
    fails_with(sim.call(&c, account("other"), "withdraw", vec![], 0).unwrap(), "you have no savings here");
    assert_eq!(sim.view(&c, "balance_of", vec![addr("saver")]).unwrap().result.unwrap(), int(10 * TCN as i128));
    sim.height = unlock as u64 + 10;
    assert_eq!(sim.view(&c, "blocks_left", vec![addr("saver")]).unwrap().result.unwrap(), int(0));
    let before = sim.balance_of(&account("saver"));
    sim.call(&c, account("saver"), "withdraw", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("saver")), before + 10 * TCN);
    assert_eq!(sim.view(&c, "total", vec![]).unwrap().result.unwrap(), int(0));
    fails_with(sim.call(&c, account("saver"), "withdraw", vec![], 0).unwrap(), "you have no savings here");
}

#[test]
fn treasury_needs_threshold_approvals() {
    let mut sim = Simulator::new();
    let owners = Value::List(vec![addr("ann"), addr("ben"), addr("cat")]);
    let (c, r) = sim.deploy(&src("treasury.tccl"), account("ann"), vec![owners, int(2)], 0).unwrap();
    r.result.unwrap();
    sim.call(&c, account("donor"), "deposit", vec![], 50 * TCN).unwrap().result.unwrap();
    fails_with(
        sim.call(&c, account("mallory"), "propose", vec![addr("mallory"), int(TCN as i128), text("")], 0).unwrap(),
        "only an owner can do this",
    );
    let r = sim.call(&c, account("ann"), "propose", vec![addr("supplier"), int(20 * TCN as i128), text("servers")], 0).unwrap();
    assert_eq!(r.result.unwrap(), int(0), "actions can return a value");
    fails_with(sim.call(&c, account("ann"), "approve", vec![int(0)], 0).unwrap(), "you already approved this proposal");
    fails_with(sim.call(&c, account("ann"), "execute", vec![int(0)], 0).unwrap(), "not enough approvals");
    sim.call(&c, account("cat"), "approve", vec![int(0)], 0).unwrap().result.unwrap();
    assert_eq!(sim.view(&c, "has_approved", vec![int(0), addr("cat")]).unwrap().result.unwrap(), Value::Bool(true));
    assert_eq!(sim.view(&c, "has_approved", vec![int(0), addr("ben")]).unwrap().result.unwrap(), Value::Bool(false));
    let before = sim.balance_of(&account("supplier"));
    sim.call(&c, account("ben"), "execute", vec![int(0)], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("supplier")), before + 20 * TCN);
    fails_with(sim.call(&c, account("ben"), "execute", vec![int(0)], 0).unwrap(), "already executed");
    assert_eq!(
        sim.view(&c, "proposal", vec![int(0)]).unwrap().result.unwrap(),
        text("executed: 2000000000 motes, 2 approval(s), memo: servers")
    );
    // Too large for the balance: approvals are recorded, execution waits for funds.
    sim.call(&c, account("ben"), "propose", vec![addr("supplier"), int(100 * TCN as i128), text("")], 0).unwrap().result.unwrap();
    sim.call(&c, account("cat"), "approve", vec![int(1)], 0).unwrap().result.unwrap();
    fails_with(sim.call(&c, account("cat"), "execute", vec![int(1)], 0).unwrap(), "treasury balance too low");
    let dup = Value::List(vec![addr("ann"), addr("ann")]);
    assert!(sim.deploy(&src("treasury.tccl"), account("ann"), vec![dup, int(1)], 0).unwrap().1.result.is_err());
}

#[test]
fn name_service_registration_and_expiry() {
    let mut sim = Simulator::new();
    let (c, _) = sim.deploy(&src("names.tccl"), account("admin"), vec![], 0).unwrap();
    let price = TCN / 10;
    fails_with(
        sim.call(&c, account("alice"), "register", vec![text("Alice"), int(1)], price).unwrap(),
        "names have 3 to 32 characters: a-z, 0-9 and '-'",
    );
    fails_with(sim.call(&c, account("alice"), "register", vec![text("alice"), int(2)], price).unwrap(), "send exactly 0.1 TCN per year");
    sim.call(&c, account("alice"), "register", vec![text("alice"), int(1)], price).unwrap().result.unwrap();
    fails_with(sim.call(&c, account("bob"), "register", vec![text("alice"), int(1)], price).unwrap(), "this name is taken");
    assert_eq!(sim.view(&c, "resolve", vec![text("alice")]).unwrap().result.unwrap(), addr("alice"));
    assert_eq!(sim.view(&c, "available", vec![text("alice")]).unwrap().result.unwrap(), Value::Bool(false));
    assert_eq!(sim.view(&c, "available", vec![text("my-shop-42")]).unwrap().result.unwrap(), Value::Bool(true));
    fails_with(sim.call(&c, account("bob"), "point_to", vec![text("alice"), addr("bob")], 0).unwrap(), "you do not own this name");
    sim.call(&c, account("alice"), "point_to", vec![text("alice"), addr("alice-cold")], 0).unwrap().result.unwrap();
    assert_eq!(sim.view(&c, "resolve", vec![text("alice")]).unwrap().result.unwrap(), addr("alice-cold"));
    sim.call(&c, account("bob"), "renew", vec![text("alice"), int(1)], price).unwrap().result.unwrap();
    sim.call(&c, account("alice"), "transfer", vec![text("alice"), addr("carol")], 0).unwrap().result.unwrap();
    assert_eq!(sim.view(&c, "whois", vec![text("alice")]).unwrap().result.unwrap(), addr("carol"));
    fails_with(sim.call(&c, account("bob"), "collect_fees", vec![], 0).unwrap(), "only the admin");
    let before = sim.balance_of(&account("admin"));
    sim.call(&c, account("admin"), "collect_fees", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.balance_of(&account("admin")), before + 2 * price);
    // Two years later the name is free again.
    sim.height += 2 * 525_600 + 10;
    assert_eq!(
        sim.view(&c, "resolve", vec![text("alice")]).unwrap().result,
        Err(VmError::Require("name not registered or expired".into()))
    );
    sim.call(&c, account("bob"), "register", vec![text("alice"), int(1)], price).unwrap().result.unwrap();
    assert_eq!(sim.view(&c, "whois", vec![text("alice")]).unwrap().result.unwrap(), addr("bob"));
}

#[test]
fn shop_sells_at_the_listed_price() {
    let mut sim = Simulator::new();
    let (c, _) = sim.deploy(&src("shop.tccl"), account("owner"), vec![], 0).unwrap();
    fails_with(sim.call(&c, account("eve"), "set_price", vec![text("book"), int(TCN as i128)], 0).unwrap(), "only the owner");
    sim.call(&c, account("owner"), "set_price", vec![text("book"), int(2 * TCN as i128)], 0).unwrap().result.unwrap();
    fails_with(sim.call(&c, account("buyer"), "buy", vec![text("book")], TCN).unwrap(), "wrong price");
    fails_with(sim.call(&c, account("buyer"), "buy", vec![text("pen")], TCN).unwrap(), "unknown item");
    let before = sim.balance_of(&account("owner"));
    let r = sim.call(&c, account("buyer"), "buy", vec![text("book")], 2 * TCN).unwrap();
    r.result.unwrap();
    assert_eq!(r.events.len(), 1);
    assert_eq!(sim.balance_of(&account("owner")), before + 2 * TCN);
    // A price of 0 is the default value: the entry is deleted and the item is no longer for sale.
    sim.call(&c, account("owner"), "set_price", vec![text("book"), int(0)], 0).unwrap().result.unwrap();
    fails_with(sim.call(&c, account("buyer"), "buy", vec![text("book")], 0).unwrap(), "unknown item");
}

#[test]
fn compound_assignment_evaluates_the_index_once() {
    let source = r#"
contract Twice
state next: int
state counts: map[int, int]
state items: list[int]

fn new_id() -> int:
    next += 1
    return next

action bump():
    counts[new_id()] += 1

action bump_list():
    if len(items) == 0:
        items.push(0)
        items.push(0)
    let l: list[int] = [5, 6, 7]
    l[new_id() % 3] += 10
    items[new_id() % 2] += 1

view get(k: int) -> int:
    return counts[k]

view n() -> int:
    return next

view item(i: int) -> int:
    return items[i]
"#;
    let mut sim = Simulator::new();
    let (c, r) = sim.deploy(source, account("alice"), vec![], 0).unwrap();
    r.result.unwrap();
    sim.call(&c, account("alice"), "bump", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.view(&c, "n", vec![]).unwrap().result.unwrap(), int(1), "new_id() ran once");
    assert_eq!(sim.view(&c, "get", vec![int(1)]).unwrap().result.unwrap(), int(1));
    sim.call(&c, account("alice"), "bump_list", vec![], 0).unwrap().result.unwrap();
    assert_eq!(sim.view(&c, "n", vec![]).unwrap().result.unwrap(), int(3));
    assert_eq!(sim.view(&c, "item", vec![int(1)]).unwrap().result.unwrap(), int(1));
    assert_eq!(sim.view(&c, "item", vec![int(0)]).unwrap().result.unwrap(), int(0));
}
