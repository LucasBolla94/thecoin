//! CoinHash (RandomX) cost for verification and mining.
//! `cargo test -p thecoin-core --release --test pow_bench -- --ignored --nocapture`

use std::time::Instant;
use thecoin_core::params::MAINNET;
use thecoin_core::pow::{PowHasher, PowMode};

fn measure(mode: PowMode, rounds: u32) -> (f64, PowMode) {
    let t0 = Instant::now();
    let mut h = PowHasher::with_mode(MAINNET.chain_id, MAINNET.pow, 0, mode);
    let setup = t0.elapsed();
    let got = h.mode();
    let mut header = [7u8; 180];
    let t = Instant::now();
    for i in 0..rounds {
        header[..4].copy_from_slice(&i.to_le_bytes());
        std::hint::black_box(h.hash(0, &header));
    }
    let per = t.elapsed().as_secs_f64() / rounds as f64;
    println!("{got:?}: setup {setup:.1?}, {:.1} ms/hash, {:.1} H/s/thread", per * 1e3, 1.0 / per);
    (per, got)
}

#[test]
#[ignore]
fn coinhash_cost() {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!("machine: {cores} logical cores");
    let (light, _) = measure(PowMode::Light, 30);
    println!("  verifying 1 000 blocks: {:.0} s", light * 1000.0);
    let (fast, got) = measure(PowMode::Fast, 30);
    if got == PowMode::Fast {
        println!("  fast mode is {:.1}x the light speed", light / fast);
    } else {
        println!("  fast mode unavailable on this machine (needs ~2.5 GiB free)");
    }
}
