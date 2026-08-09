//! Content-addressed pack store — ported from
//! `research/dynamic-core/assembled/store.rs` (Q3's mechanism, Q18's
//! verdict: *"discovery is not a runtime problem... the name->hash step is
//! fully hoistable to build time, at which point runtime discovery
//! residue = 0."*). Q22's version still kept one `<name>.manifest` file per
//! pack — a name->hash lookup, even if build-time-pinned. The design doc is
//! stricter for v1 ("Pack 加载时 hash 由调用方给定，store 只做给 hash 取内容，
//! 不做给 name 找 hash") so that lookup is cut here too: `Store` has no
//! notion of a pack "name" at all, only `put`/`get` keyed by content hash.
//! `pack::PackManifest` is where a hash is recorded for a caller to hold and
//! pass back in — the manifest lives outside this file on purpose.
//!
//! Hash = FNV-1a/64, same constants as the research track's own experiments
//! (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`).

use std::fs;
use std::path::{Path, PathBuf};

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        // The canonical FNV-1a/64 prime is 0x100000001b3, which grouped in
        // 4-hex-digit chunks is 0x0000_0100_0000_01b3 -- NOT
        // 0x0000_0001_0000_01b3 (an extra zero group; caught while building
        // `agenterm-nativecore`'s own copy of this file, see its history for
        // the reproduction). Harmless for content addressing (put/get only
        // ever need internal self-consistency, and both use this same
        // function), but this was not really FNV-1a/64 until now.
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
    /// the requested hash — this function never returns tampered/corrupt
    /// bytes silently.
    pub fn get(&self, hash: &str) -> Option<Vec<u8>> {
        let bytes = fs::read(self.blob_path(hash)).ok()?;
        if hash_hex(&bytes) != hash {
            return None;
        }
        Some(bytes)
    }
}
