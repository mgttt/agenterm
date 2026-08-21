//! agenterm-tinyvm — an owned, bounded WebAssembly virtual machine.
//!
//! The product execution face loads ordinary standard `.wasm`, validates and
//! interprets it without JIT, and gives the host deterministic fuel, memory and
//! table control on platforms including iOS and `wasm32`. It is its own crate
//! and product line; it is *not* a layer of `agenterm-dyn`. dyn remains desktop
//! live assembly over host-ISA bytes. The early compact [`Vm`]/[`Instr`] face
//! remains for compatibility and tests, but it does not define the cartridge
//! format or the platform architecture.
//!
//! Properties, on purpose:
//! - **`no_std`, no `fmt`, no `unsafe`, no `mmap`.** The crate is `no_std`
//!   (only `alloc`) and never pulls in the formatting machinery — errors are
//!   `&'static str` codes — so a stripped static core stays tiny. It runs on
//!   any target Rust can build, including platforms that forbid
//!   runtime-generated native code (iOS, `wasm32`).
//! - **Turing-complete** in the usual bounded-by-memory sense: a value stack, a
//!   linear memory (load/store), integer arithmetic, and a data-dependent
//!   conditional branch — enough to express loops and recursion.
//! - **Loud failure, never silent or hung.** Stack underflow, division by
//!   zero, out-of-range memory/branch, and a runaway program (step budget) all
//!   return an error; the VM never corrupts silently and never spins forever.
//!
//! It does not link, import, or otherwise touch `agenterm-dyn`, `agenterm-cu`,
//! or `agenterm-chassis`.
//!
//! # Example — 40 + 2
//! ```
//! use agenterm_tinyvm::{Instr, Vm};
//! let mut vm = Vm::new(16);
//! let result = vm.run(&[Instr::Push(40), Instr::Push(2), Instr::Add, Instr::Halt]);
//! assert!(matches!(result, Ok(Some(42))));
//! ```
#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;
#[cfg(any(test, feature = "std"))]
extern crate std;

use alloc::vec;
use alloc::vec::Vec;

mod asm;
pub use asm::{AsmError, assemble};

pub mod cartridge;
pub use cartridge::CartridgeManifest;

pub mod game;
pub use game::{
    CartridgeDescriptor, CartridgeOrigin, ExecutionStats, GAME_ABI_VERSION, GameFrame, GameInput,
    GameLifecycle, GameLimits, GameRuntime, KNOWN_BUTTON_MASK, MAX_CARTRIDGE_BYTES,
    MAX_NATIVE_ARITY, MAX_NATIVE_CALLS_PER_LIFECYCLE, MAX_NATIVE_FUNCTIONS, NativeModuleRegistry,
};

pub mod host_profile;
pub use host_profile::{HostFunctionV1, HostProfileV1, MAX_HOST_PROFILE_BYTES};

pub mod host;
pub use host::{
    DescriptorRights, FileStat, FileType, GuestFd, HostBackend, HostClock, HostContext, HostError,
    HostHandle, HostLimits, HostResult, OpenOptions, SeekWhence,
};

#[cfg(feature = "wasi-p1")]
pub mod wasi_p1;
#[cfg(feature = "wasi-p1")]
pub use wasi_p1::{WASI_SNAPSHOT_PREVIEW1, WasiErrno, WasiPreview1};

pub mod media;
pub use media::{Grid3dCell, Grid3dFrame, Indexed2dFrame, RenderFrame, ToneBatch, ToneEvent};

#[cfg(feature = "cartridge-trust")]
pub mod trust;
#[cfg(feature = "cartridge-trust")]
pub use trust::{CartridgeTrustStore, CatalogEntry, cartridge_sha256};

#[cfg(feature = "replay")]
pub mod replay;
#[cfg(feature = "replay")]
pub use replay::{
    MAX_REPLAY_BYTES, MAX_REPLAY_SNAPSHOT_BYTES, MAX_REPLAY_STEPS, ReplayRecorderV1, ReplayStepV1,
    ReplayTraceV1,
};

