//! Lowerer A — SysV x86_64 ABI + Linux reach (raw syscalls).
//!
//! TARGET-SPECIFIC. Leak candidates per spec §②: SysV arg placement (rdi,rsi,rdx,
//! r10,r8,r9; result in rax; no shadow space, a red zone is available), and — the
//! reach content — Linux syscall NUMBERS baked directly into the code. Note the
//! contrast with win64.rs: Win64 reaches by resolving symbol *strings* at runtime;
//! SysV reaches by *numbers* compiled in. Both are per-target reach facts; neither
//! is expressible in the neutral IR.
//!
//! EXECUTION: byte-measured only. The host is Windows with no WSL, so SysV artifacts
//! that touch the OS cannot run here (they would place args for, and syscall into, a
//! Linux kernel). `pure_compute` is the exception — it has no OS surface, so its
//! SysV lowering is byte-identical to Win64 and is executed on the Win64 side as
//! proof (see main.rs). This mirrors the prior round's honest split.

use crate::asm::*;
use crate::common::{Frame, Target};
use crate::ir::*;

// SysV syscall arg registers, in order.
const SREG: [u8; 6] = [RDI, RSI, RDX, R10, R8, R9];

// Linux/x86_64 syscall numbers (reach content — a per-target leak).
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_MMAP: u64 = 9;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;

enum SArg {
    Imm(u64),
    Slot(i32),
    OutPtr,
}

pub struct SysV64;

impl SysV64 {
    fn load_sarg(a: &mut Asm, reg: u8, arg: &SArg) {
        match arg {
            SArg::Imm(x) => a.mov_ri64(reg, *x),
            SArg::Slot(d) => a.mov_rm(reg, RBP, *d),
            SArg::OutPtr => a.lea(reg, RBP, Frame::OUTDW_DISP),
        }
    }
    /// emit one Linux syscall; result left in rax
    fn sc(a: &mut Asm, num: u64, args: &[SArg]) {
        for (i, arg) in args.iter().enumerate() {
            Self::load_sarg(a, SREG[i], arg);
        }
        a.mov_ri64(RAX, num);
        a.syscall();
    }
}

impl Target for SysV64 {
    fn name(&self) -> &'static str { "sysv64" }
    fn ctx_reg(&self) -> u8 { RDI }

    fn max_outgoing(&self, _m: &Module) -> i32 {
        // syscalls pass everything in registers; no outgoing stack area needed for
        // the syscall itself. Keep a small cushion for red-zone-free safety.
        0
    }

    fn emit_call(&self, a: &mut Asm, f: &Frame, intent: Intent, s: &[i32], dest: i32) {
        match intent {
            Intent::Alloc => {
                // mmap(0, n, PROT_READ|WRITE=3, MAP_PRIVATE|ANON=0x22, -1, 0)
                Self::sc(a, SYS_MMAP, &[
                    SArg::Imm(0), SArg::Slot(s[0]), SArg::Imm(3),
                    SArg::Imm(0x22), SArg::Imm(u64::MAX), SArg::Imm(0),
                ]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::FileOpen => {
                // open(path, O_RDONLY=0, 0)
                Self::sc(a, SYS_OPEN, &[SArg::Slot(s[0]), SArg::Imm(0), SArg::Imm(0)]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::FileRead => {
                // read(h, buf, cap)  -- returns count directly (no out-param needed)
                Self::sc(a, SYS_READ, &[SArg::Slot(s[0]), SArg::Slot(s[1]), SArg::Slot(s[2])]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::FileClose => {
                Self::sc(a, SYS_CLOSE, &[SArg::Slot(s[0])]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::WriteStdout => {
                // write(1, buf, len)
                Self::sc(a, SYS_WRITE, &[SArg::Imm(1), SArg::Slot(s[0]), SArg::Slot(s[1])]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::SpawnWait => emit_spawn(a, f, dest),
        }
    }
}

/// fork/execve/wait4 — the SysV/Linux realization of SpawnWait. Note it shares
/// NOTHING with the Win64 realization: no struct, different arity, different reach.
/// The neutral IR contributed only the intent tag; all substance is per-target.
/// Byte-measured only (not executed here).
fn emit_spawn(a: &mut Asm, f: &Frame, dest: i32) {
    let pid = f.scratch(0);
    let page = f.scratch(1);
    let lparent = a.new_label();

    // pid = fork()
    SysV64::sc(a, SYS_FORK, &[]);
    a.mov_mr(RBP, pid, RAX);
    a.test_rr(RAX, RAX);
    a.jnz(lparent);

    // ---- child ----
    // page = mmap(0,128,3,0x22,-1,0)
    SysV64::sc(a, SYS_MMAP, &[
        SArg::Imm(0), SArg::Imm(128), SArg::Imm(3), SArg::Imm(0x22), SArg::Imm(u64::MAX), SArg::Imm(0),
    ]);
    a.mov_mr(RBP, page, RAX);
    // lay out strings at page: [0]="/bin/sh\0" [8]="-c\0" [11]="exit 7\0"
    a.mov_rm(RAX, RBP, page);
    for (i, &c) in b"/bin/sh\0-c\0exit 7\0".iter().enumerate() {
        a.mov_mi8(RAX, i as i32, c);
    }
    // argv at page+32: [sh, -c, script, NULL]; envp at page+64: [NULL]
    // argv[0] = page+0
    a.mov_rm(RCX, RBP, page);
    a.mov_mr(RAX, 32, RCX);
    // argv[1] = page+8
    a.lea(RDX, RCX, 8);
    a.mov_mr(RAX, 40, RDX);
    // argv[2] = page+11
    a.lea(RDX, RCX, 11);
    a.mov_mr(RAX, 48, RDX);
    // argv[3] = 0 ; envp[0] = 0
    a.mov_ri64(RDX, 0);
    a.mov_mr(RAX, 56, RDX);
    a.mov_mr(RAX, 64, RDX);
    // execve(page+0, page+32, page+64)
    a.mov_rm(RDI, RBP, page); // path = page
    a.lea(RSI, RDI, 32);      // argv
    a.lea(RDX, RDI, 64);      // envp
    a.mov_ri64(RAX, SYS_EXECVE);
    a.syscall();
    // execve failed -> exit(127)
    SysV64::sc(a, SYS_EXIT, &[SArg::Imm(127)]);

    // ---- parent ----
    a.bind(lparent);
    // wait4(pid, &status, 0, 0)
    a.mov_mi32(RBP, Frame::OUTDW_DISP, 0);
    SysV64::sc(a, SYS_WAIT4, &[SArg::Slot(pid), SArg::OutPtr, SArg::Imm(0), SArg::Imm(0)]);
    // exit = (status >> 8) & 0xff
    a.mov_rm32(RAX, RBP, Frame::OUTDW_DISP);
    a.shr_ri(RAX, 8);
    a.and_ri(RAX, 0xff);
    a.mov_mr(RBP, dest, RAX);
}
