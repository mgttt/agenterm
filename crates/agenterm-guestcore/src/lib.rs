//! agenterm-guestcore -- Phase 1 of `plan/design-dynacore-emulated-guest-core.md`:
//! a real x86_64 machine-code interpreter, running on Windows, translating a
//! useful subset of the Linux x86_64 syscall ABI to real Win32 calls via
//! `agenterm_nativecore::seam::do_intent`. Same-ISA only (x86_64 guest on
//! x86_64 host) -- no JIT, no codegen, no executable memory ever requested;
//! guest bytes are read as data and switched on, never executed by the CPU.
//! See the crate README for the full syscall-mapping table and what is
//! explicitly NOT supported.
//!
//! Three-layer structure (kept separate deliberately, so a Phase 2 aarch64
//! decoder is an additive sibling module, not a rewrite -- see each module's
//! own header for the full rationale):
//! 1. [`decode_x86_64`] -- the ISA layer. Pure x86_64 semantics; knows
//!    nothing about Linux or Windows.
//! 2. [`abi_linux_x86_64`] -- the guest-OS ABI layer. Turns a raw register
//!    snapshot into [`abi_linux_x86_64::GuestSyscall`], an ISA-agnostic
//!    request enum.
//! 3. [`intent_map`] -- the host-OS mapping layer. `GuestSyscall` ->
//!    `agenterm_nativecore::ir::Intent` + real Win32 calls. Never reads a
//!    guest register or knows an x86_64 opcode exists.
//!
//! This module (`lib.rs`) is the concrete x86_64+Linux+Win32 assembly point
//! that wires all three together into [`run_guest`] -- the one place in the
//! crate that currently knows "we are running an x86_64/Linux guest".

pub mod abi_linux_x86_64;
pub mod cpu;
pub mod decode;
pub mod decode_x86_64;
pub mod elf;
pub mod fault;
pub mod intent_map;
pub mod mem;
pub mod testutil;

pub use cpu::Cpu;
pub use fault::GuestFault;
pub use intent_map::ExitMode;

use cpu::RSP;
use decode_x86_64::StepOutcome;
use intent_map::{Dispatched, FdTable};

/// A guest program's whole address space: one flat, fixed-size buffer
/// (code + rodata + stack, contiguous), allocated once and never resized
/// afterward -- see `mem.rs`'s header for why that stability is load-bearing
/// (every "pointer" the guest computes is a real host pointer into this
/// buffer, valid for the buffer's entire lifetime).
pub struct GuestImage {
    pub buf: Vec<u8>,
    /// `buf.as_ptr() as u64`, captured once after `buf` reached its final
    /// size. RIP-relative addressing (`decode::resolve_addr`) adds this
    /// explicitly -- the exact step the reference prototype (`guest_interp.rs`)
    /// originally got wrong.
    pub base: u64,
    /// Guest-image-relative offset of the first instruction.
    pub entry: usize,
    /// Preset initial `rsp` for images that already have a fully prepared
    /// System-V-shaped stack frame written into `buf` (real ELF loads --
    /// see `elf.rs::load_elf`'s `setup_stack`). `None` (the hand-crafted-
    /// program path, `GuestImage::new`) makes `run_guest` fall back to its
    /// own generic "near the top of the image, 16-byte aligned, no
    /// argv/envp/auxv" placement -- unchanged from before this field
    /// existed.
    pub initial_rsp: Option<u64>,
    /// `Some` when `buf`'s backing memory is a real `VirtualAlloc` mapping
    /// (`elf::load_elf`) rather than the normal Rust global-allocator `Vec`
    /// (`GuestImage::new`) -- see this type's `Drop` impl below for why that
    /// distinction matters.
    mapped_region: Option<mem::MappedRegion>,
}

impl GuestImage {
    /// `code_and_data`: guest code followed by any rodata, placed at offset
    /// 0. `stack_bytes`: zero-filled space appended after it; initial `rsp`
    /// is set near the top of that region (see `run_guest`).
    pub fn new(code_and_data: &[u8], entry: usize, stack_bytes: usize) -> GuestImage {
        let mut buf = vec![0u8; code_and_data.len() + stack_bytes];
        buf[..code_and_data.len()].copy_from_slice(code_and_data);
        let base = buf.as_ptr() as u64;
        GuestImage { buf, base, entry, initial_rsp: None, mapped_region: None }
    }

