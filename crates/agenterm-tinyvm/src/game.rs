//! Versioned, bounded native host contract for tinyvm games.

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use core::mem;

use crate::cartridge::{valid_native_field, valid_native_namespace};
use crate::media::INDEXED2D_MAGIC;
use crate::{CartridgeManifest, Limits, Val, WasmError, WasmInstance, WasmModule};

/// Guest/host contract implemented by this module.
pub const GAME_ABI_VERSION: i32 = 1;
pub const MAX_CARTRIDGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NATIVE_FUNCTIONS: usize = 64;
pub const MAX_NATIVE_ARITY: usize = 16;
pub const MAX_NATIVE_CALLS_PER_LIFECYCLE: u32 = 64;
const ABI_MODULE: &str = "tinyarcade:core/v1";
const SNAPSHOT_MAGIC: &[u8; 4] = b"TGS1";
type NativeImpl = dyn Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError>;

/// Immutable policy surface that admitted a cartridge instance.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CartridgeOrigin {
    /// Shipped inside the signed app bundle.
    Bundled = 0,
    /// Exact bytes accepted by the reviewed signed-catalog trust gate.
    OfficialReviewed = 1,
    /// User-selected local import, never catalog publication authority.
    PrivateUser = 2,
}

/// Stable v1 bits returned by the `input_bits` core import.
pub mod button {
    pub const LEFT: u32 = 1 << 0;
    pub const RIGHT: u32 = 1 << 1;
    pub const UP: u32 = 1 << 2;
    pub const DOWN: u32 = 1 << 3;
    pub const PRIMARY: u32 = 1 << 4;
    pub const SECONDARY: u32 = 1 << 5;
    pub const TERTIARY: u32 = 1 << 6;
    pub const START: u32 = 1 << 7;
    pub const MENU: u32 = 1 << 8;
}

/// Host-owned byte ceilings for one rendered frame.
#[derive(Clone, Copy)]
pub struct GameLimits {
    pub max_render_bytes: usize,
    pub max_audio_bytes: usize,
    pub max_state_bytes: usize,
}

impl Default for GameLimits {
    fn default() -> Self {
        Self {
            max_render_bytes: 64 * 1024,
            max_audio_bytes: 16 * 1024,
            max_state_bytes: 256 * 1024,
        }
    }
}

/// Complete deterministic input visible during one call to `game_tick`.
#[derive(Clone, Copy, Default)]
pub struct GameInput {
    /// ABI v1 buttons packed using the stable [`button`] bit assignments.
    pub buttons: u32,
    /// Host-provided monotonic game time. Wall-clock time is never exposed.
    pub clock_ms: u32,
}

/// Bounded command streams emitted by one successful game tick.
pub struct GameFrame {
    pub render: Vec<u8>,
    pub audio: Vec<u8>,
}

struct NativeFunction {
    module: String,
    field: String,
    n_params: usize,
    n_results: usize,
    max_calls_per_lifecycle: u32,
    callback: Rc<NativeImpl>,
}

/// Native capabilities explicitly made available by the app host.
///
/// Namespaces must be versioned, such as `studio:physics/v1`, and cannot
/// replace the core ABI. A cartridge import grants nothing by itself: its
/// exact function and i32 signature must exist in this registry.
#[derive(Default)]
pub struct NativeModuleRegistry {
    functions: Vec<NativeFunction>,
}

