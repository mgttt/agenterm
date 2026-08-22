//! Versioned C ownership boundary for Swift/Objective-C hosts.

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use core::slice;
use std::boxed::Box;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Mutex;
use std::thread::{self, ThreadId};

use crate::{
    CartridgeCache, CartridgeDescriptor, CartridgeTrustStore, CatalogEntry, CompletionError,
    ExecutionStats, GameFrame, GameInput, GameLimits, GameRuntime, GuestResourceHandle,
    HostCompatibilityReportV1, HostCompletionQueue, HostProfileV1, Limits,
    MAX_NATIVE_CALLS_PER_LIFECYCLE, MAX_NATIVE_FUNCTIONS, NativeModuleRegistry, ReplayRecorderV1,
    ReplayTraceV1, ResourceDomainAllocator, WasmError,
};

pub const STATUS_OK: i32 = 0;
pub const STATUS_INVALID_ARGUMENT: i32 = 1;
pub const STATUS_DECODE: i32 = 2;
pub const STATUS_TRAP: i32 = 3;
pub const STATUS_BUFFER_TOO_SMALL: i32 = 4;
pub const STATUS_WRONG_THREAD: i32 = 5;
pub const STATUS_FAILED_INSTANCE: i32 = 6;
pub const STATUS_PANIC: i32 = 7;
pub const STATUS_TRUST: i32 = 8;
pub const STATUS_STORAGE: i32 = 9;

thread_local! {
    static LAST_ERROR: Cell<&'static str> = const { Cell::new("") };
    /// Native callbacks may call arbitrary host code while the VM owns borrowed
    /// guest state, so every C entry point must reject attempts to borrow any
    /// runtime handle until the callback returns.
    static ACTIVE_NATIVE_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

struct NativeCallbackGuard;

impl NativeCallbackGuard {
    fn enter() -> Result<Self, FfiError> {
        ACTIVE_NATIVE_CALLBACK.with(|active| {
            if active.replace(true) {
                Err(FfiError::new(
                    STATUS_INVALID_ARGUMENT,
                    "native callback reentrancy is forbidden",
                ))
            } else {
                Ok(Self)
            }
        })
    }
}

impl Drop for NativeCallbackGuard {
    fn drop(&mut self) {
        ACTIVE_NATIVE_CALLBACK.with(|active| active.set(false));
    }
}

#[repr(C)]
pub struct TinyArcadeConfigV1 {
    pub struct_size: u32,
    pub max_table_elems: u32,
    pub max_memory_pages: u32,
    pub max_steps: u64,
    pub max_render_bytes: u32,
    pub max_audio_bytes: u32,
    pub max_state_bytes: u32,
    pub rng_seed: u32,
    pub max_call_depth: u32,
    pub max_activation_slots: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TinyArcadeConfigV1Prefix {
    struct_size: u32,
    max_table_elems: u32,
    max_memory_pages: u32,
    max_steps: u64,
    max_render_bytes: u32,
    max_audio_bytes: u32,
    max_state_bytes: u32,
    rng_seed: u32,
}

struct RuntimeConfig {
    vm_limits: Limits,
    game_limits: GameLimits,
    rng_seed: u32,
}

#[repr(C)]
pub struct TinyArcadeExecutionStatsV1 {
    pub struct_size: u32,
    pub lifecycle: u32,
    pub wasm_steps: u64,
    pub memory_pages: u32,
    pub table_elements: u32,
    pub native_calls: u32,
    pub render_bytes: u32,
    pub audio_bytes: u32,
    pub state_bytes: u32,
}

#[repr(C)]
pub struct TinyArcadeExecutionStatsV2 {
    pub struct_size: u32,
    pub lifecycle: u32,
    pub wasm_steps: u64,
    pub peak_call_depth: u32,
    pub peak_activation_slots: u32,
    pub memory_pages: u32,
    pub table_elements: u32,
    pub native_calls: u32,
    pub render_bytes: u32,
    pub audio_bytes: u32,
    pub state_bytes: u32,
}

#[repr(C)]
pub struct TinyArcadeCatalogEntryV1 {
    pub struct_size: u32,
    pub game_id: *const u8,
    pub game_id_len: usize,
    pub game_version: *const u8,
    pub game_version_len: usize,
    pub abi_version: u32,
    pub state_version: u32,
    pub wasm_length: u64,
    pub wasm_sha256: *const u8,
    pub wasm_sha256_len: usize,
    pub signing_key_id: *const u8,
    pub signing_key_id_len: usize,
    pub signature: *const u8,
    pub signature_len: usize,
}

pub type TinyArcadeNativeCallbackV1 = unsafe extern "C" fn(
    context: *mut c_void,
    params: *const i32,
    n_params: usize,
    results: *mut i32,
    n_results: usize,
    memory: *mut u8,
    memory_len: usize,
) -> i32;

#[repr(C)]
pub struct TinyArcadeNativeFunctionV1 {
    pub struct_size: u32,
    pub module: *const u8,
    pub module_len: usize,
    pub field: *const u8,
    pub field_len: usize,
    pub n_params: u32,
    pub n_results: u32,
    pub max_calls_per_lifecycle: u32,
    pub callback: Option<TinyArcadeNativeCallbackV1>,
    pub context: *mut c_void,
}

/// App-owned completion channel. It may be called from a native callback and
/// is bound to at most one live runtime at a time.
pub struct TinyArcadeCompletionV1 {
    owner: ThreadId,
    module: String,
    max_calls_per_lifecycle: u32,
    queue: Rc<RefCell<HostCompletionQueue>>,
    bound: bool,
}

static COMPLETION_DOMAINS: Mutex<ResourceDomainAllocator> =
    Mutex::new(ResourceDomainAllocator::new());

struct CompletionBindings {
    channels: Vec<*mut TinyArcadeCompletionV1>,
}

impl Drop for CompletionBindings {
    fn drop(&mut self) {
        for channel in &self.channels {
            // A bound channel cannot be closed, and runtime/channel operations
            // share one owner thread, so these pointers remain live here.
            if let Some(channel) = unsafe { channel.as_mut() } {
                let Ok(mut queue) = channel.queue.try_borrow_mut() else {
                    continue;
                };
                queue.clear();
                drop(queue);
                channel.bound = false;
            }
        }
    }
}

pub struct TinyArcadeTrustStoreV1 {
    owner: ThreadId,
    store: CartridgeTrustStore,
}

pub struct TinyArcadeCartridgeCacheV1 {
    owner: ThreadId,
    cache: CartridgeCache,
    wasm: Vec<u8>,
}

pub struct TinyArcadeRuntimeV1 {
    owner: ThreadId,
    runtime: GameRuntime,
    frame: Option<GameFrame>,
    snapshot: Vec<u8>,
    replay_recorder: Option<ReplayRecorderV1>,
    replay: Vec<u8>,
    _completion_bindings: CompletionBindings,
}

#[derive(Clone, Copy)]
struct FfiError {
    status: i32,
    message: &'static str,
}

impl FfiError {
    const fn new(status: i32, message: &'static str) -> Self {
        Self { status, message }
    }
}

unsafe fn read_runtime_config(
    config: *const TinyArcadeConfigV1,
) -> Result<RuntimeConfig, FfiError> {
    if config.is_null() {
        return Err(FfiError::new(STATUS_INVALID_ARGUMENT, "null config"));
    }
    // Read only the original 40-byte prefix first. This keeps an already-built
    // ABI v1.8 caller valid after v1.9 appends call-stack limits to the struct.
    let prefix = unsafe { config.cast::<TinyArcadeConfigV1Prefix>().read() };
    if prefix.struct_size < size_of::<TinyArcadeConfigV1Prefix>() as u32
        || prefix.max_table_elems == 0
        || prefix.max_memory_pages == 0
        || prefix.max_steps == 0
    {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid runtime configuration",
        ));
    }
    let defaults = Limits::default();
    let (max_call_depth, max_activation_slots) =
        if prefix.struct_size >= size_of::<TinyArcadeConfigV1>() as u32 {
            let config = unsafe { config.read() };
            (
                config.max_call_depth as usize,
                config.max_activation_slots as usize,
            )
        } else {
            (defaults.max_call_depth, defaults.max_activation_slots)
        };
    if max_call_depth == 0 || max_activation_slots == 0 {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid runtime configuration",
        ));
    }
    Ok(RuntimeConfig {
        vm_limits: Limits {
            max_table_elems: prefix.max_table_elems as usize,
            max_memory_pages: prefix.max_memory_pages as usize,
            max_steps: prefix.max_steps,
            max_call_depth,
            max_activation_slots,
        },
        game_limits: GameLimits {
            max_render_bytes: prefix.max_render_bytes as usize,
            max_audio_bytes: prefix.max_audio_bytes as usize,
            max_state_bytes: prefix.max_state_bytes as usize,
        },
        rng_seed: prefix.rng_seed,
    })
}

fn set_error(message: &'static str) {
    LAST_ERROR.with(|slot| slot.set(message));
}

fn boundary(f: impl FnOnce() -> Result<(), FfiError>) -> i32 {
    set_error("");
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => {
            set_error("");
            STATUS_OK
        }
        Ok(Err(error)) => {
            set_error(error.message);
            error.status
        }
        Err(_) => {
            set_error("panic inside tinyarcade runtime");
            STATUS_PANIC
        }
    }
}

fn runtime_boundary(
    runtime: *mut TinyArcadeRuntimeV1,
    f: impl FnOnce(&mut TinyArcadeRuntimeV1) -> Result<(), FfiError>,
) -> i32 {
    set_error("");
    match catch_unwind(AssertUnwindSafe(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        f(runtime)
    })) {
        Ok(Ok(())) => {
            set_error("");
            STATUS_OK
        }
        Ok(Err(error)) => {
            set_error(error.message);
            error.status
        }
        Err(_) => {
            if let Ok(runtime) = unsafe { runtime_mut(runtime) } {
                runtime.runtime.latch_host_panic();
                runtime.frame = None;
                runtime.snapshot.clear();
                runtime.replay_recorder = None;
                runtime.replay.clear();
            }
            set_error("panic inside tinyarcade runtime");
            STATUS_PANIC
        }
    }
}

fn wasm_error(error: WasmError) -> FfiError {
    match error {
        WasmError::Decode(message) => FfiError::new(STATUS_DECODE, message),
        WasmError::Trap("invalid game input") => {
            FfiError::new(STATUS_INVALID_ARGUMENT, "invalid game input")
        }
        WasmError::Trap("game instance failed") => {
            FfiError::new(STATUS_FAILED_INSTANCE, "game instance failed")
        }
        WasmError::Trap(message) => FfiError::new(STATUS_TRAP, message),
    }
}

fn cache_error(error: WasmError) -> FfiError {
    let message = error.message();
    let status = if matches!(
        message,
        "invalid signed catalog entry"
            | "catalog signing allocation"
            | "invalid catalog trust key"
            | "unknown catalog trust key"
            | "untrusted or revoked catalog key"
            | "revoked cartridge content"
            | "invalid catalog signature"
            | "cartridge length mismatch"
            | "cartridge hash mismatch"
            | "catalog manifest mismatch"
            | "catalog string length"
            | "active catalog entry mismatch"
            | "rollback catalog entry mismatch"
    ) {
        STATUS_TRUST
    } else {
        STATUS_STORAGE
    };
    FfiError::new(status, message)
}

unsafe fn runtime_mut<'a>(
    runtime: *mut TinyArcadeRuntimeV1,
) -> Result<&'a mut TinyArcadeRuntimeV1, FfiError> {
    if ACTIVE_NATIVE_CALLBACK.with(Cell::get) {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "runtime reentrancy is forbidden",
        ));
    }
    let runtime = unsafe { runtime.as_mut() }.ok_or(FfiError::new(
        STATUS_INVALID_ARGUMENT,
        "null runtime handle",
    ))?;
    if runtime.owner != thread::current().id() {
        return Err(FfiError::new(
            STATUS_WRONG_THREAD,
            "runtime used from a different thread",
        ));
    }
    Ok(runtime)
}

unsafe fn completion_mut<'a>(
    completion: *mut TinyArcadeCompletionV1,
) -> Result<&'a mut TinyArcadeCompletionV1, FfiError> {
    let completion = unsafe { completion.as_mut() }.ok_or(FfiError::new(
        STATUS_INVALID_ARGUMENT,
        "null completion channel",
    ))?;
    if completion.owner != thread::current().id() {
        return Err(FfiError::new(
            STATUS_WRONG_THREAD,
            "completion channel used from a different thread",
        ));
    }
    Ok(completion)
}

fn completion_error(error: CompletionError) -> FfiError {
    match error {
        CompletionError::Full => FfiError::new(STATUS_STORAGE, "completion queue is full"),
        CompletionError::AllocationFailed => {
            FfiError::new(STATUS_STORAGE, "completion allocation failed")
        }
        CompletionError::StaleHandle => {
            FfiError::new(STATUS_INVALID_ARGUMENT, "stale completion ticket")
        }
        CompletionError::AlreadyCompleted => {
            FfiError::new(STATUS_INVALID_ARGUMENT, "completion already delivered")
        }
        CompletionError::NotReady => {
            FfiError::new(STATUS_INVALID_ARGUMENT, "completion is not ready")
        }
        CompletionError::PayloadTooLarge => FfiError::new(
            STATUS_BUFFER_TOO_SMALL,
            "completion payload exceeds reservation",
        ),
        CompletionError::ByteBudgetExceeded => {
            FfiError::new(STATUS_BUFFER_TOO_SMALL, "completion byte budget exceeded")
        }
    }
}

