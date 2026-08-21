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
pub const KNOWN_BUTTON_MASK: u32 = (1 << 9) - 1;
const ABI_MODULE: &str = "tinyarcade:core/v1";
const SNAPSHOT_MAGIC: &[u8; 4] = b"TGS1";
type NativeImpl = dyn Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError>;
type NativeInPlaceImpl = dyn Fn(&[i32], &mut [i32], &mut [u8]) -> Result<(), WasmError>;

#[derive(Clone)]
enum NativeCallback {
    Returning(Rc<NativeImpl>),
    InPlace(Rc<NativeInPlaceImpl>),
}

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
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct GameInput {
    /// ABI v1 buttons packed using the stable [`button`] bit assignments.
    pub buttons: u32,
    /// Host-provided monotonic game time. Wall-clock time is never exposed.
    pub clock_ms: u32,
}

/// Static compatibility description of one standard WASM cartridge.
///
/// Inspection parses and validates the manifest, function imports and
/// lifecycle exports without instantiating the module, running its start
/// function or calling guest code. Native imports are described but do not
/// need to be available in a host registry until a runtime is opened.
pub struct CartridgeDescriptor {
    pub manifest: CartridgeManifest,
    pub imports: Vec<crate::ImportDesc>,
}

impl CartridgeDescriptor {
    pub fn inspect(wasm: &[u8], vm_limits: Limits) -> Result<Self, WasmError> {
        let (manifest, module) = parse_cartridge(wasm, vm_limits)?;
        Ok(Self {
            manifest,
            imports: module.imports().to_vec(),
        })
    }
}

/// Bounded command streams emitted by one successful game tick.
#[derive(Default)]
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
    callback: NativeCallback,
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

    /// Describe this exact app-compiled registry as a callback-free,
    /// converter-facing host profile.
    pub fn host_profile(
        &self,
        vm_limits: Limits,
        game_limits: GameLimits,
    ) -> Result<crate::HostProfileV1, WasmError> {
        let mut profile = crate::HostProfileV1::new(vm_limits, game_limits)?;
        for function in &self.functions {
            profile.add_native_function(
                &function.module,
                &function.field,
                function.n_params,
                function.n_results,
                function.max_calls_per_lifecycle,
            )?;
        }
        Ok(profile)
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
        self.register_callback(
            module,
            field,
            n_params,
            n_results,
            max_calls_per_lifecycle,
            NativeCallback::Returning(Rc::new(callback)),
        )
    }

    /// Register a native callback without callback-owned heap staging. The
    /// runtime supplies an exact-size result slice to write in place.
    pub fn register_in_place<F>(
        &mut self,
        module: &str,
        field: &str,
        n_params: usize,
        n_results: usize,
        callback: F,
    ) -> Result<(), WasmError>
    where
        F: Fn(&[i32], &mut [i32], &mut [u8]) -> Result<(), WasmError> + 'static,
    {
        self.register_in_place_with_call_limit(module, field, n_params, n_results, 1, callback)
    }

    /// Register an in-place native callback with a per-lifecycle dispatch
    /// ceiling. Parameter and result arities remain capped at 16.
    pub fn register_in_place_with_call_limit<F>(
        &mut self,
        module: &str,
        field: &str,
        n_params: usize,
        n_results: usize,
        max_calls_per_lifecycle: u32,
        callback: F,
    ) -> Result<(), WasmError>
    where
        F: Fn(&[i32], &mut [i32], &mut [u8]) -> Result<(), WasmError> + 'static,
    {
        self.register_callback(
            module,
            field,
            n_params,
            n_results,
            max_calls_per_lifecycle,
            NativeCallback::InPlace(Rc::new(callback)),
        )
    }

    fn register_callback(
        &mut self,
        module: &str,
        field: &str,
        n_params: usize,
        n_results: usize,
        max_calls_per_lifecycle: u32,
        callback: NativeCallback,
    ) -> Result<(), WasmError> {
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
            callback,
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
    #[cfg(feature = "replay")]
    cartridge_sha256: [u8; 32],
    origin: CartridgeOrigin,
    failed: bool,
    last_clock_ms: Option<u32>,
    last_execution_stats: ExecutionStats,
}