impl NativeModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one function in a versioned native module namespace.
    pub fn register<F>(
        &mut self,
        module: &str,
        field: &str,
        n_params: usize,
        n_results: usize,
        callback: F,
    ) -> Result<(), WasmError>
    where
        F: Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError> + 'static,
    {
        self.register_with_call_limit(module, field, n_params, n_results, 1, callback)
    }

    /// Register one function with an exact per-lifecycle dispatch ceiling.
    ///
    /// The quota resets before each init/tick/suspend/resume call and is
    /// charged before app code runs. Capability implementations remain trusted
    /// app code and must themselves be bounded and nonblocking.
    pub fn register_with_call_limit<F>(
        &mut self,
        module: &str,
        field: &str,
        n_params: usize,
        n_results: usize,
        max_calls_per_lifecycle: u32,
        callback: F,
    ) -> Result<(), WasmError>
    where
        F: Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError> + 'static,
    {
        if module == ABI_MODULE
            || !valid_native_namespace(module)
            || !valid_native_field(field)
            || n_params > MAX_NATIVE_ARITY
            || n_results > MAX_NATIVE_ARITY
            || max_calls_per_lifecycle == 0
            || max_calls_per_lifecycle > MAX_NATIVE_CALLS_PER_LIFECYCLE
            || self.functions.len() >= MAX_NATIVE_FUNCTIONS
            || self.find(module, field).is_some()
        {
            return Err(WasmError::Trap("invalid native module registration"));
        }
        self.functions.push(NativeFunction {
            module: module.to_string(),
            field: field.to_string(),
            n_params,
            n_results,
            max_calls_per_lifecycle,
            callback: Rc::new(callback),
        });
        Ok(())
    }

    fn find(&self, module: &str, field: &str) -> Option<&NativeFunction> {
        self.functions
            .iter()
            .find(|function| function.module == module && function.field == field)
    }

    fn find_with_index(&self, module: &str, field: &str) -> Option<(usize, &NativeFunction)> {
        self.functions
            .iter()
            .enumerate()
            .find(|(_, function)| function.module == module && function.field == field)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Init,
    Tick,
    Suspend,
    Resume,
}

struct HostState {
    limits: GameLimits,
    phase: Phase,
    input: GameInput,
    rng: u32,
    render: Vec<u8>,
    audio: Vec<u8>,
    saved_state: Vec<u8>,
    restore_state: Vec<u8>,
    render_submitted: bool,
    audio_submitted: bool,
    state_submitted: bool,
    state_loaded: bool,
    native_calls: Vec<u32>,
}

impl HostState {
    fn active(&self) -> Result<(), WasmError> {
        match self.phase {
            Phase::Init | Phase::Tick | Phase::Suspend | Phase::Resume => Ok(()),
            Phase::Idle => Err(WasmError::Trap("game host call outside lifecycle")),
        }
    }

    fn frame_active(&self) -> Result<(), WasmError> {
        match self.phase {
            Phase::Init | Phase::Tick => Ok(()),
            _ => Err(WasmError::Trap("game frame call outside init/tick")),
        }
    }

    fn reset_output(&mut self) {
        self.render.clear();
        self.audio.clear();
        self.render_submitted = false;
        self.audio_submitted = false;
    }

    fn charge_native(&mut self, index: usize, limit: u32) -> Result<(), WasmError> {
        self.active()?;
        let calls = self
            .native_calls
            .get_mut(index)
            .ok_or(WasmError::Trap("native capability registry mismatch"))?;
        if *calls >= limit {
            return Err(WasmError::Trap("native capability call budget"));
        }
        *calls += 1;
        Ok(())
    }
}

/// One single-thread-owned game instance.
///
/// Required guest exports are `game_abi_version() -> i32`, `game_init() ->
/// i32`, and `game_tick() -> i32`. A lifecycle function succeeds only when it
/// returns zero. Imports are optional, but every import must belong to the
/// `tinyarcade:core/v1` whitelist; arbitrary host capabilities are rejected
/// before instantiation. Future native modules use separate versioned import
/// namespaces and a host capability registry, without changing WASM bytes or
/// adding a private cartridge instruction format.
pub struct GameRuntime {
    instance: WasmInstance,
    host: Rc<RefCell<HostState>>,
    manifest: CartridgeManifest,
    origin: CartridgeOrigin,
    failed: bool,
}

impl GameRuntime {
    /// Validate, bind, instantiate and initialise a bundled WASM game.
    pub fn from_bytes(
        wasm: &[u8],
        vm_limits: Limits,
        game_limits: GameLimits,
        rng_seed: u32,
    ) -> Result<Self, WasmError> {
        Self::from_bytes_with_origin_and_registry(
            wasm,
            vm_limits,
            game_limits,
            rng_seed,
            CartridgeOrigin::Bundled,
            &NativeModuleRegistry::new(),
        )
    }

