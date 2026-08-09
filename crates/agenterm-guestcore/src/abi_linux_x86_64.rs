//! **Layer 2 of 3 (guest-OS ABI layer) -- Linux/x86_64 calling convention
//! only.** Knows exactly one thing `decode_x86_64.rs` (layer 1) deliberately
//! does not: that THIS guest program was compiled against the Linux x86_64
//! syscall ABI (`rax` = syscall number, args in `rdi, rsi, rdx, r10, r8,
//! r9`). Turns a raw register-file snapshot into `GuestSyscall`, a small,
//! structured, ISA-agnostic request enum with no x86_64-specific or
//! Windows-specific fields.
//!
//! This is the file a future `abi_linux_aarch64.rs` (Phase 2, not built this
//! round -- aarch64 uses `x8` = syscall number, args in `x0..x5`) would sit
//! next to: same `GuestSyscall` enum, different register convention read
//! out of a different ISA layer's register file. `intent_map.rs` (layer 3)
//! never needs to change for that to work.
//!
//! Also owns the ONE piece of Linux-ABI-specific interpretation that isn't
//! just "which register is arg N": deciding whether a given `open`/`openat`
//! flags word is the read-only case `Intent::FileOpen` actually models (see
//! `intent_map.rs`'s header for why write-mode opens are out of scope), and
//! whether a given `mmap` call is the simple anonymous case `Intent::Alloc`
//! models.

use crate::cpu::{Cpu, R8, R9, R10, RAX, RDI, RDX, RSI};

pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_MMAP: u64 = 9;
pub const SYS_EXIT: u64 = 60;
pub const SYS_OPENAT: u64 = 257;
pub const SYS_EXIT_GROUP: u64 = 231;

const O_WRONLY: u64 = 0x1;
const O_RDWR: u64 = 0x2;
const O_CREAT: u64 = 0x40;
const O_TRUNC: u64 = 0x200;

const MAP_ANONYMOUS: u64 = 0x20;

/// An ISA-agnostic, guest-syscall-shaped request. Nothing in this enum is
/// x86_64-specific or Win32-specific -- `intent_map.rs` (layer 3) consumes
/// it without ever reading a guest register.
pub enum GuestSyscall {
    /// `write(fd, buf, len)`.
    Write { fd: u64, buf: u64, len: u64 },
    /// A read-mode `open`/`openat` -- the only case `Intent::FileOpen`
    /// honestly models (see this module's header).
    OpenRead { path_ptr: u64 },
    /// A write-mode `open`/`openat` (`O_WRONLY`/`O_RDWR`/`O_CREAT`/`O_TRUNC`
    /// set) -- nativecore has no Intent for this, always rejected.
    OpenUnsupportedFlags { flags: u64 },
    /// `read(fd, buf, count)`.
    Read { fd: u64, buf: u64, cap: u64 },
    /// `close(fd)`.
    Close { fd: u64 },
    /// A simple anonymous `mmap` -- the only case `Intent::Alloc` models.
    MmapAnonSimple { len: u64 },
    /// An `mmap` outside that case (`MAP_FIXED` addr hint, or file-backed).
    MmapUnsupported { reason: &'static str },
    /// `exit`/`exit_group`.
    Exit { code: u64 },
    /// Any syscall number this ABI layer does not translate at all --
    /// includes syscalls with no honest nativecore `Intent` equivalent
    /// (`fork`/`execve`/`brk`/`arch_prctl`/...), left unimplemented on
    /// purpose (design doc's explicit instruction; see README).
    Unsupported { nr: u64 },
}

/// Read the current register file as a Linux x86_64 syscall request. Called
/// by the top-level run loop (`lib.rs`) exactly once per `StepOutcome::Syscall`.
pub fn normalize(cpu: &Cpu) -> GuestSyscall {
    let num = cpu.get(RAX);
    let a0 = cpu.get(RDI);
    let a1 = cpu.get(RSI);
    let a2 = cpu.get(RDX);
    let a3 = cpu.get(R10);
    let _a4 = cpu.get(R8); // present for ABI-table completeness; no supported syscall needs a 5th arg
    let _a5 = cpu.get(R9); // present for ABI-table completeness; no supported syscall needs a 6th arg

    match num {
        SYS_WRITE => GuestSyscall::Write { fd: a0, buf: a1, len: a2 },
        SYS_OPEN => open_from_flags(a0, a1), // open(path, flags, mode): rdi=path, rsi=flags
        SYS_OPENAT => open_from_flags(a1, a2), // openat(dirfd, path, flags, mode): rsi=path, rdx=flags
        SYS_READ => GuestSyscall::Read { fd: a0, buf: a1, cap: a2 },
        SYS_CLOSE => GuestSyscall::Close { fd: a0 },
        SYS_MMAP => mmap_from(a0, a1, a3), // mmap(addr,len,prot,flags,fd,off): rdi=addr,rsi=len,r10=flags
        SYS_EXIT | SYS_EXIT_GROUP => GuestSyscall::Exit { code: a0 },
        other => GuestSyscall::Unsupported { nr: other },
    }
}

fn open_from_flags(path_ptr: u64, flags: u64) -> GuestSyscall {
    if flags & (O_WRONLY | O_RDWR | O_CREAT | O_TRUNC) != 0 {
        GuestSyscall::OpenUnsupportedFlags { flags }
    } else {
        GuestSyscall::OpenRead { path_ptr }
    }
}

fn mmap_from(addr: u64, len: u64, flags: u64) -> GuestSyscall {
    if addr != 0 {
        return GuestSyscall::MmapUnsupported {
            reason: "MAP_FIXED / non-zero addr hint not supported -- Intent::Alloc always lets Win32 choose the address",
        };
    }
    if flags & MAP_ANONYMOUS == 0 {
        return GuestSyscall::MmapUnsupported { reason: "file-backed mmap not supported -- Intent::Alloc only models anonymous VirtualAlloc" };
    }
    GuestSyscall::MmapAnonSimple { len }
}