#[cfg(feature = "cartridge-trust")]
pub mod cartridge_cache;
#[cfg(feature = "cartridge-trust")]
pub use cartridge_cache::CartridgeCache;

#[cfg(feature = "ios-c-api")]
mod ios_c_api;

pub mod wasm;
#[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
pub use wasm::{
    ExternReference as WasmExternReference, FunctionReference as WasmFunctionReference,
};
pub use wasm::{
    Function as WasmFunction, Global as WasmGlobal, GlobalImportDesc, ImportDesc,
    Instance as WasmInstance, Limits, Memory as WasmMemory, MemoryImportDesc,
    MemoryView as WasmMemoryView, MemoryViewMut as WasmMemoryViewMut, Module as WasmModule,
    Store as WasmStore, Table as WasmTable, TableImportDesc, Val, ValueType, WasmError, eval,
    eval_with,
};

/// One bytecode instruction.
///
/// Branch and call targets are absolute instruction indices into the program
/// slice passed to [`Vm::run`]. Memory addresses index the VM's linear memory.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, PartialEq, Eq)]
pub enum Instr {
    // -- stack shuffling --
    /// Push an immediate onto the value stack.
    Push(i64),
    /// Discard the top of stack.
    Pop,
    /// Duplicate the top of stack.
    Dup,
    /// Swap the top two stack values.
    Swap,

    // -- integer arithmetic (wrapping add/sub/mul; checked div/mod) --
    /// `a b -> a+b`
    Add,
    /// `a b -> a-b`
    Sub,
    /// `a b -> a*b`
    Mul,
    /// `a b -> a/b` (errors on `b == 0`)
    Div,
    /// `a b -> a%b` (errors on `b == 0`)
    Mod,

    // -- comparisons and logic (push 1 for true, 0 for false) --
    /// `a b -> (a==b)`
    Eq,
    /// `a b -> (a<b)`
    Lt,
    /// `a b -> (a>b)`
    Gt,
    /// `a -> (a==0)`
    Not,

    // -- linear memory --
    /// Push `mem[addr]`.
    Load(usize),
    /// Pop into `mem[addr]`.
    Store(usize),

    // -- control flow --
    /// Unconditional jump to instruction `target`.
    Jmp(usize),
    /// Pop; if it is zero, jump to instruction `target`.
    Jz(usize),
    /// Push the return address and jump to instruction `target`.
    Call(usize),
    /// Pop the return address and jump back to it.
    Ret,
    /// Stop; the result is the current top of stack (if any).
    Halt,
}

/// A run-time fault. Every one is loud: the VM stops and returns it rather than
/// corrupting state or hanging.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, PartialEq, Eq)]
pub enum VmError {
    /// An op needed more stack values than were present.
    StackUnderflow { pc: usize, op: &'static str },
    /// The value stack exceeded its limit.
    StackOverflow { pc: usize },
    /// The call stack exceeded its depth limit.
    CallOverflow { pc: usize },
    /// `Div`/`Mod` by zero.
    DivideByZero { pc: usize },
    /// A memory address was outside the linear memory.
    MemoryOutOfRange { pc: usize, addr: usize },
    /// A branch/call/return target was outside the program.
    BranchOutOfRange { pc: usize, target: usize },
    /// `Ret` with an empty call stack.
    ReturnWithoutCall { pc: usize },
    /// The program ran past its last instruction without halting.
    RanOffEnd { pc: usize },
    /// The step budget was exhausted (runaway loop/recursion).
    StepBudgetExceeded { limit: u64 },
}

impl VmError {
    /// A static description of this fault (no formatting).
    pub fn message(&self) -> &'static str {
        match self {
            Self::StackUnderflow { .. } => "stack underflow",
            Self::StackOverflow { .. } => "value-stack overflow",
            Self::CallOverflow { .. } => "call-stack overflow",
            Self::DivideByZero { .. } => "divide by zero",
            Self::MemoryOutOfRange { .. } => "memory address out of range",
            Self::BranchOutOfRange { .. } => "branch target out of range",
            Self::ReturnWithoutCall { .. } => "ret without matching call",
            Self::RanOffEnd { .. } => "ran off the end of the program",
            Self::StepBudgetExceeded { .. } => "step budget exceeded",
        }
    }
}