    /// Load a user-selected local cartridge with only the standard core ABI.
    /// This path cannot grant native-module imports or official catalog status.
    pub fn from_private_bytes(
        wasm: &[u8],
        vm_limits: Limits,
        game_limits: GameLimits,
        rng_seed: u32,
    ) -> Result<Self, WasmError> {
        Self::from_bytes_with_origin_and_registry(
            wasm,
            vm_limits,
            game_limits,
            rng_seed,
            CartridgeOrigin::PrivateUser,
            &NativeModuleRegistry::new(),
        )
    }

    /// Load exact bytes admitted by a signed reviewed catalog record.
    #[cfg(feature = "cartridge-trust")]
    pub fn from_reviewed_bytes(
        wasm: &[u8],
        entry: &crate::CatalogEntry,
        trust: &crate::CartridgeTrustStore,
        vm_limits: Limits,
        game_limits: GameLimits,
        rng_seed: u32,
    ) -> Result<Self, WasmError> {
        trust.verify(entry, wasm)?;
        Self::from_bytes_with_origin_and_registry(
            wasm,
            vm_limits,
            game_limits,
            rng_seed,
            CartridgeOrigin::OfficialReviewed,
            &NativeModuleRegistry::new(),
        )
    }

    /// Load exact reviewed bytes with app-provided, manifest-declared native
    /// capabilities. Trust verification happens before any guest or native
    /// callback can execute.
    #[cfg(feature = "cartridge-trust")]
    pub fn from_reviewed_bytes_with_registry(
        wasm: &[u8],
        entry: &crate::CatalogEntry,
        trust: &crate::CartridgeTrustStore,
        vm_limits: Limits,
        game_limits: GameLimits,
        rng_seed: u32,
        registry: &NativeModuleRegistry,
    ) -> Result<Self, WasmError> {
        trust.verify(entry, wasm)?;
        Self::from_bytes_with_origin_and_registry(
            wasm,
            vm_limits,
            game_limits,
            rng_seed,
            CartridgeOrigin::OfficialReviewed,
            registry,
        )
    }

    /// Load a game with an explicit set of app-provided native capabilities.
    pub fn from_bytes_with_registry(
        wasm: &[u8],
        vm_limits: Limits,
        game_limits: GameLimits,
        rng_seed: u32,
        registry: &NativeModuleRegistry,
    ) -> Result<Self, WasmError> {
        Self::from_bytes_with_origin_and_registry(
            wasm,
            vm_limits,
            game_limits,
            rng_seed,
            CartridgeOrigin::Bundled,
            registry,
        )
    }

    fn from_bytes_with_origin_and_registry(
        wasm: &[u8],
        vm_limits: Limits,
        game_limits: GameLimits,
        rng_seed: u32,
        origin: CartridgeOrigin,
        registry: &NativeModuleRegistry,
    ) -> Result<Self, WasmError> {
        if wasm.is_empty() || wasm.len() > MAX_CARTRIDGE_BYTES {
            return Err(WasmError::Decode("game cartridge size limit"));
        }
        let manifest = CartridgeManifest::from_wasm(wasm)?;
        if manifest.abi_version != GAME_ABI_VERSION as u32 {
            return Err(WasmError::Trap("unsupported game ABI version"));
        }
        let mut module = WasmModule::from_bytes_with(wasm, vm_limits)?;
        validate_imports(&module, &manifest, registry)?;
        for export in [
            "game_abi_version",
            "game_init",
            "game_tick",
            "game_suspend",
            "game_resume",
        ] {
            if module.export_i32_arity(export) != Some((0, 1)) {
                return Err(WasmError::Trap("invalid game lifecycle export"));
            }
        }
        let mut native_calls = Vec::new();
        native_calls
            .try_reserve_exact(registry.functions.len())
            .map_err(|_| WasmError::Trap("native capability allocation"))?;
        native_calls.resize(registry.functions.len(), 0);
        let host = Rc::new(RefCell::new(HostState {
            limits: game_limits,
            phase: Phase::Idle,
            input: GameInput::default(),
            rng: rng_seed,
            render: Vec::new(),
            audio: Vec::new(),
            saved_state: Vec::new(),
            restore_state: Vec::new(),
            render_submitted: false,
            audio_submitted: false,
            state_submitted: false,
            state_loaded: false,
            native_calls,
        }));
        bind_imports(&mut module, &host, registry)?;
        let instance = module.instantiate()?;
        let mut runtime = Self {
            instance,
            host,
            manifest,
            origin,
            failed: false,
        };

        let version = runtime.instance.invoke_by_name("game_abi_version", &[])?;
        if !matches!(version.as_slice(), [Val::I32(GAME_ABI_VERSION)]) {
            return Err(WasmError::Trap("unsupported game ABI version"));
        }

        runtime.enter(Phase::Init, GameInput::default());
        let init = runtime.instance.invoke_by_name("game_init", &[]);
        runtime.leave();
        require_success(init?, "game_init failed")?;
        runtime.host.borrow_mut().reset_output();
        Ok(runtime)
    }

