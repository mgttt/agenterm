//! Target A — AArch64 + Linux reach (SVC syscalls). PER-TARGET (ISA x OS).
//!
//! Leak content per spec: the Linux/AArch64 syscall NUMBERS baked in. Note these are
//! DIFFERENT from x86-64 Linux (read is 63 not 0, write 64 not 1, mmap 222 not 9,
//! exit 93 not 60). And AArch64 Linux has NO `open` and NO `fork` syscalls at all —
//! you must use `openat`/`clone`. That is a NEW ISA-axis leak absent from Q1's ABI
//! axis: the reach table is per-(ISA,OS), not merely per-OS, and even the SET of
//! available primitives shifts with the ISA.
//!
//! EXECUTION: byte-measured only (no aarch64 host / no qemu). The encoder is validated
//! instruction-by-instruction against LLVM; the lowering structure is identical to
//! Q1's executed x86 path.

use crate::a64::*;
use crate::common_a64::{place_args, CallArg, Frame, Target};
use crate::ir::*;

// Linux/AArch64 syscall numbers (reach content — a per-(ISA,OS) leak).
const SYS_OPENAT: u64 = 56;
const SYS_CLOSE: u64 = 57;
const SYS_READ: u64 = 63;
const SYS_WRITE: u64 = 64;
const SYS_EXIT: u64 = 93;
const SYS_MMAP: u64 = 222;
const SYS_CLONE: u64 = 220; // AArch64 has no fork; clone is the realization
const SYS_EXECVE: u64 = 221;
const SYS_WAIT4: u64 = 260;
const AT_FDCWD: u64 = 0xffff_ffff_ffff_ff9c; // -100

pub struct A64Linux;

impl A64Linux {
    /// emit one Linux syscall: place args in x0.., set x8 = number, svc #0; result x0
    fn sc(a: &mut A64, f: &Frame, num: u64, args: &[CallArg]) {
        place_args(a, f, args, 8); // all our syscalls fit in registers
        a.mov_imm(X8_SYSNO, num);
        a.svc0();
    }
}

const X8_SYSNO: u8 = 8;

impl Target for A64Linux {
    fn name(&self) -> &'static str { "a64-linux" }
    fn ctx_reg(&self) -> u8 { X0 } // AAPCS64: first arg in x0
    fn max_outgoing(&self, _m: &Module) -> i32 { 0 } // syscalls are register-only

    fn emit_call(&self, a: &mut A64, f: &Frame, intent: Intent, s: &[i32], dest: i32) {
        match intent {
            Intent::Alloc => {
                // mmap(0, n, PROT_READ|WRITE=3, MAP_PRIVATE|ANON=0x22, -1, 0)
                Self::sc(a, f, SYS_MMAP, &[
                    CallArg::Imm(0), CallArg::Slot(s[0]), CallArg::Imm(3),
                    CallArg::Imm(0x22), CallArg::Imm(u64::MAX), CallArg::Imm(0),
                ]);
                a.str(X0, SP, dest);
            }
            Intent::FileOpen => {
                // openat(AT_FDCWD, path, O_RDONLY=0, 0)   <-- no `open` on aarch64
                Self::sc(a, f, SYS_OPENAT, &[
                    CallArg::Imm(AT_FDCWD), CallArg::Slot(s[0]), CallArg::Imm(0), CallArg::Imm(0),
                ]);
                a.str(X0, SP, dest);
            }
            Intent::FileRead => {
                Self::sc(a, f, SYS_READ, &[CallArg::Slot(s[0]), CallArg::Slot(s[1]), CallArg::Slot(s[2])]);
                a.str(X0, SP, dest);
            }
            Intent::FileClose => {
                Self::sc(a, f, SYS_CLOSE, &[CallArg::Slot(s[0])]);
                a.str(X0, SP, dest);
            }
            Intent::WriteStdout => {
                Self::sc(a, f, SYS_WRITE, &[CallArg::Imm(1), CallArg::Slot(s[0]), CallArg::Slot(s[1])]);
                a.str(X0, SP, dest);
            }
            Intent::SpawnWait => emit_spawn(a, f, dest),
        }
    }
}

/// clone/execve/wait4 — the AArch64/Linux realization of SpawnWait. Shares NOTHING
/// with the Windows realization, and even differs from x86 Linux (no fork; clone's
/// arg order is arch-specific). Byte-measured only.
fn emit_spawn(a: &mut A64, f: &Frame, dest: i32) {
    let pid = f.scratch(0);
    let page = f.scratch(1);
    let lparent = a.new_label();

    // pid = clone(SIGCHLD=17, newsp=0, 0, 0, 0)
    A64Linux::sc(a, f, SYS_CLONE, &[
        CallArg::Imm(17), CallArg::Imm(0), CallArg::Imm(0), CallArg::Imm(0), CallArg::Imm(0),
    ]);
    a.str(X0, SP, pid);
    a.cbnz(X0, lparent);

    // ---- child ----
    A64Linux::sc(a, f, SYS_MMAP, &[
        CallArg::Imm(0), CallArg::Imm(128), CallArg::Imm(3), CallArg::Imm(0x22), CallArg::Imm(u64::MAX), CallArg::Imm(0),
    ]);
    a.str(X0, SP, page);
    // lay out "/bin/sh\0-c\0exit 7\0" at page
    a.ldr(X9, SP, page);
    for (i, &c) in b"/bin/sh\0-c\0exit 7\0".iter().enumerate() {
        a.mov_imm(X10, c as u64);
        a.strb(X10, X9, i as i32);
    }
    // argv at page+32: [page, page+8, page+11, NULL]; envp at page+64: [NULL]
    a.ldr(X9, SP, page);
    a.str(X9, X9, 32);
    a.add_imm(X10, X9, 8);
    a.str(X10, X9, 40);
    a.add_imm(X10, X9, 11);
    a.str(X10, X9, 48);
    a.mov_imm(X10, 0);
    a.str(X10, X9, 56);
    a.str(X10, X9, 64);
    // execve(page, page+32, page+64)
    a.ldr(X0, SP, page);
    a.add_imm(X1, X0, 32);
    a.add_imm(X2, X0, 64);
    a.mov_imm(X8_SYSNO, SYS_EXECVE);
    a.svc0();
    // execve failed -> exit(127)
    A64Linux::sc(a, f, SYS_EXIT, &[CallArg::Imm(127)]);

    // ---- parent ----
    a.bind(lparent);
    a.str_w(XZR, SP, f.outdw_disp()); // *status = 0
    A64Linux::sc(a, f, SYS_WAIT4, &[CallArg::Slot(pid), CallArg::OutPtr, CallArg::Imm(0), CallArg::Imm(0)]);
    // exit = (status >> 8) & 0xff
    a.ldr_w(X9, SP, f.outdw_disp());
    a.lsr_imm(X9, X9, 8);
    a.mov_imm(X10, 0xff);
    a.and(X9, X9, X10);
    a.str(X9, SP, dest);
}