/// A fault from [`Vm::eval`]: either assembling the text or running it.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, PartialEq, Eq)]
pub enum EvalError {
    /// The text could not be assembled.
    Assemble(AsmError),
    /// The assembled program faulted at run time.
    Run(VmError),
}

impl EvalError {
    /// A static description of this fault (no formatting).
    pub fn message(&self) -> &'static str {
        match self {
            Self::Assemble(e) => e.msg,
            Self::Run(e) => e.message(),
        }
    }
}

/// Default cap on executed instructions per [`Vm::run`] call.
pub const DEFAULT_MAX_STEPS: u64 = 16_000_000;
/// Default cap on value-stack depth.
pub const DEFAULT_STACK_LIMIT: usize = 65_536;
/// Default cap on call-stack depth.
pub const DEFAULT_CALL_LIMIT: usize = 8_192;

/// A tiny stack machine with a fixed-size zero-initialised linear memory.
#[derive(Clone)]
pub struct Vm {
    stack: Vec<i64>,
    mem: Vec<i64>,
    call: Vec<usize>,
    max_steps: u64,
    stack_limit: usize,
    call_limit: usize,
}

impl Vm {
    /// Create a VM with `mem_cells` words of zeroed linear memory and default
    /// resource bounds.
    pub fn new(mem_cells: usize) -> Self {
        Self {
            stack: Vec::new(),
            mem: vec![0; mem_cells],
            call: Vec::new(),
            max_steps: DEFAULT_MAX_STEPS,
            stack_limit: DEFAULT_STACK_LIMIT,
            call_limit: DEFAULT_CALL_LIMIT,
        }
    }

    /// Override the step budget (runaway-program guard).
    pub fn with_max_steps(mut self, max_steps: u64) -> Self {
        self.max_steps = max_steps;
        self
    }

    /// Read a cell of linear memory (for inspecting results after a run).
    pub fn mem(&self, addr: usize) -> Option<i64> {
        self.mem.get(addr).copied()
    }

    /// A view of the current value stack.
    pub fn stack(&self) -> &[i64] {
        &self.stack
    }

    /// Execute `program` from instruction 0 until [`Instr::Halt`].
    ///
    /// Returns the top of the value stack at halt (`None` if the stack is
    /// empty), or a [`VmError`] on any fault. The VM's value stack and call
    /// stack are reset at entry; its linear memory persists across runs so a
    /// caller can seed inputs and read outputs.
    pub fn run(&mut self, program: &[Instr]) -> Result<Option<i64>, VmError> {
        self.stack.clear();
        self.exec(program)
    }

    /// Assemble one-line-per-instruction `src` and execute it **without
    /// clearing the value stack**, so the stack (and linear memory) form a live
    /// image that persists across calls — the basis of a REPL: one [`Vm`],
    /// evaluated repeatedly.
    ///
    /// A trailing implicit halt is supplied, so a bare snippet like
    /// `push 40\npush 2\nadd` ends by returning the current stack top rather
    /// than running off the end. Branch/call operands index the assembled
    /// snippet (0-based). Returns the stack top after execution.
    pub fn eval(&mut self, src: &str) -> Result<Option<i64>, EvalError> {
        let mut program = asm::assemble(src).map_err(EvalError::Assemble)?;
        program.push(Instr::Halt);
        self.exec(&program).map_err(EvalError::Run)
    }

