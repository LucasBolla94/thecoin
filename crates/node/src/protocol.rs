//! Peer-to-peer wire protocol (version 1).
//!
//! Every message is sent as a frame over TCP:
//!
//! ```text
//! +-----------+----------------+-------------------+----------------------+
//! | magic (4) | length (4, LE) | checksum (4)      | payload (length)     |
//! +-----------+----------------+-------------------+----------------------+
//! checksum = first 4 bytes of tagged_hash("p2p", payload)
//! payload  = borsh(Message)
//! ```
//!
//! Frames larger than [`MAX_FRAME_BYTES`] or with a wrong magic/checksum cause
//! an immediate disconnect.

use anyhow::{anyhow, bail, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use thecoin_core::hash::{tagged_hash, tags, Hash32};
use thecoin_core::{Block, Transaction};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 9 * 1024 * 1024;
pub const MAX_INV_ITEMS: usize = 2_000;
pub const MAX_ADDRS: usize = 1_000;
pub const MAX_LOCATOR: usize = 64;
pub const MAX_BLOCKS_PER_INV: usize = 500;

/// Service bits advertised in `Version`.
pub const SERVICE_ARCHIVE: u64 = 1;
pub const SERVICE_INDEX: u64 = 2;

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct VersionMsg {
    pub protocol: u32,
    pub genesis: Hash32,
    pub height: u64,
    pub tip: Hash32,
    pub services: u64,
    pub user_agent: String,
    /// Port on which the sender accepts connections (0 = none).
    pub listen_port: u16,
    /// Random value to detect connections to ourselves.
    pub nonce: u64,
    pub timestamp: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub enum InvKind {
    Tx,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub struct InvItem {
    pub kind: InvKind,
    pub hash: Hash32,
}

/// IPv6 (IPv4-mapped for v4) address + port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub struct NetAddr {
    pub ip: [u8; 16],
    pub port: u16,
}

impl NetAddr {
    pub fn from_socket(s: &SocketAddr) -> NetAddr {
        let ip = match s.ip() {
            IpAddr::V4(v4) => v4.to_ipv6_mapped(),
            IpAddr::V6(v6) => v6,
        };
        NetAddr { ip: ip.octets(), port: s.port() }
    }

    pub fn to_socket(&self) -> SocketAddr {
        let v6 = Ipv6Addr::from(self.ip);
        match v6.to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(IpAddr::V4(v4), self.port),
            None => SocketAddr::new(IpAddr::V6(v6), self.port),
        }
    }
}

/// **Variant order is part of the protocol** — append only.
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum Message {
    Version(VersionMsg),
    Verack,
    Ping(u64),
    Pong(u64),
    GetAddr,
    Addr(Vec<NetAddr>),
    Inv(Vec<InvItem>),
    GetData(Vec<InvItem>),
    NotFound(Vec<InvItem>),
    /// Asks for up to 500 main-chain block hashes after the first known locator hash.
    GetBlocks {
        locator: Vec<Hash32>,
        stop: Hash32,
    },
    Block(Box<Block>),
    Tx(Box<Transaction>),
    /// Asks for an `Inv` of the peer's mempool.
    GetMempool,
}

impl Message {
    pub fn name(&self) -> &'static str {
        match self {
            Message::Version(_) => "version",
            Message::Verack => "verack",
            Message::Ping(_) => "ping",
            Message::Pong(_) => "pong",
            Message::GetAddr => "getaddr",
            Message::Addr(_) => "addr",
            Message::Inv(_) => "inv",
            Message::GetData(_) => "getdata",
            Message::NotFound(_) => "notfound",
            Message::GetBlocks { .. } => "getblocks",
            Message::Block(_) => "block",
            Message::Tx(_) => "tx",
            Message::GetMempool => "getmempool",
        }
    }

    /// Structural limits checked right after decoding.
    pub fn check_limits(&self) -> Result<()> {
        match self {
            Message::Addr(a) if a.len() > MAX_ADDRS => bail!("too many addrs"),
            Message::Inv(i) | Message::GetData(i) | Message::NotFound(i) if i.len() > MAX_INV_ITEMS => bail!("too many inv items"),
            Message::GetBlocks { locator, .. } if locator.len() > MAX_LOCATOR => bail!("locator too long"),
            Message::Version(v) if v.user_agent.len() > 256 => bail!("user agent too long"),
            _ => Ok(()),
        }
    }
}

fn checksum(payload: &[u8]) -> [u8; 4] {
    let h = tagged_hash(tags::P2P_CHECKSUM, &[payload]);
    [h.0[0], h.0[1], h.0[2], h.0[3]]
}

pub fn encode_frame(magic: [u8; 4], msg: &Message) -> Result<Vec<u8>> {
    let payload = borsh::to_vec(msg)?;
    if payload.len() > MAX_FRAME_BYTES {
        bail!("message too large");
    }
    let mut out = Vec::with_capacity(12 + payload.len());
    out.extend_from_slice(&magic);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&checksum(&payload));
    out.extend_from_slice(&payload);
    Ok(out)
}

pub async fn read_message<R: AsyncReadExt + Unpin>(r: &mut R, magic: [u8; 4]) -> Result<Message> {
    let mut head = [0u8; 12];
    r.read_exact(&mut head).await?;
    if head[0..4] != magic {
        bail!("wrong network magic");
    }
    let len = u32::from_le_bytes(head[4..8].try_into().expect("4 bytes")) as usize;
    if len > MAX_FRAME_BYTES {
        bail!("frame too large ({len} bytes)");
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).await?;
    if checksum(&payload) != head[8..12] {
        bail!("bad checksum");
    }
    let msg = Message::try_from_slice(&payload).map_err(|e| anyhow!("malformed message: {e}"))?;
    msg.check_limits()?;
    Ok(msg)
}

pub async fn write_frame<W: AsyncWriteExt + Unpin>(w: &mut W, frame: &[u8]) -> Result<()> {
    w.write_all(frame).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frame_roundtrip_and_rejects() {
        let magic = *b"TEST";
        let msg = Message::Inv(vec![InvItem { kind: InvKind::Block, hash: Hash32([3; 32]) }]);
        let frame = encode_frame(magic, &msg).unwrap();
        let mut cur = std::io::Cursor::new(frame.clone());
        let got = read_message(&mut cur, magic).await.unwrap();
        assert!(matches!(got, Message::Inv(v) if v.len() == 1));

        let mut bad = frame.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(read_message(&mut std::io::Cursor::new(bad), magic).await.is_err());
        assert!(read_message(&mut std::io::Cursor::new(frame), *b"XXXX").await.is_err());

        let addr: SocketAddr = "1.2.3.4:7333".parse().unwrap();
        assert_eq!(NetAddr::from_socket(&addr).to_socket(), addr);
    }
}
