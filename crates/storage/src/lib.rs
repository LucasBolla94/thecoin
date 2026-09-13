//! # thecoin-storage
//!
//! Persistent storage for The Coin nodes, built on [redb] — a pure-Rust,
//! crash-safe (ACID, copy-on-write B-trees) embedded database with a small
//! memory footprint.
//!
//! | table       | key                              | value                                   |
//! |-------------|----------------------------------|-----------------------------------------|
//! | `headers`   | block hash                       | [`HeaderRecord`] (header, chainwork...) |
//! | `blocks`    | block hash                       | zstd(borsh(Vec<Transaction>))           |
//! | `main`      | height                           | block hash of the active chain          |
//! | `state`     | state key                        | state record (see `thecoin_core::state`)|
//! | `undo`      | height                           | zstd(borsh(BlockUndo))                  |
//! | `txindex`   | txid                             | height (8 LE) + position (4 LE)         |
//! | `addrindex` | address + height BE + position BE| txid                                    |
//! | `meta`      | name                             | bytes                                   |
//!
//! Everything a block changes (state, undo, indexes, tip, state commitment) is
//! written in **one** transaction, so a crash can never leave a half-applied
//! block on disk.

use anyhow::{anyhow, Context, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use thecoin_core::block::{Block, BlockHeader};
use thecoin_core::hash::Hash32;
use thecoin_core::lthash::LtHash;
use thecoin_core::state::{StateChange, StateError, StateReader};
use thecoin_core::tx::Transaction;
use thecoin_core::Address;

const HEADERS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("headers");
const BLOCKS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("blocks");
const MAIN: TableDefinition<u64, &[u8]> = TableDefinition::new("main");
const STATE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("state");
const UNDO: TableDefinition<u64, &[u8]> = TableDefinition::new("undo");
const TXINDEX: TableDefinition<&[u8], &[u8]> = TableDefinition::new("txindex");
const ADDRINDEX: TableDefinition<&[u8], &[u8]> = TableDefinition::new("addrindex");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

pub const SCHEMA_VERSION: u32 = 1;
const ZSTD_LEVEL: i32 = 3;

pub const META_TIP: &str = "tip";
pub const META_LTHASH: &str = "lthash";
pub const META_SCHEMA: &str = "schema";
pub const META_GENESIS: &str = "genesis";
pub const META_PRUNED_BELOW: &str = "pruned_below";

/// Validation status of a stored header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum BlockStatus {
    /// Header and body passed context checks; body not yet (or no longer) connected.
    Stored,
    /// Block was connected to the active chain at some point (fully valid).
    Valid,
    /// Block (or an ancestor) failed validation.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct HeaderRecord {
    pub header: BlockHeader,
    /// Cumulative work up to and including this block (big-endian U256).
    pub chainwork: [u8; 32],
    pub status: BlockStatus,
    /// Whether the body is available (false after pruning).
    pub has_body: bool,
    pub tx_count: u32,
    pub size: u32,
}

/// Data needed to disconnect a block during a reorganization.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BlockUndo {
    /// State changes made by the block (old and new values).
    pub changes: Vec<StateChange>,
    /// Address-index entries written for the block: `(address, tx position)`.
    pub addr_index: Vec<(Address, u32)>,
}

fn compress(data: &[u8]) -> Result<Vec<u8>> {
    zstd::bulk::compress(data, ZSTD_LEVEL).context("zstd compress")
}

fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    zstd::stream::decode_all(data).context("zstd decompress")
}

fn addr_index_key(addr: &Address, height: u64, pos: u32) -> Vec<u8> {
    let mut k = Vec::with_capacity(32);
    k.extend_from_slice(&addr.0);
    k.extend_from_slice(&height.to_be_bytes());
    k.extend_from_slice(&pos.to_be_bytes());
    k
}

/// Options for opening the database.
#[derive(Clone, Debug)]
pub struct DbOptions {
    /// Page cache size in bytes.
    pub cache_bytes: usize,
}

impl Default for DbOptions {
    fn default() -> Self {
        DbOptions { cache_bytes: 64 * 1024 * 1024 }
    }
}

