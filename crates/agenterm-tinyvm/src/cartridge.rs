//! Canonical metadata carried by a standard WebAssembly custom section.

use alloc::string::String;
use alloc::vec::Vec;

use crate::WasmError;

const SECTION_NAME: &str = "tinyarcade.manifest.v1";
const PAYLOAD_MAGIC: &[u8; 4] = b"TAM1";
const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const MAX_CAPABILITIES: usize = 64;

/// Compatibility metadata embedded in a standard WASM custom section.
///
/// The executable remains an ordinary `.wasm` module. Converters can emit the
/// `tinyarcade.manifest.v1` custom section without knowing tinyvm internals.
#[derive(Clone, PartialEq, Eq)]
pub struct CartridgeManifest {
    pub game_id: String,
    pub game_version: String,
    pub abi_version: u32,
    pub state_version: u32,
    /// Versioned native import-module namespaces required by the cartridge.
    pub capabilities: Vec<String>,
}

impl CartridgeManifest {
    /// Read and validate the one canonical manifest custom section.
    pub fn from_wasm(wasm: &[u8]) -> Result<Self, WasmError> {
        let payload =
            find_manifest_payload(wasm)?.ok_or(WasmError::Decode("missing game manifest"))?;
        parse_payload(payload)
    }

    /// Append this manifest as one standard custom section.
    ///
    /// The original module bytes are preserved exactly. A module that already
    /// contains the manifest section is rejected rather than silently rewritten.
    pub fn append_to_wasm(&self, wasm: &[u8]) -> Result<Vec<u8>, WasmError> {
        if find_manifest_payload(wasm)?.is_some() {
            return Err(WasmError::Decode("game manifest already exists"));
        }
        validate_manifest(self)?;

        let payload_length = self
            .capabilities
            .iter()
            .try_fold(
                4usize + 4 + 4 + 2 + self.game_id.len() + 2 + self.game_version.len() + 2,
                |length, capability| length.checked_add(2 + capability.len()),
            )
            .filter(|&length| length <= MAX_MANIFEST_BYTES)
            .ok_or(WasmError::Decode("oversized game manifest"))?;
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(payload_length)
            .map_err(|_| WasmError::Decode("game manifest allocation"))?;
        payload.extend_from_slice(PAYLOAD_MAGIC);
        payload.extend_from_slice(&self.abi_version.to_le_bytes());
        payload.extend_from_slice(&self.state_version.to_le_bytes());
        write_string(&mut payload, &self.game_id)?;
        write_string(&mut payload, &self.game_version)?;
        payload.extend_from_slice(&(self.capabilities.len() as u16).to_le_bytes());
        for capability in &self.capabilities {
            write_string(&mut payload, capability)?;
        }
        debug_assert_eq!(payload.len(), payload_length);

        let name_length = u32::try_from(SECTION_NAME.len())
            .map_err(|_| WasmError::Decode("game manifest section size"))?;
        let mut section = Vec::new();
        section
            .try_reserve_exact(SECTION_NAME.len() + payload.len() + 10)
            .map_err(|_| WasmError::Decode("game manifest allocation"))?;
        write_leb_u32(&mut section, name_length);
        section.extend_from_slice(SECTION_NAME.as_bytes());
        section.extend_from_slice(&payload);
        let section_length = u32::try_from(section.len())
            .map_err(|_| WasmError::Decode("game manifest section size"))?;

        let mut result = Vec::new();
        let result_length = wasm
            .len()
            .checked_add(section.len())
            .and_then(|length| length.checked_add(6))
            .ok_or(WasmError::Decode("game manifest allocation"))?;
        result
            .try_reserve_exact(result_length)
            .map_err(|_| WasmError::Decode("game manifest allocation"))?;
        result.extend_from_slice(wasm);
        result.push(0);
        write_leb_u32(&mut result, section_length);
        result.extend_from_slice(&section);
        Ok(result)
    }
}

fn find_manifest_payload(wasm: &[u8]) -> Result<Option<&[u8]>, WasmError> {
    if wasm.len() < 8 || &wasm[..4] != b"\0asm" || wasm[4..8] != [1, 0, 0, 0] {
        return Err(WasmError::Decode(
            "not a wasm module (bad manifest envelope)",
        ));
    }
    let mut cursor = 8;
    let mut found = None;
    while cursor < wasm.len() {
        let id = wasm[cursor];
        cursor += 1;
        let size = read_leb_u32(wasm, &mut cursor)? as usize;
        let end = cursor
            .checked_add(size)
            .filter(|&end| end <= wasm.len())
            .ok_or(WasmError::Decode("manifest section bounds"))?;
        if id == 0 {
            let mut section_cursor = cursor;
            let name_len = read_leb_u32(wasm, &mut section_cursor)? as usize;
            let name_end = section_cursor
                .checked_add(name_len)
                .filter(|&name_end| name_end <= end)
                .ok_or(WasmError::Decode("manifest name bounds"))?;
            if &wasm[section_cursor..name_end] == SECTION_NAME.as_bytes() {
                if found.is_some() || end - name_end > MAX_MANIFEST_BYTES {
                    return Err(WasmError::Decode("duplicate or oversized game manifest"));
                }
                found = Some(&wasm[name_end..end]);
            }
        }
        cursor = end;
    }
    Ok(found)
}

