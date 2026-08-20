//! Signed catalog authority for reviewed remote cartridges.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use ring::{digest, signature};

use crate::{CartridgeManifest, WasmError};

const SIGNING_DOMAIN: &[u8] = b"TinyArcade signed catalog entry v1\0";
const CATALOG_SCHEMA_VERSION: u32 = 1;
const ED25519_PUBLIC_KEY_BYTES: usize = 32;

/// One reviewed catalog record. The signature covers every field except itself.
/// The WASM stays a separate cacheable object named by `wasm_sha256`.
pub struct CatalogEntry {
    pub game_id: String,
    pub game_version: String,
    pub abi_version: u32,
    pub state_version: u32,
    pub wasm_length: u64,
    pub wasm_sha256: [u8; 32],
    pub signing_key_id: String,
    pub signature: [u8; 64],
}

impl CatalogEntry {
    /// Canonical bytes signed by an offline catalog key.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, WasmError> {
        if self.abi_version == 0
            || self.state_version == 0
            || !valid_token(&self.game_id, 3, 128)
            || !valid_token(&self.game_version, 1, 64)
            || !valid_token(&self.signing_key_id, 1, 64)
        {
            return Err(WasmError::Trap("invalid signed catalog entry"));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(
                SIGNING_DOMAIN.len()
                    + 4
                    + 2
                    + self.game_id.len()
                    + 2
                    + self.game_version.len()
                    + 4
                    + 4
                    + 8
                    + 32
                    + 2
                    + self.signing_key_id.len(),
            )
            .map_err(|_| WasmError::Trap("catalog signing allocation"))?;
        bytes.extend_from_slice(SIGNING_DOMAIN);
        bytes.extend_from_slice(&CATALOG_SCHEMA_VERSION.to_le_bytes());
        put_string(&mut bytes, &self.game_id)?;
        put_string(&mut bytes, &self.game_version)?;
        bytes.extend_from_slice(&self.abi_version.to_le_bytes());
        bytes.extend_from_slice(&self.state_version.to_le_bytes());
        bytes.extend_from_slice(&self.wasm_length.to_le_bytes());
        bytes.extend_from_slice(&self.wasm_sha256);
        put_string(&mut bytes, &self.signing_key_id)?;
        Ok(bytes)
    }
}

struct TrustedKey {
    id: String,
    public_key: [u8; ED25519_PUBLIC_KEY_BYTES],
    revoked: bool,
}

/// App-bundled keyring plus fail-closed key/content revocations.
#[derive(Default)]
pub struct CartridgeTrustStore {
    keys: Vec<TrustedKey>,
    revoked_content: Vec<[u8; 32]>,
}

impl CartridgeTrustStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_key(&mut self, id: &str, public_key: &[u8]) -> Result<(), WasmError> {
        if !valid_token(id, 1, 64)
            || public_key.len() != ED25519_PUBLIC_KEY_BYTES
            || self.keys.iter().any(|key| key.id == id)
        {
            return Err(WasmError::Trap("invalid catalog trust key"));
        }
        let mut fixed = [0; ED25519_PUBLIC_KEY_BYTES];
        fixed.copy_from_slice(public_key);
        self.keys.push(TrustedKey {
            id: id.to_string(),
            public_key: fixed,
            revoked: false,
        });
        Ok(())
    }

    pub fn revoke_key(&mut self, id: &str) -> Result<(), WasmError> {
        let key = self
            .keys
            .iter_mut()
            .find(|key| key.id == id)
            .ok_or(WasmError::Trap("unknown catalog trust key"))?;
        key.revoked = true;
        Ok(())
    }

    pub fn revoke_content(&mut self, sha256: [u8; 32]) {
        if !self.revoked_content.contains(&sha256) {
            self.revoked_content.push(sha256);
        }
    }

    /// Verify key status, signature, object bytes and embedded manifest as one gate.
    pub fn verify(
        &self,
        entry: &CatalogEntry,
        wasm: &[u8],
    ) -> Result<CartridgeManifest, WasmError> {
        let key = self
            .keys
            .iter()
            .find(|key| key.id == entry.signing_key_id)
            .filter(|key| !key.revoked)
            .ok_or(WasmError::Trap("untrusted or revoked catalog key"))?;
        if self.revoked_content.contains(&entry.wasm_sha256) {
            return Err(WasmError::Trap("revoked cartridge content"));
        }
        let signing_bytes = entry.signing_bytes()?;
        signature::UnparsedPublicKey::new(&signature::ED25519, key.public_key)
            .verify(&signing_bytes, &entry.signature)
            .map_err(|_| WasmError::Trap("invalid catalog signature"))?;
        if usize::try_from(entry.wasm_length).ok() != Some(wasm.len()) {
            return Err(WasmError::Trap("cartridge length mismatch"));
        }
        let actual_hash = cartridge_sha256(wasm);
        if actual_hash != entry.wasm_sha256 {
            return Err(WasmError::Trap("cartridge hash mismatch"));
        }
        let manifest = CartridgeManifest::from_wasm(wasm)?;
        if manifest.game_id != entry.game_id
            || manifest.game_version != entry.game_version
            || manifest.abi_version != entry.abi_version
            || manifest.state_version != entry.state_version
        {
            return Err(WasmError::Trap("catalog manifest mismatch"));
        }
        Ok(manifest)
    }
}

pub fn cartridge_sha256(wasm: &[u8]) -> [u8; 32] {
    let value = digest::digest(&digest::SHA256, wasm);
    let mut fixed = [0; 32];
    fixed.copy_from_slice(value.as_ref());
    fixed
}

fn put_string(out: &mut Vec<u8>, value: &str) -> Result<(), WasmError> {
    let len = u16::try_from(value.len()).map_err(|_| WasmError::Trap("catalog string length"))?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn valid_token(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}
