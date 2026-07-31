use cid::Cid;
use multihash_codetable::{Code, MultihashDigest};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_BLOCK_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_STORE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_STORE_BLOCKS: usize = 1_024;

#[derive(Clone, Debug, Serialize)]
pub struct StoreSnapshot {
    pub schema: &'static str,
    pub path: String,
    pub block_count: usize,
    pub pinned_count: usize,
    pub verified_bytes: u64,
    pub stored_bytes: u64,
    pub corrupt_blocks: usize,
    pub max_block_bytes: usize,
    pub max_store_bytes: u64,
    pub max_store_blocks: usize,
}

#[derive(Debug, Serialize)]
pub struct PutResult {
    pub cid: String,
    pub bytes: usize,
    pub pinned: bool,
    pub already_present: bool,
    pub snapshot: StoreSnapshot,
}

#[derive(Debug, Serialize)]
pub struct GetResult {
    pub cid: String,
    pub bytes: usize,
    pub verified: bool,
    pub output: String,
}

#[derive(Debug, Serialize)]
pub struct PinResult {
    pub cid: String,
    pub pinned: bool,
    pub snapshot: StoreSnapshot,
}

#[derive(Debug, Serialize)]
pub struct GcResult {
    pub removed_blocks: usize,
    pub removed_bytes: u64,
    pub snapshot: StoreSnapshot,
}

pub struct PersistentStore {
    root: PathBuf,
}

impl PersistentStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("blocks"))
            .map_err(|error| format!("create block store: {error}"))?;
        fs::create_dir_all(root.join("pins"))
            .map_err(|error| format!("create pin store: {error}"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, bytes: &[u8], pin: bool) -> Result<PutResult, String> {
        let cid = cid_for(bytes)?;
        let path = self.block_path(&cid);
        let already_present = path.exists();
        if already_present {
            self.get(&cid)?;
        } else {
            let before = self.snapshot()?;
            if before.block_count >= MAX_STORE_BLOCKS {
                return Err(format!("store block budget exhausted ({MAX_STORE_BLOCKS})"));
            }
            let bytes_u64 = u64::try_from(bytes.len()).map_err(|_| "block size overflow")?;
            if before.stored_bytes.saturating_add(bytes_u64) > MAX_STORE_BYTES {
                return Err(format!("store byte budget exhausted ({MAX_STORE_BYTES})"));
            }
            atomic_write(&path, bytes)?;
            if self.get(&cid)? != bytes {
                let _ = fs::remove_file(&path);
                return Err("written block failed read-back verification".to_string());
            }
        }
        if pin {
            self.set_pin(&cid, true)?;
        }
        Ok(PutResult {
            cid: cid.to_string(),
            bytes: bytes.len(),
            pinned: self.pin_path(&cid).exists(),
            already_present,
            snapshot: self.snapshot()?,
        })
    }

    pub fn get(&self, cid: &Cid) -> Result<Vec<u8>, String> {
        let path = self.block_path(cid);
        let metadata =
            fs::metadata(&path).map_err(|error| format!("read block metadata: {error}"))?;
        if metadata.len() > MAX_BLOCK_BYTES as u64 {
            return Err("stored block exceeds per-block budget".to_string());
        }
        let bytes = fs::read(&path).map_err(|error| format!("read block: {error}"))?;
        if cid_for(&bytes)? != *cid {
            return Err("stored block failed CID verification".to_string());
        }
        Ok(bytes)
    }

    pub fn get_to(&self, cid: &Cid, output: impl AsRef<Path>) -> Result<GetResult, String> {
        let bytes = self.get(cid)?;
        atomic_write(output.as_ref(), &bytes)?;
        Ok(GetResult {
            cid: cid.to_string(),
            bytes: bytes.len(),
            verified: true,
            output: output.as_ref().display().to_string(),
        })
    }

    pub fn set_pin(&self, cid: &Cid, pinned: bool) -> Result<PinResult, String> {
        self.get(cid)?;
        let path = self.pin_path(cid);
        if pinned {
            atomic_write(&path, b"pinned\n")?;
        } else if let Err(error) = fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(format!("remove pin: {error}"));
        }
        Ok(PinResult {
            cid: cid.to_string(),
            pinned,
            snapshot: self.snapshot()?,
        })
    }

    pub fn gc(&self) -> Result<GcResult, String> {
        let mut removed_blocks = 0;
        let mut removed_bytes = 0_u64;
        for entry in fs::read_dir(self.root.join("blocks"))
            .map_err(|error| format!("read block store: {error}"))?
        {
            let entry = entry.map_err(|error| format!("read block entry: {error}"))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if self.root.join("pins").join(name.as_ref()).exists() {
                continue;
            }
            let bytes = entry.metadata().map(|value| value.len()).unwrap_or(0);
            fs::remove_file(entry.path()).map_err(|error| format!("remove block: {error}"))?;
            removed_blocks += 1;
            removed_bytes = removed_bytes.saturating_add(bytes);
        }
        Ok(GcResult {
            removed_blocks,
            removed_bytes,
            snapshot: self.snapshot()?,
        })
    }

    pub fn snapshot(&self) -> Result<StoreSnapshot, String> {
        let mut block_count = 0;
        let mut verified_bytes = 0_u64;
        let mut stored_bytes = 0_u64;
        let mut corrupt_blocks = 0;
        for entry in fs::read_dir(self.root.join("blocks"))
            .map_err(|error| format!("read block store: {error}"))?
        {
            let entry = entry.map_err(|error| format!("read block entry: {error}"))?;
            if !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
            {
                continue;
            }
            block_count += 1;
            stored_bytes =
                stored_bytes.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
            let valid = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<Cid>().ok())
                .and_then(|cid| self.get(&cid).ok())
                .map(|bytes| bytes.len() as u64);
            if let Some(bytes) = valid {
                verified_bytes = verified_bytes.saturating_add(bytes);
            } else {
                corrupt_blocks += 1;
            }
        }
        let pinned_count = fs::read_dir(self.root.join("pins"))
            .map_err(|error| format!("read pin store: {error}"))?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            })
            .count();
        Ok(StoreSnapshot {
            schema: "agenterm-net/store-snapshot/v1",
            path: self.root.display().to_string(),
            block_count,
            pinned_count,
            verified_bytes,
            stored_bytes,
            corrupt_blocks,
            max_block_bytes: MAX_BLOCK_BYTES,
            max_store_bytes: MAX_STORE_BYTES,
            max_store_blocks: MAX_STORE_BLOCKS,
        })
    }

    fn block_path(&self, cid: &Cid) -> PathBuf {
        self.root.join("blocks").join(cid.to_string())
    }

    fn pin_path(&self, cid: &Cid) -> PathBuf {
        self.root.join("pins").join(cid.to_string())
    }
}

