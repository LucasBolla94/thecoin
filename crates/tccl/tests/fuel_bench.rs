//! Wall-clock cost of fuel for the most expensive operations. The block fuel
//! limit must keep the worst-case block execution time small on a 2 vCPU VPS.
//! `cargo test -p tccl --release --test fuel_bench -- --ignored --nocapture`

use std::collections::HashMap;
use std::time::Instant;
use tccl::program::Value;
use tccl::vm::{self, CallContext, Host, Mode};
use tccl::{compile, CompileOptions, VmError};

#[derive(Default)]
struct MemHost {
    storage: HashMap<Vec<u8>, Vec<u8>>,
}

impl Host for MemHost {
    fn storage_read(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>, VmError> {
        Ok(self.storage.get(key).cloned())
    }
    fn storage_write(&mut self, key: &[u8], value: Option<Vec<u8>>) -> Result<(), VmError> {
        match value {
            Some(v) => self.storage.insert(key.to_vec(), v),
            None => self.storage.remove(key),
        };
        Ok(())
    }
    fn balance(&mut self) -> Result<u64, VmError> {
        Ok(0)
    }
    fn send(&mut self, _: &[u8; 20], _: u64) -> Result<(), VmError> {
        Ok(())
    }
    fn emit(&mut self, _: &str, _: Vec<(String, Value)>) -> Result<(), VmError> {
        Ok(())
    }
    fn storage_items(&mut self) -> Result<u64, VmError> {
        Ok(self.storage.len() as u64)
    }
    fn destroy(&mut self, _: &[u8; 20]) -> Result<(), VmError> {
        Ok(())
    }
}

const FUEL: u64 = 10_000_000;

fn measure(name: &str, src: &str, function: &str, args: Vec<Value>) -> f64 {
    let program = compile(src, &CompileOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let ctx = CallContext { caller: [1; 20], value: 0, height: 1, self_address: [2; 20] };
    let mut host = MemHost::default();
    let t = Instant::now();
    let out = vm::execute(&program, Mode::Action, function, args, &ctx, &mut host, FUEL);
    let el = t.elapsed();
    let ns_per_fuel = el.as_nanos() as f64 / out.fuel_used.max(1) as f64;
    println!(
        "{name:<28} fuel {:>9}  time {:>8.1} ms  {:>6.1} ns/fuel  → 50M-fuel block {:>6.2} s  ({:?})",
        out.fuel_used,
        el.as_secs_f64() * 1e3,
        ns_per_fuel,
        ns_per_fuel * 50e6 / 1e9,
        out.result.as_ref().err()
    );
    ns_per_fuel
}

#[test]
#[ignore]
fn fuel_cost_of_expensive_operations() {
    let mut worst: f64 = 0.0;
    worst = worst.max(measure(
        "arithmetic loop",
        "contract B\n\naction run():\n    let x: int = 0\n    while true:\n        x = (x * 3 + 7) % 1000003\n",
        "run",
        vec![],
    ));
    worst = worst.max(measure(
        "function calls",
        "contract B\n\nfn f(a: int) -> int:\n    return a + 1\n\naction run():\n    let x: int = 0\n    while true:\n        x = f(f(f(x)))\n",
        "run",
        vec![],
    ));
    worst = worst.max(measure(
        "sha256 64 kB",
        "contract B\n\naction run(d: bytes):\n    while true:\n        let h: bytes = sha256(d)\n",
        "run",
        vec![Value::Bytes(vec![7; 60_000])],
    ));
    worst = worst.max(measure(
        "blake3 small",
        "contract B\n\naction run():\n    let h: bytes = 0x01\n    while true:\n        h = blake3(h)\n",
        "run",
        vec![],
    ));
    worst = worst.max(measure(
        "sha256 small",
        "contract B\n\naction run():\n    let h: bytes = 0x01\n    while true:\n        h = sha256(h)\n",
        "run",
        vec![],
    ));
    let key = ed25519_dalek::SigningKey::from_bytes(&[9; 32]);
    use ed25519_dalek::Signer;
    let sig = key.sign(b"m").to_bytes().to_vec();
    worst = worst.max(measure(
        "verify_ed25519",
        "contract B\n\naction run(pk: bytes, sig: bytes):\n    while true:\n        let ok: bool = verify_ed25519(pk, 0x6d, sig)\n",
        "run",
        vec![Value::Bytes(key.verifying_key().to_bytes().to_vec()), Value::Bytes(sig)],
    ));
    let keys: Vec<([u8; 32], [u8; 32])> = (0..64).map(|i| tccl::ring::keypair_from_seed(&[i as u8 + 1; 32])).collect();
    let ring: Vec<[u8; 32]> = keys.iter().map(|k| k.1).collect();
    let (rsig, ki) = tccl::ring::sign(b"m", &ring, 5, &keys[5].0, &mut rand::thread_rng()).unwrap();
    worst = worst.max(measure(
        "ring_verify 64 members",
        "contract B\n\naction run(ring: list[bytes], sig: bytes, ki: bytes):\n    while true:\n        let ok: bool = ring_verify(ring, 0x6d, sig, ki)\n",
        "run",
        vec![Value::List(ring.iter().map(|k| Value::Bytes(k.to_vec())).collect()), Value::Bytes(rsig), Value::Bytes(ki.to_vec())],
    ));
    let small_ring = ring[..2].to_vec();
    let (rsig2, ki2) = tccl::ring::sign(b"m", &small_ring, 1, &keys[1].0, &mut rand::thread_rng()).unwrap();
    worst = worst.max(measure(
        "ring_verify 2 members",
        "contract B\n\naction run(ring: list[bytes], sig: bytes, ki: bytes):\n    while true:\n        let ok: bool = ring_verify(ring, 0x6d, sig, ki)\n",
        "run",
        vec![Value::List(small_ring.iter().map(|k| Value::Bytes(k.to_vec())).collect()), Value::Bytes(rsig2), Value::Bytes(ki2.to_vec())],
    ));
    worst = worst.max(measure(
        "big value copies",
        "contract B\n\naction run(d: bytes):\n    while true:\n        let a: bytes = d\n        let b: int = len(a + a)\n",
        "run",
        vec![Value::Bytes(vec![7; 32_000])],
    ));
    worst = worst.max(measure(
        "list building",
        "contract B\n\naction run():\n    while true:\n        let l: list[int] = []\n        for i in range(0, 4000):\n            l.push(i)\n",
        "run",
        vec![],
    ));
    worst = worst.max(measure(
        "map writes",
        "contract B\nstate m: map[int, bytes]\n\naction run(d: bytes):\n    let i: int = 0\n    while true:\n        m[i] = d\n        i += 1\n",
        "run",
        vec![Value::Bytes(vec![7; 1_000])],
    ));
    worst = worst.max(measure(
        "map reads",
        "contract B\nstate m: map[int, int]\n\naction run():\n    let i: int = 0\n    let s: int = 0\n    while true:\n        s += m[i]\n        i += 1\n",
        "run",
        vec![],
    ));
    worst = worst.max(measure(
        "text building",
        "contract B\n\naction run():\n    while true:\n        let t: text = \"\"\n        for i in range(0, 500):\n            t = t + to_text(i)\n",
        "run",
        vec![],
    ));
    println!("worst: {worst:.1} ns/fuel → a full 50M-fuel block ≈ {:.2} s of execution", worst * 50e6 / 1e9);
}
