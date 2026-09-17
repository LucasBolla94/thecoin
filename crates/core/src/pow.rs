//! **CoinHash** — The Coin's proof of work: RandomX with a key that rotates per
//! epoch.
//!
//! ```text
//! epoch          = height / pow.epoch_blocks
//! key            = tagged_hash("randomx-key", chain_id ‖ epoch)
//! CoinHash(head) = RandomX(key, borsh(header))
//! ```
//!
//! **Why RandomX.** The Coin is meant to be mined by ordinary computers. A
//! memory-hard hash like Argon2id is bound by memory *bandwidth*, and a graphics
//! card has ~1 000 GB/s against ~2 GB/s of a small server, so one card is worth
//! tens to hundreds of CPUs. RandomX instead executes a random program made of
//! the operations a CPU is good at (branches, integer and floating point
//! arithmetic, latency-bound memory access), so a graphics card gains little or
//! nothing: mining stays with ordinary processors, which is what keeps the
//! network hard to take over.
//!
//! **Two modes.** Verification (every node, including small VPS machines) uses
//! *light mode*: a 256 MiB cache shared by every thread, a few tens of
//! milliseconds per block. Mining can use *fast mode*, a 2 GiB dataset that is
//! several times faster, when the machine has the memory to spare.
//!
//! **Why the key comes from the height, not from a past block.** The epoch key
//! is derived from the chain id and the epoch number alone, so any node can
//! verify a header knowing only its height, without reading the chain, and
//! reorganisations never change a block's key. Predictability costs nothing: the
//! dataset is public and everyone can compute it in about a minute.
//!
//! The 32-byte output is a big-endian 256-bit integer and must be `<= target`.

use crate::hash::tagged_hash;
use crate::U256;
use randomx_rs::{RandomXCache, RandomXDataset, RandomXFlag, RandomXVM};
use std::sync::{Arc, Mutex, OnceLock};

/// Domain tag of the epoch key.
pub const POW_KEY_TAG: &str = "randomx-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowParams {
    /// Blocks between key rotations (one RandomX epoch).
    pub epoch_blocks: u64,
}

/// Key of the epoch that contains `height`.
pub fn epoch_key(chain_id: u32, p: PowParams, height: u64) -> [u8; 32] {
    let epoch = height / p.epoch_blocks.max(1);
    tagged_hash(POW_KEY_TAG, &[&chain_id.to_le_bytes(), &epoch.to_le_bytes()]).0
}

/// A RandomX cache (light mode) or dataset (fast mode) shared by every thread.
///
/// RandomX caches and datasets are read-only once initialised, and the C library
/// documents sharing them between virtual machines in different threads; the
/// Rust wrapper only lacks the `Send`/`Sync` marks.
struct Shared<T>(T);
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}

type SharedCache = Arc<Shared<RandomXCache>>;
type SharedDataset = Arc<Shared<RandomXDataset>>;

fn light_flags() -> RandomXFlag {
    RandomXFlag::get_recommended_flags()
}

/// Caches in use, newest last. Two are kept so that blocks around an epoch
/// boundary (and short reorganisations) do not rebuild the cache every time.
fn cache_registry() -> &'static Mutex<Vec<([u8; 32], SharedCache)>> {
    static R: OnceLock<Mutex<Vec<([u8; 32], SharedCache)>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}

/// The cache for `key`, building it (about one second) only if no thread has it.
fn shared_cache(key: &[u8; 32]) -> SharedCache {
    let mut reg = cache_registry().lock().expect("cache registry");
    if let Some(pos) = reg.iter().position(|(k, _)| k == key) {
        let entry = reg.remove(pos);
        reg.push(entry.clone());
        return entry.1;
    }
    let cache = RandomXCache::new(light_flags(), key).expect("RandomX cache allocation failed");
    let shared: SharedCache = Arc::new(Shared(cache));
    reg.push((*key, shared.clone()));
    if reg.len() > 2 {
        reg.remove(0);
    }
    shared
}

/// How a hasher uses memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowMode {
    /// 256 MiB cache: what every node uses to verify blocks.
    Light,
    /// 2 GiB dataset: several times faster, for mining on machines with spare memory.
    Fast,
}

/// Computes CoinHash. Create one per thread; the heavy memory is shared.
pub struct PowHasher {
    chain_id: u32,
    params: PowParams,
    mode: PowMode,
    key: [u8; 32],
    cache: SharedCache,
    dataset: Option<SharedDataset>,
    vm: RandomXVM,
}

/// A hasher owns its virtual machine and uses it from one thread at a time, so
/// it can be moved between threads (the RandomX library only forbids using the
/// *same* virtual machine from two threads at once). It is deliberately not
/// `Sync`: `&PowHasher` cannot be shared.
unsafe impl Send for PowHasher {}

