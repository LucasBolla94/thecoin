//! Linkable ring signatures (bLSAG over Ristretto255).
//!
//! A ring signature proves "the signer owns the secret key of ONE of these N
//! public keys" without revealing which one. The **key image**
//! `I = x · Hp(P)` is unique per secret key, so the same key cannot sign twice
//! without being noticed (contracts store used key images) — yet the key image
//! reveals nothing about which ring member produced it.
//!
//! This is the primitive behind privacy contracts in TCCL (`ring_verify`).
//! Construction: Back's LSAG (bLSAG) as used by Monero, on the prime-order
//! Ristretto group (no cofactor pitfalls). All hashes are domain separated.

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT as G;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::{Identity, IsIdentity};
use sha2::{Digest, Sha512};

const HP_DOMAIN: &[u8] = b"TheCoin:ring:hash-to-point\x00";
const HS_DOMAIN: &[u8] = b"TheCoin:ring:challenge\x00";
const KEY_DOMAIN: &[u8] = b"TheCoin:ring:secret\x00";

fn hash_to_point(p: &CompressedRistretto) -> RistrettoPoint {
    let mut data = Vec::with_capacity(HP_DOMAIN.len() + 32);
    data.extend_from_slice(HP_DOMAIN);
    data.extend_from_slice(p.as_bytes());
    RistrettoPoint::hash_from_bytes::<Sha512>(&data)
}

fn ring_digest(ring: &[CompressedRistretto]) -> [u8; 64] {
    let mut h = Sha512::new();
    h.update(b"TheCoin:ring:members\x00");
    for p in ring {
        h.update(p.as_bytes());
    }
    h.finalize().into()
}

fn challenge(ring_hash: &[u8; 64], msg: &[u8], key_image: &CompressedRistretto, l: &RistrettoPoint, r: &RistrettoPoint) -> Scalar {
    let mut h = Sha512::new();
    h.update(HS_DOMAIN);
    h.update(ring_hash);
    h.update((msg.len() as u64).to_le_bytes());
    h.update(msg);
    h.update(key_image.as_bytes());
    h.update(l.compress().as_bytes());
    h.update(r.compress().as_bytes());
    Scalar::from_hash(h)
}

/// Derives a ring key pair from 32 bytes of seed material.
/// Returns `(secret, public)`.
pub fn keypair_from_seed(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let mut h = Sha512::new();
    h.update(KEY_DOMAIN);
    h.update(seed);
    let x = Scalar::from_hash(h);
    (x.to_bytes(), (x * G).compress().to_bytes())
}

pub fn public_key(secret: &[u8; 32]) -> Option<[u8; 32]> {
    let x = Option::<Scalar>::from(Scalar::from_canonical_bytes(*secret))?;
    Some((x * G).compress().to_bytes())
}