fn parse_payload(payload: &[u8]) -> Result<CartridgeManifest, WasmError> {
    let mut cursor = 0;
    if take(payload, &mut cursor, 4)? != PAYLOAD_MAGIC {
        return Err(WasmError::Decode("game manifest magic"));
    }
    let abi_version = read_u32(payload, &mut cursor)?;
    let state_version = read_u32(payload, &mut cursor)?;
    let game_id = read_string(payload, &mut cursor)?;
    let game_version = read_string(payload, &mut cursor)?;
    let capability_count = read_u16(payload, &mut cursor)? as usize;
    if capability_count > MAX_CAPABILITIES {
        return Err(WasmError::Decode("too many game capabilities"));
    }
    let mut capabilities = Vec::new();
    capabilities
        .try_reserve_exact(capability_count)
        .map_err(|_| WasmError::Decode("game manifest allocation"))?;
    for _ in 0..capability_count {
        capabilities.push(read_string(payload, &mut cursor)?);
    }
    let manifest = CartridgeManifest {
        game_id,
        game_version,
        abi_version,
        state_version,
        capabilities,
    };
    if cursor != payload.len() {
        return Err(WasmError::Decode("invalid game manifest"));
    }
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &CartridgeManifest) -> Result<(), WasmError> {
    if manifest.abi_version == 0
        || manifest.state_version == 0
        || !valid_game_id(&manifest.game_id)
        || !valid_version(&manifest.game_version)
        || manifest.capabilities.len() > MAX_CAPABILITIES
    {
        return Err(WasmError::Decode("invalid game manifest"));
    }
    for (index, capability) in manifest.capabilities.iter().enumerate() {
        if !valid_native_namespace(capability)
            || (index > 0 && manifest.capabilities[index - 1].as_str() >= capability.as_str())
        {
            return Err(WasmError::Decode("invalid game capability"));
        }
    }
    Ok(())
}

fn write_string(output: &mut Vec<u8>, value: &str) -> Result<(), WasmError> {
    let length =
        u16::try_from(value.len()).map_err(|_| WasmError::Decode("game manifest string length"))?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_leb_u32(output: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn valid_game_id(value: &str) -> bool {
    (3..=128).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_'))
}

pub(crate) fn valid_native_namespace(value: &str) -> bool {
    let Some((qualified_name, version)) = value.rsplit_once('/') else {
        return false;
    };
    let Some((authority, module)) = qualified_name.split_once(':') else {
        return false;
    };
    !authority.contains(':')
        && value.len() <= 128
        && valid_name_part(authority, true)
        && valid_name_part(module, false)
        && version.strip_prefix('v').is_some_and(|digits| {
            !digits.is_empty()
                && digits.len() <= 10
                && digits.bytes().all(|byte| byte.is_ascii_digit())
                && !digits.starts_with('0')
        })
}

pub(crate) fn valid_native_field(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_name_part(value: &str, allow_dot: bool) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'-')
                || (allow_dot && byte == b'.')
        })
}

fn read_string(bytes: &[u8], cursor: &mut usize) -> Result<String, WasmError> {
    let len = read_u16(bytes, cursor)? as usize;
    if len == 0 || len > 1024 {
        return Err(WasmError::Decode("game manifest string length"));
    }
    let raw = take(bytes, cursor, len)?;
    let value = core::str::from_utf8(raw).map_err(|_| WasmError::Decode("game manifest utf8"))?;
    let mut string = String::new();
    string
        .try_reserve_exact(value.len())
        .map_err(|_| WasmError::Decode("game manifest allocation"))?;
    string.push_str(value);
    Ok(string)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, WasmError> {
    let raw = take(bytes, cursor, 2)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, WasmError> {
    let raw = take(bytes, cursor, 4)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8], WasmError> {
    let end = cursor
        .checked_add(len)
        .filter(|&end| end <= bytes.len())
        .ok_or(WasmError::Decode("truncated game manifest"))?;
    let value = &bytes[*cursor..end];
    *cursor = end;
    Ok(value)
}

fn read_leb_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, WasmError> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes
            .get(*cursor)
            .ok_or(WasmError::Decode("truncated manifest section size"))?;
        *cursor += 1;
        if shift == 28 && byte & 0xf0 != 0 {
            return Err(WasmError::Decode("manifest section size overflow"));
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(WasmError::Decode("manifest section size overflow"))
}