    /// Run `program` from instruction 0, resetting only the call stack (the
    /// value stack and memory are left as the caller set them). Shared engine
    /// behind [`Vm::run`] (which clears the stack first) and [`Vm::eval`]
    /// (which does not).
    fn exec(&mut self, program: &[Instr]) -> Result<Option<i64>, VmError> {
        self.call.clear();
        let mut pc: usize = 0;
        let mut steps: u64 = 0;

        loop {
            steps += 1;
            if steps > self.max_steps {
                return Err(VmError::StepBudgetExceeded {
                    limit: self.max_steps,
                });
            }
            let instr = program.get(pc).ok_or(VmError::RanOffEnd { pc })?;
            let here = pc;
            pc += 1;

            match instr {
                Instr::Push(v) => self.push(here, *v)?,
                Instr::Pop => {
                    self.pop(here, "pop")?;
                }
                Instr::Dup => {
                    let v = *self.stack.last().ok_or(VmError::StackUnderflow {
                        pc: here,
                        op: "dup",
                    })?;
                    self.push(here, v)?;
                }
                Instr::Swap => {
                    let n = self.stack.len();
                    if n < 2 {
                        return Err(VmError::StackUnderflow {
                            pc: here,
                            op: "swap",
                        });
                    }
                    self.stack.swap(n - 1, n - 2);
                }
                Instr::Add => self.binop(here, "add", |a, b| Ok(a.wrapping_add(b)))?,
                Instr::Sub => self.binop(here, "sub", |a, b| Ok(a.wrapping_sub(b)))?,
                Instr::Mul => self.binop(here, "mul", |a, b| Ok(a.wrapping_mul(b)))?,
                Instr::Div => self.binop(here, "div", |a, b| {
                    if b == 0 {
                        Err(VmError::DivideByZero { pc: here })
                    } else {
                        Ok(a.wrapping_div(b))
                    }
                })?,
                Instr::Mod => self.binop(here, "mod", |a, b| {
                    if b == 0 {
                        Err(VmError::DivideByZero { pc: here })
                    } else {
                        Ok(a.wrapping_rem(b))
                    }
                })?,
                Instr::Eq => self.binop(here, "eq", |a, b| Ok(i64::from(a == b)))?,
                Instr::Lt => self.binop(here, "lt", |a, b| Ok(i64::from(a < b)))?,
                Instr::Gt => self.binop(here, "gt", |a, b| Ok(i64::from(a > b)))?,
                Instr::Not => {
                    let a = self.pop(here, "not")?;
                    self.push(here, i64::from(a == 0))?;
                }
                Instr::Load(addr) => {
                    let v = *self.mem.get(*addr).ok_or(VmError::MemoryOutOfRange {
                        pc: here,
                        addr: *addr,
                    })?;
                    self.push(here, v)?;
                }
                Instr::Store(addr) => {
                    let v = self.pop(here, "store")?;
                    let cell = self.mem.get_mut(*addr).ok_or(VmError::MemoryOutOfRange {
                        pc: here,
                        addr: *addr,
                    })?;
                    *cell = v;
                }
                Instr::Jmp(target) => {
                    Self::check_target(here, *target, program.len())?;
                    pc = *target;
                }
                Instr::Jz(target) => {
                    let v = self.pop(here, "jz")?;
                    if v == 0 {
                        Self::check_target(here, *target, program.len())?;
                        pc = *target;
                    }
                }
                Instr::Call(target) => {
                    Self::check_target(here, *target, program.len())?;
                    if self.call.len() >= self.call_limit {
                        return Err(VmError::CallOverflow { pc: here });
                    }
                    self.call.push(pc);
                    pc = *target;
                }
                Instr::Ret => {
                    pc = self
                        .call
                        .pop()
                        .ok_or(VmError::ReturnWithoutCall { pc: here })?;
                }
                Instr::Halt => return Ok(self.stack.last().copied()),
            }
        }
    }

    fn push(&mut self, pc: usize, v: i64) -> Result<(), VmError> {
        if self.stack.len() >= self.stack_limit {
            return Err(VmError::StackOverflow { pc });
        }
        self.stack.push(v);
        Ok(())
    }

    fn pop(&mut self, pc: usize, op: &'static str) -> Result<i64, VmError> {
        self.stack.pop().ok_or(VmError::StackUnderflow { pc, op })
    }

    fn binop(
        &mut self,
        pc: usize,
        op: &'static str,
        f: impl FnOnce(i64, i64) -> Result<i64, VmError>,
    ) -> Result<(), VmError> {
        let b = self.pop(pc, op)?;
        let a = self.pop(pc, op)?;
        let r = f(a, b)?;
        self.push(pc, r)
    }