    /// Drive one deterministic frame and take ownership of its command bytes.
    pub fn tick(&mut self, input: GameInput) -> Result<GameFrame, WasmError> {
        self.ensure_live()?;
        self.enter(Phase::Tick, input);
        let tick = self.instance.invoke_by_name("game_tick", &[]);
        self.leave();
        self.accept_lifecycle(tick, "game_tick failed")?;
        let mut host = self.host.borrow_mut();
        Ok(GameFrame {
            render: mem::take(&mut host.render),
            audio: mem::take(&mut host.audio),
        })
    }

    /// Suspend the guest and return a portable, cartridge-bound snapshot.
    pub fn suspend(&mut self) -> Result<Vec<u8>, WasmError> {
        self.ensure_live()?;
        {
            let mut host = self.host.borrow_mut();
            host.saved_state.clear();
            host.state_submitted = false;
        }
        self.enter(Phase::Suspend, GameInput::default());
        let suspended = self.instance.invoke_by_name("game_suspend", &[]);
        self.leave();
        self.accept_lifecycle(suspended, "game_suspend failed")?;
        let (guest, rng) = {
            let mut host = self.host.borrow_mut();
            if !host.state_submitted {
                self.failed = true;
                return Err(WasmError::Trap("game did not submit state"));
            }
            (mem::take(&mut host.saved_state), host.rng)
        };
        encode_snapshot(&self.manifest, rng, &guest)
    }

    /// Restore a snapshot made by the same game and state-schema version.
    pub fn resume(&mut self, snapshot: &[u8]) -> Result<(), WasmError> {
        self.ensure_live()?;
        let (rng, guest) = decode_snapshot(
            snapshot,
            &self.manifest,
            self.host.borrow().limits.max_state_bytes,
        )?;
        {
            let mut host = self.host.borrow_mut();
            host.restore_state.clear();
            host.restore_state
                .try_reserve_exact(guest.len())
                .map_err(|_| WasmError::Trap("game state allocation"))?;
            host.restore_state.extend_from_slice(guest);
            host.state_loaded = false;
            host.rng = rng;
        }
        self.enter(Phase::Resume, GameInput::default());
        let resumed = self.instance.invoke_by_name("game_resume", &[]);
        self.leave();
        self.accept_lifecycle(resumed, "game_resume failed")?;
        if !self.host.borrow().state_loaded {
            self.failed = true;
            return Err(WasmError::Trap("game did not load state"));
        }
        self.host.borrow_mut().restore_state.clear();
        Ok(())
    }

    pub fn manifest(&self) -> &CartridgeManifest {
        &self.manifest
    }

    pub fn origin(&self) -> CartridgeOrigin {
        self.origin
    }

    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// Permanently reject further lifecycle execution after a host boundary
    /// catches a panic with potentially partial guest/host mutation.
    #[cfg(feature = "ios-c-api")]
    pub(crate) fn latch_host_panic(&mut self) {
        self.failed = true;
        self.host.borrow_mut().phase = Phase::Idle;
    }

