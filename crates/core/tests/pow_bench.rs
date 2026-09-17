//! CoinHash cost for several Argon2id parameters (mining and verification).
//! `cargo test -p thecoin-core --release --test pow_bench -- --ignored --nocapture`

use std::time::Instant;
use thecoin_core::pow::{PowHasher, PowParams};

fn measure(mem_kib: u32, iterations: u32, rounds: u32) -> f64 {
    let mut h = PowHasher::new(PowParams { mem_kib, iterations });
    let mut header = [7u8; 180];
    let t = Instant::now();
    for i in 0..rounds {
        header[..4].copy_from_slice(&i.to_le_bytes());
        std::hint::black_box(h.hash(&header));
    }
    t.elapsed().as_secs_f64() / rounds as f64
}

#[test]
#[ignore]
fn coinhash_cost() {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!("machine: {cores} logical cores");
    println!("{:>8} {:>3} {:>10} {:>10} {:>12} {:>14}", "mem", "t", "s/hash", "H/s/core", "MiB/s/core", "verify 1000 blk");
    for (mem_mib, iters) in [(16, 1), (16, 2), (32, 1), (64, 1), (128, 1), (256, 1), (512, 1)] {
        let mem_kib = mem_mib * 1024;
        let rounds = if mem_mib <= 64 { 30 } else { 10 };
        let s = measure(mem_kib, iters, rounds);
        println!(
            "{:>6} MiB {:>3} {:>10.4} {:>10.1} {:>12.0} {:>13.1}s",
            mem_mib,
            iters,
            s,
            1.0 / s,
            mem_mib as f64 * iters as f64 / s,
            s * 1000.0
        );
    }
}
