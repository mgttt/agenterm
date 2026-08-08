//! Lowerer B — Win64 ABI + Windows reach (symbol resolution).
//!
//! TARGET-SPECIFIC. Everything in this file is a leak candidate per spec §②: the
//! Win64 arg placement (rcx,rdx,r8,r9 + 32-byte shadow + stack spill), the kernel32
//! symbol names, the target-only constant args (GENERIC_READ, OPEN_EXISTING, ...),
//! and — the headline — the STARTUPINFOA / PROCESS_INFORMATION struct layout that
//! CreateProcessA requires but the neutral IR is forbidden to describe.
//!
//! ctx (passed in rcx) points at a u64[]: ctx[0] = &bindings[], ctx[1] = rodata base.
//! bindings[k] = resolved address of the k-th kernel32 symbol (see SYMBOLS).

use crate::asm::*;
use crate::common::{Frame, Target};
use crate::ir::*;

/// The kernel32 symbols this target may reach. Binding index = position here.
/// This list IS part of the leak: it grows with every OS capability.
pub const SYMBOLS: &[&str] = &[
    "VirtualAlloc",         // 0
    "CreateFileA",          // 1
    "ReadFile",             // 2
    "CloseHandle",          // 3
    "GetStdHandle",         // 4
    "WriteFile",            // 5
    "CreateProcessA",       // 6
    "WaitForSingleObject",  // 7
    "GetExitCodeProcess",   // 8
];

// Win64 integer arg registers.
const AREG: [u8; 4] = [RCX, RDX, R8, R9];

enum NArg {
    Imm(u64),
    Slot(i32),
    OutPtr, // &[rbp+OUTDW_DISP]
}

pub struct Win64;

impl Win64 {
    fn load_narg(a: &mut Asm, reg: u8, arg: &NArg) {
        match arg {
            NArg::Imm(x) => a.mov_ri64(reg, *x),
            NArg::Slot(d) => a.mov_rm(reg, RBP, *d),
            NArg::OutPtr => a.lea(reg, RBP, Frame::OUTDW_DISP),
        }
    }
    /// Emit one native Win64 call to SYMBOLS[sym], leaving the return in rax.
    fn win_call(a: &mut Asm, sym: usize, args: &[NArg]) {
        for (i, arg) in args.iter().enumerate() {
            if i < 4 {
                Self::load_narg(a, AREG[i], arg);
            } else {
                Self::load_narg(a, RAX, arg);
                a.mov_mr(RSP, 32 + 8 * (i as i32 - 4), RAX); // above the 32B shadow
            }
        }
        // reach: addr = ctx[0][sym]
        a.mov_rm(R11, RBP, Frame::CTX_DISP);
        a.mov_rm(R11, R11, 0);
        a.mov_rm(RAX, R11, 8 * sym as i32);
        a.call_r(RAX);
    }
}

impl Target for Win64 {
    fn name(&self) -> &'static str { "win64" }
    fn ctx_reg(&self) -> u8 { RCX }

    fn max_outgoing(&self, m: &Module) -> i32 {
        // A leaf function (no calls) needs no outgoing area at all — including no
        // shadow space. Only functions that call reserve the 32-byte shadow.
        if m.externs.is_empty() {
            return 0;
        }
        // widest native call. CreateProcessA = 10 args -> 6 stack. shadow=32.
        let mut widest = 0usize;
        for e in &m.externs {
            let n = match e.intent {
                Intent::Alloc => 4,
                Intent::FileOpen => 7,
                Intent::FileRead => 5,
                Intent::FileClose => 1,
                Intent::WriteStdout => 5,
                Intent::SpawnWait => 10, // CreateProcessA
            };
            widest = widest.max(n);
        }
        32 + 8 * widest.saturating_sub(4) as i32
    }