pub struct ChainDb {
    db: Database,
}

impl ChainDb {
    pub fn open(path: &Path, opts: &DbOptions) -> Result<ChainDb> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let db = Database::builder()
            .set_cache_size(opts.cache_bytes)
            .create(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        let w = db.begin_write()?;
        {
            w.open_table(HEADERS)?;
            w.open_table(BLOCKS)?;
            w.open_table(MAIN)?;
            w.open_table(STATE)?;
            w.open_table(UNDO)?;
            w.open_table(TXINDEX)?;
            w.open_table(ADDRINDEX)?;
            let mut meta = w.open_table(META)?;
            let existing = meta.get(META_SCHEMA)?.map(|g| g.value().to_vec());
            match existing {
                None => {
                    meta.insert(META_SCHEMA, SCHEMA_VERSION.to_le_bytes().as_slice())?;
                }
                Some(v) if v == SCHEMA_VERSION.to_le_bytes() => {}
                Some(_) => return Err(anyhow!("unsupported database schema version; resync required")),
            }
        }
        w.commit()?;
        Ok(ChainDb { db })
    }

    /// Rewrites the database file to reclaim free pages. Requires exclusive access.
    pub fn compact(&mut self) -> Result<bool> {
        Ok(self.db.compact()?)
    }

    pub fn read(&self) -> Result<ReadTx> {
        Ok(ReadTx { tx: self.db.begin_read()? })
    }

    pub fn write(&self) -> Result<WriteTx> {
        Ok(WriteTx { tx: self.db.begin_write()? })
    }
}

/// Common read operations for read and write transactions.
pub trait DbRead {
    fn get_bytes(&self, table: TableDefinition<&[u8], &[u8]>, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn get_by_height(&self, table: TableDefinition<u64, &[u8]>, height: u64) -> Result<Option<Vec<u8>>>;
    fn get_meta(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn scan_prefix(&self, table: TableDefinition<&[u8], &[u8]>, prefix: &[u8], limit: usize) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
    fn scan_prefix_rev(
        &self,
        table: TableDefinition<&[u8], &[u8]>,
        prefix: &[u8],
        before: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;

    fn header(&self, hash: &Hash32) -> Result<Option<HeaderRecord>> {
        match self.get_bytes(HEADERS, &hash.0)? {
            Some(b) => Ok(Some(HeaderRecord::try_from_slice(&b)?)),
            None => Ok(None),
        }
    }

    fn block_txs(&self, hash: &Hash32) -> Result<Option<Vec<Transaction>>> {
        match self.get_bytes(BLOCKS, &hash.0)? {
            Some(b) => Ok(Some(Vec::<Transaction>::try_from_slice(&decompress(&b)?)?)),
            // Empty bodies are not stored (see `WriteTx::put_block_txs`).
            None => match self.header(hash)? {
                Some(h) if h.has_body && h.tx_count == 0 => Ok(Some(Vec::new())),
                _ => Ok(None),
            },
        }
    }

    fn block(&self, hash: &Hash32) -> Result<Option<Block>> {
        let Some(h) = self.header(hash)? else { return Ok(None) };
        let Some(txs) = self.block_txs(hash)? else { return Ok(None) };
        Ok(Some(Block { header: h.header, txs }))
    }

    fn main_hash(&self, height: u64) -> Result<Option<Hash32>> {
        Ok(self.get_by_height(MAIN, height)?.map(|b| {
            let mut h = [0u8; 32];
            h.copy_from_slice(&b);
            Hash32(h)
        }))
    }

    fn tip(&self) -> Result<Option<Hash32>> {
        Ok(self.get_meta(META_TIP)?.map(|b| {
            let mut h = [0u8; 32];
            h.copy_from_slice(&b);
            Hash32(h)
        }))
    }

    fn lthash(&self) -> Result<Option<LtHash>> {
        match self.get_meta(META_LTHASH)? {
            Some(b) => Ok(Some(LtHash::from_bytes(&b).ok_or_else(|| anyhow!("corrupt lthash"))?)),
            None => Ok(None),
        }
    }

    fn undo(&self, height: u64) -> Result<Option<BlockUndo>> {
        match self.get_by_height(UNDO, height)? {
            Some(b) => Ok(Some(BlockUndo::try_from_slice(&decompress(&b)?)?)),
            None => Ok(None),
        }
    }

    /// `(height, position)` of a confirmed transaction on the active chain.
    fn tx_location(&self, txid: &Hash32) -> Result<Option<(u64, u32)>> {
        Ok(self.get_bytes(TXINDEX, &txid.0)?.map(|b| {
            let height = u64::from_le_bytes(b[0..8].try_into().expect("8 bytes"));
            let pos = u32::from_le_bytes(b[8..12].try_into().expect("4 bytes"));
            (height, pos)
        }))
    }

    /// Transactions touching `addr`, newest first: `(height, position, txid)`.
    fn address_txs(&self, addr: &Address, before: Option<(u64, u32)>, limit: usize) -> Result<Vec<(u64, u32, Hash32)>> {
        let before_key = before.map(|(h, p)| addr_index_key(addr, h, p));
        let rows = self.scan_prefix_rev(ADDRINDEX, &addr.0, before_key.as_deref(), limit)?;
        Ok(rows
            .into_iter()
            .map(|(k, v)| {
                let height = u64::from_be_bytes(k[20..28].try_into().expect("8"));
                let pos = u32::from_be_bytes(k[28..32].try_into().expect("4"));
                let mut h = [0u8; 32];
                h.copy_from_slice(&v);
                (height, pos, Hash32(h))
            })
            .collect())
    }

    fn state_prefix(&self, prefix: &[u8], limit: usize) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.scan_prefix(STATE, prefix, limit)
    }

    fn state_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.get_bytes(STATE, key)
    }
}

pub struct ReadTx {
    tx: redb::ReadTransaction,
}

pub struct WriteTx {
    tx: redb::WriteTransaction,
}

fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.pop() {
        if last < 0xff {
            end.push(last + 1);
            return Some(end);
        }
    }
    None
}

