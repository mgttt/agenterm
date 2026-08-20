//! Same-filesystem atomic cache with one-generation activation rollback.

use alloc::string::String;
use alloc::vec::Vec;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{CartridgeTrustStore, CatalogEntry, WasmError};

const STATE_MAGIC: &[u8; 4] = b"TAS1";
const STATE_BYTES: usize = 4 + 32 + 1 + 32;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ActiveState {
    current: [u8; 32],
    previous: Option<[u8; 32]>,
}

/// App-owned cache. Network download remains outside this type and must be
/// bounded separately; only fully received bytes enter the trust gate.
pub struct CartridgeCache {
    root: PathBuf,
    max_wasm_bytes: usize,
}

impl CartridgeCache {
    pub fn open(root: impl AsRef<Path>, max_wasm_bytes: usize) -> Result<Self, WasmError> {
        if max_wasm_bytes == 0 {
            return Err(WasmError::Trap("invalid cartridge cache limit"));
        }
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("objects"))
            .and_then(|_| fs::create_dir_all(root.join("active")))
            .map_err(|_| WasmError::Trap("create cartridge cache"))?;
        Ok(Self {
            root,
            max_wasm_bytes,
        })
    }

    /// Verify/store an object and atomically select it as current for its game.
    pub fn activate(
        &self,
        entry: &CatalogEntry,
        wasm: &[u8],
        trust: &CartridgeTrustStore,
    ) -> Result<(), WasmError> {
        self.install(entry, wasm, trust)?;
        let old = self.read_state(&entry.game_id)?;
        let state = match old {
            Some(old) if old.current == entry.wasm_sha256 => old,
            Some(old) => ActiveState {
                current: entry.wasm_sha256,
                previous: Some(old.current),
            },
            None => ActiveState {
                current: entry.wasm_sha256,
                previous: None,
            },
        };
        self.write_state(&entry.game_id, &state)
    }

    /// Load the selected object and re-run current key/content revocation.
    pub fn load_active(
        &self,
        game_id: &str,
        entry: &CatalogEntry,
        trust: &CartridgeTrustStore,
    ) -> Result<Vec<u8>, WasmError> {
        let state = self
            .read_state(game_id)?
            .ok_or(WasmError::Trap("no active cartridge"))?;
        if state.current != entry.wasm_sha256 || entry.game_id != game_id {
            return Err(WasmError::Trap("active catalog entry mismatch"));
        }
        self.load_verified(entry, trust)
    }

    /// Swap current/previous after verifying the previous generation against
    /// the current trust store. A revoked previous object can never reactivate.
    pub fn rollback(
        &self,
        game_id: &str,
        previous_entry: &CatalogEntry,
        trust: &CartridgeTrustStore,
    ) -> Result<Vec<u8>, WasmError> {
        let old = self
            .read_state(game_id)?
            .ok_or(WasmError::Trap("no active cartridge"))?;
        let previous = old
            .previous
            .ok_or(WasmError::Trap("no cartridge rollback generation"))?;
        if previous != previous_entry.wasm_sha256 || previous_entry.game_id != game_id {
            return Err(WasmError::Trap("rollback catalog entry mismatch"));
        }
        let wasm = self.load_verified(previous_entry, trust)?;
        self.write_state(
            game_id,
            &ActiveState {
                current: previous,
                previous: Some(old.current),
            },
        )?;
        Ok(wasm)
    }

    fn install(
        &self,
        entry: &CatalogEntry,
        wasm: &[u8],
        trust: &CartridgeTrustStore,
    ) -> Result<(), WasmError> {
        if wasm.len() > self.max_wasm_bytes {
            return Err(WasmError::Trap("cartridge cache size limit"));
        }
        trust.verify(entry, wasm)?;
        let path = self.object_path(&entry.wasm_sha256);
        if path.exists() {
            let existing = read_regular_bounded(&path, self.max_wasm_bytes)?;
            trust.verify(entry, &existing)?;
            return Ok(());
        }
        atomic_write(&path, wasm)
    }

    fn load_verified(
        &self,
        entry: &CatalogEntry,
        trust: &CartridgeTrustStore,
    ) -> Result<Vec<u8>, WasmError> {
        let wasm =
            read_regular_bounded(&self.object_path(&entry.wasm_sha256), self.max_wasm_bytes)?;
        trust.verify(entry, &wasm)?;
        Ok(wasm)
    }

    fn object_path(&self, hash: &[u8; 32]) -> PathBuf {
        self.root.join("objects").join(format_hash(hash) + ".wasm")
    }

    fn state_path(&self, game_id: &str) -> Result<PathBuf, WasmError> {
        if game_id.is_empty()
            || !game_id.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
        {
            return Err(WasmError::Trap("invalid cache game id"));
        }
        Ok(self.root.join("active").join(game_id.to_owned() + ".state"))
    }

    fn read_state(&self, game_id: &str) -> Result<Option<ActiveState>, WasmError> {
        let path = self.state_path(game_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = read_regular_bounded(&path, STATE_BYTES)?;
        if bytes.len() != STATE_BYTES || &bytes[..4] != STATE_MAGIC || bytes[36] > 1 {
            return Err(WasmError::Trap("invalid cartridge activation state"));
        }
        let mut current = [0; 32];
        current.copy_from_slice(&bytes[4..36]);
        let previous = if bytes[36] == 1 {
            let mut hash = [0; 32];
            hash.copy_from_slice(&bytes[37..69]);
            Some(hash)
        } else {
            if bytes[37..69].iter().any(|byte| *byte != 0) {
                return Err(WasmError::Trap("invalid cartridge activation state"));
            }
            None
        };
        Ok(Some(ActiveState { current, previous }))
    }

    fn write_state(&self, game_id: &str, state: &ActiveState) -> Result<(), WasmError> {
        let mut bytes = [0; STATE_BYTES];
        bytes[..4].copy_from_slice(STATE_MAGIC);
        bytes[4..36].copy_from_slice(&state.current);
        if let Some(previous) = state.previous {
            bytes[36] = 1;
            bytes[37..69].copy_from_slice(&previous);
        }
        atomic_write(&self.state_path(game_id)?, &bytes)
    }
}

fn read_regular_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, WasmError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| WasmError::Trap("read cartridge cache"))?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes as u64 {
        return Err(WasmError::Trap("invalid cartridge cache object"));
    }
    let mut file = File::open(path).map_err(|_| WasmError::Trap("read cartridge cache"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(metadata.len() as usize)
        .map_err(|_| WasmError::Trap("cartridge cache allocation"))?;
    file.read_to_end(&mut bytes)
        .map_err(|_| WasmError::Trap("read cartridge cache"))?;
    if bytes.len() > max_bytes {
        return Err(WasmError::Trap("cartridge cache size limit"));
    }
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), WasmError> {
    let parent = path
        .parent()
        .ok_or(WasmError::Trap("invalid cartridge cache path"))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(WasmError::Trap("invalid cartridge cache path"))?;
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| WasmError::Trap("create cartridge cache staging"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| WasmError::Trap("write cartridge cache staging"))?;
        fs::rename(&temporary, path).map_err(|_| WasmError::Trap("promote cartridge cache"))?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| WasmError::Trap("sync cartridge cache"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn format_hash(hash: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in hash {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}