    fn emit_call(&self, a: &mut Asm, f: &Frame, intent: Intent, s: &[i32], dest: i32) {
        match intent {
            Intent::Alloc => {
                // VirtualAlloc(NULL, n, MEM_COMMIT|RESERVE, PAGE_READWRITE)
                Self::win_call(a, 0, &[NArg::Imm(0), NArg::Slot(s[0]), NArg::Imm(0x3000), NArg::Imm(0x04)]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::FileOpen => {
                // CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, 0, OPEN_EXISTING, 0, 0)
                Self::win_call(a, 1, &[
                    NArg::Slot(s[0]), NArg::Imm(0x8000_0000), NArg::Imm(1),
                    NArg::Imm(0), NArg::Imm(3), NArg::Imm(0), NArg::Imm(0),
                ]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::FileRead => {
                // ReadFile(h, buf, cap, &got, NULL); result n is the out DWORD.
                a.mov_mi32(RBP, Frame::OUTDW_DISP, 0);
                Self::win_call(a, 2, &[
                    NArg::Slot(s[0]), NArg::Slot(s[1]), NArg::Slot(s[2]),
                    NArg::OutPtr, NArg::Imm(0),
                ]);
                a.mov_rm32(RAX, RBP, Frame::OUTDW_DISP); // n (zero-extended)
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::FileClose => {
                Self::win_call(a, 3, &[NArg::Slot(s[0])]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::WriteStdout => {
                // h = GetStdHandle(STD_OUTPUT_HANDLE); WriteFile(h, buf, len, &wrote, NULL)
                Self::win_call(a, 4, &[NArg::Imm(0xFFFF_FFF5)]);
                a.mov_mr(RBP, dest, RAX); // stash handle in dest slot
                a.mov_mi32(RBP, Frame::OUTDW_DISP, 0);
                Self::win_call(a, 5, &[
                    NArg::Slot(dest), NArg::Slot(s[0]), NArg::Slot(s[1]),
                    NArg::OutPtr, NArg::Imm(0),
                ]);
                a.mov_mr(RBP, dest, RAX);
            }
            Intent::SpawnWait => emit_spawn(a, f, dest),
        }
    }
}

/// The known hard bone. NONE of this is expressible in the neutral IR: the
/// STARTUPINFOA size (104) and cb offset, the PROCESS_INFORMATION hProcess offset,
/// and the literal command line are all target ABI/OS facts. They live here, in the
/// lowerer, entirely per-target. The SysV lowering of the same intent (fork/execve)
/// shares literally nothing with this.
fn emit_spawn(a: &mut Asm, f: &Frame, dest: i32) {
    let si = f.scratch(0);
    let pi = f.scratch(1);
    let cmd = f.scratch(2);
    let hproc = f.scratch(3);

    // si = VirtualAlloc(0,104,...); *(u32*)si = 104   <-- STRUCT LAYOUT LEAK
    Win64::win_call(a, 0, &[NArg::Imm(0), NArg::Imm(104), NArg::Imm(0x3000), NArg::Imm(0x04)]);
    a.mov_mr(RBP, si, RAX);
    a.mov_rm(RAX, RBP, si);
    a.mov_mi32(RAX, 0, 104); // STARTUPINFOA.cb = sizeof(STARTUPINFOA)

    // pi = VirtualAlloc(0,24,...)
    Win64::win_call(a, 0, &[NArg::Imm(0), NArg::Imm(24), NArg::Imm(0x3000), NArg::Imm(0x04)]);
    a.mov_mr(RBP, pi, RAX);

    // cmd = VirtualAlloc(0,32,...); copy "cmd.exe /c exit 7\0"  <-- WINDOWS STRING LEAK
    Win64::win_call(a, 0, &[NArg::Imm(0), NArg::Imm(32), NArg::Imm(0x3000), NArg::Imm(0x04)]);
    a.mov_mr(RBP, cmd, RAX);
    a.mov_rm(RAX, RBP, cmd);
    let s = b"cmd.exe /c exit 7\0";
    for (i, &c) in s.iter().enumerate() {
        a.mov_mi8(RAX, i as i32, c);
    }

    // CreateProcessA(NULL, cmd, 0,0,0,0,0,0, &si, &pi)  -- 10 args, 6 on the stack
    Win64::win_call(a, 6, &[
        NArg::Imm(0), NArg::Slot(cmd), NArg::Imm(0), NArg::Imm(0), NArg::Imm(0),
        NArg::Imm(0), NArg::Imm(0), NArg::Imm(0), NArg::Slot(si), NArg::Slot(pi),
    ]);

    // hproc = *(HANDLE*)pi   <-- PROCESS_INFORMATION.hProcess at offset 0 (LEAK)
    a.mov_rm(RAX, RBP, pi);
    a.mov_rm(RAX, RAX, 0);
    a.mov_mr(RBP, hproc, RAX);

    // WaitForSingleObject(hproc, INFINITE)
    Win64::win_call(a, 7, &[NArg::Slot(hproc), NArg::Imm(0xFFFF_FFFF)]);

    // GetExitCodeProcess(hproc, &code)
    a.mov_mi32(RBP, Frame::OUTDW_DISP, 0);
    Win64::win_call(a, 8, &[NArg::Slot(hproc), NArg::OutPtr]);
    a.mov_rm32(RAX, RBP, Frame::OUTDW_DISP);
    a.mov_mr(RBP, dest, RAX);

    // CloseHandle(hproc)
    Win64::win_call(a, 3, &[NArg::Slot(hproc)]);
}