unsafe fn trust_mut<'a>(
    trust: *mut TinyArcadeTrustStoreV1,
) -> Result<&'a mut TinyArcadeTrustStoreV1, FfiError> {
    let trust = unsafe { trust.as_mut() }.ok_or(FfiError::new(
        STATUS_INVALID_ARGUMENT,
        "null trust store handle",
    ))?;
    if trust.owner != thread::current().id() {
        return Err(FfiError::new(
            STATUS_WRONG_THREAD,
            "trust store used from a different thread",
        ));
    }
    Ok(trust)
}

unsafe fn cache_mut<'a>(
    cache: *mut TinyArcadeCartridgeCacheV1,
) -> Result<&'a mut TinyArcadeCartridgeCacheV1, FfiError> {
    let cache = unsafe { cache.as_mut() }.ok_or(FfiError::new(
        STATUS_INVALID_ARGUMENT,
        "null cartridge cache handle",
    ))?;
    if cache.owner != thread::current().id() {
        return Err(FfiError::new(
            STATUS_WRONG_THREAD,
            "cartridge cache used from a different thread",
        ));
    }
    Ok(cache)
}

fn cache_boundary(
    cache: *mut TinyArcadeCartridgeCacheV1,
    f: impl FnOnce(&mut TinyArcadeCartridgeCacheV1) -> Result<(), FfiError>,
) -> i32 {
    set_error("");
    match catch_unwind(AssertUnwindSafe(|| {
        let cache = unsafe { cache_mut(cache)? };
        cache.wasm.clear();
        f(cache)
    })) {
        Ok(Ok(())) => {
            set_error("");
            STATUS_OK
        }
        Ok(Err(error)) => {
            set_error(error.message);
            error.status
        }
        Err(_) => {
            if let Ok(cache) = unsafe { cache_mut(cache) } {
                cache.wasm.clear();
            }
            set_error("panic inside tinyarcade cartridge cache");
            STATUS_PANIC
        }
    }
}

unsafe fn input_bytes<'a>(
    pointer: *const u8,
    length: usize,
    message: &'static str,
) -> Result<&'a [u8], FfiError> {
    if pointer.is_null() || length == 0 {
        return Err(FfiError::new(STATUS_INVALID_ARGUMENT, message));
    }
    Ok(unsafe { slice::from_raw_parts(pointer, length) })
}

unsafe fn input_string(
    pointer: *const u8,
    length: usize,
    max_length: usize,
    message: &'static str,
) -> Result<String, FfiError> {
    if length > max_length {
        return Err(FfiError::new(STATUS_INVALID_ARGUMENT, message));
    }
    let bytes = unsafe { input_bytes(pointer, length, message)? };
    let value =
        core::str::from_utf8(bytes).map_err(|_| FfiError::new(STATUS_INVALID_ARGUMENT, message))?;
    Ok(value.to_owned())
}

unsafe fn catalog_entry(entry: *const TinyArcadeCatalogEntryV1) -> Result<CatalogEntry, FfiError> {
    let entry = unsafe { entry.as_ref() }
        .ok_or(FfiError::new(STATUS_INVALID_ARGUMENT, "null catalog entry"))?;
    if entry.struct_size < size_of::<TinyArcadeCatalogEntryV1>() as u32
        || entry.wasm_sha256_len != 32
        || entry.signature_len != 64
    {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid catalog entry layout",
        ));
    }
    let hash = unsafe {
        input_bytes(
            entry.wasm_sha256,
            entry.wasm_sha256_len,
            "invalid catalog hash",
        )?
    };
    let signature = unsafe {
        input_bytes(
            entry.signature,
            entry.signature_len,
            "invalid catalog signature",
        )?
    };
    let mut fixed_hash = [0; 32];
    fixed_hash.copy_from_slice(hash);
    let mut fixed_signature = [0; 64];
    fixed_signature.copy_from_slice(signature);
    Ok(CatalogEntry {
        game_id: unsafe {
            input_string(
                entry.game_id,
                entry.game_id_len,
                128,
                "invalid catalog game id",
            )?
        },
        game_version: unsafe {
            input_string(
                entry.game_version,
                entry.game_version_len,
                64,
                "invalid catalog game version",
            )?
        },
        abi_version: entry.abi_version,
        state_version: entry.state_version,
        wasm_length: entry.wasm_length,
        wasm_sha256: fixed_hash,
        signing_key_id: unsafe {
            input_string(
                entry.signing_key_id,
                entry.signing_key_id_len,
                64,
                "invalid catalog signing key id",
            )?
        },
        signature: fixed_signature,
    })
}

unsafe fn native_registry(
    functions: *const TinyArcadeNativeFunctionV1,
    function_count: usize,
) -> Result<NativeModuleRegistry, FfiError> {
    if function_count > MAX_NATIVE_FUNCTIONS || (function_count != 0 && functions.is_null()) {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid native function table",
        ));
    }
    let functions = if function_count == 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(functions, function_count) }
    };
    let mut registry = NativeModuleRegistry::new();
    for function in functions {
        if function.struct_size < size_of::<TinyArcadeNativeFunctionV1>() as u32 {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid native function layout",
            ));
        }
        let module = unsafe {
            input_string(
                function.module,
                function.module_len,
                128,
                "invalid native module",
            )?
        };
        let field = unsafe {
            input_string(
                function.field,
                function.field_len,
                128,
                "invalid native field",
            )?
        };
        let callback = function.callback.ok_or(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "null native callback",
        ))?;
        let n_params = function.n_params as usize;
        let n_results = function.n_results as usize;
        if function.max_calls_per_lifecycle == 0
            || function.max_calls_per_lifecycle > MAX_NATIVE_CALLS_PER_LIFECYCLE
        {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid native call budget",
            ));
        }
        let max_calls_per_lifecycle = function.max_calls_per_lifecycle;
        let context = function.context;
        registry
            .register_in_place_with_call_limit(
                &module,
                &field,
                n_params,
                n_results,
                max_calls_per_lifecycle,
                move |params, results, memory| {
                    let _active = NativeCallbackGuard::enter()
                        .map_err(|_| WasmError::Trap("native callback reentrancy is forbidden"))?;
                    let status = unsafe {
                        callback(
                            context,
                            if params.is_empty() {
                                ptr::null()
                            } else {
                                params.as_ptr()
                            },
                            params.len(),
                            if results.is_empty() {
                                ptr::null_mut()
                            } else {
                                results.as_mut_ptr()
                            },
                            results.len(),
                            memory.as_mut_ptr(),
                            memory.len(),
                        )
                    };
                    if status == STATUS_OK {
                        Ok(())
                    } else {
                        Err(WasmError::Trap("native capability callback failed"))
                    }
                },
            )
            .map_err(|_| {
                FfiError::new(
                    STATUS_INVALID_ARGUMENT,
                    "invalid native function registration",
                )
            })?;
    }
    Ok(registry)
}

unsafe fn bind_completion_channels(
    registry: &mut NativeModuleRegistry,
    channels: *const *mut TinyArcadeCompletionV1,
    channel_count: usize,
) -> Result<CompletionBindings, FfiError> {
    if channel_count > MAX_NATIVE_FUNCTIONS / 3 || (channel_count != 0 && channels.is_null()) {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid completion channel table",
        ));
    }
    let channels = if channel_count == 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(channels, channel_count) }
    };
    let mut bound = Vec::new();
    bound
        .try_reserve_exact(channels.len())
        .map_err(|_| FfiError::new(STATUS_STORAGE, "completion binding allocation failed"))?;
    for &pointer in channels {
        if bound.contains(&pointer) {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "duplicate completion channel",
            ));
        }
        let channel = unsafe { completion_mut(pointer)? };
        if channel.bound || !channel.queue.borrow().is_empty() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "completion channel is already bound or active",
            ));
        }
        registry
            .attach_completion_queue(
                &channel.module,
                channel.queue.clone(),
                channel.max_calls_per_lifecycle,
            )
            .map_err(wasm_error)?;
        bound.push(pointer);
    }
    for &pointer in &bound {
        unsafe { completion_mut(pointer)? }.bound = true;
    }
    Ok(CompletionBindings { channels: bound })
}

unsafe fn copy_bytes(
    bytes: &[u8],
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> Result<(), FfiError> {
    if output_len.is_null() {
        return Err(FfiError::new(STATUS_INVALID_ARGUMENT, "null output length"));
    }
    unsafe { output_len.write(bytes.len()) };
    if capacity < bytes.len() || (output.is_null() && !bytes.is_empty()) {
        return Err(FfiError::new(
            STATUS_BUFFER_TOO_SMALL,
            "output buffer too small",
        ));
    }
    if !bytes.is_empty() {
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn tinyarcade_v1_abi_version() -> u32 {
    (1 << 16) | 12
}

fn append_u16(output: &mut Vec<u8>, value: usize) -> Result<(), FfiError> {
    let value = u16::try_from(value)
        .map_err(|_| FfiError::new(STATUS_DECODE, "cartridge descriptor limit"))?;
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn encode_descriptor(
    descriptor: &CartridgeDescriptor,
    wasm_len: usize,
) -> Result<Vec<u8>, FfiError> {
    let wasm_len = u32::try_from(wasm_len)
        .map_err(|_| FfiError::new(STATUS_DECODE, "cartridge descriptor limit"))?;
    let mut encoded_len = 32usize
        .checked_add(descriptor.manifest.game_id.len())
        .and_then(|value| value.checked_add(descriptor.manifest.game_version.len()))
        .ok_or(FfiError::new(STATUS_DECODE, "cartridge descriptor limit"))?;
    for capability in &descriptor.manifest.capabilities {
        encoded_len = encoded_len
            .checked_add(2)
            .and_then(|value| value.checked_add(capability.len()))
            .ok_or(FfiError::new(STATUS_DECODE, "cartridge descriptor limit"))?;
    }
    for import in &descriptor.imports {
        encoded_len = encoded_len
            .checked_add(8)
            .and_then(|value| value.checked_add(import.module.len()))
            .and_then(|value| value.checked_add(import.field.len()))
            .ok_or(FfiError::new(STATUS_DECODE, "cartridge descriptor limit"))?;
    }
    if encoded_len > 64 * 1024 {
        return Err(FfiError::new(STATUS_DECODE, "cartridge descriptor limit"));
    }
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(encoded_len)
        .map_err(|_| FfiError::new(STATUS_DECODE, "cartridge descriptor allocation"))?;
    encoded.extend_from_slice(b"TAD1");
    encoded.extend_from_slice(&1u16.to_le_bytes());
    encoded.extend_from_slice(&32u16.to_le_bytes());
    encoded.extend_from_slice(&descriptor.manifest.abi_version.to_le_bytes());
    encoded.extend_from_slice(&descriptor.manifest.state_version.to_le_bytes());
    append_u16(&mut encoded, descriptor.manifest.game_id.len())?;
    append_u16(&mut encoded, descriptor.manifest.game_version.len())?;
    append_u16(&mut encoded, descriptor.manifest.capabilities.len())?;
    append_u16(&mut encoded, descriptor.imports.len())?;
    encoded.extend_from_slice(&wasm_len.to_le_bytes());
    encoded.extend_from_slice(&0u32.to_le_bytes());
    encoded.extend_from_slice(descriptor.manifest.game_id.as_bytes());
    encoded.extend_from_slice(descriptor.manifest.game_version.as_bytes());
    for capability in &descriptor.manifest.capabilities {
        append_u16(&mut encoded, capability.len())?;
        encoded.extend_from_slice(capability.as_bytes());
    }
    for import in &descriptor.imports {
        append_u16(&mut encoded, import.module.len())?;
        append_u16(&mut encoded, import.field.len())?;
        encoded.push(
            u8::try_from(import.n_params)
                .map_err(|_| FfiError::new(STATUS_DECODE, "cartridge descriptor import arity"))?,
        );
        encoded.push(
            u8::try_from(import.n_results)
                .map_err(|_| FfiError::new(STATUS_DECODE, "cartridge descriptor import arity"))?,
        );
        encoded.push(u8::from(import.module != "tinyarcade:core/v1"));
        encoded.push(0);
        encoded.extend_from_slice(import.module.as_bytes());
        encoded.extend_from_slice(import.field.as_bytes());
    }
    debug_assert_eq!(encoded.len(), encoded_len);
    Ok(encoded)
}

fn encode_compatibility_report(
    report: &HostCompatibilityReportV1,
    wasm_len: usize,
) -> Result<Vec<u8>, FfiError> {
    let descriptor = encode_descriptor(&report.descriptor, wasm_len)?;
    let mut encoded_len = 16usize
        .checked_add(descriptor.len())
        .ok_or(FfiError::new(STATUS_DECODE, "compatibility report limit"))?;
    for issue in &report.issues {
        encoded_len = encoded_len
            .checked_add(8)
            .and_then(|value| value.checked_add(issue.module.len()))
            .and_then(|value| value.checked_add(issue.field.len()))
            .ok_or(FfiError::new(STATUS_DECODE, "compatibility report limit"))?;
    }
    if encoded_len > 64 * 1024 || report.issues.len() > u16::MAX as usize {
        return Err(FfiError::new(STATUS_DECODE, "compatibility report limit"));
    }
    let descriptor_len = u32::try_from(descriptor.len())
        .map_err(|_| FfiError::new(STATUS_DECODE, "compatibility report limit"))?;
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(encoded_len)
        .map_err(|_| FfiError::new(STATUS_DECODE, "compatibility report allocation"))?;
    encoded.extend_from_slice(b"TAC1");
    encoded.extend_from_slice(&1u16.to_le_bytes());
    encoded.extend_from_slice(&16u16.to_le_bytes());
    encoded.extend_from_slice(&(report.issues.len() as u16).to_le_bytes());
    encoded.extend_from_slice(&0u16.to_le_bytes());
    encoded.extend_from_slice(&descriptor_len.to_le_bytes());
    encoded.extend_from_slice(&descriptor);
    for issue in &report.issues {
        append_u16(&mut encoded, issue.module.len())?;
        append_u16(&mut encoded, issue.field.len())?;
        encoded.push(issue.required_params);
        encoded.push(issue.required_results);
        encoded.push(issue.available_params.unwrap_or(u8::MAX));
        encoded.push(issue.available_results.unwrap_or(u8::MAX));
        encoded.extend_from_slice(issue.module.as_bytes());
        encoded.extend_from_slice(issue.field.as_bytes());
    }
    debug_assert_eq!(encoded.len(), encoded_len);
    Ok(encoded)
}

/// Statically validate and describe a cartridge without instantiating it or
/// running its start/lifecycle functions. The canonical TAD1 result uses the
/// ordinary two-stage copy protocol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_cartridge_descriptor(
    wasm: *const u8,
    wasm_len: usize,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let wasm = unsafe { input_bytes(wasm, wasm_len, "invalid cartridge input")? };
        let descriptor =
            CartridgeDescriptor::inspect(wasm, Limits::default()).map_err(wasm_error)?;
        let encoded = encode_descriptor(&descriptor, wasm.len())?;
        unsafe { copy_bytes(&encoded, output, capacity, output_len) }
    })
}

/// Export the exact runtime limits and app-compiled native registry as the
/// callback-free canonical TAH1 profile consumed by converters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_host_profile(
    config: *const TinyArcadeConfigV1,
    functions: *const TinyArcadeNativeFunctionV1,
    function_count: usize,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let config = unsafe { read_runtime_config(config)? };
        let registry = unsafe { native_registry(functions, function_count)? };
        let profile = registry
            .host_profile(config.vm_limits, config.game_limits)
            .map_err(wasm_error)?;
        let encoded = profile.encode().map_err(wasm_error)?;
        unsafe { copy_bytes(&encoded, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_host_profile_with_completions(
    config: *const TinyArcadeConfigV1,
    functions: *const TinyArcadeNativeFunctionV1,
    function_count: usize,
    completions: *const *mut TinyArcadeCompletionV1,
    completion_count: usize,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let config = unsafe { read_runtime_config(config)? };
        let mut registry = unsafe { native_registry(functions, function_count)? };
        let _bindings =
            unsafe { bind_completion_channels(&mut registry, completions, completion_count)? };
        let profile = registry
            .host_profile(config.vm_limits, config.game_limits)
            .map_err(wasm_error)?;
        let encoded = profile.encode().map_err(wasm_error)?;
        unsafe { copy_bytes(&encoded, output, capacity, output_len) }
    })
}

/// Statically check standard cartridge bytes against one exact TAH1 profile.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_check_cartridge_host_profile(
    wasm: *const u8,
    wasm_len: usize,
    profile: *const u8,
    profile_len: usize,
) -> i32 {
    boundary(|| {
        let wasm = unsafe { input_bytes(wasm, wasm_len, "invalid cartridge input")? };
        let profile = unsafe { input_bytes(profile, profile_len, "invalid host profile input")? };
        HostProfileV1::decode(profile)
            .and_then(|profile| profile.inspect_cartridge(wasm))
            .map_err(wasm_error)?;
        Ok(())
    })
}