/// Lifecycle associated with one deterministic execution measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum GameLifecycle {
    Init = 1,
    Tick = 2,
    Suspend = 3,
    Resume = 4,
}

/// Host-observable resource use of the last completed lifecycle attempt.
///
/// These values are deterministic properties of the guest and host ABI. Wall
/// time and process memory remain platform measurements and are deliberately
/// not mixed into this record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionStats {
    pub lifecycle: GameLifecycle,
    pub wasm_steps: u64,
    pub peak_call_depth: usize,
    pub peak_activation_slots: usize,
    pub memory_pages: usize,
    pub table_elements: usize,
    pub native_calls: u32,
    pub render_bytes: usize,
    pub audio_bytes: usize,
    pub state_bytes: usize,
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
        let (manifest, mut module) = parse_cartridge(wasm, vm_limits)?;
        if module.memory_count() != 1 {
            return Err(WasmError::Decode(
                "game cartridge requires exactly one memory",
            ));
        }
        if !module.global_imports().is_empty() {
            return Err(WasmError::Decode(
                "game cartridge does not support global imports",
            ));
        }
        validate_native_availability(&module, registry)?;
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
            #[cfg(feature = "replay")]
            cartridge_sha256: crate::cartridge_sha256(wasm),
            origin,
            failed: false,
            last_clock_ms: None,
            last_execution_stats: ExecutionStats {
                lifecycle: GameLifecycle::Init,
                wasm_steps: 0,
                peak_call_depth: 0,
                peak_activation_slots: 0,
                memory_pages: 0,
                table_elements: 0,
                native_calls: 0,
                render_bytes: 0,
                audio_bytes: 0,
                state_bytes: 0,
            },
        };

        let version = runtime.instance.invoke_by_name("game_abi_version", &[])?;
        if !matches!(version.as_slice(), [Val::I32(GAME_ABI_VERSION)]) {
            return Err(WasmError::Trap("unsupported game ABI version"));
        }

        runtime.enter(Phase::Init, GameInput::default());
        let init = runtime.instance.invoke_by_name("game_init", &[]);
        runtime.leave();
        runtime.capture_execution_stats(GameLifecycle::Init);
        require_success(init?, "game_init failed")?;
        runtime.host.borrow_mut().reset_output();
        Ok(runtime)
    }

    #[cfg(feature = "replay")]
    pub(crate) fn cartridge_sha256(&self) -> [u8; 32] {
        self.cartridge_sha256
    }

    /// Drive one deterministic frame and take ownership of its command bytes.
    pub fn tick(&mut self, input: GameInput) -> Result<GameFrame, WasmError> {
        let mut frame = GameFrame::default();
        self.tick_into(input, &mut frame)?;
        Ok(frame)
    }

    /// Drive one deterministic frame, reusing the output storage owned by
    /// `frame` when its capacity is sufficient.
    ///
    /// The prior contents are cleared before input validation, so an error
    /// never leaves a stale completed frame visible to the embedding. Buffer
    /// capacity remains available for a later runtime or successful tick.
    pub fn tick_into(&mut self, input: GameInput, frame: &mut GameFrame) -> Result<(), WasmError> {
        frame.render.clear();
        frame.audio.clear();
        self.ensure_live()?;
        if input.buttons & !KNOWN_BUTTON_MASK != 0
            || self
                .last_clock_ms
                .is_some_and(|previous| input.clock_ms < previous)
        {
            return Err(WasmError::Trap("invalid game input"));
        }
        self.enter(Phase::Tick, input);
        {
            let mut host = self.host.borrow_mut();
            if frame.render.capacity() > host.render.capacity() {
                mem::swap(&mut frame.render, &mut host.render);
            }
            if frame.audio.capacity() > host.audio.capacity() {
                mem::swap(&mut frame.audio, &mut host.audio);
            }
        }
        let tick = self.instance.invoke_by_name("game_tick", &[]);
        self.leave();
        self.capture_execution_stats(GameLifecycle::Tick);
        let accepted = self.accept_lifecycle(tick, "game_tick failed");
        {
            let mut host = self.host.borrow_mut();
            mem::swap(&mut frame.render, &mut host.render);
            mem::swap(&mut frame.audio, &mut host.audio);
        }
        if let Err(error) = accepted {
            frame.render.clear();
            frame.audio.clear();
            return Err(error);
        }
        self.last_clock_ms = Some(input.clock_ms);
        Ok(())
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
        self.capture_execution_stats(GameLifecycle::Suspend);
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
        self.capture_execution_stats(GameLifecycle::Resume);
        self.accept_lifecycle(resumed, "game_resume failed")?;
        if !self.host.borrow().state_loaded {
            self.failed = true;
            return Err(WasmError::Trap("game did not load state"));
        }
        self.host.borrow_mut().restore_state.clear();
        self.last_clock_ms = None;
        Ok(())
    }

    pub fn manifest(&self) -> &CartridgeManifest {
        &self.manifest
    }

    pub fn origin(&self) -> CartridgeOrigin {
        self.origin
    }

    /// Resource use of the last completed init/tick/suspend/resume attempt.
    pub fn last_execution_stats(&self) -> ExecutionStats {
        self.last_execution_stats
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

    fn capture_execution_stats(&mut self, lifecycle: GameLifecycle) {
        let host = self.host.borrow();
        self.last_execution_stats = ExecutionStats {
            lifecycle,
            wasm_steps: self.instance.last_steps(),
            peak_call_depth: self.instance.last_peak_call_depth(),
            peak_activation_slots: self.instance.last_peak_activation_slots(),
            memory_pages: self.instance.memory_pages(),
            table_elements: self.instance.table_elements(),
            native_calls: host.native_calls.iter().copied().sum(),
            render_bytes: host.render.len(),
            audio_bytes: host.audio.len(),
            state_bytes: match lifecycle {
                GameLifecycle::Suspend => host.saved_state.len(),
                GameLifecycle::Resume => host.restore_state.len(),
                GameLifecycle::Init | GameLifecycle::Tick => 0,
            },
        };
    }
}