    fn check_target(pc: usize, target: usize, len: usize) -> Result<(), VmError> {
        if target >= len {
            Err(VmError::BranchOutOfRange { pc, target })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_returns_42() {
        let mut vm = Vm::new(4);
        let r = vm.run(&[Instr::Push(40), Instr::Push(2), Instr::Add, Instr::Halt]);
        assert_eq!(r.unwrap(), Some(42));
    }

    #[test]
    fn subtraction_and_division_order() {
        let mut vm = Vm::new(4);
        // (10 - 3) then (that) / 2  ->  7 / 2 = 3
        let r = vm
            .run(&[
                Instr::Push(10),
                Instr::Push(3),
                Instr::Sub,
                Instr::Push(2),
                Instr::Div,
                Instr::Halt,
            ])
            .unwrap();
        assert_eq!(r, Some(3));
    }

    #[test]
    fn conditional_selects_branch() {
        // if 0: push 111 else push 222  -> since top is 0, Jz taken -> 111
        let prog = [
            Instr::Push(0),   // 0
            Instr::Jz(4),     // 1: pop 0 -> jump to 4
            Instr::Push(222), // 2
            Instr::Halt,      // 3
            Instr::Push(111), // 4
            Instr::Halt,      // 5
        ];
        let mut vm = Vm::new(4);
        assert_eq!(vm.run(&prog).unwrap(), Some(111));
    }

    /// Loop with memory: sum 1..=5 via a countdown. Proves branch + memory +
    /// arithmetic compose into real (Turing-complete-class) computation.
    #[test]
    fn loop_sums_one_through_five() {
        let prog = [
            Instr::Push(0),
            Instr::Store(1), // 1: acc = 0
            Instr::Push(5),
            Instr::Store(0), // 3: i = 5
            Instr::Load(0),  // 4: L
            Instr::Jz(15),   // 5: if i==0 -> END
            Instr::Load(1),
            Instr::Load(0),
            Instr::Add,
            Instr::Store(1), // 9: acc += i
            Instr::Load(0),
            Instr::Push(1),
            Instr::Sub,
            Instr::Store(0), // 13: i -= 1
            Instr::Jmp(4),   // 14: loop
            Instr::Load(1),  // 15: END -> push acc
            Instr::Halt,     // 16
        ];
        let mut vm = Vm::new(4);
        assert_eq!(vm.run(&prog).unwrap(), Some(15));
        assert_eq!(vm.mem(1), Some(15));
    }

    /// Factorial(5) = 120 — the same loop shape with Mul.
    #[test]
    fn loop_computes_factorial_five() {
        let prog = [
            Instr::Push(1),
            Instr::Store(1), // acc = 1
            Instr::Push(5),
            Instr::Store(0), // i = 5
            Instr::Load(0),  // 4: L
            Instr::Jz(15),   // 5: END if i==0
            Instr::Load(1),
            Instr::Load(0),
            Instr::Mul,
            Instr::Store(1), // acc *= i
            Instr::Load(0),
            Instr::Push(1),
            Instr::Sub,
            Instr::Store(0), // i -= 1
            Instr::Jmp(4),
            Instr::Load(1), // 15: END
            Instr::Halt,
        ];
        let mut vm = Vm::new(4);
        assert_eq!(vm.run(&prog).unwrap(), Some(120));
    }

    /// A called subroutine that squares the top of stack, via Call/Ret.
    #[test]
    fn call_and_ret_run_a_subroutine() {
        // main: push 9; call square; halt.  square: dup; mul; ret.
        let prog = [
            Instr::Push(9), // 0
            Instr::Call(4), // 1 -> square
            Instr::Halt,    // 2
            Instr::Halt,    // 3 (padding, unreachable)
            Instr::Dup,     // 4: square
            Instr::Mul,     // 5
            Instr::Ret,     // 6
        ];
        let mut vm = Vm::new(4);
        assert_eq!(vm.run(&prog).unwrap(), Some(81));
    }

    #[test]
    fn divide_by_zero_is_loud() {
        let mut vm = Vm::new(4);
        let e = vm
            .run(&[Instr::Push(1), Instr::Push(0), Instr::Div, Instr::Halt])
            .unwrap_err();
        assert!(matches!(e, VmError::DivideByZero { .. }));
    }

    #[test]
    fn stack_underflow_is_loud() {
        let mut vm = Vm::new(4);
        let e = vm.run(&[Instr::Add, Instr::Halt]).unwrap_err();
        assert!(matches!(e, VmError::StackUnderflow { .. }));
    }

    #[test]
    fn memory_out_of_range_is_loud() {
        let mut vm = Vm::new(2);
        let e = vm.run(&[Instr::Load(99), Instr::Halt]).unwrap_err();
        assert!(matches!(e, VmError::MemoryOutOfRange { .. }));
    }

    #[test]
    fn branch_out_of_range_is_loud() {
        let mut vm = Vm::new(2);
        let e = vm.run(&[Instr::Jmp(99), Instr::Halt]).unwrap_err();
        assert!(matches!(e, VmError::BranchOutOfRange { .. }));
    }

    #[test]
    fn runaway_loop_hits_step_budget_not_hang() {
        let mut vm = Vm::new(2).with_max_steps(10_000);
        let e = vm.run(&[Instr::Jmp(0)]).unwrap_err();
        assert!(matches!(e, VmError::StepBudgetExceeded { .. }));
    }

    #[test]
    fn program_without_halt_fails_loudly() {
        let mut vm = Vm::new(2);
        let e = vm.run(&[Instr::Push(1)]).unwrap_err();
        assert!(matches!(e, VmError::RanOffEnd { .. }));
    }

    #[test]
    fn eval_assembles_and_runs_text() {
        let mut vm = Vm::new(4);
        let top = vm.eval("push 40\npush 2\nadd").unwrap();
        assert_eq!(top, Some(42));
    }

    #[test]
    fn eval_keeps_the_stack_image_across_calls() {
        let mut vm = Vm::new(4);
        assert_eq!(vm.eval("push 40\npush 2\nadd").unwrap(), Some(42));
        // Same Vm, no clear: the 42 is still on the stack.
        assert_eq!(vm.eval("push 1\nadd").unwrap(), Some(43));
    }

    #[test]
    fn run_still_clears_the_stack_at_entry() {
        let mut vm = Vm::new(4);
        vm.eval("push 99").unwrap();
        // run() clears the value stack, so the earlier 99 does not survive.
        let r = vm.run(&[Instr::Push(7), Instr::Halt]).unwrap();
        assert_eq!(r, Some(7));
        assert_eq!(vm.stack(), &[7]);
    }

    #[test]
    fn eval_reports_assembly_errors() {
        let mut vm = Vm::new(4);
        assert!(matches!(vm.eval("bogus 1"), Err(EvalError::Assemble(_))));
    }
}

/// Freestanding static-core support: a minimal global allocator, panic handler,
/// and a C-ABI self-test entry, enabled only for the size-measurement build
/// (`--features staticcore`). Not compiled for normal builds or tests, so it
/// never clashes with `std`'s allocator/panic handler.
#[cfg(all(feature = "staticcore", not(feature = "std")))]
mod staticcore {
    use core::alloc::{GlobalAlloc, Layout};
    use core::cell::UnsafeCell;
    use core::sync::atomic::{AtomicUsize, Ordering};

    // A large arena lives in .bss (zero-initialised) so it costs no file bytes.
    const ARENA_BYTES: usize = 32 * 1024 * 1024;

    struct Bump {
        buf: UnsafeCell<[u8; ARENA_BYTES]>,
        off: AtomicUsize,
    }
    // Single-threaded static core; access is serialised by the bump CAS loop.
    unsafe impl Sync for Bump {}

    #[global_allocator]
    static ALLOC: Bump = Bump {
        buf: UnsafeCell::new([0; ARENA_BYTES]),
        off: AtomicUsize::new(0),
    };

    unsafe impl GlobalAlloc for Bump {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let align = layout.align();
            let size = layout.size();
            let base = self.buf.get() as usize;
            loop {
                let cur = self.off.load(Ordering::Relaxed);
                let ptr = (base + cur + align - 1) & !(align - 1);
                let new_off = ptr - base + size;
                if new_off > ARENA_BYTES {
                    return core::ptr::null_mut();
                }
                if self
                    .off
                    .compare_exchange(cur, new_off, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    return ptr as *mut u8;
                }
            }
        }
        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
    }

    #[panic_handler]
    fn panic(_info: &core::panic::PanicInfo) -> ! {
        loop {}
    }

    /// Run a hand-written `.wasm` module through the interpreter and return the
    /// exported function's i32 result (or a small positive code on error).
    ///
    /// The module reaches 42 only if several opcode families agree, so the
    /// size gate is also a correctness gate: type/function/table/memory/global/
    /// export/elem/code sections all have to parse, and the answer flows
    /// through `i32.div_s`/`rem_s`/`add`, `call_indirect` (table + elem), a
    /// linear-memory `store`/`load`, `global.get`/`set`, a `block`/`loop` with
    /// `br_if`, `select` on an `i32.eq`, and the
    /// `f64 -> i64.trunc_f64_s -> i64.mul -> i32.wrap_i64` conversion path.
    #[unsafe(no_mangle)]
    pub extern "C" fn tinyvm_selftest() -> i32 {
        const WASM: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0B, 0x02, 0x60, 0x02, 0x7F,
            0x7F, 0x01, 0x7F, 0x60, 0x00, 0x01, 0x7F, 0x03, 0x03, 0x02, 0x00, 0x01, 0x04, 0x04,
            0x01, 0x70, 0x00, 0x01, 0x05, 0x03, 0x01, 0x00, 0x01, 0x06, 0x06, 0x01, 0x7F, 0x01,
            0x41, 0x07, 0x0B, 0x07, 0x0A, 0x01, 0x06, 0x61, 0x6E, 0x73, 0x77, 0x65, 0x72, 0x00,
            0x01, 0x09, 0x07, 0x01, 0x00, 0x41, 0x00, 0x0B, 0x01, 0x00, 0x0A, 0x72, 0x02, 0x07,
            0x00, 0x20, 0x00, 0x20, 0x01, 0x6A, 0x0B, 0x68, 0x01, 0x02, 0x7F, 0x41, 0xE4, 0x00,
            0x41, 0x05, 0x6D, 0x41, 0x2B, 0x41, 0x0F, 0x6F, 0x41, 0x00, 0x11, 0x00, 0x00, 0x21,
            0x00, 0x41, 0x04, 0x20, 0x00, 0x36, 0x02, 0x00, 0x41, 0x04, 0x28, 0x02, 0x00, 0x21,
            0x00, 0x20, 0x00, 0x23, 0x00, 0x6A, 0x21, 0x00, 0x41, 0x01, 0x24, 0x00, 0x41, 0x02,
            0x21, 0x01, 0x02, 0x40, 0x03, 0x40, 0x20, 0x01, 0x45, 0x0D, 0x01, 0x20, 0x00, 0x23,
            0x00, 0x6A, 0x21, 0x00, 0x20, 0x01, 0x41, 0x01, 0x6B, 0x21, 0x01, 0x0C, 0x00, 0x0B,
            0x0B, 0x41, 0x2A, 0x41, 0x00, 0x44, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x35, 0x40,
            0xB0, 0x42, 0x02, 0x7E, 0xA7, 0x20, 0x00, 0x46, 0x1B, 0x41, 0x00, 0x10, 0x00, 0x0B,
        ];
        match crate::WasmModule::from_bytes(WASM) {
            Ok(m) => match m.invoke_val(1, &[]) {
                Ok(r) => match r.first() {
                    Some(crate::Val::I32(v)) => *v,
                    _ => 2,
                },
                Err(_) => 3,
            },
            Err(_) => 4,
        }
    }
}
