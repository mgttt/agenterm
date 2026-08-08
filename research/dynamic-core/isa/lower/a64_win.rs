//! Target B — AArch64 + Windows reach (symbol resolution). PER-TARGET (ISA x OS).
//!
//! Leak content per spec: the kernel32 symbol NAMES, the target-only constant args
//! (GENERIC_READ, OPEN_EXISTING, ...), and the STARTUPINFOA / PROCESS_INFORMATION
//! struct layout. CRUCIAL FINDING: every one of those is IDENTICAL to Q1's x86-64
//! Win64 target — the symbol strings, the constants, and the struct sizes/offsets
//! (STARTUPINFOA.cb == 104, hProcess at offset 0) are all *Windows-64* facts, not
//! ISA facts (both x64 and ARM64 Windows are LLP64 with 8-byte pointers). So the
//! per-target OS-content does NOT change with the ISA; only the instruction ENCODING
//! (per-ISA, in a64.rs/common_a64.rs) and the reach-into-ctx does. Arg placement is
//! AAPCS64 — the SAME as a64-linux (per-ISA), unlike x86 where SysV != Win64.
//!
//! EXECUTION: byte-measured only (no aarch64 host / no qemu).

use crate::a64::*;
use crate::common_a64::{place_args, CallArg, Frame, Target};
use crate::ir::*;

/// kernel32 symbols this target may reach. Binding index = position here. IDENTICAL
/// list to Q1's x86 Win64 target — this table is an OS leak, not an ISA one.
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

pub struct A64Win;

impl A64Win {
    /// Emit one Win64/AArch64 call to SYMBOLS[sym]; result left in x0.
    fn win_call(a: &mut A64, f: &Frame, sym: usize, args: &[CallArg]) {
        place_args(a, f, args, 8); // AAPCS64: 8 register args, rest on stack
        // reach: addr = ctx[0][sym]
        a.ldr(X11, SP, f.ctx_disp());
        a.ldr(X11, X11, 0);
        a.ldr(X11, X11, 8 * sym as i32);
        a.blr(X11);
    }
}

impl Target for A64Win {
    fn name(&self) -> &'static str { "a64-win" }
    fn ctx_reg(&self) -> u8 { X0 } // AAPCS64: first arg in x0 (x86 Win64 used rcx)

    fn max_outgoing(&self, m: &Module) -> i32 {
        if m.externs.is_empty() {
            return 0;
        }
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
        8 * widest.saturating_sub(8) as i32 // AAPCS64: no shadow space, spill past x7
    }

    fn emit_call(&self, a: &mut A64, f: &Frame, intent: Intent, s: &[i32], dest: i32) {
        match intent {
            Intent::Alloc => {
                Self::win_call(a, f, 0, &[CallArg::Imm(0), CallArg::Slot(s[0]), CallArg::Imm(0x3000), CallArg::Imm(0x04)]);
                a.str(X0, SP, dest);
            }
            Intent::FileOpen => {
                Self::win_call(a, f, 1, &[
                    CallArg::Slot(s[0]), CallArg::Imm(0x8000_0000), CallArg::Imm(1),
                    CallArg::Imm(0), CallArg::Imm(3), CallArg::Imm(0), CallArg::Imm(0),
                ]);
                a.str(X0, SP, dest);
            }
            Intent::FileRead => {
                a.str_w(XZR, SP, f.outdw_disp());
                Self::win_call(a, f, 2, &[
                    CallArg::Slot(s[0]), CallArg::Slot(s[1]), CallArg::Slot(s[2]),
                    CallArg::OutPtr, CallArg::Imm(0),
                ]);
                a.ldr_w(X0, SP, f.outdw_disp());
                a.str(X0, SP, dest);
            }
            Intent::FileClose => {
                Self::win_call(a, f, 3, &[CallArg::Slot(s[0])]);
                a.str(X0, SP, dest);
            }
            Intent::WriteStdout => {
                Self::win_call(a, f, 4, &[CallArg::Imm(0xFFFF_FFF5)]);
                a.str(X0, SP, dest);
                a.str_w(XZR, SP, f.outdw_disp());
                Self::win_call(a, f, 5, &[
                    CallArg::Slot(dest), CallArg::Slot(s[0]), CallArg::Slot(s[1]),
                    CallArg::OutPtr, CallArg::Imm(0),
                ]);
                a.str(X0, SP, dest);
            }
            Intent::SpawnWait => emit_spawn(a, f, dest),
        }
    }
}

/// STARTUPINFOA / PROCESS_INFORMATION realization. The struct sizes/offsets (104, 0)
/// are Windows-64 facts, IDENTICAL to the x86 Win64 lowering — only the instruction
/// encoding differs. Byte-measured only.
fn emit_spawn(a: &mut A64, f: &Frame, dest: i32) {
    let si = f.scratch(0);
    let pi = f.scratch(1);
    let cmd = f.scratch(2);
    let hproc = f.scratch(3);

    // si = VirtualAlloc(0,104,...); *(u32*)si = 104
    A64Win::win_call(a, f, 0, &[CallArg::Imm(0), CallArg::Imm(104), CallArg::Imm(0x3000), CallArg::Imm(0x04)]);
    a.str(X0, SP, si);
    a.ldr(X9, SP, si);
    a.mov_imm(X10, 104);
    a.str_w(X10, X9, 0); // STARTUPINFOA.cb = 104

    // pi = VirtualAlloc(0,24,...)
    A64Win::win_call(a, f, 0, &[CallArg::Imm(0), CallArg::Imm(24), CallArg::Imm(0x3000), CallArg::Imm(0x04)]);
    a.str(X0, SP, pi);

    // cmd = VirtualAlloc(0,32,...); copy "cmd.exe /c exit 7\0"
    A64Win::win_call(a, f, 0, &[CallArg::Imm(0), CallArg::Imm(32), CallArg::Imm(0x3000), CallArg::Imm(0x04)]);
    a.str(X0, SP, cmd);
    a.ldr(X9, SP, cmd);
    for (i, &c) in b"cmd.exe /c exit 7\0".iter().enumerate() {
        a.mov_imm(X10, c as u64);
        a.strb(X10, X9, i as i32);
    }

    // CreateProcessA(NULL, cmd, 0,0,0,0,0,0, si, pi) — 10 args, 2 on the stack.
    // si/pi slots already hold the struct POINTERS (VirtualAlloc returns).
    A64Win::win_call(a, f, 6, &[
        CallArg::Imm(0), CallArg::Slot(cmd), CallArg::Imm(0), CallArg::Imm(0), CallArg::Imm(0),
        CallArg::Imm(0), CallArg::Imm(0), CallArg::Imm(0), CallArg::Slot(si), CallArg::Slot(pi),
    ]);

    // hproc = *(HANDLE*)pi
    a.ldr(X9, SP, pi);
    a.ldr(X9, X9, 0);
    a.str(X9, SP, hproc);

    // WaitForSingleObject(hproc, INFINITE)
    A64Win::win_call(a, f, 7, &[CallArg::Slot(hproc), CallArg::Imm(0xFFFF_FFFF)]);

    // GetExitCodeProcess(hproc, &code)
    a.str_w(XZR, SP, f.outdw_disp());
    A64Win::win_call(a, f, 8, &[CallArg::Slot(hproc), CallArg::OutPtr]);
    a.ldr_w(X9, SP, f.outdw_disp());
    a.str(X9, SP, dest);

    // CloseHandle(hproc)
    A64Win::win_call(a, f, 3, &[CallArg::Slot(hproc)]);
}