/// Key image of a secret key (the same for every signature by this key).
pub fn key_image(secret: &[u8; 32]) -> Option<[u8; 32]> {
    let x = Option::<Scalar>::from(Scalar::from_canonical_bytes(*secret))?;
    let p = (x * G).compress();
    Some((x * hash_to_point(&p)).compress().to_bytes())
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RingError {
    #[error("ring must have between 1 and 64 members")]
    BadRingSize,
    #[error("signer index out of range")]
    BadIndex,
    #[error("invalid secret key")]
    BadSecret,
    #[error("the secret key does not match the ring member at the signer index")]
    KeyMismatch,
    #[error("invalid public key in ring")]
    BadPublicKey,
}

/// Signs `msg` with the secret key of `ring[index]`.
/// Returns `(signature, key_image)`; the signature is `32 × (n + 1)` bytes.
pub fn sign<R: rand::RngCore + rand::CryptoRng>(msg: &[u8], ring: &[[u8; 32]], index: usize, secret: &[u8; 32], rng: &mut R) -> Result<(Vec<u8>, [u8; 32]), RingError> {
    let n = ring.len();
    if n == 0 || n > crate::vm::MAX_RING_SIZE {
        return Err(RingError::BadRingSize);
    }
    if index >= n {
        return Err(RingError::BadIndex);
    }
    let x = Option::<Scalar>::from(Scalar::from_canonical_bytes(*secret)).ok_or(RingError::BadSecret)?;
    let compressed: Vec<CompressedRistretto> = ring.iter().map(|k| CompressedRistretto(*k)).collect();
    let points: Vec<RistrettoPoint> = compressed.iter().map(|c| c.decompress().ok_or(RingError::BadPublicKey)).collect::<Result<_, _>>()?;
    if (x * G).compress() != compressed[index] {
        return Err(RingError::KeyMismatch);
    }
    let hp: Vec<RistrettoPoint> = compressed.iter().map(hash_to_point).collect();
    let image_point = x * hp[index];
    let image = image_point.compress();
    let ring_hash = ring_digest(&compressed);

    let mut c = vec![Scalar::ZERO; n];
    let mut r = vec![Scalar::ZERO; n];
    let alpha = Scalar::random(rng);
    let mut next = (index + 1) % n;
    c[next] = challenge(&ring_hash, msg, &image, &(alpha * G), &(alpha * hp[index]));
    while next != index {
        r[next] = Scalar::random(rng);
        let l = r[next] * G + c[next] * points[next];
        let rr = r[next] * hp[next] + c[next] * image_point;
        let following = (next + 1) % n;
        c[following] = challenge(&ring_hash, msg, &image, &l, &rr);
        next = following;
    }
    r[index] = alpha - c[index] * x;

    let mut sig = Vec::with_capacity(32 * (n + 1));
    sig.extend_from_slice(c[0].as_bytes());
    for s in &r {
        sig.extend_from_slice(s.as_bytes());
    }
    Ok((sig, image.to_bytes()))
}

/// Verifies a ring signature. Returns false for any malformed input.
pub fn verify(msg: &[u8], ring: &[[u8; 32]], sig: &[u8], key_image: &[u8; 32]) -> bool {
    let n = ring.len();
    if n == 0 || n > crate::vm::MAX_RING_SIZE || sig.len() != 32 * (n + 1) {
        return false;
    }
    let compressed: Vec<CompressedRistretto> = ring.iter().map(|k| CompressedRistretto(*k)).collect();
    let mut points = Vec::with_capacity(n);
    for c in &compressed {
        match c.decompress() {
            Some(p) if !p.is_identity() => points.push(p),
            _ => return false,
        }
    }
    let image = CompressedRistretto(*key_image);
    let Some(image_point) = image.decompress() else { return false };
    if image_point.is_identity() || image_point == RistrettoPoint::identity() {
        return false;
    }
    let scalar_at = |i: usize| -> Option<Scalar> {
        let bytes: [u8; 32] = sig[i * 32..(i + 1) * 32].try_into().ok()?;
        Option::<Scalar>::from(Scalar::from_canonical_bytes(bytes))
    };
    let Some(c0) = scalar_at(0) else { return false };
    let ring_hash = ring_digest(&compressed);
    let mut c = c0;
    for i in 0..n {
        let Some(r) = scalar_at(i + 1) else { return false };
        let l = r * G + c * points[i];
        let rr = r * hash_to_point(&compressed[i]) + c * image_point;
        c = challenge(&ring_hash, msg, &image, &l, &rr);
    }
    c == c0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(n: usize) -> Vec<([u8; 32], [u8; 32])> {
        (0..n).map(|i| keypair_from_seed(&[i as u8 + 1; 32])).collect()
    }

    #[test]
    fn sign_verify_link() {
        let mut rng = rand::thread_rng();
        let ks = keys(8);
        let ring: Vec<[u8; 32]> = ks.iter().map(|k| k.1).collect();
        for signer in [0, 3, 7] {
            let (sig, ki) = sign(b"withdraw to X", &ring, signer, &ks[signer].0, &mut rng).unwrap();
            assert!(verify(b"withdraw to X", &ring, &sig, &ki));
            assert_eq!(ki, key_image(&ks[signer].0).unwrap(), "key image is deterministic per key");
            assert!(!verify(b"withdraw to Y", &ring, &sig, &ki), "message bound");
            let mut other_ring = ring.clone();
            other_ring.swap(1, 2);
            assert!(!verify(b"withdraw to X", &other_ring, &sig, &ki) || signer == 1 || signer == 2);
            let mut bad = sig.clone();
            bad[40] ^= 1;
            assert!(!verify(b"withdraw to X", &ring, &bad, &ki));
            assert!(!verify(b"withdraw to X", &ring, &sig, &key_image(&ks[(signer + 1) % 8].0).unwrap()));
        }
        // Two signatures by the same key share the key image (linkability).
        let (_, a) = sign(b"m1", &ring, 2, &ks[2].0, &mut rng).unwrap();
        let (_, b) = sign(b"m2", &ring[..4], 2, &ks[2].0, &mut rng).unwrap();
        assert_eq!(a, b);
        assert_eq!(sign(b"m", &ring, 1, &ks[2].0, &mut rng).unwrap_err(), RingError::KeyMismatch);
        let one = [ring[5]];
        let (sig, ki) = sign(b"solo", &one, 0, &ks[5].0, &mut rng).unwrap();
        assert!(verify(b"solo", &one, &sig, &ki));
        assert!(!verify(b"solo", &[], &sig, &ki));
    }
}