    /// Build a `GuestImage` around a `Vec<u8>` that is secretly a real
    /// `VirtualAlloc` mapping (`region`) instead of a normal heap
    /// allocation -- `elf::load_elf`'s constructor. Not `pub`: the
    /// raw-parts invariant this requires (`buf`'s pointer/len/capacity came
    /// from `region` itself, see `elf.rs`) is only something `elf.rs`, in
    /// the same crate, can uphold.
    pub(crate) fn from_mapped(buf: Vec<u8>, base: u64, entry: usize, initial_rsp: u64, region: mem::MappedRegion) -> GuestImage {
        GuestImage { buf, base, entry, initial_rsp: Some(initial_rsp), mapped_region: Some(region) }
    }

    /// Host address of image-relative offset `off` -- the same "guest
    /// address == host address" mapping every register value that
    /// represents a pointer already carries (see `mem.rs`'s header). Test
    /// programs use this to point a guest register (e.g. a `read()` target
    /// buffer, or a scratch slot the guest copies a result into) at a known
    /// spot inside their own image, so the outer Rust test can inspect it
    /// afterward via plain safe indexing into `self.buf`.
    pub fn addr_of(&self, off: usize) -> u64 {
        self.base + off as u64
    }

    /// Write a NUL-terminated byte string into the image at `off` (test/demo
    /// helper for building `openat`/`open` path arguments -- `CreateFileA`,
    /// same as every Win32 `*A` call, expects a NUL-terminated C string).
    /// Returns the offset just past the terminating NUL.
    pub fn write_cstr(&mut self, off: usize, s: &[u8]) -> usize {
        self.buf[off..off + s.len()].copy_from_slice(s);
        self.buf[off + s.len()] = 0;
        off + s.len() + 1
    }
}

impl Drop for GuestImage {
    fn drop(&mut self) {
        if self.mapped_region.is_some() {
            // `buf`'s backing memory belongs to `mapped_region` -- a real
            // `VirtualAlloc` mapping, whose own `Drop` (running right after
            // this function returns, as part of the compiler-generated
            // field-drop glue for this struct) does the real `VirtualFree`.
            // Take `buf` out and `mem::forget` it here so its own destructor
            // never runs: `Vec<u8>`'s normal `Drop` would call the global
            // heap allocator's `dealloc` on this pointer, which was never
            // allocated by that allocator -- undefined behavior / heap
            // corruption, not a no-op.
            let taken = std::mem::take(&mut self.buf);
            std::mem::forget(taken);
        }
        // else: `buf` is a normal heap `Vec` (`GuestImage::new`'s path) --
        // left untouched here, so its own `Drop` runs normally right after.
    }
}

/// Run a guest program to completion. Loops `decode_x86_64::step`; on
/// `StepOutcome::Syscall`, normalizes the current register file through
/// `abi_linux_x86_64::normalize` and dispatches it through
/// `intent_map::dispatch`. Returns the guest's real exit code in
/// `ExitMode::Return`; in `ExitMode::Real`, `exit`/`exit_group` diverges into
/// a real `ExitProcess` and this function never returns at all for a guest
/// that reaches `exit` (see `intent_map::ExitMode`'s doc comment for why
/// both modes exist).
pub fn run_guest(image: &GuestImage, exit_mode: ExitMode) -> Result<i32, GuestFault> {
    let mut cpu = Cpu::new(image.entry);
    // Initial stack pointer: `image.initial_rsp` if the image already came
    // with a fully prepared stack frame (real ELF loads -- `elf.rs`);
    // otherwise the original generic placement -- near the top of the
    // image, 16-byte aligned, with a little slack so a `call`'s implicit
    // push never runs off the buffer's end on the very first stack write.
    let rsp = image.initial_rsp.unwrap_or_else(|| (image.base + image.buf.len() as u64 - 32) & !0xF);
    cpu.set64(RSP, rsp);
    let mut fds = FdTable::new();

    loop {
        match decode_x86_64::step(&mut cpu, &image.buf, image.base)? {
            StepOutcome::Continue => {}
            StepOutcome::Syscall => {
                let req = abi_linux_x86_64::normalize(&cpu);
                match intent_map::dispatch(req, &mut fds, exit_mode)? {
                    Dispatched::Value(v) => cpu.set64(cpu::RAX, v),
                    Dispatched::Exit(code) => return Ok(code),
                }
            }
        }
    }
}
