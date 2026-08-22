//! Canonical, converter-facing description of one TinyArcade app host.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::cartridge::{valid_native_field, valid_native_namespace};
use crate::wasm::{WASM_MAX_ACTIVATION_SLOTS, WASM_MAX_DEPTH};
use crate::{
    CartridgeDescriptor, GAME_ABI_VERSION, GameLimits, Limits, MAX_CARTRIDGE_BYTES,
    MAX_NATIVE_ARITY, MAX_NATIVE_CALLS_PER_LIFECYCLE, MAX_NATIVE_FUNCTIONS, WasmError,
};

const MAGIC: &[u8; 4] = b"TAH1";
const LEGACY_SCHEMA_VERSION: u16 = 1;
const LEGACY_HEADER_LENGTH: usize = 56;
const PRIOR_SCHEMA_VERSION: u16 = 2;
const PRIOR_HEADER_LENGTH: usize = 64;
const SCHEMA_VERSION: u16 = 3;
const HEADER_LENGTH: usize = 68;
const FUNCTION_HEADER_LENGTH: usize = 12;
pub const MAX_HOST_PROFILE_BYTES: usize = 64 * 1024;

/// One app-compiled native function advertised by a host profile.
#[derive(Clone, PartialEq, Eq)]
pub struct HostFunctionV1 {
    pub module: String,
    pub field: String,
    pub n_params: u8,
    pub n_results: u8,
    pub max_calls_per_lifecycle: u32,
}

/// One exact native-import incompatibility found before cartridge execution.
#[derive(Clone, PartialEq, Eq)]
pub struct HostCompatibilityIssueV1 {
    pub module: String,
    pub field: String,
    pub required_params: u8,
    pub required_results: u8,
    /// Present when the profile has the same module/field with a different
    /// signature; absent when the function is wholly unavailable.
    pub available_params: Option<u8>,
    pub available_results: Option<u8>,
}

/// Converter-facing result of comparing a valid cartridge with one exact host
/// profile. No guest code is instantiated or executed.
pub struct HostCompatibilityReportV1 {
    pub descriptor: CartridgeDescriptor,
    pub issues: Vec<HostCompatibilityIssueV1>,
}