macro_rules! impl_db_read {
    ($t:ty) => {
        impl DbRead for $t {
            fn get_bytes(&self, table: TableDefinition<&[u8], &[u8]>, key: &[u8]) -> Result<Option<Vec<u8>>> {
                let t = self.tx.open_table(table)?;
                let v = t.get(key)?.map(|g| g.value().to_vec());
                Ok(v)
            }

            fn get_by_height(&self, table: TableDefinition<u64, &[u8]>, height: u64) -> Result<Option<Vec<u8>>> {
                let t = self.tx.open_table(table)?;
                let v = t.get(height)?.map(|g| g.value().to_vec());
                Ok(v)
            }

            fn get_meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
                let t = self.tx.open_table(META)?;
                let v = t.get(key)?.map(|g| g.value().to_vec());
                Ok(v)
            }

            fn scan_prefix(&self, table: TableDefinition<&[u8], &[u8]>, prefix: &[u8], limit: usize) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
                let t = self.tx.open_table(table)?;
                let mut out = Vec::new();
                let iter = match prefix_end(prefix) {
                    Some(end) => t.range::<&[u8]>(prefix..end.as_slice())?,
                    None => t.range::<&[u8]>(prefix..)?,
                };
                for item in iter {
                    let (k, v) = item?;
                    out.push((k.value().to_vec(), v.value().to_vec()));
                    if out.len() >= limit {
                        break;
                    }
                }
                Ok(out)
            }

            fn scan_prefix_rev(
                &self,
                table: TableDefinition<&[u8], &[u8]>,
                prefix: &[u8],
                before: Option<&[u8]>,
                limit: usize,
            ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
                let t = self.tx.open_table(table)?;
                let mut out = Vec::new();
                let end = match before {
                    Some(b) => Some(b.to_vec()),
                    None => prefix_end(prefix),
                };
                let iter = match &end {
                    Some(end) => t.range::<&[u8]>(prefix..end.as_slice())?,
                    None => t.range::<&[u8]>(prefix..)?,
                };
                for item in iter.rev() {
                    let (k, v) = item?;
                    out.push((k.value().to_vec(), v.value().to_vec()));
                    if out.len() >= limit {
                        break;
                    }
                }
                Ok(out)
            }
        }

        impl StateReader for $t {
            fn get_raw(&self, key: &[u8]) -> std::result::Result<Option<Vec<u8>>, StateError> {
                self.get_bytes(STATE, key).map_err(|e| StateError(e.to_string()))
            }
        }
    };
}