/// Statically check a cartridge against one exact TAH1 profile and return the
/// canonical TAD1 descriptor produced by that same bounded inspection pass.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_compatible_cartridge_descriptor(
    wasm: *const u8,
    wasm_len: usize,
    profile: *const u8,
    profile_len: usize,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let wasm = unsafe { input_bytes(wasm, wasm_len, "invalid cartridge input")? };
        let profile = unsafe { input_bytes(profile, profile_len, "invalid host profile input")? };
        let descriptor = HostProfileV1::decode(profile)
            .and_then(|profile| profile.inspect_cartridge(wasm))
            .map_err(wasm_error)?;
        let encoded = encode_descriptor(&descriptor, wasm.len())?;
        unsafe { copy_bytes(&encoded, output, capacity, output_len) }
    })
}

/// Return one bounded, canonical TAC1 compatibility report without
/// instantiating the cartridge. The report embeds the profile-bound TAD1
/// descriptor and every unavailable or signature-mismatched import.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_host_compatibility_report(
    wasm: *const u8,
    wasm_len: usize,
    profile: *const u8,
    profile_len: usize,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let wasm = unsafe { input_bytes(wasm, wasm_len, "invalid cartridge input")? };
        let profile = unsafe { input_bytes(profile, profile_len, "invalid host profile input")? };
        let report = HostProfileV1::decode(profile)
            .and_then(|profile| profile.compatibility_report(wasm))
            .map_err(wasm_error)?;
        let encoded = encode_compatibility_report(&report, wasm.len())?;
        unsafe { copy_bytes(&encoded, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_default_config(config: *mut TinyArcadeConfigV1) -> i32 {
    boundary(|| {
        if config.is_null() {
            return Err(FfiError::new(STATUS_INVALID_ARGUMENT, "null config"));
        }
        let defaults = Limits::default();
        let game = GameLimits::default();
        let value = TinyArcadeConfigV1 {
            struct_size: size_of::<TinyArcadeConfigV1>() as u32,
            max_table_elems: defaults.max_table_elems as u32,
            max_memory_pages: defaults.max_memory_pages as u32,
            max_steps: defaults.max_steps,
            max_render_bytes: game.max_render_bytes as u32,
            max_audio_bytes: game.max_audio_bytes as u32,
            max_state_bytes: game.max_state_bytes as u32,
            rng_seed: 1,
            max_call_depth: defaults.max_call_depth as u32,
            max_activation_slots: defaults.max_activation_slots as u32,
        };
        unsafe { config.write(value) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_completion_create(
    module: *const u8,
    module_len: usize,
    max_pending: u32,
    max_reserved_bytes: usize,
    max_calls_per_lifecycle: u32,
    output: *mut *mut TinyArcadeCompletionV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null completion channel output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let module = unsafe { input_string(module, module_len, 128, "invalid native module")? };
        let max_pending = u16::try_from(max_pending).map_err(|_| {
            FfiError::new(STATUS_INVALID_ARGUMENT, "invalid completion request limit")
        })?;
        if max_pending == 0
            || max_reserved_bytes == 0
            || max_calls_per_lifecycle == 0
            || max_calls_per_lifecycle > MAX_NATIVE_CALLS_PER_LIFECYCLE
        {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid completion channel limits",
            ));
        }
        let queue = {
            let mut allocator = COMPLETION_DOMAINS.lock().map_err(|_| {
                FfiError::new(STATUS_PANIC, "completion domain allocator unavailable")
            })?;
            HostCompletionQueue::with_domain_allocator(
                max_pending,
                max_reserved_bytes,
                &mut allocator,
            )
            .map_err(completion_error)?
        };
        let queue = Rc::new(RefCell::new(queue));
        let mut validation_registry = NativeModuleRegistry::new();
        validation_registry
            .attach_completion_queue(&module, queue.clone(), max_calls_per_lifecycle)
            .map_err(wasm_error)?;
        let handle = Box::new(TinyArcadeCompletionV1 {
            owner: thread::current().id(),
            module,
            max_calls_per_lifecycle,
            queue,
            bound: false,
        });
        unsafe { output.write(Box::into_raw(handle)) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_completion_close(
    completion: *mut TinyArcadeCompletionV1,
) -> i32 {
    boundary(|| {
        let completion_ref = unsafe { completion_mut(completion)? };
        if completion_ref.bound {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "completion channel is bound to a runtime",
            ));
        }
        drop(unsafe { Box::from_raw(completion) });
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_completion_begin(
    completion: *mut TinyArcadeCompletionV1,
    max_payload_bytes: usize,
    ticket: *mut i32,
) -> i32 {
    boundary(|| {
        if ticket.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null completion ticket",
            ));
        }
        unsafe { ticket.write(0) };
        let completion = unsafe { completion_mut(completion)? };
        if !completion.bound {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "completion channel is not bound",
            ));
        }
        let handle = completion
            .queue
            .try_borrow_mut()
            .map_err(|_| FfiError::new(STATUS_INVALID_ARGUMENT, "completion reentrancy"))?
            .begin(max_payload_bytes)
            .map_err(completion_error)?;
        unsafe { ticket.write(handle.as_i32()) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_completion_complete(
    completion: *mut TinyArcadeCompletionV1,
    ticket: i32,
    native_status: i32,
    payload: *const u8,
    payload_len: usize,
) -> i32 {
    boundary(|| {
        let completion = unsafe { completion_mut(completion)? };
        if !completion.bound {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "completion channel is not bound",
            ));
        }
        let handle = GuestResourceHandle::from_i32(ticket).ok_or(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid completion ticket",
        ))?;
        if payload_len
            > completion
                .queue
                .try_borrow()
                .map_err(|_| FfiError::new(STATUS_INVALID_ARGUMENT, "completion reentrancy"))?
                .max_reserved_bytes()
        {
            return Err(FfiError::new(
                STATUS_BUFFER_TOO_SMALL,
                "completion payload exceeds channel budget",
            ));
        }
        let input = if payload_len == 0 {
            &[][..]
        } else {
            unsafe { input_bytes(payload, payload_len, "invalid completion payload")? }
        };
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(input.len())
            .map_err(|_| FfiError::new(STATUS_STORAGE, "completion payload allocation failed"))?;
        owned.extend_from_slice(input);
        completion
            .queue
            .try_borrow_mut()
            .map_err(|_| FfiError::new(STATUS_INVALID_ARGUMENT, "completion reentrancy"))?
            .try_complete(handle, native_status, owned)
            .map_err(|rejection| completion_error(rejection.error))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_completion_cancel(
    completion: *mut TinyArcadeCompletionV1,
    ticket: i32,
) -> i32 {
    boundary(|| {
        let completion = unsafe { completion_mut(completion)? };
        if !completion.bound {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "completion channel is not bound",
            ));
        }
        let handle = GuestResourceHandle::from_i32(ticket).ok_or(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid completion ticket",
        ))?;
        completion
            .queue
            .try_borrow_mut()
            .map_err(|_| FfiError::new(STATUS_INVALID_ARGUMENT, "completion reentrancy"))?
            .cancel(handle)
            .map_err(completion_error)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_trust_store_create(
    output: *mut *mut TinyArcadeTrustStoreV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null trust store output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let trust = Box::new(TinyArcadeTrustStoreV1 {
            owner: thread::current().id(),
            store: CartridgeTrustStore::new(),
        });
        unsafe { output.write(Box::into_raw(trust)) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_trust_store_close(
    trust: *mut TinyArcadeTrustStoreV1,
) -> i32 {
    boundary(|| {
        unsafe { trust_mut(trust)? };
        drop(unsafe { Box::from_raw(trust) });
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_trust_store_add_key(
    trust: *mut TinyArcadeTrustStoreV1,
    key_id: *const u8,
    key_id_len: usize,
    public_key: *const u8,
    public_key_len: usize,
) -> i32 {
    boundary(|| {
        let trust = unsafe { trust_mut(trust)? };
        let key_id = unsafe { input_string(key_id, key_id_len, 64, "invalid trust key id")? };
        if public_key_len != 32 {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid trust public key",
            ));
        }
        let public_key =
            unsafe { input_bytes(public_key, public_key_len, "invalid trust public key")? };
        trust
            .store
            .add_key(&key_id, public_key)
            .map_err(|error| FfiError::new(STATUS_TRUST, error.message()))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_trust_store_revoke_key(
    trust: *mut TinyArcadeTrustStoreV1,
    key_id: *const u8,
    key_id_len: usize,
) -> i32 {
    boundary(|| {
        let trust = unsafe { trust_mut(trust)? };
        let key_id = unsafe { input_string(key_id, key_id_len, 64, "invalid trust key id")? };
        trust
            .store
            .revoke_key(&key_id)
            .map_err(|error| FfiError::new(STATUS_TRUST, error.message()))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_trust_store_revoke_content(
    trust: *mut TinyArcadeTrustStoreV1,
    sha256: *const u8,
    sha256_len: usize,
) -> i32 {
    boundary(|| {
        let trust = unsafe { trust_mut(trust)? };
        if sha256_len != 32 {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid revoked content hash",
            ));
        }
        let bytes = unsafe { input_bytes(sha256, sha256_len, "invalid revoked content hash")? };
        let mut fixed = [0; 32];
        fixed.copy_from_slice(bytes);
        trust.store.revoke_content(fixed);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_cache_create(
    directory: *const u8,
    directory_len: usize,
    max_wasm_bytes: u64,
    output: *mut *mut TinyArcadeCartridgeCacheV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null cartridge cache output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let directory = unsafe {
            input_string(
                directory,
                directory_len,
                4_096,
                "invalid cartridge cache directory",
            )?
        };
        if max_wasm_bytes == 0 {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid cartridge cache limit",
            ));
        }
        let max_wasm_bytes = usize::try_from(max_wasm_bytes)
            .map_err(|_| FfiError::new(STATUS_INVALID_ARGUMENT, "invalid cartridge cache limit"))?;
        let cache = CartridgeCache::open(directory, max_wasm_bytes).map_err(cache_error)?;
        let handle = Box::new(TinyArcadeCartridgeCacheV1 {
            owner: thread::current().id(),
            cache,
            wasm: Vec::new(),
        });
        unsafe { output.write(Box::into_raw(handle)) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_cache_close(cache: *mut TinyArcadeCartridgeCacheV1) -> i32 {
    boundary(|| {
        unsafe { cache_mut(cache)? };
        drop(unsafe { Box::from_raw(cache) });
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_cache_activate(
    cache: *mut TinyArcadeCartridgeCacheV1,
    entry: *const TinyArcadeCatalogEntryV1,
    wasm: *const u8,
    wasm_len: usize,
    trust: *mut TinyArcadeTrustStoreV1,
) -> i32 {
    cache_boundary(cache, |cache| {
        let entry = unsafe { catalog_entry(entry)? };
        let wasm = unsafe { input_bytes(wasm, wasm_len, "invalid cartridge cache input")? };
        let trust = unsafe { trust_mut(trust)? };
        cache
            .cache
            .activate(&entry, wasm, &trust.store)
            .map_err(cache_error)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_cache_load_active(
    cache: *mut TinyArcadeCartridgeCacheV1,
    entry: *const TinyArcadeCatalogEntryV1,
    trust: *mut TinyArcadeTrustStoreV1,
) -> i32 {
    cache_boundary(cache, |cache| {
        let entry = unsafe { catalog_entry(entry)? };
        let trust = unsafe { trust_mut(trust)? };
        cache.wasm = cache
            .cache
            .load_active(&entry.game_id, &entry, &trust.store)
            .map_err(cache_error)?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_cache_rollback(
    cache: *mut TinyArcadeCartridgeCacheV1,
    previous_entry: *const TinyArcadeCatalogEntryV1,
    trust: *mut TinyArcadeTrustStoreV1,
) -> i32 {
    cache_boundary(cache, |cache| {
        let entry = unsafe { catalog_entry(previous_entry)? };
        let trust = unsafe { trust_mut(trust)? };
        cache.wasm = cache
            .cache
            .rollback(&entry.game_id, &entry, &trust.store)
            .map_err(cache_error)?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_cache_copy_wasm(
    cache: *mut TinyArcadeCartridgeCacheV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let cache = unsafe { cache_mut(cache)? };
        if cache.wasm.is_empty() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "no completed cartridge cache load",
            ));
        }
        unsafe { copy_bytes(&cache.wasm, output, capacity, output_len) }
    })
}

unsafe fn open_runtime(
    wasm: *const u8,
    wasm_len: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
    completion_bindings: CompletionBindings,
    create: impl FnOnce(&[u8], Limits, GameLimits, u32) -> Result<GameRuntime, FfiError>,
) -> Result<(), FfiError> {
    if output.is_null() {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "null runtime output",
        ));
    }
    unsafe { output.write(ptr::null_mut()) };
    let config = unsafe { read_runtime_config(config)? };
    if wasm.is_null() || wasm_len == 0 {
        return Err(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "invalid runtime configuration",
        ));
    }
    let bytes = unsafe { slice::from_raw_parts(wasm, wasm_len) };
    let runtime = create(bytes, config.vm_limits, config.game_limits, config.rng_seed)?;
    let handle = Box::new(TinyArcadeRuntimeV1 {
        owner: thread::current().id(),
        runtime,
        frame: None,
        snapshot: Vec::new(),
        replay_recorder: None,
        replay: Vec::new(),
        _completion_bindings: completion_bindings,
    });
    unsafe { output.write(Box::into_raw(handle)) };
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open(
    wasm: *const u8,
    wasm_len: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| unsafe {
        open_runtime(
            wasm,
            wasm_len,
            config,
            output,
            CompletionBindings {
                channels: Vec::new(),
            },
            |bytes, vm, game, seed| {
                GameRuntime::from_bytes(bytes, vm, game, seed).map_err(wasm_error)
            },
        )
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open_with_native_modules(
    wasm: *const u8,
    wasm_len: usize,
    functions: *const TinyArcadeNativeFunctionV1,
    function_count: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null runtime output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let registry = unsafe { native_registry(functions, function_count)? };
        unsafe {
            open_runtime(
                wasm,
                wasm_len,
                config,
                output,
                CompletionBindings {
                    channels: Vec::new(),
                },
                |bytes, vm, game, seed| {
                    GameRuntime::from_bytes_with_registry(bytes, vm, game, seed, registry)
                        .map_err(wasm_error)
                },
            )
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open_with_native_completions(
    wasm: *const u8,
    wasm_len: usize,
    functions: *const TinyArcadeNativeFunctionV1,
    function_count: usize,
    completions: *const *mut TinyArcadeCompletionV1,
    completion_count: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null runtime output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let mut registry = unsafe { native_registry(functions, function_count)? };
        let bindings =
            unsafe { bind_completion_channels(&mut registry, completions, completion_count)? };
        unsafe {
            open_runtime(
                wasm,
                wasm_len,
                config,
                output,
                bindings,
                |bytes, vm, game, seed| {
                    GameRuntime::from_bytes_with_registry(bytes, vm, game, seed, registry)
                        .map_err(wasm_error)
                },
            )
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open_private(
    wasm: *const u8,
    wasm_len: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| unsafe {
        open_runtime(
            wasm,
            wasm_len,
            config,
            output,
            CompletionBindings {
                channels: Vec::new(),
            },
            |bytes, vm, game, seed| {
                GameRuntime::from_private_bytes(bytes, vm, game, seed).map_err(wasm_error)
            },
        )
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open_reviewed(
    wasm: *const u8,
    wasm_len: usize,
    entry: *const TinyArcadeCatalogEntryV1,
    trust: *mut TinyArcadeTrustStoreV1,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null runtime output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let entry = unsafe { catalog_entry(entry)? };
        let trust = unsafe { trust_mut(trust)? };
        unsafe {
            open_runtime(
                wasm,
                wasm_len,
                config,
                output,
                CompletionBindings {
                    channels: Vec::new(),
                },
                |bytes, vm, game, seed| {
                    GameRuntime::from_reviewed_bytes(bytes, &entry, &trust.store, vm, game, seed)
                        .map_err(|error| FfiError::new(STATUS_TRUST, error.message()))
                },
            )
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open_reviewed_with_native_modules(
    wasm: *const u8,
    wasm_len: usize,
    entry: *const TinyArcadeCatalogEntryV1,
    trust: *mut TinyArcadeTrustStoreV1,
    functions: *const TinyArcadeNativeFunctionV1,
    function_count: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null runtime output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let entry = unsafe { catalog_entry(entry)? };
        let trust = unsafe { trust_mut(trust)? };
        let registry = unsafe { native_registry(functions, function_count)? };
        unsafe {
            open_runtime(
                wasm,
                wasm_len,
                config,
                output,
                CompletionBindings {
                    channels: Vec::new(),
                },
                |bytes, vm, game, seed| {
                    GameRuntime::from_reviewed_bytes_with_registry(
                        bytes,
                        &entry,
                        &trust.store,
                        vm,
                        game,
                        seed,
                        registry,
                    )
                    .map_err(|error| FfiError::new(STATUS_TRUST, error.message()))
                },
            )
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_open_reviewed_with_native_completions(
    wasm: *const u8,
    wasm_len: usize,
    entry: *const TinyArcadeCatalogEntryV1,
    trust: *mut TinyArcadeTrustStoreV1,
    functions: *const TinyArcadeNativeFunctionV1,
    function_count: usize,
    completions: *const *mut TinyArcadeCompletionV1,
    completion_count: usize,
    config: *const TinyArcadeConfigV1,
    output: *mut *mut TinyArcadeRuntimeV1,
) -> i32 {
    boundary(|| {
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null runtime output",
            ));
        }
        unsafe { output.write(ptr::null_mut()) };
        let entry = unsafe { catalog_entry(entry)? };
        let trust = unsafe { trust_mut(trust)? };
        let mut registry = unsafe { native_registry(functions, function_count)? };
        let bindings =
            unsafe { bind_completion_channels(&mut registry, completions, completion_count)? };
        unsafe {
            open_runtime(
                wasm,
                wasm_len,
                config,
                output,
                bindings,
                |bytes, vm, game, seed| {
                    GameRuntime::from_reviewed_bytes_with_registry(
                        bytes,
                        &entry,
                        &trust.store,
                        vm,
                        game,
                        seed,
                        registry,
                    )
                    .map_err(|error| FfiError::new(STATUS_TRUST, error.message()))
                },
            )
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_close(runtime: *mut TinyArcadeRuntimeV1) -> i32 {
    boundary(|| {
        unsafe { runtime_mut(runtime)? };
        drop(unsafe { Box::from_raw(runtime) });
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_tick(
    runtime: *mut TinyArcadeRuntimeV1,
    buttons: u32,
    clock_ms: u32,
) -> i32 {
    runtime_boundary(runtime, |runtime| {
        let mut frame = runtime.frame.take().unwrap_or_default();
        let input = GameInput { buttons, clock_ms };
        if let Some(recorder) = runtime.replay_recorder.as_mut() {
            recorder
                .record_tick_into(&mut runtime.runtime, input, &mut frame)
                .map_err(wasm_error)?;
        } else {
            runtime
                .runtime
                .tick_into(input, &mut frame)
                .map_err(wasm_error)?;
        }
        runtime.frame = Some(frame);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_replay_begin(runtime: *mut TinyArcadeRuntimeV1) -> i32 {
    runtime_boundary(runtime, |runtime| {
        if runtime.replay_recorder.is_some() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "replay recording already active",
            ));
        }
        runtime.replay.clear();
        runtime.snapshot.clear();
        runtime.replay_recorder =
            Some(ReplayRecorderV1::start_runtime(&mut runtime.runtime).map_err(wasm_error)?);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_replay_cancel(runtime: *mut TinyArcadeRuntimeV1) -> i32 {
    runtime_boundary(runtime, |runtime| {
        runtime.replay_recorder = None;
        runtime.replay.clear();
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_replay_finish(runtime: *mut TinyArcadeRuntimeV1) -> i32 {
    runtime_boundary(runtime, |runtime| {
        let recorder = runtime.replay_recorder.as_ref().ok_or(FfiError::new(
            STATUS_INVALID_ARGUMENT,
            "no active replay recording",
        ))?;
        let replay = recorder.finish().map_err(wasm_error)?;
        runtime.replay = replay;
        runtime.replay_recorder = None;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_replay(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if runtime.replay.is_empty() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "no completed replay",
            ));
        }
        unsafe { copy_bytes(&runtime.replay, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_replay_check(
    runtime: *mut TinyArcadeRuntimeV1,
    replay: *const u8,
    replay_len: usize,
    verified_steps: *mut u32,
) -> i32 {
    runtime_boundary(runtime, |runtime| {
        if verified_steps.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null replay step output",
            ));
        }
        unsafe { verified_steps.write(0) };
        if runtime.replay_recorder.is_some() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "cannot verify while replay recording is active",
            ));
        }
        let replay = unsafe { input_bytes(replay, replay_len, "invalid replay input")? };
        let trace = ReplayTraceV1::decode(replay).map_err(wasm_error)?;
        let steps = u32::try_from(trace.steps.len())
            .map_err(|_| FfiError::new(STATUS_DECODE, "replay step limit"))?;
        runtime.frame = None;
        runtime.snapshot.clear();
        let final_index = trace.steps.len().checked_sub(1);
        let mut last_frame = None;
        trace
            .replay_loaded(&mut runtime.runtime, |index, frame| {
                if Some(index) == final_index {
                    last_frame = Some(GameFrame {
                        render: frame.render.clone(),
                        audio: frame.audio.clone(),
                    });
                }
                Ok(())
            })
            .map_err(wasm_error)?;
        runtime.frame = last_frame;
        unsafe { verified_steps.write(steps) };
        Ok(())
    })
}

unsafe fn copy_frame(
    runtime: *mut TinyArcadeRuntimeV1,
    render: bool,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        let frame = runtime
            .frame
            .as_ref()
            .ok_or(FfiError::new(STATUS_INVALID_ARGUMENT, "no completed frame"))?;
        let bytes = if render { &frame.render } else { &frame.audio };
        unsafe { copy_bytes(bytes, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_render(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_frame(runtime, true, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_audio(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_frame(runtime, false, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_suspend(runtime: *mut TinyArcadeRuntimeV1) -> i32 {
    runtime_boundary(runtime, |runtime| {
        if runtime.replay_recorder.is_some() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "cannot suspend while replay recording is active",
            ));
        }
        runtime.snapshot.clear();
        runtime.snapshot = runtime.runtime.suspend().map_err(wasm_error)?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_snapshot(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if runtime.snapshot.is_empty() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "no completed snapshot",
            ));
        }
        unsafe { copy_bytes(&runtime.snapshot, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_resume(
    runtime: *mut TinyArcadeRuntimeV1,
    snapshot: *const u8,
    snapshot_len: usize,
) -> i32 {
    runtime_boundary(runtime, |runtime| {
        if runtime.replay_recorder.is_some() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "cannot resume while replay recording is active",
            ));
        }
        if snapshot.is_null() || snapshot_len == 0 {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "invalid snapshot input",
            ));
        }
        let snapshot = unsafe { slice::from_raw_parts(snapshot, snapshot_len) };
        runtime.frame = None;
        runtime.runtime.resume(snapshot).map_err(wasm_error)?;
        Ok(())
    })
}

unsafe fn copy_manifest_string(
    runtime: *mut TinyArcadeRuntimeV1,
    game_id: bool,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        let manifest = runtime.runtime.manifest();
        let value = if game_id {
            manifest.game_id.as_bytes()
        } else {
            manifest.game_version.as_bytes()
        };
        unsafe { copy_bytes(value, output, capacity, output_len) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_game_id(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_manifest_string(runtime, true, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_copy_game_version(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    unsafe { copy_manifest_string(runtime, false, output, capacity, output_len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_is_failed(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut i32,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null failed-state output",
            ));
        }
        unsafe { output.write(i32::from(runtime.runtime.is_failed())) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_origin(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut u32,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null cartridge origin output",
            ));
        }
        unsafe { output.write(runtime.runtime.origin() as u32) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_last_execution_stats(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut TinyArcadeExecutionStatsV1,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null execution-stats output",
            ));
        }
        let ExecutionStats {
            lifecycle,
            wasm_steps,
            peak_call_depth: _,
            peak_activation_slots: _,
            memory_pages,
            table_elements,
            native_calls,
            render_bytes,
            audio_bytes,
            state_bytes,
        } = runtime.runtime.last_execution_stats();
        let count = |value: usize| {
            u32::try_from(value)
                .map_err(|_| FfiError::new(STATUS_DECODE, "execution stats overflow"))
        };
        unsafe {
            output.write(TinyArcadeExecutionStatsV1 {
                struct_size: size_of::<TinyArcadeExecutionStatsV1>() as u32,
                lifecycle: lifecycle as u32,
                wasm_steps,
                memory_pages: count(memory_pages)?,
                table_elements: count(table_elements)?,
                native_calls,
                render_bytes: count(render_bytes)?,
                audio_bytes: count(audio_bytes)?,
                state_bytes: count(state_bytes)?,
            })
        };
        Ok(())
    })
}

/// Extended deterministic stats added in ABI v1.9. This is a separate output
/// type/function so an older 40-byte v1 caller can never be overwritten.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_last_execution_stats_v2(
    runtime: *mut TinyArcadeRuntimeV1,
    output: *mut TinyArcadeExecutionStatsV2,
) -> i32 {
    boundary(|| {
        let runtime = unsafe { runtime_mut(runtime)? };
        if output.is_null() {
            return Err(FfiError::new(
                STATUS_INVALID_ARGUMENT,
                "null execution-stats output",
            ));
        }
        let stats = runtime.runtime.last_execution_stats();
        let count = |value: usize| {
            u32::try_from(value)
                .map_err(|_| FfiError::new(STATUS_DECODE, "execution stats overflow"))
        };
        unsafe {
            output.write(TinyArcadeExecutionStatsV2 {
                struct_size: size_of::<TinyArcadeExecutionStatsV2>() as u32,
                lifecycle: stats.lifecycle as u32,
                wasm_steps: stats.wasm_steps,
                peak_call_depth: count(stats.peak_call_depth)?,
                peak_activation_slots: count(stats.peak_activation_slots)?,
                memory_pages: count(stats.memory_pages)?,
                table_elements: count(stats.table_elements)?,
                native_calls: stats.native_calls,
                render_bytes: count(stats.render_bytes)?,
                audio_bytes: count(stats.audio_bytes)?,
                state_bytes: count(stats.state_bytes)?,
            })
        };
        Ok(())
    })
}

/// Copy the last error for the calling thread. This call intentionally does
/// not clear the error before reading it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tinyarcade_v1_last_error(
    output: *mut u8,
    capacity: usize,
    output_len: *mut usize,
) -> i32 {
    let message = LAST_ERROR.with(Cell::get);
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        copy_bytes(message.as_bytes(), output, capacity, output_len)
    })) {
        Ok(Ok(())) => STATUS_OK,
        Ok(Err(error)) => error.status,
        Err(_) => STATUS_PANIC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CartridgeManifest, CartridgeOrigin, cartridge_sha256};
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::mem::MaybeUninit;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::OnceLock;

    fn leb(output: &mut Vec<u8>, mut value: usize) {
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

    fn name(output: &mut Vec<u8>, value: &str) {
        leb(output, value.len());
        output.extend_from_slice(value.as_bytes());
    }

    fn section(module: &mut Vec<u8>, id: u8, payload: &[u8]) {
        module.push(id);
        leb(module, payload.len());
        module.extend_from_slice(payload);
    }

    fn body(code: &[u8]) -> Vec<u8> {
        let mut body = vec![0];
        body.extend_from_slice(code);
        body
    }

    fn cartridge() -> Vec<u8> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        let mut manifest = Vec::new();
        name(&mut manifest, "tinyarcade.manifest.v1");
        manifest.extend_from_slice(b"TAM1");
        manifest.extend_from_slice(&1u32.to_le_bytes());
        manifest.extend_from_slice(&1u32.to_le_bytes());
        for value in ["c.test", "1.0.0"] {
            manifest.extend_from_slice(&(value.len() as u16).to_le_bytes());
            manifest.extend_from_slice(value.as_bytes());
        }
        manifest.extend_from_slice(&0u16.to_le_bytes());
        section(&mut module, 0, &manifest);
        section(
            &mut module,
            1,
            &[
                0x02, 0x60, 0x00, 0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f,
            ],
        );
        let imports = ["save_state", "load_state", "submit_render"];
        let mut import_section = vec![imports.len() as u8];
        for field in imports {
            name(&mut import_section, "tinyarcade:core/v1");
            name(&mut import_section, field);
            import_section.extend_from_slice(&[0x00, 0x01]);
        }
        section(&mut module, 2, &import_section);
        section(&mut module, 3, &[0x05, 0, 0, 0, 0, 0]);
        section(&mut module, 5, &[0x01, 0x00, 0x01]);
        let mut exports = vec![0x05];
        for (field, index) in [
            ("game_abi_version", 3usize),
            ("game_init", 4),
            ("game_tick", 5),
            ("game_suspend", 6),
            ("game_resume", 7),
        ] {
            name(&mut exports, field);
            exports.push(0);
            leb(&mut exports, index);
        }
        section(&mut module, 7, &exports);
        let functions = [
            body(&[0x41, 0x01, 0x0b]),
            body(&[0x41, 0x00, 0x0b]),
            body(&[0x41, 0x00, 0x41, 0x01, 0x10, 0x02, 0x1a, 0x41, 0x00, 0x0b]),
            body(&[0x41, 0x00, 0x41, 0x01, 0x10, 0x00, 0x1a, 0x41, 0x00, 0x0b]),
            body(&[0x41, 0x00, 0x41, 0x01, 0x10, 0x01, 0x1a, 0x41, 0x00, 0x0b]),
        ];
        let mut code = vec![0x05];
        for function in &functions {
            leb(&mut code, function.len());
            code.extend_from_slice(function);
        }
        section(&mut module, 10, &code);
        section(&mut module, 11, &[0x01, 0x00, 0x41, 0x00, 0x0b, 0x01, 0x09]);
        module
    }

    fn native_cartridge() -> Vec<u8> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        let capability = "fan:physics/v1";
        let mut manifest = Vec::new();
        name(&mut manifest, "tinyarcade.manifest.v1");
        manifest.extend_from_slice(b"TAM1");
        manifest.extend_from_slice(&1u32.to_le_bytes());
        manifest.extend_from_slice(&1u32.to_le_bytes());
        for value in ["c.native", "1.0.0"] {
            manifest.extend_from_slice(&(value.len() as u16).to_le_bytes());
            manifest.extend_from_slice(value.as_bytes());
        }
        manifest.extend_from_slice(&1u16.to_le_bytes());
        manifest.extend_from_slice(&(capability.len() as u16).to_le_bytes());
        manifest.extend_from_slice(capability.as_bytes());
        section(&mut module, 0, &manifest);
        section(
            &mut module,
            1,
            &[
                0x02, 0x60, 0x00, 0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f,
            ],
        );
        let mut imports = vec![0x02];
        for (namespace, field) in [
            (capability, "step_world"),
            ("tinyarcade:core/v1", "submit_render"),
        ] {
            name(&mut imports, namespace);
            name(&mut imports, field);
            imports.extend_from_slice(&[0x00, 0x01]);
        }
        section(&mut module, 2, &imports);
        section(&mut module, 3, &[0x05, 0, 0, 0, 0, 0]);
        section(&mut module, 5, &[0x01, 0x00, 0x01]);
        let mut exports = vec![0x05];
        for (field, index) in [
            ("game_abi_version", 2usize),
            ("game_init", 3),
            ("game_tick", 4),
            ("game_suspend", 5),
            ("game_resume", 6),
        ] {
            name(&mut exports, field);
            exports.push(0);
            leb(&mut exports, index);
        }
        section(&mut module, 7, &exports);
        let functions = [
            body(&[0x41, 0x01, 0x0b]),
            body(&[0x41, 0x00, 0x0b]),
            body(&[
                0x41, 0x28, 0x41, 0x02, 0x10, 0x00, 0x1a, 0x41, 0x00, 0x41, 0x28, 0x41, 0x02, 0x10,
                0x00, 0x36, 0x02, 0x00, 0x41, 0x00, 0x41, 0x08, 0x10, 0x01, 0x1a, 0x41, 0x00, 0x0b,
            ]),
            body(&[0x41, 0x00, 0x0b]),
            body(&[0x41, 0x00, 0x0b]),
        ];
        let mut code = vec![0x05];
        for function in &functions {
            leb(&mut code, function.len());
            code.extend_from_slice(function);
        }
        section(&mut module, 10, &code);
        module
    }

    struct NativeProbe {
        calls: Cell<u32>,
        fail: Cell<bool>,
    }

    unsafe extern "C" fn native_step(
        context: *mut c_void,
        params: *const i32,
        n_params: usize,
        results: *mut i32,
        n_results: usize,
        memory: *mut u8,
        memory_len: usize,
    ) -> i32 {
        let probe = unsafe { &mut *context.cast::<NativeProbe>() };
        probe.calls.set(probe.calls.get() + 1);
        if probe.fail.get() {
            return 41;
        }
        assert_eq!(unsafe { slice::from_raw_parts(params, n_params) }, [40, 2]);
        assert_eq!(n_results, 1);
        assert!(memory_len >= 8);
        unsafe {
            results.write(42);
            memory.add(4).write(9);
        }
        STATUS_OK
    }

    struct CompletionProbe {
        channel: *mut TinyArcadeCompletionV1,
        ticket: Cell<i32>,
    }

    unsafe extern "C" fn native_completion_start(
        context: *mut c_void,
        _params: *const i32,
        n_params: usize,
        results: *mut i32,
        n_results: usize,
        _memory: *mut u8,
        _memory_len: usize,
    ) -> i32 {
        assert_eq!(n_params, 0);
        assert_eq!(n_results, 1);
        let probe = unsafe { &*context.cast::<CompletionProbe>() };
        let mut ticket = 0;
        let status = unsafe { tinyarcade_v1_completion_begin(probe.channel, 4, &mut ticket) };
        if status == STATUS_OK {
            probe.ticket.set(ticket);
            unsafe { results.write(ticket) };
        }
        status
    }

    struct ReentrantProbe {
        runtime: Cell<*mut TinyArcadeRuntimeV1>,
        other_runtime: Cell<*mut TinyArcadeRuntimeV1>,
        calls: Cell<u32>,
        status: Cell<i32>,
        other_status: Cell<i32>,
    }

    unsafe extern "C" fn reentrant_native_step(
        context: *mut c_void,
        params: *const i32,
        n_params: usize,
        results: *mut i32,
        n_results: usize,
        memory: *mut u8,
        memory_len: usize,
    ) -> i32 {
        let probe = unsafe { &*context.cast::<ReentrantProbe>() };
        probe.calls.set(probe.calls.get() + 1);
        let mut failed = -1;
        probe
            .status
            .set(unsafe { tinyarcade_v1_is_failed(probe.runtime.get(), &mut failed) });
        assert_eq!(failed, -1, "reentrant call must not touch its output");
        probe
            .other_status
            .set(unsafe { tinyarcade_v1_is_failed(probe.other_runtime.get(), &mut failed) });
        assert_eq!(failed, -1, "reentrant call must not touch its output");
        assert_eq!(unsafe { slice::from_raw_parts(params, n_params) }, [40, 2]);
        assert_eq!(n_results, 1);
        assert!(memory_len >= 8);
        unsafe {
            results.write(42);
            memory.add(4).write(9);
        }
        STATUS_OK
    }

    unsafe fn config() -> TinyArcadeConfigV1 {
        let mut config = MaybeUninit::uninit();
        assert_eq!(
            unsafe { tinyarcade_v1_default_config(config.as_mut_ptr()) },
            STATUS_OK
        );
        unsafe { config.assume_init() }
    }

    unsafe fn open(wasm: &[u8]) -> *mut TinyArcadeRuntimeV1 {
        let config = unsafe { config() };
        let mut runtime = ptr::null_mut();
        assert_eq!(
            unsafe { tinyarcade_v1_open(wasm.as_ptr(), wasm.len(), &config, &mut runtime) },
            STATUS_OK
        );
        assert!(!runtime.is_null());
        runtime
    }

    fn replay_cartridge() -> Vec<u8> {
        static CARTRIDGE: OnceLock<Vec<u8>> = OnceLock::new();
        CARTRIDGE
            .get_or_init(|| {
                let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let output =
                    crate_dir.join("../../target/tinyvm-ios-c-replay-test/depth-well-0.1.0.wasm");
                let status = Command::new(crate_dir.join("build-depth-well-cartridge.sh"))
                    .arg(&output)
                    .status()
                    .expect("run replay cartridge builder");
                assert!(status.success(), "replay cartridge build failed");
                std::fs::read(output).expect("read replay cartridge")
            })
            .clone()
    }

    unsafe fn open_replay_runtime(wasm: &[u8]) -> *mut TinyArcadeRuntimeV1 {
        let mut config = unsafe { config() };
        config.max_memory_pages = 17;
        config.max_steps = 100_000;
        config.max_render_bytes = 4 * 1024;
        config.max_audio_bytes = 64;
        config.max_state_bytes = 512;
        config.rng_seed = 0x5eed_1234;
        let mut runtime = ptr::null_mut();
        assert_eq!(
            unsafe { tinyarcade_v1_open_private(wasm.as_ptr(), wasm.len(), &config, &mut runtime) },
            STATUS_OK
        );
        assert!(!runtime.is_null());
        runtime
    }

    #[test]
    fn c_replay_owner_records_copies_verifies_and_binds_loaded_bytes() {
        let wasm = replay_cartridge();
        let recorder = unsafe { open_replay_runtime(&wasm) };
        assert_eq!(unsafe { tinyarcade_v1_replay_begin(recorder) }, STATUS_OK);
        let address = recorder as usize;
        assert_eq!(
            std::thread::spawn(move || unsafe {
                tinyarcade_v1_replay_cancel(address as *mut TinyArcadeRuntimeV1)
            })
            .join()
            .expect("wrong-thread replay probe"),
            STATUS_WRONG_THREAD
        );
        assert_eq!(
            unsafe { tinyarcade_v1_replay_begin(recorder) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { tinyarcade_v1_suspend(recorder) },
            STATUS_INVALID_ARGUMENT
        );
        for (buttons, clock) in [(0, 0), (1, 16), (1 << 4, 32), (1 << 7, 48)] {
            assert_eq!(
                unsafe { tinyarcade_v1_tick(recorder, buttons, clock) },
                STATUS_OK
            );
        }
        assert_eq!(unsafe { tinyarcade_v1_replay_finish(recorder) }, STATUS_OK);
        let mut replay_len = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_copy_replay(recorder, ptr::null_mut(), 0, &mut replay_len) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(replay_len, 749);
        let mut replay = vec![0; replay_len];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_replay(
                    recorder,
                    replay.as_mut_ptr(),
                    replay.len(),
                    &mut replay_len,
                )
            },
            STATUS_OK
        );
        assert_eq!(unsafe { tinyarcade_v1_close(recorder) }, STATUS_OK);

        let verifier = unsafe { open_replay_runtime(&wasm) };
        let mut steps = u32::MAX;
        assert_eq!(
            unsafe {
                tinyarcade_v1_replay_check(verifier, replay.as_ptr(), replay.len(), &mut steps)
            },
            STATUS_OK
        );
        assert_eq!(steps, 4);
        assert_eq!(unsafe { tinyarcade_v1_close(verifier) }, STATUS_OK);

        let oversized_runtime = unsafe { open_replay_runtime(&wasm) };
        let oversized = vec![0; crate::MAX_REPLAY_BYTES + 1];
        steps = u32::MAX;
        assert_eq!(
            unsafe {
                tinyarcade_v1_replay_check(
                    oversized_runtime,
                    oversized.as_ptr(),
                    oversized.len(),
                    &mut steps,
                )
            },
            STATUS_DECODE
        );
        assert_eq!(steps, 0);
        assert_eq!(unsafe { tinyarcade_v1_close(oversized_runtime) }, STATUS_OK);

        let mut changed = wasm.clone();
        changed.extend_from_slice(&[0, 1, 0]);
        let changed_runtime = unsafe { open_replay_runtime(&changed) };
        steps = u32::MAX;
        assert_ne!(
            unsafe {
                tinyarcade_v1_replay_check(
                    changed_runtime,
                    replay.as_ptr(),
                    replay.len(),
                    &mut steps,
                )
            },
            STATUS_OK
        );
        assert_eq!(steps, 0);
        assert_eq!(
            unsafe { tinyarcade_v1_replay_cancel(changed_runtime) },
            STATUS_OK
        );
        assert_eq!(unsafe { tinyarcade_v1_close(changed_runtime) }, STATUS_OK);
    }

    #[test]
    fn c_handle_drives_frame_snapshot_resume_and_thread_owner() {
        let wasm = cartridge();
        let runtime = unsafe { open(&wasm) };
        assert_eq!(tinyarcade_v1_abi_version(), (1 << 16) | 12);
        let mut origin = u32::MAX;
        assert_eq!(
            unsafe { tinyarcade_v1_origin(runtime, &mut origin) },
            STATUS_OK
        );
        assert_eq!(origin, CartridgeOrigin::Bundled as u32);
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 16) }, STATUS_OK);
        let mut stats = MaybeUninit::<TinyArcadeExecutionStatsV1>::uninit();
        assert_eq!(
            unsafe { tinyarcade_v1_last_execution_stats(runtime, stats.as_mut_ptr()) },
            STATUS_OK
        );
        let stats = unsafe { stats.assume_init() };
        assert_eq!(
            stats.struct_size,
            size_of::<TinyArcadeExecutionStatsV1>() as u32
        );
        assert_eq!(stats.lifecycle, crate::GameLifecycle::Tick as u32);
        assert!(stats.wasm_steps > 0);
        assert_eq!(stats.memory_pages, 1);
        assert_eq!(stats.table_elements, 0);
        assert_eq!(stats.native_calls, 0);
        assert_eq!((stats.render_bytes, stats.audio_bytes), (1, 0));
        assert_eq!(stats.state_bytes, 0);
        let mut stats_v2 = MaybeUninit::<TinyArcadeExecutionStatsV2>::uninit();
        assert_eq!(
            unsafe { tinyarcade_v1_last_execution_stats_v2(runtime, stats_v2.as_mut_ptr()) },
            STATUS_OK
        );
        let stats_v2 = unsafe { stats_v2.assume_init() };
        assert_eq!(
            stats_v2.struct_size,
            size_of::<TinyArcadeExecutionStatsV2>() as u32
        );
        assert_eq!(stats_v2.lifecycle, crate::GameLifecycle::Tick as u32);
        assert_eq!(stats_v2.wasm_steps, stats.wasm_steps);
        assert!(stats_v2.peak_call_depth > 0);
        assert!(stats_v2.peak_activation_slots > 0);
        assert_eq!(stats_v2.memory_pages, stats.memory_pages);
        assert_eq!(stats_v2.render_bytes, stats.render_bytes);
        assert_eq!(
            unsafe { tinyarcade_v1_tick(runtime, 1 << 31, 17) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { tinyarcade_v1_tick(runtime, 0, 15) },
            STATUS_INVALID_ARGUMENT
        );
        let mut failed = -1;
        assert_eq!(
            unsafe { tinyarcade_v1_is_failed(runtime, &mut failed) },
            STATUS_OK
        );
        assert_eq!(failed, 0);
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 16) }, STATUS_OK);

        let mut required = 0usize;
        assert_eq!(
            unsafe { tinyarcade_v1_copy_render(runtime, ptr::null_mut(), 0, &mut required) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert_eq!(required, 1);

        let mut error_len = 0usize;
        assert_eq!(
            unsafe { tinyarcade_v1_last_error(ptr::null_mut(), 0, &mut error_len) },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut error = vec![0; error_len];
        assert_eq!(
            unsafe { tinyarcade_v1_last_error(error.as_mut_ptr(), error.len(), &mut error_len) },
            STATUS_OK
        );
        assert_eq!(error, b"output buffer too small");

        let mut render = [0u8; 1];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_render(runtime, render.as_mut_ptr(), render.len(), &mut required)
            },
            STATUS_OK
        );
        assert_eq!(render, [9]);

        assert_eq!(unsafe { tinyarcade_v1_suspend(runtime) }, STATUS_OK);
        let mut stats = MaybeUninit::<TinyArcadeExecutionStatsV1>::uninit();
        assert_eq!(
            unsafe { tinyarcade_v1_last_execution_stats(runtime, stats.as_mut_ptr()) },
            STATUS_OK
        );
        let stats = unsafe { stats.assume_init() };
        assert_eq!(stats.lifecycle, crate::GameLifecycle::Suspend as u32);
        assert!(stats.state_bytes > 0);
        let mut snapshot_len = 0usize;
        assert_eq!(
            unsafe { tinyarcade_v1_copy_snapshot(runtime, ptr::null_mut(), 0, &mut snapshot_len) },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut snapshot = vec![0; snapshot_len];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_snapshot(
                    runtime,
                    snapshot.as_mut_ptr(),
                    snapshot.len(),
                    &mut snapshot_len,
                )
            },
            STATUS_OK
        );

        let address = runtime as usize;
        assert_eq!(
            std::thread::spawn(move || {
                let mut failed = 0;
                unsafe { tinyarcade_v1_is_failed(address as *mut TinyArcadeRuntimeV1, &mut failed) }
            })
            .join()
            .expect("thread probe"),
            STATUS_WRONG_THREAD
        );

        let restored = unsafe { open(&wasm) };
        assert_eq!(
            unsafe { tinyarcade_v1_resume(restored, snapshot.as_ptr(), snapshot.len()) },
            STATUS_OK
        );
        assert_eq!(unsafe { tinyarcade_v1_close(restored) }, STATUS_OK);
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);

        let config = unsafe { config() };
        let mut private = ptr::null_mut();
        assert_eq!(
            unsafe { tinyarcade_v1_open_private(wasm.as_ptr(), wasm.len(), &config, &mut private) },
            STATUS_OK
        );
        assert_eq!(
            unsafe { tinyarcade_v1_origin(private, &mut origin) },
            STATUS_OK
        );
        assert_eq!(origin, CartridgeOrigin::PrivateUser as u32);
        assert_eq!(unsafe { tinyarcade_v1_close(private) }, STATUS_OK);
    }

    #[test]
    fn c_descriptor_is_bounded_static_and_reports_native_requirements() {
        let wasm = native_cartridge();
        let mut required = 0usize;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_cartridge_descriptor(
                    wasm.as_ptr(),
                    wasm.len(),
                    ptr::null_mut(),
                    0,
                    &mut required,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        assert!((32..=4096).contains(&required));
        let mut encoded = vec![0; required];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_cartridge_descriptor(
                    wasm.as_ptr(),
                    wasm.len(),
                    encoded.as_mut_ptr(),
                    encoded.len(),
                    &mut required,
                )
            },
            STATUS_OK
        );
        assert_eq!(&encoded[0..4], b"TAD1");
        assert_eq!(u16::from_le_bytes([encoded[4], encoded[5]]), 1);
        assert_eq!(u16::from_le_bytes([encoded[6], encoded[7]]), 32);
        assert_eq!(u16::from_le_bytes([encoded[20], encoded[21]]), 1);
        assert_eq!(u16::from_le_bytes([encoded[22], encoded[23]]), 2);
        assert_eq!(
            u32::from_le_bytes(encoded[24..28].try_into().expect("descriptor length")),
            wasm.len() as u32
        );
        assert!(encoded.windows(14).any(|bytes| bytes == b"fan:physics/v1"));
        assert!(encoded.windows(10).any(|bytes| bytes == b"step_world"));

        let invalid = [0u8];
        required = usize::MAX;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_cartridge_descriptor(
                    invalid.as_ptr(),
                    invalid.len(),
                    ptr::null_mut(),
                    0,
                    &mut required,
                )
            },
            STATUS_DECODE
        );
        assert_eq!(required, usize::MAX);
    }

    #[test]
    fn c_host_profile_exports_exact_app_registry_and_checks_without_execution() {
        let wasm = native_cartridge();
        let mut config = unsafe { config() };
        config.max_call_depth = 37;
        config.max_activation_slots = 4096;
        config.max_audio_bytes = 0;
        let mut probe = NativeProbe {
            calls: Cell::new(0),
            fail: Cell::new(false),
        };
        let module = b"fan:physics/v1";
        let field = b"step_world";
        let function = TinyArcadeNativeFunctionV1 {
            struct_size: size_of::<TinyArcadeNativeFunctionV1>() as u32,
            module: module.as_ptr(),
            module_len: module.len(),
            field: field.as_ptr(),
            field_len: field.len(),
            n_params: 2,
            n_results: 1,
            max_calls_per_lifecycle: 2,
            callback: Some(native_step),
            context: (&mut probe as *mut NativeProbe).cast(),
        };
        let mut required = 0usize;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_profile(
                    &config,
                    &function,
                    1,
                    ptr::null_mut(),
                    0,
                    &mut required,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        assert!((64..=4096).contains(&required));
        let mut profile = vec![0; required];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_profile(
                    &config,
                    &function,
                    1,
                    profile.as_mut_ptr(),
                    profile.len(),
                    &mut required,
                )
            },
            STATUS_OK
        );
        assert_eq!(&profile[..4], b"TAH1");
        assert_eq!(u16::from_le_bytes([profile[4], profile[5]]), 3);
        assert_eq!(u16::from_le_bytes([profile[6], profile[7]]), 68);
        assert_eq!(u16::from_le_bytes([profile[50], profile[51]]), 1);
        assert_eq!(u16::from_le_bytes([profile[52], profile[53]]), 1);
        let decoded = HostProfileV1::decode(&profile).expect("decode exported host profile");
        assert_eq!(decoded.vm_limits().max_call_depth, 37);
        assert_eq!(decoded.vm_limits().max_activation_slots, 4096);
        assert_eq!(decoded.game_limits().max_audio_bytes, 0);
        assert!(profile.windows(module.len()).any(|value| value == module));
        assert_eq!(
            unsafe {
                tinyarcade_v1_check_cartridge_host_profile(
                    wasm.as_ptr(),
                    wasm.len(),
                    profile.as_ptr(),
                    profile.len(),
                )
            },
            STATUS_OK
        );
        let mut compatible_len = 0usize;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_compatible_cartridge_descriptor(
                    wasm.as_ptr(),
                    wasm.len(),
                    profile.as_ptr(),
                    profile.len(),
                    ptr::null_mut(),
                    0,
                    &mut compatible_len,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut compatible = vec![0; compatible_len];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_compatible_cartridge_descriptor(
                    wasm.as_ptr(),
                    wasm.len(),
                    profile.as_ptr(),
                    profile.len(),
                    compatible.as_mut_ptr(),
                    compatible.len(),
                    &mut compatible_len,
                )
            },
            STATUS_OK
        );
        assert_eq!(&compatible[..4], b"TAD1");
        assert_eq!(
            u32::from_le_bytes(compatible[24..28].try_into().expect("descriptor length")),
            wasm.len() as u32
        );
        let mut report_len = 0usize;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_compatibility_report(
                    wasm.as_ptr(),
                    wasm.len(),
                    profile.as_ptr(),
                    profile.len(),
                    ptr::null_mut(),
                    0,
                    &mut report_len,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut report = vec![0; report_len];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_compatibility_report(
                    wasm.as_ptr(),
                    wasm.len(),
                    profile.as_ptr(),
                    profile.len(),
                    report.as_mut_ptr(),
                    report.len(),
                    &mut report_len,
                )
            },
            STATUS_OK
        );
        assert_eq!(&report[..4], b"TAC1");
        assert_eq!(u16::from_le_bytes([report[8], report[9]]), 0);
        let report_descriptor_len =
            u32::from_le_bytes(report[12..16].try_into().expect("report descriptor length"))
                as usize;
        assert_eq!(&report[16..20], b"TAD1");
        assert_eq!(report.len(), 16 + report_descriptor_len);
        assert_eq!(
            probe.calls.get(),
            0,
            "profile operations must not call app code"
        );

        let mut wrong = function;
        wrong.n_params = 1;
        required = 0;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_profile(
                    &config,
                    &wrong,
                    1,
                    ptr::null_mut(),
                    0,
                    &mut required,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut wrong_profile = vec![0; required];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_profile(
                    &config,
                    &wrong,
                    1,
                    wrong_profile.as_mut_ptr(),
                    wrong_profile.len(),
                    &mut required,
                )
            },
            STATUS_OK
        );
        assert_eq!(
            unsafe {
                tinyarcade_v1_check_cartridge_host_profile(
                    wasm.as_ptr(),
                    wasm.len(),
                    wrong_profile.as_ptr(),
                    wrong_profile.len(),
                )
            },
            STATUS_TRAP
        );
        compatible_len = usize::MAX;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_compatible_cartridge_descriptor(
                    wasm.as_ptr(),
                    wasm.len(),
                    wrong_profile.as_ptr(),
                    wrong_profile.len(),
                    ptr::null_mut(),
                    0,
                    &mut compatible_len,
                )
            },
            STATUS_TRAP
        );
        assert_eq!(compatible_len, usize::MAX);
        report_len = 0;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_compatibility_report(
                    wasm.as_ptr(),
                    wasm.len(),
                    wrong_profile.as_ptr(),
                    wrong_profile.len(),
                    ptr::null_mut(),
                    0,
                    &mut report_len,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        report.resize(report_len, 0);
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_compatibility_report(
                    wasm.as_ptr(),
                    wasm.len(),
                    wrong_profile.as_ptr(),
                    wrong_profile.len(),
                    report.as_mut_ptr(),
                    report.len(),
                    &mut report_len,
                )
            },
            STATUS_OK
        );
        assert_eq!(u16::from_le_bytes([report[8], report[9]]), 1);
        let descriptor_len =
            u32::from_le_bytes(report[12..16].try_into().expect("report descriptor length"))
                as usize;
        let issue = 16 + descriptor_len;
        let module_len = u16::from_le_bytes([report[issue], report[issue + 1]]) as usize;
        let field_len = u16::from_le_bytes([report[issue + 2], report[issue + 3]]) as usize;
        assert_eq!(&report[issue + 4..issue + 8], &[2, 1, 1, 1]);
        assert_eq!(&report[issue + 8..issue + 8 + module_len], module);
        assert_eq!(
            &report[issue + 8 + module_len..issue + 8 + module_len + field_len],
            field
        );
        assert_eq!(probe.calls.get(), 0);
    }

    #[test]
    fn c_old_40_byte_config_prefix_remains_accepted() {
        assert_eq!(size_of::<TinyArcadeConfigV1Prefix>(), 40);
        assert_eq!(size_of::<TinyArcadeConfigV1>(), 48);
        assert_eq!(size_of::<TinyArcadeExecutionStatsV1>(), 40);
        assert_eq!(size_of::<TinyArcadeExecutionStatsV2>(), 48);

        let defaults = Limits::default();
        let game = GameLimits::default();
        let old = TinyArcadeConfigV1Prefix {
            struct_size: size_of::<TinyArcadeConfigV1Prefix>() as u32,
            max_table_elems: defaults.max_table_elems as u32,
            max_memory_pages: defaults.max_memory_pages as u32,
            max_steps: defaults.max_steps,
            max_render_bytes: game.max_render_bytes as u32,
            max_audio_bytes: game.max_audio_bytes as u32,
            max_state_bytes: game.max_state_bytes as u32,
            rng_seed: 7,
        };
        let config = (&old as *const TinyArcadeConfigV1Prefix).cast::<TinyArcadeConfigV1>();
        let mut required = 0;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_profile(
                    config,
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut required,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut encoded = vec![0; required];
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_profile(
                    config,
                    ptr::null(),
                    0,
                    encoded.as_mut_ptr(),
                    encoded.len(),
                    &mut required,
                )
            },
            STATUS_OK
        );
        let profile = HostProfileV1::decode(&encoded).expect("decode old-config profile");
        assert_eq!(profile.vm_limits().max_call_depth, defaults.max_call_depth);
        assert_eq!(
            profile.vm_limits().max_activation_slots,
            defaults.max_activation_slots
        );
    }

    #[test]
    fn c_runtime_panic_latches_instance_and_discards_partial_outputs() {
        let wasm = cartridge();
        let runtime = unsafe { open(&wasm) };
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 0) }, STATUS_OK);
        assert_eq!(unsafe { tinyarcade_v1_suspend(runtime) }, STATUS_OK);

        assert_eq!(
            runtime_boundary(runtime, |_| -> Result<(), FfiError> {
                panic!("injected lifecycle panic")
            }),
            STATUS_PANIC
        );
        let mut failed = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_is_failed(runtime, &mut failed) },
            STATUS_OK
        );
        assert_eq!(failed, 1);
        assert_eq!(
            unsafe { tinyarcade_v1_tick(runtime, 0, 1) },
            STATUS_FAILED_INSTANCE
        );

        let mut output_len = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_copy_render(runtime, ptr::null_mut(), 0, &mut output_len) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { tinyarcade_v1_copy_snapshot(runtime, ptr::null_mut(), 0, &mut output_len) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);
    }

    #[test]
    fn c_native_table_binds_exact_callback_and_latches_failure() {
        let wasm = native_cartridge();
        let config = unsafe { config() };
        let module = b"fan:physics/v1";
        let field = b"step_world";
        let mut probe = NativeProbe {
            calls: Cell::new(0),
            fail: Cell::new(false),
        };
        let function = TinyArcadeNativeFunctionV1 {
            struct_size: size_of::<TinyArcadeNativeFunctionV1>() as u32,
            module: module.as_ptr(),
            module_len: module.len(),
            field: field.as_ptr(),
            field_len: field.len(),
            n_params: 2,
            n_results: 1,
            max_calls_per_lifecycle: 2,
            callback: Some(native_step),
            context: (&mut probe as *mut NativeProbe).cast(),
        };

        let mut runtime = ptr::dangling_mut();
        assert_eq!(
            unsafe { tinyarcade_v1_open(wasm.as_ptr(), wasm.len(), &config, &mut runtime) },
            STATUS_TRAP
        );
        assert!(runtime.is_null());
        assert_eq!(probe.calls.get(), 0);

        assert_eq!(
            unsafe {
                tinyarcade_v1_open_with_native_modules(
                    wasm.as_ptr(),
                    wasm.len(),
                    &function,
                    1,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_OK
        );
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 0) }, STATUS_OK);
        assert_eq!(probe.calls.get(), 2);
        let mut render = [0; 8];
        let mut render_len = 0;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_render(
                    runtime,
                    render.as_mut_ptr(),
                    render.len(),
                    &mut render_len,
                )
            },
            STATUS_OK
        );
        assert_eq!(render_len, 8);
        assert_eq!(&render[..4], &42i32.to_le_bytes());
        assert_eq!(render[4], 9);

        probe.fail.set(true);
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 1) }, STATUS_TRAP);
        let mut failed = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_is_failed(runtime, &mut failed) },
            STATUS_OK
        );
        assert_eq!(failed, 1);
        render_len = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_copy_render(runtime, ptr::null_mut(), 0, &mut render_len) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { tinyarcade_v1_tick(runtime, 0, 2) },
            STATUS_FAILED_INSTANCE
        );
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);

        probe.calls.set(0);
        probe.fail.set(false);
        let one_call = TinyArcadeNativeFunctionV1 {
            max_calls_per_lifecycle: 1,
            ..function
        };
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_with_native_modules(
                    wasm.as_ptr(),
                    wasm.len(),
                    &one_call,
                    1,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_OK
        );
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 0) }, STATUS_TRAP);
        assert_eq!(probe.calls.get(), 1);
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);

        let no_budget = TinyArcadeNativeFunctionV1 {
            max_calls_per_lifecycle: 0,
            ..function
        };
        runtime = ptr::dangling_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_with_native_modules(
                    wasm.as_ptr(),
                    wasm.len(),
                    &no_budget,
                    1,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_INVALID_ARGUMENT
        );
        assert!(runtime.is_null());

        let invalid = TinyArcadeNativeFunctionV1 {
            n_results: 17,
            ..function
        };
        runtime = ptr::dangling_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_with_native_modules(
                    wasm.as_ptr(),
                    wasm.len(),
                    &invalid,
                    1,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_INVALID_ARGUMENT
        );
        assert!(runtime.is_null());
    }

    #[test]
    fn c_completion_channel_survives_callback_and_rejects_late_delivery() {
        let module = "fan:async/v1";
        let bare = wat::parse_str(format!(
            r#"(module
                (import "{module}" "start" (func $start (result i32)))
                (import "{module}" "completion_poll"
                  (func $poll (param i32 i32 i32) (result i32)))
                (import "{module}" "completion_take"
                  (func $take (param i32 i32 i32) (result i32)))
                (import "{module}" "completion_cancel"
                  (func $cancel (param i32) (result i32)))
                (import "tinyarcade:core/v1" "submit_render"
                  (func $submit_render (param i32 i32) (result i32)))
                (memory 1)
                (global $ticket (mut i32) (i32.const 0))
                (func (export "game_abi_version") (result i32) (i32.const 1))
                (func (export "game_init") (result i32)
                  call $start global.set $ticket i32.const 0)
                (func (export "game_tick") (result i32)
                  global.get $ticket i32.const 0 i32.const 4 call $poll
                  i32.const 1 i32.eq
                  if
                    global.get $ticket i32.const 8 i32.const 4 call $take drop
                    i32.const 8 i32.const 4 call $submit_render drop
                  end
                  i32.const 0)
                (func (export "game_suspend") (result i32) (i32.const 0))
                (func (export "game_resume") (result i32) (i32.const 0)))"#
        ))
        .expect("compile C completion cartridge");
        let wasm = CartridgeManifest {
            game_id: "c.async".to_owned(),
            game_version: "1.0.0".to_owned(),
            abi_version: 1,
            state_version: 1,
            capabilities: vec![module.to_owned()],
        }
        .append_to_wasm(&bare)
        .expect("attach C completion manifest");

        let mut completion = ptr::null_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_completion_create(
                    module.as_ptr(),
                    module.len(),
                    2,
                    8,
                    8,
                    &mut completion,
                )
            },
            STATUS_OK
        );
        let start = b"start";
        let mut probe = CompletionProbe {
            channel: completion,
            ticket: Cell::new(0),
        };
        let function = TinyArcadeNativeFunctionV1 {
            struct_size: size_of::<TinyArcadeNativeFunctionV1>() as u32,
            module: module.as_ptr(),
            module_len: module.len(),
            field: start.as_ptr(),
            field_len: start.len(),
            n_params: 0,
            n_results: 1,
            max_calls_per_lifecycle: 1,
            callback: Some(native_completion_start),
            context: (&mut probe as *mut CompletionProbe).cast(),
        };
        let config = unsafe { config() };
        let mut profile_len = 0;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_host_profile_with_completions(
                    &config,
                    &function,
                    1,
                    &completion,
                    1,
                    ptr::null_mut(),
                    0,
                    &mut profile_len,
                )
            },
            STATUS_BUFFER_TOO_SMALL
        );
        assert!(profile_len > 56);
        let mut runtime = ptr::null_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_with_native_completions(
                    wasm.as_ptr(),
                    wasm.len(),
                    &function,
                    1,
                    &completion,
                    1,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_OK
        );
        assert_eq!(
            unsafe { tinyarcade_v1_completion_close(completion) },
            STATUS_INVALID_ARGUMENT
        );
        let ticket = probe.ticket.get();
        assert_ne!(ticket, 0);
        let address = completion as usize;
        assert_eq!(
            std::thread::spawn(move || unsafe {
                tinyarcade_v1_completion_complete(
                    address as *mut TinyArcadeCompletionV1,
                    ticket,
                    7,
                    [1u8, 2, 3, 4].as_ptr(),
                    4,
                )
            })
            .join()
            .expect("wrong-thread completion probe"),
            STATUS_WRONG_THREAD
        );
        let payload = [1u8, 2, 3, 4];
        assert_eq!(
            unsafe {
                tinyarcade_v1_completion_complete(
                    completion,
                    ticket,
                    7,
                    payload.as_ptr(),
                    payload.len(),
                )
            },
            STATUS_OK
        );
        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 0) }, STATUS_OK);
        let mut render = [0; 4];
        let mut render_len = 0;
        assert_eq!(
            unsafe {
                tinyarcade_v1_copy_render(
                    runtime,
                    render.as_mut_ptr(),
                    render.len(),
                    &mut render_len,
                )
            },
            STATUS_OK
        );
        assert_eq!(render, payload);
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);
        assert_eq!(
            unsafe {
                tinyarcade_v1_completion_complete(
                    completion,
                    ticket,
                    7,
                    payload.as_ptr(),
                    payload.len(),
                )
            },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { tinyarcade_v1_completion_close(completion) },
            STATUS_OK
        );
    }

    #[test]
    fn c_native_callback_cannot_reenter_a_runtime_handle() {
        let wasm = native_cartridge();
        let config = unsafe { config() };
        let other_wasm = cartridge();
        let mut other_runtime = ptr::null_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open(
                    other_wasm.as_ptr(),
                    other_wasm.len(),
                    &config,
                    &mut other_runtime,
                )
            },
            STATUS_OK
        );
        let module = b"fan:physics/v1";
        let field = b"step_world";
        let mut probe = ReentrantProbe {
            runtime: Cell::new(ptr::null_mut()),
            other_runtime: Cell::new(other_runtime),
            calls: Cell::new(0),
            status: Cell::new(STATUS_OK),
            other_status: Cell::new(STATUS_OK),
        };
        let function = TinyArcadeNativeFunctionV1 {
            struct_size: size_of::<TinyArcadeNativeFunctionV1>() as u32,
            module: module.as_ptr(),
            module_len: module.len(),
            field: field.as_ptr(),
            field_len: field.len(),
            n_params: 2,
            n_results: 1,
            max_calls_per_lifecycle: 2,
            callback: Some(reentrant_native_step),
            context: (&mut probe as *mut ReentrantProbe).cast(),
        };
        let mut runtime = ptr::null_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_with_native_modules(
                    wasm.as_ptr(),
                    wasm.len(),
                    &function,
                    1,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_OK
        );
        probe.runtime.set(runtime);

        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 0) }, STATUS_OK);
        assert_eq!(probe.calls.get(), 2);
        assert_eq!(probe.status.get(), STATUS_INVALID_ARGUMENT);
        assert_eq!(probe.other_status.get(), STATUS_INVALID_ARGUMENT);
        let mut error_len = usize::MAX;
        assert_eq!(
            unsafe { tinyarcade_v1_last_error(ptr::null_mut(), 0, &mut error_len) },
            STATUS_OK,
            "successful outer tick must clear the rejected nested call's error"
        );
        assert_eq!(error_len, 0);

        assert_eq!(unsafe { tinyarcade_v1_tick(runtime, 0, 1) }, STATUS_OK);
        assert_eq!(probe.calls.get(), 4);
        let mut other_failed = -1;
        assert_eq!(
            unsafe { tinyarcade_v1_is_failed(other_runtime, &mut other_failed) },
            STATUS_OK
        );
        assert_eq!(other_failed, 0);
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);
        assert_eq!(unsafe { tinyarcade_v1_close(other_runtime) }, STATUS_OK);
    }

    #[test]
    fn c_reviewed_open_binds_signature_revocation_and_origin() {
        let wasm = cartridge();
        let key_id = b"ios-test-key";
        let key_pair = Ed25519KeyPair::from_seed_unchecked(&[0x6c; 32]).expect("signing key");
        let mut entry = CatalogEntry {
            game_id: "c.test".into(),
            game_version: "1.0.0".into(),
            abi_version: 1,
            state_version: 1,
            wasm_length: wasm.len() as u64,
            wasm_sha256: cartridge_sha256(&wasm),
            signing_key_id: "ios-test-key".into(),
            signature: [0; 64],
        };
        let signing = entry.signing_bytes().expect("catalog signing bytes");
        entry
            .signature
            .copy_from_slice(key_pair.sign(&signing).as_ref());
        let c_entry = TinyArcadeCatalogEntryV1 {
            struct_size: size_of::<TinyArcadeCatalogEntryV1>() as u32,
            game_id: entry.game_id.as_ptr(),
            game_id_len: entry.game_id.len(),
            game_version: entry.game_version.as_ptr(),
            game_version_len: entry.game_version.len(),
            abi_version: entry.abi_version,
            state_version: entry.state_version,
            wasm_length: entry.wasm_length,
            wasm_sha256: entry.wasm_sha256.as_ptr(),
            wasm_sha256_len: entry.wasm_sha256.len(),
            signing_key_id: entry.signing_key_id.as_ptr(),
            signing_key_id_len: entry.signing_key_id.len(),
            signature: entry.signature.as_ptr(),
            signature_len: entry.signature.len(),
        };
        let mut trust = ptr::null_mut();
        assert_eq!(
            unsafe { tinyarcade_v1_trust_store_create(&mut trust) },
            STATUS_OK
        );
        assert_eq!(
            unsafe {
                tinyarcade_v1_trust_store_add_key(
                    trust,
                    key_id.as_ptr(),
                    key_id.len(),
                    key_pair.public_key().as_ref().as_ptr(),
                    key_pair.public_key().as_ref().len(),
                )
            },
            STATUS_OK
        );
        let config = unsafe { config() };
        let mut runtime = ptr::null_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_reviewed(
                    wasm.as_ptr(),
                    wasm.len(),
                    &c_entry,
                    trust,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_OK
        );
        let mut origin = u32::MAX;
        assert_eq!(
            unsafe { tinyarcade_v1_origin(runtime, &mut origin) },
            STATUS_OK
        );
        assert_eq!(origin, CartridgeOrigin::OfficialReviewed as u32);
        assert_eq!(unsafe { tinyarcade_v1_close(runtime) }, STATUS_OK);

        let directory = tempfile::tempdir().expect("temporary C cache");
        let directory = directory.path().to_string_lossy().into_owned();
        let mut cache = ptr::null_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_cache_create(
                    directory.as_ptr(),
                    directory.len(),
                    1_048_576,
                    &mut cache,
                )
            },
            STATUS_OK
        );
        assert_eq!(
            unsafe {
                tinyarcade_v1_cache_activate(cache, &c_entry, wasm.as_ptr(), wasm.len(), trust)
            },
            STATUS_OK
        );
        assert_eq!(
            unsafe { tinyarcade_v1_cache_load_active(cache, &c_entry, trust) },
            STATUS_OK
        );
        let mut cached_len = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_cache_copy_wasm(cache, ptr::null_mut(), 0, &mut cached_len) },
            STATUS_BUFFER_TOO_SMALL
        );
        let mut cached = vec![0; cached_len];
        assert_eq!(
            unsafe {
                tinyarcade_v1_cache_copy_wasm(
                    cache,
                    cached.as_mut_ptr(),
                    cached.len(),
                    &mut cached_len,
                )
            },
            STATUS_OK
        );
        assert_eq!(cached, wasm);

        let cache_address = cache as usize;
        assert_eq!(
            std::thread::spawn(move || unsafe {
                let mut required = 0;
                tinyarcade_v1_cache_copy_wasm(
                    cache_address as *mut TinyArcadeCartridgeCacheV1,
                    ptr::null_mut(),
                    0,
                    &mut required,
                )
            })
            .join()
            .expect("cache thread probe"),
            STATUS_WRONG_THREAD
        );

        assert_eq!(
            unsafe {
                tinyarcade_v1_trust_store_revoke_content(
                    trust,
                    entry.wasm_sha256.as_ptr(),
                    entry.wasm_sha256.len(),
                )
            },
            STATUS_OK
        );
        runtime = ptr::dangling_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_reviewed(
                    wasm.as_ptr(),
                    wasm.len(),
                    &c_entry,
                    trust,
                    &config,
                    &mut runtime,
                )
            },
            STATUS_TRUST
        );
        assert!(runtime.is_null());
        assert_eq!(
            unsafe { tinyarcade_v1_cache_load_active(cache, &c_entry, trust) },
            STATUS_TRUST
        );
        assert_eq!(
            unsafe { tinyarcade_v1_cache_copy_wasm(cache, ptr::null_mut(), 0, &mut cached_len) },
            STATUS_INVALID_ARGUMENT
        );
        assert_eq!(unsafe { tinyarcade_v1_cache_close(cache) }, STATUS_OK);
        assert_eq!(unsafe { tinyarcade_v1_trust_store_close(trust) }, STATUS_OK);
    }

    #[test]
    fn c_open_nulls_output_and_preserves_decode_detail() {
        let config = unsafe { config() };
        let bad = b"not wasm";
        let mut runtime = ptr::dangling_mut::<TinyArcadeRuntimeV1>();
        assert_eq!(
            unsafe { tinyarcade_v1_open(bad.as_ptr(), bad.len(), &config, &mut runtime) },
            STATUS_DECODE
        );
        assert!(runtime.is_null());
        let mut len = 0;
        assert_eq!(
            unsafe { tinyarcade_v1_last_error(ptr::null_mut(), 0, &mut len) },
            STATUS_BUFFER_TOO_SMALL
        );
        assert!(len > 0);

        let invalid_entry = TinyArcadeCatalogEntryV1 {
            struct_size: 0,
            game_id: ptr::null(),
            game_id_len: 0,
            game_version: ptr::null(),
            game_version_len: 0,
            abi_version: 0,
            state_version: 0,
            wasm_length: 0,
            wasm_sha256: ptr::null(),
            wasm_sha256_len: 0,
            signing_key_id: ptr::null(),
            signing_key_id_len: 0,
            signature: ptr::null(),
            signature_len: 0,
        };
        runtime = ptr::dangling_mut();
        assert_eq!(
            unsafe {
                tinyarcade_v1_open_reviewed(
                    bad.as_ptr(),
                    bad.len(),
                    &invalid_entry,
                    ptr::null_mut(),
                    &config,
                    &mut runtime,
                )
            },
            STATUS_INVALID_ARGUMENT
        );
        assert!(runtime.is_null());
    }

    #[test]
    fn c_header_declares_every_versioned_export() {
        let header =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/include/tinyarcade.h"))
                .expect("read C header");
        for symbol in [
            "tinyarcade_v1_abi_version",
            "tinyarcade_v1_default_config",
            "tinyarcade_v1_copy_cartridge_descriptor",
            "tinyarcade_v1_copy_host_profile",
            "tinyarcade_v1_copy_host_profile_with_completions",
            "tinyarcade_v1_check_cartridge_host_profile",
            "tinyarcade_v1_copy_compatible_cartridge_descriptor",
            "tinyarcade_v1_copy_host_compatibility_report",
            "tinyarcade_v1_completion_create",
            "tinyarcade_v1_completion_close",
            "tinyarcade_v1_completion_begin",
            "tinyarcade_v1_completion_complete",
            "tinyarcade_v1_completion_cancel",
            "tinyarcade_v1_trust_store_create",
            "tinyarcade_v1_trust_store_close",
            "tinyarcade_v1_trust_store_add_key",
            "tinyarcade_v1_trust_store_revoke_key",
            "tinyarcade_v1_trust_store_revoke_content",
            "tinyarcade_v1_cache_create",
            "tinyarcade_v1_cache_close",
            "tinyarcade_v1_cache_activate",
            "tinyarcade_v1_cache_load_active",
            "tinyarcade_v1_cache_rollback",
            "tinyarcade_v1_cache_copy_wasm",
            "tinyarcade_v1_open",
            "tinyarcade_v1_open_with_native_modules",
            "tinyarcade_v1_open_with_native_completions",
            "tinyarcade_v1_open_private",
            "tinyarcade_v1_open_reviewed",
            "tinyarcade_v1_open_reviewed_with_native_modules",
            "tinyarcade_v1_open_reviewed_with_native_completions",
            "tinyarcade_v1_close",
            "tinyarcade_v1_tick",
            "tinyarcade_v1_replay_begin",
            "tinyarcade_v1_replay_cancel",
            "tinyarcade_v1_replay_finish",
            "tinyarcade_v1_copy_replay",
            "tinyarcade_v1_replay_check",
            "tinyarcade_v1_copy_render",
            "tinyarcade_v1_copy_audio",
            "tinyarcade_v1_suspend",
            "tinyarcade_v1_copy_snapshot",
            "tinyarcade_v1_resume",
            "tinyarcade_v1_copy_game_id",
            "tinyarcade_v1_copy_game_version",
            "tinyarcade_v1_is_failed",
            "tinyarcade_v1_origin",
            "tinyarcade_v1_last_execution_stats",
            "tinyarcade_v1_last_execution_stats_v2",
            "tinyarcade_v1_last_error",
        ] {
            assert!(
                header.contains(&format!("{symbol}(")),
                "C header is missing {symbol}"
            );
        }
    }
}
