//! Account addresses.
//!
//! An address is the first 20 bytes of `tagged_hash("address", public_key)`.
//! Its text form is bech32m (BIP-350) with a network-specific prefix:
//! `tc1...` (mainnet), `tct1...` (testnet), `tcr1...` (regtest). The checksum
//! covers the prefix, so an address of one network is rejected by another.

use crate::hash::{tagged_hash, tags};
use crate::params::Network;
use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32m, Hrp};
use borsh::{BorshDeserialize, BorshSerialize};
use std::fmt;

pub const ADDRESS_LEN: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, BorshSerialize, BorshDeserialize)]
pub struct Address(pub [u8; ADDRESS_LEN]);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AddressError {
    #[error("invalid bech32m encoding")]
    Encoding,
    #[error("address belongs to another network (expected prefix '{expected}', got '{got}')")]
    WrongNetwork { expected: String, got: String },
    #[error("invalid address length")]
    Length,
}

impl Address {
    /// The all-zero address. It has no known private key; coins sent there are
    /// unspendable. It is used as the genesis "miner".
    pub const ZERO: Address = Address([0u8; ADDRESS_LEN]);

    pub fn from_public_key(pk: &[u8; 32]) -> Address {
        let h = tagged_hash(tags::ADDRESS, &[pk]);
        let mut a = [0u8; ADDRESS_LEN];
        a.copy_from_slice(&h.0[..ADDRESS_LEN]);
        Address(a)
    }

    pub fn encode(&self, network: Network) -> String {
        let hrp = Hrp::parse(network.hrp()).expect("static hrp is valid");
        bech32::encode::<Bech32m>(hrp, &self.0).expect("20 bytes always encodable")
    }

    pub fn decode(s: &str, network: Network) -> Result<Address, AddressError> {
        let checked = CheckedHrpstring::new::<Bech32m>(s.trim()).map_err(|_| AddressError::Encoding)?;
        let hrp = checked.hrp().to_lowercase();
        if hrp != network.hrp() {
            return Err(AddressError::WrongNetwork { expected: network.hrp().to_string(), got: hrp });
        }
        let bytes: Vec<u8> = checked.byte_iter().collect();
        if bytes.len() != ADDRESS_LEN {
            return Err(AddressError::Length);
        }
        let mut a = [0u8; ADDRESS_LEN];
        a.copy_from_slice(&bytes);
        Ok(Address(a))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_network_check() {
        let a = Address::from_public_key(&[1u8; 32]);
        let s = a.encode(Network::Mainnet);
        assert!(s.starts_with("tc1"));
        assert_eq!(Address::decode(&s, Network::Mainnet).unwrap(), a);
        assert!(matches!(Address::decode(&s, Network::Testnet), Err(AddressError::WrongNetwork { .. })));
        let mut broken = s.clone();
        let last = broken.pop().unwrap();
        broken.push(if last == 'q' { 'p' } else { 'q' });
        assert_eq!(Address::decode(&broken, Network::Mainnet), Err(AddressError::Encoding));
    }
}
