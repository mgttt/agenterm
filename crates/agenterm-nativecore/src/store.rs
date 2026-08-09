//! Content-addressed pack store — copied from
//! `crates/agenterm-dynacore/src/store.rs` per the design doc's explicit
//! instruction ("store.rs：内容寻址，可直接照抄 agenterm-dynacore 的
//! store.rs（intent 无关）") — this mechanism has no opinion about what kind
//! of IR it stores, so it needs no adaptation for this crate's raw-memory
//! ops or seven native intents. `Store` has no notion of a pack "name", only
//! `put`/`get` keyed by content hash; `pack.rs`'s `PackManifest` is where a
//! hash is recorded for a caller to hold and pass back in.
//!
//! Hash = FNV-1a/64, same constants as the research track's own experiments
//! (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`).

use std::fs;
use std::path::{Path, PathBuf};

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        // NOTE: the canonical FNV-1a/64 prime 0x100000001b3, correctly
        // grouped in 4-hex-digit chunks, is 0x0000_0100_0000_01b3 -- NOT
        // 0x0000_0001_0000_01b3 (an extra zero group; a real off-by-one-group
        // typo found while building this crate's own black-box tests, whose
        // independent reference hash didn't match a real interpreter run
        // until this was corrected). `crates/agenterm-dynacore/src/store.rs`
        // (the logic pack, out of this crate's scope, not touched here) has
        // the identical typo in its own copy of this function -- harmless
        // there too (content addressing only needs a hash to be internally
        // consistent between `put`/`get`, not to literally BE FNV-1a/64),
        // but flagged so it doesn't silently propagate further.
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn hash_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("store"))?;
        Ok(Store { root })
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        self.root.join("store").join(format!("{hash}.bin"))
    }

    /// Write `bytes` into the store under its own content hash. Returns the
    /// hash. Idempotent: packing the same bytes twice writes the same path.
    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let hash = hash_hex(bytes);
        fs::write(self.blob_path(&hash), bytes)?;
        Ok(hash)
    }

    /// Fetch the blob for `hash` (given by the caller — see file header) and
    /// verify it (recompute FNV-1a/64, compare to `hash`) before returning.
    /// Returns `None` if the blob is absent or its content does not match
    /// the requested hash — never returns tampered/corrupt bytes silently.
    pub fn get(&self, hash: &str) -> Option<Vec<u8>> {
        let bytes = fs::read(self.blob_path(hash)).ok()?;
        if hash_hex(&bytes) != hash {
            return None;
        }
        Some(bytes)
    }
}