pub fn cid_for(bytes: &[u8]) -> Result<Cid, String> {
    if bytes.len() > MAX_BLOCK_BYTES {
        return Err(format!("block exceeds {MAX_BLOCK_BYTES} byte bound"));
    }
    Ok(Cid::new_v1(0x55, Code::Sha2_256.digest(bytes)))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("create parent directory: {error}"))?;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("tmp-{}-{nonce:x}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("create temporary file: {error}"))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("write temporary file: {error}"));
    }
    drop(file);
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("replace existing file: {error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("commit file: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "agenterm-net-{label}-{}-{nonce:x}",
            std::process::id()
        ))
    }

    #[test]
    fn persistent_store_verifies_pins_and_collects() {
        let root = test_path("store");
        let store = PersistentStore::open(&root).unwrap();
        let pinned = store.put(b"pinned", true).unwrap();
        let transient = store.put(b"transient", false).unwrap();
        assert_eq!(store.snapshot().unwrap().block_count, 2);
        let gc = store.gc().unwrap();
        assert_eq!(gc.removed_blocks, 1);
        assert_eq!(gc.snapshot.block_count, 1);
        assert!(store.get(&pinned.cid.parse().unwrap()).is_ok());
        assert!(store.get(&transient.cid.parse().unwrap()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corruption_is_counted_and_never_returned() {
        let root = test_path("corrupt");
        let store = PersistentStore::open(&root).unwrap();
        let put = store.put(b"verified", false).unwrap();
        let cid: Cid = put.cid.parse().unwrap();
        fs::write(store.block_path(&cid), b"changed").unwrap();
        assert!(store.get(&cid).unwrap_err().contains("CID verification"));
        assert_eq!(store.snapshot().unwrap().corrupt_blocks, 1);
        fs::remove_dir_all(root).unwrap();
    }
}
