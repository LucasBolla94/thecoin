//! Published test vectors.
//!
//! Every value printed here is copied verbatim into `docs/WALLET_DEVELOPERS.md`
//! (§8) and `docs/PROTOCOL.md` (§20–21). If this test fails, the documentation
//! is out of date (or consensus encoding changed by accident).
//!
//! ```text
//! cargo test -p thecoin-wallet --test vectors -- --nocapture
//! ```

use thecoin_core::contracts::contract_id;
use thecoin_core::execution::required_fee;
use thecoin_core::genesis::genesis_block;
use thecoin_core::governance::proposal_id;
use thecoin_core::hash::tagged_hash;
use thecoin_core::lthash::LtHash;
use thecoin_core::merkle::merkle_root;
use thecoin_core::params::{Network, MAINNET};
use thecoin_core::pow::pow_hash;
use thecoin_core::programs::program_address;
use thecoin_core::tccl::{ring, Value};
use thecoin_core::tx::{TxAction, TxBody, FLAG_REPLACEABLE, TX_VERSION};
use thecoin_core::{Address, Transaction};
use thecoin_wallet::builder::{build_tx, FeePolicy};
use thecoin_wallet::keys::{parse_mnemonic, HdKeys};

const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

/// Collects mismatches so that a single run prints every vector.
struct Vectors {
    failures: Vec<String>,
}

impl Vectors {
    fn new() -> Self {
        Vectors { failures: Vec::new() }
    }

    fn check(&mut self, label: &str, got: impl ToString, expected: &str) {
        let got = got.to_string();
        println!("{label} = {got}");
        if got != expected {
            self.failures.push(format!("{label}: expected {expected}, got {got}"));
        }
    }

    fn finish(self) {
        assert!(self.failures.is_empty(), "vectors changed:\n{}", self.failures.join("\n"));
    }
}

fn keys() -> HdKeys {
    HdKeys::from_mnemonic(&parse_mnemonic(MNEMONIC).unwrap(), "")
}

/// Signs `body` after setting the minimum mainnet fee at 1× congestion.
fn sign_with_min_fee(keys: &HdKeys, mut body: TxBody) -> Transaction {
    let key = keys.secret_key(0, 0);
    body.fee = 0;
    let trial = body.clone().sign(&key);
    let (fee, fee_at_1x) = required_fee(&MAINNET.gov_defaults, 10_000, trial.size(), trial.max_fuel());
    assert_eq!(fee, fee_at_1x);
    body.fee = fee;
    body.sign(&key)
}

#[test]
fn key_and_address_vectors() {
    let k = keys();
    let mut v = Vectors::new();
    for (index, secret, public, addr20, main, test, reg) in [
        (
            0,
            "a146f5dbeff18a4189d3bb0a03c636252ee07dba1fbd6e2e954fa3feff27db97",
            "67a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe06",
            "4d54f2145ebf35d8509935f16fb1ebb7efa47d64",
            "tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj",
            "tct1f420y9z7hu6as5yexhcklv0tklh6gltyfk2we0",
            "tcr1f420y9z7hu6as5yexhcklv0tklh6glty6qra7u",
        ),
        (
            1,
            "34e4f81414e800631d613d26011450e307870118a6ca68a99d2ff833cebbd028",
            "c4595ca32d93a95c85896b9988ee0d2181d867b892433358314f7887f365cf5a",
            "4200a67d1774885d87731decb1c4743c3b070454",
            "tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4",
            "tct1ggq2vlghwjy9mpmnrhktr3r58saswpz58a3dfg",
            "tcr1ggq2vlghwjy9mpmnrhktr3r58saswpz55tc7wm",
        ),
    ] {
        let sk = k.secret_key(0, index);
        let a = k.address(0, index);
        v.check(&format!("[{index}] secret_key"), hex::encode(sk.to_bytes()), secret);
        v.check(&format!("[{index}] public_key"), hex::encode(sk.public_key()), public);
        v.check(&format!("[{index}] address20"), a.to_hex(), addr20);
        v.check(&format!("[{index}] mainnet"), a.encode(Network::Mainnet), main);
        v.check(&format!("[{index}] testnet"), a.encode(Network::Testnet), test);
        v.check(&format!("[{index}] regtest"), a.encode(Network::Regtest), reg);
    }
    // Ring (privacy pool) key of index 0: m/44'/7333'/0'/7'/0'.
    let (secret, public) = k.ring_keypair(0);
    v.check("ring[0] secret", hex::encode(secret), "39d100ebdc3a849a4c856e976d4a3d684b70cb5fd9368915937e3a17710b9d09");
    v.check("ring[0] public", hex::encode(public), "34fde0fcd54780a5d77aca8583e9ff0af6cf676a6e81a28d45e5cda3bd446971");
    v.check(
        "ring[0] key_image",
        hex::encode(ring::key_image(&secret).unwrap()),
        "6a51935c920b9d18bd046c0383e9168f3b86d4bd1bee81a1924cd85413a4f75e",
    );
    v.finish();
}