impl PowHasher {
    /// Light-mode hasher (verification).
    pub fn new(chain_id: u32, params: PowParams) -> Self {
        Self::with_mode(chain_id, params, 0, PowMode::Light)
    }

    /// Hasher for a given height and mode. `PowMode::Fast` builds the 2 GiB
    /// dataset of the epoch (about a minute) and falls back to light mode if the
    /// memory cannot be allocated.
    pub fn with_mode(chain_id: u32, params: PowParams, height: u64, mode: PowMode) -> Self {
        let key = epoch_key(chain_id, params, height);
        let cache = shared_cache(&key);
        let (mode, dataset, vm) = Self::build(&key, cache.clone(), mode);
        PowHasher { chain_id, params, mode, key, cache, dataset, vm }
    }

    fn build(key: &[u8; 32], cache: SharedCache, mode: PowMode) -> (PowMode, Option<SharedDataset>, RandomXVM) {
        if mode == PowMode::Fast {
            if let Some(dataset) = shared_dataset(key, &cache) {
                let flags = light_flags() | RandomXFlag::FLAG_FULL_MEM;
                if let Ok(vm) = RandomXVM::new(flags, None, Some(dataset.0.clone())) {
                    return (PowMode::Fast, Some(dataset), vm);
                }
            }
        }
        let vm = RandomXVM::new(light_flags(), Some(cache.0.clone()), None).expect("RandomX VM creation failed");
        (PowMode::Light, None, vm)
    }

    pub fn mode(&self) -> PowMode {
        self.mode
    }

    /// CoinHash of already-serialized header bytes at `height`.
    pub fn hash(&mut self, height: u64, header_bytes: &[u8]) -> [u8; 32] {
        let key = epoch_key(self.chain_id, self.params, height);
        if key != self.key {
            self.cache = shared_cache(&key);
            let (mode, dataset, vm) = Self::build(&key, self.cache.clone(), self.mode);
            self.mode = mode;
            self.dataset = dataset;
            self.vm = vm;
            self.key = key;
        }
        let out = self.vm.calculate_hash(header_bytes).expect("RandomX hashing failed");
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&out);
        hash
    }
}

/// Datasets are huge (2 GiB), so at most one is kept.
fn dataset_registry() -> &'static Mutex<Option<([u8; 32], SharedDataset)>> {
    static R: OnceLock<Mutex<Option<([u8; 32], SharedDataset)>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

/// The 2 GiB dataset for `key`, or `None` when it cannot be allocated.
fn shared_dataset(key: &[u8; 32], cache: &SharedCache) -> Option<SharedDataset> {
    let mut reg = dataset_registry().lock().expect("dataset registry");
    if let Some((k, d)) = reg.as_ref() {
        if k == key {
            return Some(d.clone());
        }
    }
    let flags = light_flags() | RandomXFlag::FLAG_FULL_MEM;
    let dataset = RandomXDataset::new(flags, cache.0.clone(), 0).ok()?;
    let shared: SharedDataset = Arc::new(Shared(dataset));
    *reg = Some((*key, shared.clone()));
    Some(shared)
}

/// One-shot CoinHash (allocates and discards a VM; for tools and tests).
pub fn pow_hash(chain_id: u32, params: PowParams, height: u64, header_bytes: &[u8]) -> [u8; 32] {
    PowHasher::new(chain_id, params).hash(height, header_bytes)
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

    const P: PowParams = PowParams { epoch_blocks: 2_048 };

    #[test]
    fn deterministic_and_key_rotates() {
        let a = pow_hash(7, P, 0, b"header");
        let mut h = PowHasher::new(7, P);
        assert_eq!(a, h.hash(0, b"header"));
        assert_eq!(a, h.hash(P.epoch_blocks - 1, b"header"), "same epoch, same key");
        assert_ne!(a, h.hash(1, b"header2"), "different input");
        assert_ne!(a, h.hash(P.epoch_blocks, b"header"), "next epoch, different key");
        assert_eq!(a, h.hash(0, b"header"), "back to the first epoch");
        assert_ne!(a, pow_hash(8, P, 0, b"header"), "different network");
    }

    #[test]
    fn epoch_keys() {
        assert_eq!(epoch_key(1, P, 0), epoch_key(1, P, P.epoch_blocks - 1));
        assert_ne!(epoch_key(1, P, 0), epoch_key(1, P, P.epoch_blocks));
        assert_eq!(epoch_key(1, P, P.epoch_blocks), epoch_key(1, P, P.epoch_blocks + 5));
    }

    #[test]
    fn work() {
        assert_eq!(work_for_target(&U256::MAX), U256::one());
        let t = U256::MAX >> 10;
        assert_eq!(work_for_target(&t), U256::from(1024u64));
    }
}
