//! **CoinHash** — The Coin's proof-of-work function.
//!
//! `CoinHash(header) = Argon2id(password = borsh(header), salt = "TheCoin/PoW/v1..",
//!                             m = mem_kib, t = iterations, p = 1, len = 32)`
//!
//! Argon2id (RFC 9106, winner of the Password Hashing Competition) is
//! *memory-hard*: each hash needs `mem_kib` of RAM filled in a data-dependent
//! order. That makes CPUs of cheap VPS machines competitive and removes most of
//! the advantage of GPUs/ASICs, while a single hash stays cheap enough (a few
//! milliseconds) for low-end nodes to verify every block.
//!
//! The 32-byte output is interpreted as a big-endian 256-bit integer and must be
//! `<= target`.

use crate::U256;
use argon2::{Algorithm, Argon2, Block as ArgonBlock, Params, Version};

/// Fixed 16-byte salt. The header already contains the previous block hash, so
/// no per-block salt is required.
pub const POW_SALT: &[u8; 16] = b"TheCoin/PoW/v1..";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowParams {
    /// Argon2 memory in KiB.
    pub mem_kib: u32,
    /// Argon2 passes.
    pub iterations: u32,
}

/// Reusable hasher that keeps its Argon2 memory allocated between hashes.
/// Create one per mining/verification thread.
pub struct PowHasher {
    argon: Argon2<'static>,
    memory: Vec<ArgonBlock>,
}

impl PowHasher {
    pub fn new(p: PowParams) -> Self {
        let params = Params::new(p.mem_kib, p.iterations, 1, Some(32)).expect("valid argon2 params");
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let memory = vec![ArgonBlock::default(); p.mem_kib as usize];
        PowHasher { argon, memory }
    }

    /// Computes CoinHash of already-serialized header bytes.
    pub fn hash(&mut self, header_bytes: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        self.argon
            .hash_password_into_with_memory(header_bytes, POW_SALT, &mut out, &mut self.memory)
            .expect("argon2 with valid params cannot fail");
        out
    }
}

/// One-shot CoinHash (allocates memory each call).
pub fn pow_hash(p: PowParams, header_bytes: &[u8]) -> [u8; 32] {
    PowHasher::new(p).hash(header_bytes)
}

pub fn hash_to_u256(hash: &[u8; 32]) -> U256 {
    U256::from_big_endian(hash)
}

pub fn meets_target(hash: &[u8; 32], target: &U256) -> bool {
    hash_to_u256(hash) <= *target
}

/// Expected number of hashes needed to find a block at `target`
/// (`2^256 / (target + 1)`), used as the "work" of a block.
pub fn work_for_target(target: &U256) -> U256 {
    if *target == U256::MAX {
        return U256::one();
    }
    // (~target / (target + 1)) + 1 == 2^256 / (target + 1)
    (!*target / (*target + U256::one())) + U256::one()
}

/// Human-friendly difficulty: work relative to target = 2^256 - 1 (difficulty 1).
pub fn difficulty_from_target(target: &U256) -> f64 {
    let work = work_for_target(target);
    u256_to_f64(&work)
}

pub fn u256_to_f64(v: &U256) -> f64 {
    let mut out = 0f64;
    for (i, limb) in v.0.iter().enumerate() {
        out += (*limb as f64) * 2f64.powi(64 * i as i32);
    }
    out
}

pub fn u256_to_bytes(v: &U256) -> [u8; 32] {
    let mut b = [0u8; 32];
    v.to_big_endian(&mut b);
    b
}

pub fn u256_from_bytes(b: &[u8; 32]) -> U256 {
    U256::from_big_endian(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let p = PowParams { mem_kib: 64, iterations: 1 };
        let a = pow_hash(p, b"header");
        let mut h = PowHasher::new(p);
        assert_eq!(a, h.hash(b"header"));
        assert_eq!(a, h.hash(b"header"));
        assert_ne!(a, h.hash(b"header2"));
    }

    #[test]
    fn work() {
        assert_eq!(work_for_target(&U256::MAX), U256::one());
        let t = U256::MAX >> 10;
        assert_eq!(work_for_target(&t), U256::from(1024u64));
    }

    /// `cargo test -p thecoin-core --release -- --ignored bench_pow --nocapture`
    #[test]
    #[ignore]
    fn bench_pow() {
        for mem in [4096u32, 8192, 16384, 32768] {
            let mut h = PowHasher::new(PowParams { mem_kib: mem, iterations: 1 });
            let n = 200u32;
            let start = std::time::Instant::now();
            for i in 0..n {
                h.hash(&i.to_le_bytes());
            }
            let el = start.elapsed();
            println!("mem={mem}KiB: {:.2} ms/hash, {:.1} H/s", el.as_secs_f64() * 1000.0 / n as f64, n as f64 / el.as_secs_f64());
        }
    }
}