#[test]
fn transfer_vectors() {
    let k = keys();
    let to = k.address(0, 1);
    let mut v = Vectors::new();

    let body = TxBody {
        version: TX_VERSION,
        chain_id: MAINNET.chain_id,
        flags: 0,
        nonce: 0,
        fee: 0,
        expiry_height: 0,
        action: TxAction::Transfer { to, amount: 150_000_000, memo: b"test".to_vec() },
    };
    let tx = sign_with_min_fee(&k, body);
    v.check("transfer size", tx.size(), "163");
    v.check("transfer fee", tx.body.fee, "2630");
    v.check("transfer body", hex::encode(borsh::to_vec(&tx.body).unwrap()), "0101004354000000000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f008000000000400000074657374");
    v.check("transfer signing_hash", tx.body.signing_hash(), "e81ec27086aa0dfe1bfc204fade8dee9290eefef33b511bc865eb32770fd6fed");
    v.check(
        "transfer signature",
        hex::encode(tx.signature),
        "16e8dec69deadd63934267d3a321def787aa20ad7ceb09633a2440f1e67a167fcdeb13b4e0375490de0bfa1e0d445c88babf44d205545000546376bd7c274409",
    );
    v.check("transfer tx", hex::encode(tx.to_bytes()), "0101004354000000000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f00800000000040000007465737467a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe0616e8dec69deadd63934267d3a321def787aa20ad7ceb09633a2440f1e67a167fcdeb13b4e0375490de0bfa1e0d445c88babf44d205545000546376bd7c274409");
    v.check("transfer txid", tx.txid(), "492e5c610cd3913d94a20a2cd5ddc076cec7d3f3b8abcc718923fdc9fe595d02");
    v.check("transfer sender", tx.sender().encode(Network::Mainnet), "tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj");
    v.check("merkle_root([transfer txid])", merkle_root(&[tx.txid()]), "53e021906eeffc0562bf1e9e52249c620d33a011d09a322d7d92147b17eff617");

    // The reference builder produces the same transaction with the "low" priority at 1×.
    let policy = FeePolicy { base_fee: 1_000, fee_per_kb: 10_000, fee_per_kfuel: 1_000, congestion_bp: 10_000, priority_bp: 10_000 };
    let built = build_tx(&k.secret_key(0, 0), Network::Mainnet, 0, &policy, 0, 0, tx.body.action.clone());
    assert_eq!(built, tx);
    // "normal" priority (12 500 bp) pays ceil(min × 1.25).
    let normal = FeePolicy { priority_bp: 12_500, ..policy.clone() };
    v.check("transfer fee (normal priority)", normal.fee(tx.size(), 0), "3288");

    // Same payment, replaceable (FLAG_REPLACEABLE), nonce 1.
    let body = TxBody {
        version: TX_VERSION,
        chain_id: MAINNET.chain_id,
        flags: FLAG_REPLACEABLE,
        nonce: 1,
        fee: 0,
        expiry_height: 0,
        action: TxAction::Transfer { to, amount: 150_000_000, memo: b"test".to_vec() },
    };
    let rtx = sign_with_min_fee(&k, body);
    assert!(rtx.is_replaceable());
    v.check("replaceable fee", rtx.body.fee, "2630");
    v.check("replaceable body", hex::encode(borsh::to_vec(&rtx.body).unwrap()), "0101004354010100000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f008000000000400000074657374");
    v.check("replaceable tx", hex::encode(rtx.to_bytes()), "0101004354010100000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f00800000000040000007465737467a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe06a51c9ba4cbad1ffae073abd7b0418daff5aff0359998f74f5018f80f7490bd677b801e24a7336f0df63040f6a01ac87e952cebc913be442c088332c0f4165809");
    v.check("replaceable txid", rtx.txid(), "e3d311122d703363416d23c266dde108b19a47b756359117b3d388f9cfa7dcbe");
    // Minimum fee of a replacement: at least 25% more (and at least +1 mote).
    v.check("replacement min fee", rtx.body.fee + rtx.body.fee / 4, "3287");
    v.finish();
}

