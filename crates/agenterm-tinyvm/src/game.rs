//! Versioned, bounded native host contract for tinyvm games.

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use core::mem;

use crate::{Limits, Val, WasmError, WasmInstance, WasmModule};

/// Guest/host contract implemented by this module.
pub const GAME_ABI_VERSION: i32 = 1;
const ABI_MODULE: &str = "tinyarcade:core/v1";
type NativeImpl = dyn Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError>;

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
}

impl Default for GameLimits {
    fn default() -> Self {
        Self {
            max_render_bytes: 64 * 1024,
            max_audio_bytes: 16 * 1024,
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
        let versioned = module.rsplit('/').next().is_some_and(|part| {
            part.strip_prefix('v').is_some_and(|version| {
                !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
            })
        });
        if module == ABI_MODULE
            || !module.contains(':')
            || !versioned
            || field.is_empty()
            || self.find(module, field).is_some()
        {
            return Err(WasmError::Trap("invalid native module registration"));
        }
        self.functions.push(NativeFunction {
            module: module.to_string(),
            field: field.to_string(),
            n_params,
            n_results,
            callback: Rc::new(callback),
        });
        Ok(())
    }

    fn find(&self, module: &str, field: &str) -> Option<&NativeFunction> {
        self.functions
            .iter()
            .find(|function| function.module == module && function.field == field)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Init,
    Tick,
}

struct HostState {
    limits: GameLimits,
    phase: Phase,
    input: GameInput,
    rng: u32,
    render: Vec<u8>,
    audio: Vec<u8>,
    render_submitted: bool,
    audio_submitted: bool,
}

impl HostState {
    fn active(&self) -> Result<(), WasmError> {
        match self.phase {
            Phase::Init | Phase::Tick => Ok(()),
            Phase::Idle => Err(WasmError::Trap("game host call outside lifecycle")),
        }
    }

    fn reset_output(&mut self) {
        self.render.clear();
        self.audio.clear();
        self.render_submitted = false;
        self.audio_submitted = false;
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
}

impl GameRuntime {
    /// Validate, bind, instantiate and initialise a reviewed WASM game.
    pub fn from_bytes(
        wasm: &[u8],
        vm_limits: Limits,
        game_limits: GameLimits,
        rng_seed: u32,
    ) -> Result<Self, WasmError> {
        Self::from_bytes_with_registry(
            wasm,
            vm_limits,
            game_limits,
            rng_seed,
            &NativeModuleRegistry::new(),
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
        let mut module = WasmModule::from_bytes_with(wasm, vm_limits)?;
        validate_imports(&module, registry)?;
        let host = Rc::new(RefCell::new(HostState {
            limits: game_limits,
            phase: Phase::Idle,
            input: GameInput::default(),
            rng: rng_seed,
            render: Vec::new(),
            audio: Vec::new(),
            render_submitted: false,
            audio_submitted: false,
        }));
        bind_imports(&mut module, &host, registry)?;
        let instance = module.instantiate()?;
        let mut runtime = Self { instance, host };

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
        self.enter(Phase::Tick, input);
        let tick = self.instance.invoke_by_name("game_tick", &[]);
        self.leave();
        require_success(tick?, "game_tick failed")?;
        let mut host = self.host.borrow_mut();
        Ok(GameFrame {
            render: mem::take(&mut host.render),
            audio: mem::take(&mut host.audio),
        })
    }

    fn enter(&mut self, phase: Phase, input: GameInput) {
        let mut host = self.host.borrow_mut();
        host.phase = phase;
        host.input = input;
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

fn validate_imports(module: &WasmModule, registry: &NativeModuleRegistry) -> Result<(), WasmError> {
    let mut seen = [false; 5];
    for (index, import) in module.imports().iter().enumerate() {
        if !import.i32_only
            || module.imports()[..index]
                .iter()
                .any(|prior| prior.module == import.module && prior.field == import.field)
        {
            return Err(WasmError::Trap("game import is not allowed"));
        }
        if import.module != ABI_MODULE {
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
            _ => return Err(WasmError::Trap("game import is not allowed")),
        };
        if seen[slot] || import.n_params != params || import.n_results != 1 {
            return Err(WasmError::Trap("invalid game import signature"));
        }
        seen[slot] = true;
    }
    Ok(())
}

fn bind_imports(
    module: &mut WasmModule,
    host: &Rc<RefCell<HostState>>,
    registry: &NativeModuleRegistry,
) -> Result<(), WasmError> {
    let fields: Vec<_> = module
        .imports()
        .iter()
        .map(|import| (import.module.clone(), import.field.clone()))
        .collect();
    for (namespace, field) in fields {
        if namespace != ABI_MODULE {
            let callback = registry
                .find(&namespace, &field)
                .ok_or(WasmError::Trap("game import is not allowed"))?
                .callback
                .clone();
            let lifecycle = host.clone();
            module.bind_import(&namespace, &field, move |args, memory| {
                lifecycle.borrow().active()?;
                callback(args, memory)
            })?;
            continue;
        }
        let shared = host.clone();
        match field.as_str() {
            "input_bits" => module.bind_import(ABI_MODULE, &field, move |_, _| {
                let state = shared.borrow();
                state.active()?;
                Ok(alloc::vec![state.input.buttons as i32])
            })?,
            "clock_ms" => module.bind_import(ABI_MODULE, &field, move |_, _| {
                let state = shared.borrow();
                state.active()?;
                Ok(alloc::vec![state.input.clock_ms as i32])
            })?,
            "random_u32" => module.bind_import(ABI_MODULE, &field, move |_, _| {
                let mut state = shared.borrow_mut();
                state.active()?;
                let mut value = state.rng;
                value ^= value << 13;
                value ^= value >> 17;
                value ^= value << 5;
                state.rng = value;
                Ok(alloc::vec![value as i32])
            })?,
            "submit_render" => bind_submit(module, &field, shared, true)?,
            "submit_audio" => bind_submit(module, &field, shared, false)?,
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
) -> Result<(), WasmError> {
    module.bind_import(ABI_MODULE, field, move |args, memory| {
        let mut state = host.borrow_mut();
        state.active()?;
        let ptr = usize::try_from(args[0]).map_err(|_| WasmError::Trap("game output bounds"))?;
        let len = usize::try_from(args[1]).map_err(|_| WasmError::Trap("game output bounds"))?;
        let end = ptr
            .checked_add(len)
            .filter(|&end| end <= memory.len())
            .ok_or(WasmError::Trap("game output bounds"))?;
        let (limit, submitted) = if render {
            (state.limits.max_render_bytes, &mut state.render_submitted)
        } else {
            (state.limits.max_audio_bytes, &mut state.audio_submitted)
        };
        if *submitted || len > limit {
            return Err(WasmError::Trap("game output budget"));
        }
        *submitted = true;
        if render {
            state.render.extend_from_slice(&memory[ptr..end]);
        } else {
            state.audio.extend_from_slice(&memory[ptr..end]);
        }
        Ok(alloc::vec![0])
    })
}