    fn ensure_live(&self) -> Result<(), WasmError> {
        if self.failed {
            Err(WasmError::Trap("game instance failed"))
        } else {
            Ok(())
        }
    }

    fn accept_lifecycle(
        &mut self,
        result: Result<Vec<Val>, WasmError>,
        message: &'static str,
    ) -> Result<(), WasmError> {
        let accepted = result.and_then(|values| require_success(values, message));
        if accepted.is_err() {
            self.failed = true;
        }
        accepted
    }

    fn enter(&mut self, phase: Phase, input: GameInput) {
        let mut host = self.host.borrow_mut();
        host.phase = phase;
        host.input = input;
        host.native_calls.fill(0);
        host.reset_output();
    }

    fn leave(&mut self) {
        self.host.borrow_mut().phase = Phase::Idle;
    }
}

fn require_success(values: Vec<Val>, message: &'static str) -> Result<(), WasmError> {
    if matches!(values.as_slice(), [Val::I32(0)]) {
        Ok(())
    } else {
        Err(WasmError::Trap(message))
    }
}

fn validate_imports(
    module: &WasmModule,
    manifest: &CartridgeManifest,
    registry: &NativeModuleRegistry,
) -> Result<(), WasmError> {
    let mut seen = [false; 8];
    let mut actual_capabilities: Vec<&str> = Vec::new();
    actual_capabilities
        .try_reserve_exact(module.imports().len())
        .map_err(|_| WasmError::Trap("game capability allocation"))?;
    for (index, import) in module.imports().iter().enumerate() {
        if !import.i32_only
            || module.imports()[..index]
                .iter()
                .any(|prior| prior.module == import.module && prior.field == import.field)
        {
            return Err(WasmError::Trap("game import is not allowed"));
        }
        if import.module != ABI_MODULE {
            if !actual_capabilities.contains(&import.module.as_str()) {
                actual_capabilities.push(import.module.as_str());
            }
            let native = registry
                .find(&import.module, &import.field)
                .ok_or(WasmError::Trap("game import is not allowed"))?;
            if import.n_params != native.n_params || import.n_results != native.n_results {
                return Err(WasmError::Trap("invalid game import signature"));
            }
            continue;
        }
        let (slot, params) = match import.field.as_str() {
            "input_bits" => (0, 0),
            "clock_ms" => (1, 0),
            "random_u32" => (2, 0),
            "submit_render" => (3, 2),
            "submit_audio" => (4, 2),
            "save_state" => (5, 2),
            "load_state" => (6, 2),
            "indexed2d_version" => (7, 0),
            _ => return Err(WasmError::Trap("game import is not allowed")),
        };
        if seen[slot] || import.n_params != params || import.n_results != 1 {
            return Err(WasmError::Trap("invalid game import signature"));
        }
        seen[slot] = true;
    }
    actual_capabilities.sort();
    if actual_capabilities.len() != manifest.capabilities.len()
        || actual_capabilities
            .iter()
            .zip(&manifest.capabilities)
            .any(|(actual, declared)| *actual != declared)
    {
        return Err(WasmError::Trap("manifest capability mismatch"));
    }
    Ok(())
}