#[test]
fn invoke_vectors() {
    let k = keys();
    let sender = k.address(0, 0);
    let mut v = Vectors::new();
    // Contract deployed by the sender with nonce 0.
    let contract = program_address(&sender, 0);
    v.check("program_address(sender, 0) hex", contract.to_hex(), "ae2db50538a731352cb1d695dbba1e4eeac5ffd4");
    v.check("program_address(sender, 0) mainnet", contract.encode(Network::Mainnet), "tc14ckm2pfc5ucn2t93662ahws7fm4vtl75u534g7");

    let body = TxBody {
        version: TX_VERSION,
        chain_id: MAINNET.chain_id,
        flags: 0,
        nonce: 2,
        fee: 0,
        expiry_height: 0,
        action: TxAction::Invoke {
            contract,
            function: "transfer".into(),
            args: vec![Value::Address(k.address(0, 1).0), Value::Int(2_500)],
            value: 0,
            max_fuel: 20_000,
            max_deposit: 100_000,
        },
    };
    let tx = sign_with_min_fee(&k, body);
    v.check("invoke size", tx.size(), "225");
    v.check("invoke fee", tx.body.fee, "23250");
    v.check("invoke body", hex::encode(borsh::to_vec(&tx.body).unwrap()), "0101004354000200000000000000d25a000000000000000000000000000007ae2db50538a731352cb1d695dbba1e4eeac5ffd4080000007472616e7366657202000000044200a67d1774885d87731decb1c4743c3b07045400c40900000000000000000000000000000000000000000000204e000000000000a086010000000000");
    v.check("invoke tx", hex::encode(tx.to_bytes()), "0101004354000200000000000000d25a000000000000000000000000000007ae2db50538a731352cb1d695dbba1e4eeac5ffd4080000007472616e7366657202000000044200a67d1774885d87731decb1c4743c3b07045400c40900000000000000000000000000000000000000000000204e000000000000a08601000000000067a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe060a3277de1e233517d871382e9c26e9b9728a902033ae9fbc49ae8165c1a1dedb4f801f92127eeea68f254bec07b3addfef9ca49c99dd5200fb17ae5e259a670d");
    v.check("invoke txid", tx.txid(), "8d6fe1b36f4470a75d111c42b8d1f96e31113e90b03a60b09070df6c0518981b");
    v.finish();
}