impl HostCompatibilityReportV1 {
    pub fn is_compatible(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Canonical static compatibility profile for one reviewed app build.
///
/// The profile contains no callbacks or executable code. It lets a converter
/// prove standard import availability and decode-time resource compatibility
/// before upload. Step/output limits remain runtime ceilings and still need the
/// ordinary dynamic converter conformance run.
#[derive(Clone)]
pub struct HostProfileV1 {
    vm_limits: Limits,
    game_limits: GameLimits,
    indexed2d_metadata: bool,
    functions: Vec<HostFunctionV1>,
}

impl HostProfileV1 {
    pub fn new(vm_limits: Limits, game_limits: GameLimits) -> Result<Self, WasmError> {
        validate_limits(vm_limits, game_limits)?;
        Ok(Self {
            vm_limits,
            game_limits,
            indexed2d_metadata: true,
            functions: Vec::new(),
        })
    }

    pub fn vm_limits(&self) -> Limits {
        self.vm_limits
    }

    pub fn game_limits(&self) -> GameLimits {
        self.game_limits
    }

    pub fn native_functions(&self) -> &[HostFunctionV1] {
        &self.functions
    }

    pub fn supports_indexed2d_metadata(&self) -> bool {
        self.indexed2d_metadata
    }

    pub fn add_native_function(
        &mut self,
        module: &str,
        field: &str,
        n_params: usize,
        n_results: usize,
        max_calls_per_lifecycle: u32,
    ) -> Result<(), WasmError> {
        if !valid_native_namespace(module)
            || !valid_native_field(field)
            || n_params > MAX_NATIVE_ARITY
            || n_results > MAX_NATIVE_ARITY
            || max_calls_per_lifecycle == 0
            || max_calls_per_lifecycle > MAX_NATIVE_CALLS_PER_LIFECYCLE
            || self.functions.len() >= MAX_NATIVE_FUNCTIONS
            || self
                .functions
                .iter()
                .any(|function| function.module == module && function.field == field)
        {
            return Err(WasmError::Trap("invalid host profile function"));
        }
        self.functions
            .try_reserve(1)
            .map_err(|_| WasmError::Trap("host profile allocation"))?;
        self.functions.push(HostFunctionV1 {
            module: module.to_string(),
            field: field.to_string(),
            n_params: n_params as u8,
            n_results: n_results as u8,
            max_calls_per_lifecycle,
        });
        self.functions.sort_by(|left, right| {
            left.module
                .as_bytes()
                .cmp(right.module.as_bytes())
                .then_with(|| left.field.as_bytes().cmp(right.field.as_bytes()))
        });
        Ok(())
    }

    /// Encode the deterministic TAH1 exchange artifact.
    pub fn encode(&self) -> Result<Vec<u8>, WasmError> {
        validate_limits(self.vm_limits, self.game_limits)?;
        let mut length = HEADER_LENGTH;
        for function in &self.functions {
            length = length
                .checked_add(FUNCTION_HEADER_LENGTH)
                .and_then(|value| value.checked_add(function.module.len()))
                .and_then(|value| value.checked_add(function.field.len()))
                .ok_or(WasmError::Trap("host profile limit"))?;
        }
        if length > MAX_HOST_PROFILE_BYTES {
            return Err(WasmError::Trap("host profile limit"));
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| WasmError::Trap("host profile allocation"))?;
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
        output.extend_from_slice(&(HEADER_LENGTH as u16).to_le_bytes());
        output.extend_from_slice(&(GAME_ABI_VERSION as u32).to_le_bytes());
        output.extend_from_slice(&(MAX_CARTRIDGE_BYTES as u32).to_le_bytes());
        output.extend_from_slice(&(self.vm_limits.max_table_elems as u32).to_le_bytes());
        output.extend_from_slice(&(self.vm_limits.max_memory_pages as u32).to_le_bytes());
        output.extend_from_slice(&self.vm_limits.max_steps.to_le_bytes());
        output.extend_from_slice(&(self.game_limits.max_render_bytes as u32).to_le_bytes());
        output.extend_from_slice(&(self.game_limits.max_audio_bytes as u32).to_le_bytes());
        output.extend_from_slice(&(self.game_limits.max_state_bytes as u32).to_le_bytes());
        output.extend_from_slice(&1u16.to_le_bytes()); // grid3d v1
        output.extend_from_slice(&1u16.to_le_bytes()); // indexed2d v1
        output.extend_from_slice(&1u16.to_le_bytes()); // tones v1
        output.extend_from_slice(&1u16.to_le_bytes()); // indexed2d metadata v1
        output.extend_from_slice(&(self.functions.len() as u16).to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&(self.vm_limits.max_call_depth as u32).to_le_bytes());
        output.extend_from_slice(&(self.vm_limits.max_activation_slots as u32).to_le_bytes());
        output.extend_from_slice(&0u32.to_le_bytes());
        for function in &self.functions {
            output.extend_from_slice(&(function.module.len() as u16).to_le_bytes());
            output.extend_from_slice(&(function.field.len() as u16).to_le_bytes());
            output.push(function.n_params);
            output.push(function.n_results);
            output.extend_from_slice(&0u16.to_le_bytes());
            output.extend_from_slice(&function.max_calls_per_lifecycle.to_le_bytes());
            output.extend_from_slice(function.module.as_bytes());
            output.extend_from_slice(function.field.as_bytes());
        }
        debug_assert_eq!(output.len(), length);
        Ok(output)
    }

    /// Decode a canonical TAH1 artifact. Noncanonical order and trailing bytes
    /// are rejected so hashes remain stable across tools.
    pub fn decode(bytes: &[u8]) -> Result<Self, WasmError> {
        if bytes.len() < LEGACY_HEADER_LENGTH
            || bytes.len() > MAX_HOST_PROFILE_BYTES
            || &bytes[..4] != MAGIC
            || read_u32(bytes, 8)? != GAME_ABI_VERSION as u32
            || read_u32(bytes, 12)? as usize != MAX_CARTRIDGE_BYTES
            || read_u16(bytes, 44)? != 1
            || read_u16(bytes, 46)? != 1
            || read_u16(bytes, 48)? != 1
        {
            return Err(WasmError::Decode("invalid host profile header"));
        }
        let schema = read_u16(bytes, 4)?;
        let header_length = read_u16(bytes, 6)? as usize;
        let (max_call_depth, max_activation_slots, count_offset, indexed2d_metadata) =
            match (schema, header_length) {
                (LEGACY_SCHEMA_VERSION, LEGACY_HEADER_LENGTH) if read_u32(bytes, 52)? == 0 => {
                    (WASM_MAX_DEPTH, WASM_MAX_ACTIVATION_SLOTS, 50, false)
                }
                (PRIOR_SCHEMA_VERSION, PRIOR_HEADER_LENGTH) if read_u32(bytes, 60)? == 0 => (
                    read_u32(bytes, 52)? as usize,
                    read_u32(bytes, 56)? as usize,
                    50,
                    false,
                ),
                (SCHEMA_VERSION, HEADER_LENGTH)
                    if read_u16(bytes, 50)? == 1
                        && read_u16(bytes, 54)? == 0
                        && read_u32(bytes, 64)? == 0 =>
                {
                    (
                        read_u32(bytes, 56)? as usize,
                        read_u32(bytes, 60)? as usize,
                        52,
                        true,
                    )
                }
                _ => return Err(WasmError::Decode("invalid host profile header")),
            };
        let vm_limits = Limits {
            max_table_elems: read_u32(bytes, 16)? as usize,
            max_memory_pages: read_u32(bytes, 20)? as usize,
            max_steps: read_u64(bytes, 24)?,
            max_call_depth,
            max_activation_slots,
        };
        let game_limits = GameLimits {
            max_render_bytes: read_u32(bytes, 32)? as usize,
            max_audio_bytes: read_u32(bytes, 36)? as usize,
            max_state_bytes: read_u32(bytes, 40)? as usize,
        };
        let count = read_u16(bytes, count_offset)? as usize;
        if count > MAX_NATIVE_FUNCTIONS {
            return Err(WasmError::Decode("host profile function limit"));
        }
        let mut profile = Self::new(vm_limits, game_limits)
            .map_err(|_| WasmError::Decode("invalid host profile limits"))?;
        profile.indexed2d_metadata = indexed2d_metadata;
        let mut cursor = header_length;
        let mut previous: Option<(String, String)> = None;
        for _ in 0..count {
            if bytes.len().saturating_sub(cursor) < FUNCTION_HEADER_LENGTH {
                return Err(WasmError::Decode("truncated host profile function"));
            }
            let module_len = read_u16(bytes, cursor)? as usize;
            let field_len = read_u16(bytes, cursor + 2)? as usize;
            let n_params = bytes[cursor + 4] as usize;
            let n_results = bytes[cursor + 5] as usize;
            if read_u16(bytes, cursor + 6)? != 0 {
                return Err(WasmError::Decode("invalid host profile function"));
            }
            let max_calls = read_u32(bytes, cursor + 8)?;
            cursor += FUNCTION_HEADER_LENGTH;
            let module = read_string(bytes, &mut cursor, module_len)?;
            let field = read_string(bytes, &mut cursor, field_len)?;
            if let Some((prior_module, prior_field)) = &previous
                && (module.as_bytes(), field.as_bytes())
                    <= (prior_module.as_bytes(), prior_field.as_bytes())
            {
                return Err(WasmError::Decode("host profile is not canonical"));
            }
            profile
                .add_native_function(&module, &field, n_params, n_results, max_calls)
                .map_err(|_| WasmError::Decode("invalid host profile function"))?;
            previous = Some((module, field));
        }
        if cursor != bytes.len() {
            return Err(WasmError::Decode("trailing host profile bytes"));
        }
        Ok(profile)
    }

    /// Produce an exact, non-executing compatibility report for converter UI
    /// and CI. Parse/profile-limit failures remain typed errors; a valid
    /// cartridge with unavailable native imports returns a report with issues.
    pub fn compatibility_report(
        &self,
        wasm: &[u8],
    ) -> Result<HostCompatibilityReportV1, WasmError> {
        if wasm.len() > MAX_CARTRIDGE_BYTES {
            return Err(WasmError::Decode("cartridge exceeds byte limit"));
        }
        let descriptor = CartridgeDescriptor::inspect(wasm, self.vm_limits)?;
        let mut issues = Vec::new();
        if !self.indexed2d_metadata
            && descriptor.imports.iter().any(|import| {
                import.module == "tinyarcade:core/v1"
                    && import.field == "indexed2d_metadata_version"
            })
        {
            issues
                .try_reserve(1)
                .map_err(|_| WasmError::Trap("host compatibility report allocation"))?;
            issues.push(HostCompatibilityIssueV1 {
                module: "tinyarcade:core/v1".to_string(),
                field: "indexed2d_metadata_version".to_string(),
                required_params: 0,
                required_results: 1,
                available_params: None,
                available_results: None,
            });
        }
        for import in descriptor
            .imports
            .iter()
            .filter(|import| import.module != "tinyarcade:core/v1")
        {
            let available = self.functions.iter().find(|function| {
                function.module == import.module && function.field == import.field
            });
            if available.is_some_and(|function| {
                usize::from(function.n_params) == import.n_params
                    && usize::from(function.n_results) == import.n_results
            }) {
                continue;
            }
            issues
                .try_reserve(1)
                .map_err(|_| WasmError::Trap("host compatibility report allocation"))?;
            issues.push(HostCompatibilityIssueV1 {
                module: import.module.clone(),
                field: import.field.clone(),
                required_params: import.n_params as u8,
                required_results: import.n_results as u8,
                available_params: available.map(|function| function.n_params),
                available_results: available.map(|function| function.n_results),
            });
        }
        Ok(HostCompatibilityReportV1 { descriptor, issues })
    }

    /// Statically check one cartridge against this exact app-build profile.
    pub fn inspect_cartridge(&self, wasm: &[u8]) -> Result<CartridgeDescriptor, WasmError> {
        let report = self.compatibility_report(wasm)?;
        if !report.is_compatible() {
            return Err(WasmError::Trap("host profile native import unavailable"));
        }
        Ok(report.descriptor)
    }
}

fn validate_limits(vm: Limits, game: GameLimits) -> Result<(), WasmError> {
    if vm.max_table_elems == 0
        || vm.max_table_elems > u32::MAX as usize
        || vm.max_memory_pages == 0
        || vm.max_memory_pages > u32::MAX as usize
        || vm.max_steps == 0
        || vm.max_call_depth == 0
        || vm.max_call_depth > u32::MAX as usize
        || vm.max_activation_slots == 0
        || vm.max_activation_slots > u32::MAX as usize
        || game.max_render_bytes > u32::MAX as usize
        || game.max_audio_bytes > u32::MAX as usize
        || game.max_state_bytes > u32::MAX as usize
    {
        return Err(WasmError::Trap("invalid host profile limits"));
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, WasmError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(WasmError::Decode("truncated host profile"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, WasmError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(WasmError::Decode("truncated host profile"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, WasmError> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or(WasmError::Decode("truncated host profile"))?;
    Ok(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

fn read_string(bytes: &[u8], cursor: &mut usize, length: usize) -> Result<String, WasmError> {
    if length == 0 || length > 128 {
        return Err(WasmError::Decode("invalid host profile string"));
    }
    let end = cursor
        .checked_add(length)
        .ok_or(WasmError::Decode("host profile string overflow"))?;
    let value = core::str::from_utf8(
        bytes
            .get(*cursor..end)
            .ok_or(WasmError::Decode("truncated host profile string"))?,
    )
    .map_err(|_| WasmError::Decode("invalid host profile UTF-8"))?;
    *cursor = end;
    Ok(value.to_string())
}
