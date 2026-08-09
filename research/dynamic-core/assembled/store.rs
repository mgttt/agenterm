//! Q22 (assembled) — content-addressed store + BUILD-TIME-PINNED manifests, following
//! Q3's mechanism (hash -> file, `reuse/RESULTS.md`) and Q18's verdict (`discovery/
//! RESULTS.md` ⑤): *"discovery is not a runtime problem... the name->hash step is fully
//! hoistable to build time, at which point runtime discovery residue = 0."* This file
//! therefore has NO name->hash resolution layer at all — `pack()` writes a manifest
//! that ALREADY names a hash (the build-time-pinned choice); `load()` only ever reads a
//! hash out of a manifest and fetches `store/<hash>.bin`. That is deliberately the
//! entire "discovery" surface, matching Q18's `loader_hash` baseline (its own +0-byte
//! runtime-discovery-residue candidate), not its `loader_disc` runtime-resolver variant
//! (out of scope here — Q18 already showed the runtime layer adds nothing this
//! composition needs).
//!
//! Hash = FNV-1a/64, SAME constants as `reuse/RESULTS.md` / `discovery/RESULTS.md`
//! (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`) — kept identical so a
//! hash computed here is directly comparable to those experiments' hashes, not a
//! different algorithm wearing the same name.

use std::fs;
use std::path::{Path, PathBuf};

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn hash_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("store")).expect("create store dir");
        Store { root }
    }

    /// BUILD-TIME step: write `bytes` into the content store under its own hash, and
    /// pin `manifest_name` to that hash. Returns the hash (for logging only — the
    /// RUNTIME path below never receives it directly, it re-reads the manifest).
    pub fn pack(&self, manifest_name: &str, bytes: &[u8]) -> String {
        let hash = hash_hex(bytes);
        let blob_path = self.root.join("store").join(format!("{hash}.bin"));
        fs::write(&blob_path, bytes).expect("write blob");
        let manifest_path = self.root.join(format!("{manifest_name}.manifest"));
        fs::write(&manifest_path, &hash).expect("write manifest");
        hash
    }

    /// RUNTIME step: read a manifest (a bare hash, build-time-pinned per Q18), fetch
    /// the corresponding blob from the store, and VERIFY it (recompute FNV-1a/64,
    /// compare to the hash the manifest named) before returning the bytes — this is
    /// Q3's "+verify" loader option (`loader_ca_verify`), always-on here since the
    /// integrity property is cheap (one hash pass) and this is the property criterion
    /// ① is checking.
    pub fn load(&self, manifest_name: &str) -> Option<Vec<u8>> {
        let manifest_path = self.root.join(format!("{manifest_name}.manifest"));
        let hash = fs::read_to_string(&manifest_path).ok()?;
        let hash = hash.trim();
        let blob_path = self.root.join("store").join(format!("{hash}.bin"));
        let bytes = fs::read(&blob_path).ok()?;
        if hash_hex(&bytes) != hash {
            return None; // integrity failure — content does not match the pinned hash
        }
        Some(bytes)
    }
}