impl_db_read!(ReadTx);
impl_db_read!(WriteTx);

impl WriteTx {
    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }

    pub fn abort(self) -> Result<()> {
        self.tx.abort()?;
        Ok(())
    }

    pub fn put_header(&self, hash: &Hash32, rec: &HeaderRecord) -> Result<()> {
        let mut t = self.tx.open_table(HEADERS)?;
        t.insert(hash.0.as_slice(), borsh::to_vec(rec)?.as_slice())?;
        Ok(())
    }

    /// Stores a block body. Empty bodies are implied by `HeaderRecord::tx_count == 0`
    /// and take no space.
    pub fn put_block_txs(&self, hash: &Hash32, txs: &[Transaction]) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }
        let raw = borsh::to_vec(&txs.to_vec())?;
        let mut t = self.tx.open_table(BLOCKS)?;
        t.insert(hash.0.as_slice(), compress(&raw)?.as_slice())?;
        Ok(())
    }

    pub fn delete_block_txs(&self, hash: &Hash32) -> Result<()> {
        let mut t = self.tx.open_table(BLOCKS)?;
        t.remove(hash.0.as_slice())?;
        Ok(())
    }

    pub fn set_main(&self, height: u64, hash: &Hash32) -> Result<()> {
        let mut t = self.tx.open_table(MAIN)?;
        t.insert(height, hash.0.as_slice())?;
        Ok(())
    }

    pub fn remove_main(&self, height: u64) -> Result<()> {
        let mut t = self.tx.open_table(MAIN)?;
        t.remove(height)?;
        Ok(())
    }

    /// Writes the new values of `changes`.
    pub fn apply_state_changes(&self, changes: &[StateChange]) -> Result<()> {
        let mut t = self.tx.open_table(STATE)?;
        for c in changes {
            match &c.new {
                Some(v) => {
                    t.insert(c.key.as_slice(), v.as_slice())?;
                }
                None => {
                    t.remove(c.key.as_slice())?;
                }
            }
        }
        Ok(())
    }

    /// Restores the old values of `changes` (undo a block).
    pub fn revert_state_changes(&self, changes: &[StateChange]) -> Result<()> {
        let mut t = self.tx.open_table(STATE)?;
        for c in changes.iter().rev() {
            match &c.old {
                Some(v) => {
                    t.insert(c.key.as_slice(), v.as_slice())?;
                }
                None => {
                    t.remove(c.key.as_slice())?;
                }
            }
        }
        Ok(())
    }

    pub fn put_state_raw(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut t = self.tx.open_table(STATE)?;
        t.insert(key, value)?;
        Ok(())
    }

    pub fn put_undo(&self, height: u64, undo: &BlockUndo) -> Result<()> {
        let raw = borsh::to_vec(undo)?;
        let mut t = self.tx.open_table(UNDO)?;
        t.insert(height, compress(&raw)?.as_slice())?;
        Ok(())
    }

    pub fn delete_undo(&self, height: u64) -> Result<()> {
        let mut t = self.tx.open_table(UNDO)?;
        t.remove(height)?;
        Ok(())
    }

    /// Deletes undo records for heights `< below` (they are only needed for reorgs).
    pub fn prune_undo_below(&self, below: u64) -> Result<usize> {
        let mut t = self.tx.open_table(UNDO)?;
        let keys: Vec<u64> = t.range(..below)?.map(|r| r.map(|(k, _)| k.value())).collect::<std::result::Result<_, _>>()?;
        for k in &keys {
            t.remove(*k)?;
        }
        Ok(keys.len())
    }

    pub fn index_tx(&self, txid: &Hash32, height: u64, pos: u32) -> Result<()> {
        let mut v = [0u8; 12];
        v[0..8].copy_from_slice(&height.to_le_bytes());
        v[8..12].copy_from_slice(&pos.to_le_bytes());
        let mut t = self.tx.open_table(TXINDEX)?;
        t.insert(txid.0.as_slice(), v.as_slice())?;
        Ok(())
    }

    pub fn unindex_tx(&self, txid: &Hash32) -> Result<()> {
        let mut t = self.tx.open_table(TXINDEX)?;
        t.remove(txid.0.as_slice())?;
        Ok(())
    }

    pub fn index_address(&self, addr: &Address, height: u64, pos: u32, txid: &Hash32) -> Result<()> {
        let mut t = self.tx.open_table(ADDRINDEX)?;
        t.insert(addr_index_key(addr, height, pos).as_slice(), txid.0.as_slice())?;
        Ok(())
    }

    pub fn unindex_address(&self, addr: &Address, height: u64, pos: u32) -> Result<()> {
        let mut t = self.tx.open_table(ADDRINDEX)?;
        t.remove(addr_index_key(addr, height, pos).as_slice())?;
        Ok(())
    }

    pub fn set_tip(&self, hash: &Hash32) -> Result<()> {
        self.set_meta(META_TIP, &hash.0)
    }

    pub fn set_lthash(&self, h: &LtHash) -> Result<()> {
        self.set_meta(META_LTHASH, &h.to_bytes())
    }

    pub fn set_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        let mut t = self.tx.open_table(META)?;
        t.insert(key, value)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thecoin_core::genesis::genesis_block;
    use thecoin_core::params::REGTEST;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = ChainDb::open(&dir.path().join("chain.redb"), &DbOptions::default()).unwrap();
        let g = genesis_block(&REGTEST);
        let hash = g.hash();
        let w = db.write().unwrap();
        w.put_header(
            &hash,
            &HeaderRecord {
                header: g.header.clone(),
                chainwork: [0; 32],
                status: BlockStatus::Valid,
                has_body: true,
                tx_count: 0,
                size: 184,
            },
        )
        .unwrap();
        w.put_block_txs(&hash, &g.txs).unwrap();
        w.set_main(0, &hash).unwrap();
        w.set_tip(&hash).unwrap();
        w.apply_state_changes(&[
            StateChange { key: vec![1, 2], old: None, new: Some(vec![9]) },
            StateChange { key: vec![1, 3], old: None, new: Some(vec![8]) },
        ])
        .unwrap();
        let addr = Address([7; 20]);
        for h in 0..5u64 {
            w.index_address(&addr, h, 0, &Hash32([h as u8; 32])).unwrap();
        }
        w.put_undo(3, &BlockUndo { changes: vec![StateChange { key: vec![5], old: Some(vec![1]), new: None }], addr_index: vec![] })
            .unwrap();
        w.commit().unwrap();

        let r = db.read().unwrap();
        assert_eq!(r.block(&hash).unwrap().unwrap(), g);
        assert_eq!(r.tip().unwrap(), Some(hash));
        assert_eq!(r.main_hash(0).unwrap(), Some(hash));
        assert_eq!(r.get_raw(&[1, 2]).unwrap(), Some(vec![9]));
        assert_eq!(r.state_prefix(&[1], 10).unwrap().len(), 2);
        let hist = r.address_txs(&addr, None, 3).unwrap();
        assert_eq!(hist.iter().map(|x| x.0).collect::<Vec<_>>(), vec![4, 3, 2]);
        let hist = r.address_txs(&addr, Some((2, 0)), 10).unwrap();
        assert_eq!(hist.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 0]);
        assert_eq!(r.undo(3).unwrap().unwrap().changes.len(), 1);
        drop(r);

        let w = db.write().unwrap();
        assert_eq!(w.prune_undo_below(10).unwrap(), 1);
        w.commit().unwrap();
    }
}