fn require_success(values: Vec<Val>, message: &'static str) -> Result<(), WasmError> {
    if matches!(values.as_slice(), [Val::I32(0)]) {
        Ok(())
    } else {
        Err(WasmError::Trap(message))
    }
}

fn parse_cartridge(
    wasm: &[u8],
    vm_limits: Limits,
) -> Result<(CartridgeManifest, WasmModule), WasmError> {
    if wasm.is_empty() || wasm.len() > MAX_CARTRIDGE_BYTES {
        return Err(WasmError::Decode("game cartridge size limit"));
    }
    let manifest = CartridgeManifest::from_wasm(wasm)?;
    if manifest.abi_version != GAME_ABI_VERSION as u32 {
        return Err(WasmError::Trap("unsupported game ABI version"));
    }
    let module = WasmModule::from_bytes_with(wasm, vm_limits)?;
    validate_import_contract(&module, &manifest)?;
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
    Ok((manifest, module))
}

fn validate_import_contract(
    module: &WasmModule,
    manifest: &CartridgeManifest,
) -> Result<(), WasmError> {
    let mut seen = [false; 8];
    let mut native_function_count = 0usize;
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
            native_function_count = native_function_count
                .checked_add(1)
                .ok_or(WasmError::Trap("game capability allocation"))?;
            if !valid_native_namespace(&import.module)
                || !valid_native_field(&import.field)
                || import.n_params > MAX_NATIVE_ARITY
                || import.n_results > MAX_NATIVE_ARITY
                || native_function_count > MAX_NATIVE_FUNCTIONS
            {
                return Err(WasmError::Trap("invalid game import signature"));
            }
            if !actual_capabilities.contains(&import.module.as_str()) {
                actual_capabilities.push(import.module.as_str());
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

fn validate_native_availability(
    module: &WasmModule,
    registry: &NativeModuleRegistry,
) -> Result<(), WasmError> {
    for import in module
        .imports()
        .iter()
        .filter(|import| import.module != ABI_MODULE)
    {
        let native = registry
            .find(&import.module, &import.field)
            .ok_or(WasmError::Trap("game import is not allowed"))?;
        if import.n_params != native.n_params || import.n_results != native.n_results {
            return Err(WasmError::Trap("invalid game import signature"));
        }
    }
    Ok(())
}

fn bind_imports(
    module: &mut WasmModule,
    host: &Rc<RefCell<HostState>>,
    registry: &NativeModuleRegistry,
) -> Result<(), WasmError> {
    enum ImportPlan {
        Native {
            index: usize,
            max_calls: u32,
            callback: NativeCallback,
        },
        InputBits,
        ClockMs,
        RandomU32,
        SubmitRender,
        SubmitAudio,
        Indexed2dVersion,
        SaveState,
        LoadState,
    }

    let indexed2d_enabled = module
        .imports()
        .iter()
        .any(|import| import.module == ABI_MODULE && import.field == "indexed2d_version");
    for position in 0..module.imports().len() {
        let plan = {
            let import = &module.imports()[position];
            if import.module != ABI_MODULE {
                let (index, native) = registry
                    .find_with_index(&import.module, &import.field)
                    .ok_or(WasmError::Trap("game import is not allowed"))?;
                ImportPlan::Native {
                    index,
                    max_calls: native.max_calls_per_lifecycle,
                    callback: native.callback.clone(),
                }
            } else {
                match import.field.as_str() {
                    "input_bits" => ImportPlan::InputBits,
                    "clock_ms" => ImportPlan::ClockMs,
                    "random_u32" => ImportPlan::RandomU32,
                    "submit_render" => ImportPlan::SubmitRender,
                    "submit_audio" => ImportPlan::SubmitAudio,
                    "indexed2d_version" => ImportPlan::Indexed2dVersion,
                    "save_state" => ImportPlan::SaveState,
                    "load_state" => ImportPlan::LoadState,
                    _ => return Err(WasmError::Trap("game import is not allowed")),
                }
            }
        };
        let shared = host.clone();
        match plan {
            ImportPlan::Native {
                index,
                max_calls,
                callback,
            } => module.bind_import_at_bounded(position, move |args, results, memory| {
                shared.borrow_mut().charge_native(index, max_calls)?;
                match &callback {
                    NativeCallback::Returning(callback) => {
                        let returned = callback(args, memory)?;
                        if returned.len() != results.len() {
                            return Err(WasmError::Trap("native capability result arity"));
                        }
                        results.copy_from_slice(&returned);
                        Ok(())
                    }
                    NativeCallback::InPlace(callback) => callback(args, results, memory),
                }
            })?,
            ImportPlan::InputBits => {
                module.bind_import_at_bounded(position, move |_, results, _| {
                    let state = shared.borrow();
                    state.frame_active()?;
                    results[0] = state.input.buttons as i32;
                    Ok(())
                })?
            }
            ImportPlan::ClockMs => {
                module.bind_import_at_bounded(position, move |_, results, _| {
                    let state = shared.borrow();
                    state.frame_active()?;
                    results[0] = state.input.clock_ms as i32;
                    Ok(())
                })?
            }
            ImportPlan::RandomU32 => {
                module.bind_import_at_bounded(position, move |_, results, _| {
                    let mut state = shared.borrow_mut();
                    state.frame_active()?;
                    let mut value = state.rng;
                    value ^= value << 13;
                    value ^= value >> 17;
                    value ^= value << 5;
                    state.rng = value;
                    results[0] = value as i32;
                    Ok(())
                })?
            }
            ImportPlan::SubmitRender => {
                bind_submit(module, position, shared, true, indexed2d_enabled)?
            }
            ImportPlan::SubmitAudio => bind_submit(module, position, shared, false, false)?,
            ImportPlan::Indexed2dVersion => {
                module.bind_import_at_bounded(position, move |_, results, _| {
                    results[0] = 1;
                    Ok(())
                })?
            }
            ImportPlan::SaveState => {
                module.bind_import_at_bounded(position, move |args, results, memory| {
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
                    results[0] = 0;
                    Ok(())
                })?
            }
            ImportPlan::LoadState => {
                module.bind_import_at_bounded(position, move |args, results, memory| {
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
                    results[0] = len as i32;
                    Ok(())
                })?
            }
        }
    }
    Ok(())
}

fn bind_submit(
    module: &mut WasmModule,
    position: usize,
    host: Rc<RefCell<HostState>>,
    render: bool,
    indexed2d_enabled: bool,
) -> Result<(), WasmError> {
    module.bind_import_at_bounded(position, move |args, results, memory| {
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
        results[0] = 0;
        Ok(())
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
