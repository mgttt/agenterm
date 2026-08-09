//! **Layer 3 of 3 (host-OS mapping layer) -- `GuestSyscall` -> real Win32
//! calls, via `agenterm_nativecore`.** This is the part that was already
//! ISA/guest-OS-agnostic by construction: it never reads a guest register or
//! knows an x86_64 opcode exists, it only consumes the `GuestSyscall` enum
//! `abi_linux_x86_64.rs` (layer 2) produces. A future `abi_linux_aarch64.rs`
//! producing the same enum from `x0..x8` instead of `rdi..rax` would not
//! require touching this file at all.
//!
//! `agenterm_nativecore::ir::Intent::contract_arity` documents each Intent's
//! OWN arg order, which is generally NOT the same order as the matching
//! Linux syscall's args -- the exact mapping used for each syscall this
//! crate supports:
//!
//! | Guest syscall | nativecore Intent | Real arg-order translation |
//! |---|---|---|
//! | `write(fd,buf,len)`, fd==1 only | `WriteStdout` | `[buf, len]` (fd dropped -- WriteStdout is hardcoded to `GetStdHandle(STD_OUTPUT_HANDLE)`) |
//! | `open`/`openat`, read-mode flags only | `FileOpen` | `[path_ptr]` |
//! | `read(fd,buf,count)` | `FileRead` | `[handle, buf, cap]` (handle from our fd table, NOT the guest's fd number) |
//! | `close(fd)` | `FileClose` | `[handle]` (handle from our fd table) |
//! | `mmap`, simple anonymous only | `Alloc` | `[len]` |
//! | `exit`/`exit_group` | *(none -- real `ExitProcess`, process-level)* | `code` |
//!
//! **Design correction applied (mid-build, see this crate's README):** a
//! write-mode `open`/`openat` -> `write` -> `close` sequence has NO honest
//! nativecore Intent to route to (`FileOpen` is read-only by construction;
//! `FileWrite` is an ATOMIC create+write+close in one native call, not
//! decomposable across three separate guest syscalls without the interpreter
//! silently faking state). `abi_linux_x86_64.rs` already rejects such opens
//! as `OpenUnsupportedFlags`; `write(fd != 1, ..)` is likewise always
//! rejected here (`UnsupportedWriteFd`) -- an honest gap, not a bug, same
//! posture as `ape-vm`'s own `-ENOSYS` discipline.

use std::collections::HashMap;

use agenterm_nativecore::ir::Intent;
use agenterm_nativecore::seam::do_intent;

use crate::abi_linux_x86_64::GuestSyscall;
use crate::fault::GuestFault;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn ExitProcess(code: u32) -> !;
}

/// What to do when the guest requests `exit`/`exit_group`. Real guest
/// execution (the three `examples/verify_*.rs` binaries) uses `Real` --
/// exactly the process-terminating Win32 call the design doc calls for.
/// `Return` exists ONLY so this crate's own `#[test]`s (which run inside the
/// shared `cargo test` process) can execute a guest program that reaches
/// `exit` without killing the test harness itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExitMode {
    Real,
    Return,
}

pub enum Dispatched {
    /// The syscall's return value -- caller (the ISA-layer-aware run loop)
    /// is responsible for placing it wherever that ISA's ABI returns values
    /// (`rax` for x86_64); this layer does not know that convention exists.
    Value(u64),
    Exit(i32),
}

/// Per-run open-file-descriptor table: our own small integer "fd" (Linux
/// convention: first free number after stdin/stdout/stderr) -> the REAL
/// Win32 `HANDLE` (as a `u64`) `Intent::FileOpen` returned. A `read`/`close`
/// for a fd this table doesn't contain is an honest `UnknownFd` fault, not a
/// silent misroute.
pub struct FdTable {
    next_fd: u64,
    open: HashMap<u64, u64>,
}

impl Default for FdTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FdTable {
    pub fn new() -> Self {
        FdTable { next_fd: 3, open: HashMap::new() }
    }
}

/// Handle one normalized guest syscall request. Returns
/// `Dispatched::Value(rax)` / `Dispatched::Exit(code)` (in `ExitMode::Return`),
/// diverges into a real `ExitProcess` (in `ExitMode::Real`), or an honest
/// `GuestFault` for anything not translated.
pub fn dispatch(req: GuestSyscall, fds: &mut FdTable, exit_mode: ExitMode) -> Result<Dispatched, GuestFault> {
    match req {
        GuestSyscall::Write { fd, buf, len } => {
            if fd != 1 {
                return Err(GuestFault::UnsupportedWriteFd { fd });
            }
            let written = do_intent(Intent::WriteStdout, &[buf, len]);
            Ok(Dispatched::Value(written))
        }
        GuestSyscall::OpenRead { path_ptr } => {
            let handle = do_intent(Intent::FileOpen, &[path_ptr]);
            let fd = fds.next_fd;
            fds.next_fd += 1;
            fds.open.insert(fd, handle);
            Ok(Dispatched::Value(fd))
        }
        GuestSyscall::OpenUnsupportedFlags { flags } => Err(GuestFault::UnsupportedOpenFlags { flags }),
        GuestSyscall::Read { fd, buf, cap } => {
            let handle = *fds.open.get(&fd).ok_or(GuestFault::UnknownFd { fd })?;
            let n = do_intent(Intent::FileRead, &[handle, buf, cap]);
            Ok(Dispatched::Value(n))
        }
        GuestSyscall::Close { fd } => {
            let handle = fds.open.remove(&fd).ok_or(GuestFault::UnknownFd { fd })?;
            let ok = do_intent(Intent::FileClose, &[handle]);
            Ok(Dispatched::Value(ok))
        }
        GuestSyscall::MmapAnonSimple { len } => {
            let ptr = do_intent(Intent::Alloc, &[len]);
            Ok(Dispatched::Value(ptr))
        }
        GuestSyscall::MmapUnsupported { reason } => Err(GuestFault::UnsupportedMmap { reason }),
        GuestSyscall::Exit { code } => match exit_mode {
            ExitMode::Real => {
                #[cfg(windows)]
                unsafe {
                    ExitProcess(code as u32)
                }
                #[cfg(not(windows))]
                {
                    std::process::exit(code as i32)
                }
            }
            ExitMode::Return => Ok(Dispatched::Exit(code as i32)),
        },
        GuestSyscall::Unsupported { nr } => Err(GuestFault::UnimplementedSyscall { number: nr }),
    }
}
