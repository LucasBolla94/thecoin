//! Address manager: known peer addresses, connection history, persistence.

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

const MAX_ADDRS: usize = 5_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddrInfo {
    pub addr: SocketAddr,
    /// Unix time the address was last seen alive.
    pub last_seen: u64,
    /// Unix time of the last connection attempt.
    pub last_try: u64,
    pub failures: u32,
    /// Successfully connected at least once.
    pub tried: bool,
}

pub struct AddrMan {
    addrs: HashMap<SocketAddr, AddrInfo>,
    path: PathBuf,
    allow_private: bool,
}

pub fn is_routable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation())
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

impl AddrMan {
    pub fn load(path: &Path, allow_private: bool) -> AddrMan {
        let mut am = AddrMan { addrs: HashMap::new(), path: path.to_path_buf(), allow_private };
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(list) = serde_json::from_str::<Vec<AddrInfo>>(&text) {
                for a in list {
                    am.addrs.insert(a.addr, a);
                }
            }
        }
        am
    }

    pub fn save(&self) {
        let list: Vec<&AddrInfo> = self.addrs.values().collect();
        if let Ok(text) = serde_json::to_string(&list) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, &self.path);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.addrs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.addrs.is_empty()
    }

    pub fn add(&mut self, addr: SocketAddr, last_seen: u64) {
        if addr.port() == 0 || addr.ip().is_unspecified() || (!self.allow_private && !is_routable(&addr.ip())) {
            return;
        }
        if let Some(e) = self.addrs.get_mut(&addr) {
            e.last_seen = e.last_seen.max(last_seen);
            return;
        }
        if self.addrs.len() >= MAX_ADDRS {
            // Evict the worst entry: most failures, then oldest.
            if let Some(worst) = self.addrs.values().max_by_key(|a| (a.failures, u64::MAX - a.last_seen)).map(|a| a.addr) {
                self.addrs.remove(&worst);
            }
        }
        self.addrs.insert(addr, AddrInfo { addr, last_seen, last_try: 0, failures: 0, tried: false });
    }

    /// Seeds are added even if private (operators configure them explicitly).
    pub fn add_trusted(&mut self, addr: SocketAddr, now: u64) {
        self.addrs.entry(addr).or_insert(AddrInfo { addr, last_seen: now, last_try: 0, failures: 0, tried: false });
    }

    pub fn mark_try(&mut self, addr: &SocketAddr, now: u64) {
        if let Some(e) = self.addrs.get_mut(addr) {
            e.last_try = now;
        }
    }

    pub fn mark_success(&mut self, addr: &SocketAddr, now: u64) {
        if let Some(e) = self.addrs.get_mut(addr) {
            e.tried = true;
            e.failures = 0;
            e.last_seen = now;
        }
    }

    pub fn mark_failure(&mut self, addr: &SocketAddr) {
        if let Some(e) = self.addrs.get_mut(addr) {
            e.failures += 1;
            if e.failures > 10 && !e.tried {
                self.addrs.remove(addr);
            }
        }
    }

    /// Picks up to `n` addresses to connect to, with exponential back-off per failure.
    pub fn pick(&self, n: usize, exclude: &HashSet<SocketAddr>, now: u64) -> Vec<SocketAddr> {
        let mut candidates: Vec<&AddrInfo> = self
            .addrs
            .values()
            .filter(|a| !exclude.contains(&a.addr))
            .filter(|a| {
                let backoff = 30u64.saturating_mul(1u64 << a.failures.min(10));
                now.saturating_sub(a.last_try) >= backoff
            })
            .collect();
        candidates.shuffle(&mut rand::thread_rng());
        candidates.sort_by_key(|a| (!a.tried, a.failures));
        candidates.into_iter().take(n).map(|a| a.addr).collect()
    }

    /// Addresses to share with peers.
    pub fn sample(&self, n: usize, now: u64) -> Vec<SocketAddr> {
        let mut v: Vec<&AddrInfo> =
            self.addrs.values().filter(|a| a.failures < 3 && now.saturating_sub(a.last_seen) < 7 * 86_400).collect();
        v.shuffle(&mut rand::thread_rng());
        v.into_iter().take(n).map(|a| a.addr).collect()
    }
}