#[test]
fn hash_and_id_vectors() {
    let k = keys();
    let sender = k.address(0, 0);
    let mut v = Vectors::new();
    v.check("contract_id(sender, 0)", contract_id(&sender, 0), "81b9fbe7ce8cc6ee852d1b0a8df820e5279e127865bf00a51dd03872f3773f6a");
    v.check("proposal_id(sender, 0)", proposal_id(&sender, 0), "e7aa283fb54f5d9c6c95c57c1f148faf22f0eebc91cb905b473f144ec6675903");
    v.check("tagged_hash(txid, \"\")", tagged_hash("txid", &[]), "38aa34a475f8d3b9c7b15275271dce321fc9e6835e273e6672c9ec3c4b653b6a");
    v.check(
        "tagged_hash(block, \"abc\")",
        tagged_hash("block", &[b"abc"]),
        "09bb3b792db7c48af43cdb15ba3f1b274440d964372789fadcc4d8da9c1679d8",
    );
    v.check("merkle_root([])", merkle_root(&[]), "5740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1");
    v.check("address(pubkey 32 x 00)", Address::from_public_key(&[0u8; 32]).to_hex(), "7b32d1267e52aebd6fde6e0bdc70e394c133183e");
    v.check("LtHash empty root", LtHash::default().root(), "0fe12e22656003a7066f77e6b814dde5649b40f99249c737fc256a62277b9b1f");
    let mut h = LtHash::default();
    h.insert(b"k", b"v");
    v.check("LtHash {k:v} root", h.root(), "2ab3e575a30d01995680fbd29443b525bb861f070d548c118012a044a6d61435");
    v.finish();
}

#[test]
fn genesis_vectors() {
    let mut v = Vectors::new();
    // (network, genesis hash, prev_hash, state_root, header hex, CoinHash of the header)
    for (n, hash, prev, root, header, coinhash) in [
        (
            Network::Mainnet,
            "50abdff604b338a25290a38dc0b7f8c80efd2e76f7c1f178b8bc6d9db8dade4d",
            "628153829eca06c2fef7546b35d52d4308bc7d73e857b7c0d192518d3fa10de7",
            "e64e387d8fde654f6f1f92f7c5a6463e9318e74a0d875bf68936070c453e21d9",
            "010000000000000000000000628153829eca06c2fef7546b35d52d4308bc7d73e857b7c0d192518d3fa10de75740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1e64e387d8fde654f6f1f92f7c5a6463e9318e74a0d875bf68936070c453e21d980e7a56a00000000000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000000000005740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1",
            "da2dd8405c4bbaceabebf505f10e8b6af4a9e2852e55764b36637717cce4fe1e",
        ),
        (
            Network::Testnet,
            "c27b8a615ff411288b43388e735c381fcbf42d37eb731c4b2e98964f8d87090e",
            "8c4c3fc095f0fd4447d797cef32537b87b91ecc70ab422f237efe6f49460ef09",
            "9994189185edb75bf04ea1cc403b25fc83fb26c3e2c4381aa1c2fcfda2127b20",
            "",
            "",
        ),
        (
            Network::Regtest,
            "fde0983b16398101218dcffdaeb26f5d1748e6ed194c59856ade97cb2f6674b0",
            "770efbde639a4320419a8fb4c18ac2d2fb57c5e6d6a38d77b870945adbe7ca72",
            "b87872c9e9506f8be2d05dce0b41b6d7cefcfb8c9c59c7071ceb53d1e31cf04f",
            "010000000000000000000000770efbde639a4320419a8fb4c18ac2d2fb57c5e6d6a38d77b870945adbe7ca725740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1b87872c9e9506f8be2d05dce0b41b6d7cefcfb8c9c59c7071ceb53d1e31cf04f80e7a56a00000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000000000005740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1",
            "6d3a31871da1e601ce21910068b4c74a1ee1f35b9f9ad307a9e59854893baf7c",
        ),
    ] {
        let p = n.params();
        let g = genesis_block(p);
        v.check(&format!("{n} genesis hash"), g.hash(), hash);
        v.check(&format!("{n} prev_hash"), g.header.prev_hash, prev);
        v.check(&format!("{n} state_root"), g.header.state_root, root);
        if !header.is_empty() {
            let bytes = g.header.to_bytes();
            assert_eq!(bytes.len(), 212);
            v.check(&format!("{n} header"), hex::encode(&bytes), header);
            v.check(&format!("{n} CoinHash"), hex::encode(pow_hash(p.chain_id, p.pow, 0, &bytes)), coinhash);
        }
    }
    v.finish();
}