fn bind_imports(
    module: &mut WasmModule,
    host: &Rc<RefCell<HostState>>,
    registry: &NativeModuleRegistry,
) -> Result<(), WasmError> {
    let indexed2d_enabled = module
        .imports()
        .iter()
        .any(|import| import.module == ABI_MODULE && import.field == "indexed2d_version");
    let fields: Vec<_> = module
        .imports()
        .iter()
        .map(|import| (import.module.clone(), import.field.clone()))
        .collect();
    for (namespace, field) in fields {
        if namespace != ABI_MODULE {
            let (index, native) = registry
                .find_with_index(&namespace, &field)
                .ok_or(WasmError::Trap("game import is not allowed"))?;
            let callback = native.callback.clone();
            let max_calls = native.max_calls_per_lifecycle;
            let lifecycle = host.clone();
            module.bind_import(&namespace, &field, move |args, memory| {
                lifecycle.borrow_mut().charge_native(index, max_calls)?;
                callback(args, memory)
            })?;
            continue;
        }
        let shared = host.clone();
        match field.as_str() {
            "input_bits" => module.bind_import(ABI_MODULE, &field, move |_, _| {
                let state = shared.borrow();
                state.frame_active()?;
                Ok(alloc::vec![state.input.buttons as i32])
            })?,
            "clock_ms" => module.bind_import(ABI_MODULE, &field, move |_, _| {
                let state = shared.borrow();
                state.frame_active()?;
                Ok(alloc::vec![state.input.clock_ms as i32])
            })?,
            "random_u32" => module.bind_import(ABI_MODULE, &field, move |_, _| {
                let mut state = shared.borrow_mut();
                state.frame_active()?;
                let mut value = state.rng;
                value ^= value << 13;
                value ^= value >> 17;
                value ^= value << 5;
                state.rng = value;
                Ok(alloc::vec![value as i32])
            })?,
            "submit_render" => bind_submit(module, &field, shared, true, indexed2d_enabled)?,
            "submit_audio" => bind_submit(module, &field, shared, false, false)?,
            "indexed2d_version" => {
                module.bind_import(ABI_MODULE, &field, move |_, _| Ok(alloc::vec![1]))?
            }
            "save_state" => module.bind_import(ABI_MODULE, &field, move |args, memory| {
                let mut state = shared.borrow_mut();
                if state.phase != Phase::Suspend {
                    return Err(WasmError::Trap("game save outside suspend"));
                }
                let bytes = memory_range(args, memory)?;
                if state.state_submitted || bytes.len() > state.limits.max_state_bytes {
                    return Err(WasmError::Trap("game state budget"));
                }
                state
                    .saved_state
                    .try_reserve_exact(bytes.len())
                    .map_err(|_| WasmError::Trap("game state allocation"))?;
                state.saved_state.extend_from_slice(bytes);
                state.state_submitted = true;
                Ok(alloc::vec![0])
            })?,
            "load_state" => module.bind_import(ABI_MODULE, &field, move |args, memory| {
                let mut state = shared.borrow_mut();
                if state.phase != Phase::Resume || state.state_loaded {
                    return Err(WasmError::Trap("game load outside resume"));
                }
                let ptr = nonnegative(args[0])?;
                let capacity = nonnegative(args[1])?;
                let len = state.restore_state.len();
                let end = ptr
                    .checked_add(len)
                    .filter(|&end| end <= memory.len() && len <= capacity)
                    .ok_or(WasmError::Trap("game restore capacity"))?;
                memory[ptr..end].copy_from_slice(&state.restore_state);
                state.state_loaded = true;
                Ok(alloc::vec![len as i32])
            })?,
            _ => return Err(WasmError::Trap("game import is not allowed")),
        }
    }
    Ok(())
}

fn bind_submit(
    module: &mut WasmModule,
    field: &str,
    host: Rc<RefCell<HostState>>,
    render: bool,
    indexed2d_enabled: bool,
) -> Result<(), WasmError> {
    module.bind_import(ABI_MODULE, field, move |args, memory| {
        let mut state = host.borrow_mut();
        state.frame_active()?;
        let bytes = memory_range(args, memory)?;
        if render {
            if bytes.starts_with(INDEXED2D_MAGIC) && !indexed2d_enabled {
                return Err(WasmError::Trap("indexed2d capability not declared"));
            }
            if state.render_submitted || bytes.len() > state.limits.max_render_bytes {
                return Err(WasmError::Trap("game output budget"));
            }
            state
                .render
                .try_reserve_exact(bytes.len())
                .map_err(|_| WasmError::Trap("game output allocation"))?;
            state.render.extend_from_slice(bytes);
            state.render_submitted = true;
        } else {
            if state.audio_submitted || bytes.len() > state.limits.max_audio_bytes {
                return Err(WasmError::Trap("game output budget"));
            }
            state
                .audio
                .try_reserve_exact(bytes.len())
                .map_err(|_| WasmError::Trap("game output allocation"))?;
            state.audio.extend_from_slice(bytes);
            state.audio_submitted = true;
        }
        Ok(alloc::vec![0])
    })
}

