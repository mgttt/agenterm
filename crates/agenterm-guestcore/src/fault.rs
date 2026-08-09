//! Every way this interpreter refuses to guess. An unimplemented opcode or an
//! unimplemented/deliberately-unsupported syscall is always a `GuestFault`,
//! never silent wrong behavior -- the discipline this whole crate is built
//! to (see `plan/design-dynacore-emulated-guest-core.md` and the F1/
//! derive-not-declare discipline it inherits from `agenterm-nativecore`).

use std::fmt;

#[derive(Debug)]
pub enum GuestFault {
    /// A byte sequence at `rip` that this decoder does not recognize (either
    /// truly unimplemented, or a legacy prefix -- `66`/`67`/`F2`/`F3` -- this
    /// crate deliberately does not decode; see README's "not supported").
    UnimplementedOpcode { rip: usize, byte: u8 },
    /// A `syscall` with a `rax` this crate does not translate at all --
    /// includes syscalls with no honest nativecore `Intent` equivalent
    /// (`fork`/`execve`/`brk`/`arch_prctl`/...), see README's syscall table.
    UnimplementedSyscall { number: u64 },
    /// `write(fd, ..)` with `fd != 1`. Scope: only stdout is wired to
    /// `Intent::WriteStdout`; anything else is a clear rejection, not a
    /// silent misroute to the wrong handle.
    UnsupportedWriteFd { fd: u64 },
    /// `openat`/`open` flags this crate's fd-table model does not cover
    /// (see `syscall.rs` header for exactly which O_* combinations route to
    /// which nativecore `Intent`).
    UnsupportedOpenFlags { flags: u64 },
    /// `mmap` outside the one case `Intent::Alloc` actually models: anonymous,
    /// no fixed address, no file backing (README documents this precisely).
    UnsupportedMmap { reason: &'static str },
    /// A syscall referenced a file descriptor this interpreter's fd-table
    /// never opened (or already closed) -- e.g. `read`/`write`/`close` on a
    /// number that was never returned by our `openat` translation.
    UnknownFd { fd: u64 },
    /// `call`/`ret` underflow or overflow of the guest's own tiny return-
    /// address stack invariant this interpreter checks (see `interp.rs`).
    StackFault { reason: &'static str },
    /// `rip` ran off the end of the guest image without hitting an `exit`.
    RanOffEnd { rip: usize },
}

impl fmt::Display for GuestFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GuestFault::UnimplementedOpcode { rip, byte } => {
                write!(f, "guestcore: unimplemented opcode 0x{byte:02x} at guest rip offset {rip:#x}")
            }
            GuestFault::UnimplementedSyscall { number } => {
                write!(f, "guestcore: unimplemented/deliberately-unsupported syscall number {number} (no honest nativecore Intent equivalent, or out of Phase 1 scope -- see README)")
            }
            GuestFault::UnsupportedWriteFd { fd } => {
                write!(f, "guestcore: write(fd={fd}, ..) rejected -- only fd=1 (stdout) is wired to Intent::WriteStdout")
            }
            GuestFault::UnsupportedOpenFlags { flags } => {
                write!(f, "guestcore: openat/open flags {flags:#x} not covered by the fd-table model (see syscall.rs)")
            }
            GuestFault::UnsupportedMmap { reason } => {
                write!(f, "guestcore: mmap rejected -- {reason}")
            }
            GuestFault::UnknownFd { fd } => {
                write!(f, "guestcore: fd {fd} was never opened by this interpreter's openat translation (or already closed)")
            }
            GuestFault::StackFault { reason } => {
                write!(f, "guestcore: guest call/ret stack fault -- {reason}")
            }
            GuestFault::RanOffEnd { rip } => {
                write!(f, "guestcore: guest rip ran off the end of the image at offset {rip:#x} without an exit syscall")
            }
        }
    }
}

impl std::error::Error for GuestFault {}