fn nonnegative(value: i32) -> Result<usize, WasmError> {
    usize::try_from(value).map_err(|_| WasmError::Trap("game output bounds"))
}

fn memory_range<'a>(args: &[i32], memory: &'a [u8]) -> Result<&'a [u8], WasmError> {
    let ptr = nonnegative(args[0])?;
    let len = nonnegative(args[1])?;
    let end = ptr
        .checked_add(len)
        .filter(|&end| end <= memory.len())
        .ok_or(WasmError::Trap("game output bounds"))?;
    Ok(&memory[ptr..end])
}

fn encode_snapshot(
    manifest: &CartridgeManifest,
    rng: u32,
    guest: &[u8],
) -> Result<Vec<u8>, WasmError> {
    let id_len =
        u16::try_from(manifest.game_id.len()).map_err(|_| WasmError::Trap("snapshot identity"))?;
    let guest_len = u32::try_from(guest.len()).map_err(|_| WasmError::Trap("snapshot size"))?;
    let total = 4usize
        .checked_add(4 + 4 + 2)
        .and_then(|size| size.checked_add(manifest.game_id.len()))
        .and_then(|size| size.checked_add(4 + 4))
        .and_then(|size| size.checked_add(guest.len()))
        .ok_or(WasmError::Trap("snapshot size"))?;
    let mut snapshot = Vec::new();
    snapshot
        .try_reserve_exact(total)
        .map_err(|_| WasmError::Trap("snapshot allocation"))?;
    snapshot.extend_from_slice(SNAPSHOT_MAGIC);
    snapshot.extend_from_slice(&manifest.abi_version.to_le_bytes());
    snapshot.extend_from_slice(&manifest.state_version.to_le_bytes());
    snapshot.extend_from_slice(&id_len.to_le_bytes());
    snapshot.extend_from_slice(manifest.game_id.as_bytes());
    snapshot.extend_from_slice(&rng.to_le_bytes());
    snapshot.extend_from_slice(&guest_len.to_le_bytes());
    snapshot.extend_from_slice(guest);
    Ok(snapshot)
}

fn decode_snapshot<'a>(
    snapshot: &'a [u8],
    manifest: &CartridgeManifest,
    max_state_bytes: usize,
) -> Result<(u32, &'a [u8]), WasmError> {
    let mut cursor = 0;
    let magic = snapshot_take(snapshot, &mut cursor, 4)?;
    let abi = snapshot_u32(snapshot, &mut cursor)?;
    let state = snapshot_u32(snapshot, &mut cursor)?;
    let id_len = snapshot_u16(snapshot, &mut cursor)? as usize;
    let id = snapshot_take(snapshot, &mut cursor, id_len)?;
    let rng = snapshot_u32(snapshot, &mut cursor)?;
    let guest_len = snapshot_u32(snapshot, &mut cursor)? as usize;
    let guest = snapshot_take(snapshot, &mut cursor, guest_len)?;
    if magic != SNAPSHOT_MAGIC
        || abi != manifest.abi_version
        || state != manifest.state_version
        || id != manifest.game_id.as_bytes()
        || guest_len > max_state_bytes
        || cursor != snapshot.len()
    {
        return Err(WasmError::Trap("incompatible game snapshot"));
    }
    Ok((rng, guest))
}

fn snapshot_take<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], WasmError> {
    let end = cursor
        .checked_add(len)
        .filter(|&end| end <= bytes.len())
        .ok_or(WasmError::Trap("truncated game snapshot"))?;
    let value = &bytes[*cursor..end];
    *cursor = end;
    Ok(value)
}

fn snapshot_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, WasmError> {
    let raw = snapshot_take(bytes, cursor, 2)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn snapshot_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, WasmError> {
    let raw = snapshot_take(bytes, cursor, 4)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}
