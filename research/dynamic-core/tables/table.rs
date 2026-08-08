//! Q7 — the DATA. Nothing here is executable logic; every item is a plain-data
//! description that the fixed marshaller (`marshal.rs`) interprets. Adding an intent
//! or a target must touch ONLY this file (spec §1.1). If a change forces an edit to
//! `marshal.rs`, that intent/target is NOT data-driven — recorded as a finding (⑤).
//!
//! The claim under test: Q1's per-target hand-written `emit_call` (win64.rs /
//! sysv64.rs, ~90-110 LOC/target of OS-interface content) becomes rows in these
//! tables, over one shared marshaller.

#![allow(dead_code)]

use crate::ir::Intent;

// ---------- L1: how a target reaches a callee (a per-target *mechanism*, data-selected) ----------
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReachKind {
    /// resolve a symbol string at bind time; its address sits in ctx[0][reach_id].
    Symbol,
    /// a raw syscall: `mov rax, reach_id; syscall`.
    Syscall,
}

// ---------- per-target ABI + reach descriptor: PURE DATA ----------
// This is the whole "ABI back-end" that Q1 measured at ~20-30 fixed LOC/target — here
// it is a handful of values, not code. The marshaller reads arg_regs/shadow/reach.
pub struct AbiDesc {
    pub name: &'static str,
    pub arg_regs: &'static [u8], // integer arg registers, in ABI order
    pub shadow: i32,             // shadow-space bytes above spilled args (Win64=32, SysV=0)
    pub reach: ReachKind,
    pub ctx_reg: u8,             // register holding the ctx pointer at entry
    /// the target's intent table — a DATA field, so the engine never branches on
    /// target identity to find OS content (spec §1.1: no per-target code).
    pub table: fn(Intent) -> Option<OpSpec>,
}

// ---------- L2/L4: where one native arg's value comes from ----------
#[derive(Clone, Copy)]
pub enum Arg {
    /// the i-th SEMANTIC arg from the neutral IR call (a value slot)
    Sem(usize),
    /// an injected target constant (L2: GENERIC_READ, OPEN_EXISTING, O_RDONLY, ...)
    Const(u64),
    /// pointer to the 4/8-byte out-param scratch (L4: ReadFile's &bytesRead)
    OutPtr,
    /// a value the host/kernel resolved into ctx[idx] (L1 relocated to bind time:
    /// e.g. a stdout HANDLE). ctx[0]=&symbols, ctx[1]=rodata, ctx[2]=stdout handle.
    CtxWord(usize),
    /// (L3 probe) pointer to a struct built from `structs[idx]` of this OpSpec.
    StructPtr(usize),
}

// ---------- L4: how the native result reaches the IR dest ----------
#[derive(Clone, Copy)]
pub enum Ret {
    /// result is in rax directly (SysV read returns count in rax)
    Direct,
    /// result is a `width`-byte field the call wrote through OutPtr (Win64 ReadFile)
    OutParam { width: u8 },
}

// ---------- L3 probe: a struct laid out as DATA (offsets/sizes/const fields) ----------
// This models ONLY the layout-fact half of L3 (Q1's L3a). A field whose value is a
// runtime pointer or a copied string is NOT expressible here — that is the boundary.
#[derive(Clone, Copy)]
pub enum FieldSrc {
    /// a constant field value baked in the table = §1.3(a) "producer knows layout"
    Const(u64),
    /// §1.3(b) CO-RE form: the offset itself is a QUERY resolved by the host oracle.
    /// The table carries a NAME, not the number 104. (value still Const here.)
    Queried { struct_name: &'static str, field: &'static str, value: u64 },
}
#[derive(Clone, Copy)]
pub struct FieldInit {
    pub off: u64,
    pub width: u8,
    pub src: FieldSrc,
}
pub struct StructSpec {
    pub size: u64,
    pub fields: &'static [FieldInit],
}

// ---------- one intent lowering for one target = one row of DATA ----------
pub struct OpSpec {
    pub reach_id: u64, // symbol index (Symbol) OR syscall number (Syscall)
    pub args: &'static [Arg],
    pub structs: &'static [StructSpec], // referenced by Arg::StructPtr
    pub ret: Ret,
}

// ================= TARGET: Win64 =================
pub const WIN64: AbiDesc = AbiDesc {
    name: "win64",
    arg_regs: &[crate::asm::RCX, crate::asm::RDX, crate::asm::R8, crate::asm::R9],
    shadow: 32,
    reach: ReachKind::Symbol,
    ctx_reg: crate::asm::RCX,
    table: win_table,
};

/// Symbols this target reaches; reach_id indexes here. Single-call family only.
pub const WIN_SYMBOLS: &[&str] = &[
    "VirtualAlloc", // 0
    "CreateFileA",  // 1
    "ReadFile",     // 2
    "CloseHandle",  // 3
    "WriteFile",    // 4
];

/// The Win64 OS-interface content, as DATA. Compare to win64.rs::emit_call (code).
pub fn win_table(intent: Intent) -> Option<OpSpec> {
    Some(match intent {
        Intent::Alloc => OpSpec {
            reach_id: 0, // VirtualAlloc(NULL, n, MEM_COMMIT|RESERVE, PAGE_RW)
            args: &[Arg::Const(0), Arg::Sem(0), Arg::Const(0x3000), Arg::Const(0x04)],
            structs: &[],
            ret: Ret::Direct,
        },
        Intent::FileOpen => OpSpec {
            reach_id: 1, // CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, 0, OPEN_EXISTING, 0, 0)
            args: &[
                Arg::Sem(0),
                Arg::Const(0x8000_0000),
                Arg::Const(1),
                Arg::Const(0),
                Arg::Const(3),
                Arg::Const(0),
                Arg::Const(0),
            ],
            structs: &[],
            ret: Ret::Direct,
        },
        Intent::FileRead => OpSpec {
            reach_id: 2, // ReadFile(h, buf, cap, &got, NULL) -> got is the 32-bit out
            args: &[Arg::Sem(0), Arg::Sem(1), Arg::Sem(2), Arg::OutPtr, Arg::Const(0)],
            structs: &[],
            ret: Ret::OutParam { width: 4 },
        },
        Intent::FileClose => OpSpec {
            reach_id: 3, // CloseHandle(h)
            args: &[Arg::Sem(0)],
            structs: &[],
            ret: Ret::Direct,
        },
        Intent::WriteStdout => OpSpec {
            // WriteFile(stdout, buf, len, &wrote, NULL). The stdout HANDLE is resolved
            // by the host into ctx[2] — L1's "GetStdHandle" is relocated to bind time,
            // keeping this a SINGLE native call (see RESULTS: this relocation is the
            // price of staying single-call; noted, not hidden).
            reach_id: 4,
            args: &[Arg::CtxWord(2), Arg::Sem(0), Arg::Sem(1), Arg::OutPtr, Arg::Const(0)],
            structs: &[],
            ret: Ret::Direct,
        },
        // SpawnWait: NOT in the single-call table. See spawn_boundary() below.
        Intent::SpawnWait => return None,
    })
}

// ================= TARGET: SysV x86_64 =================
pub const SYSV64: AbiDesc = AbiDesc {
    name: "sysv64",
    arg_regs: &[
        crate::asm::RDI,
        crate::asm::RSI,
        crate::asm::RDX,
        crate::asm::R10,
        crate::asm::R8,
        crate::asm::R9,
    ],
    shadow: 0,
    reach: ReachKind::Syscall,
    ctx_reg: crate::asm::RDI,
    table: sysv_table,
};

/// The SysV OS-interface content, as DATA. reach_id = Linux/x86_64 syscall number.
/// Same INTENT keys as Win64, DIFFERENT data — the per-target divergence Q1 noted
/// (L1 number-vs-string, L4 direct-vs-outparam) shows up purely as different rows.
pub fn sysv_table(intent: Intent) -> Option<OpSpec> {
    Some(match intent {
        Intent::Alloc => OpSpec {
            reach_id: 9, // mmap(0, n, PROT_RW=3, MAP_PRIVATE|ANON=0x22, -1, 0)
            args: &[
                Arg::Const(0),
                Arg::Sem(0),
                Arg::Const(3),
                Arg::Const(0x22),
                Arg::Const(u64::MAX),
                Arg::Const(0),
            ],
            structs: &[],
            ret: Ret::Direct,
        },
        Intent::FileOpen => OpSpec {
            reach_id: 2, // open(path, O_RDONLY=0, 0)
            args: &[Arg::Sem(0), Arg::Const(0), Arg::Const(0)],
            structs: &[],
            ret: Ret::Direct,
        },
        Intent::FileRead => OpSpec {
            reach_id: 0, // read(h, buf, cap) -> count in rax (no out-param!)
            args: &[Arg::Sem(0), Arg::Sem(1), Arg::Sem(2)],
            structs: &[],
            ret: Ret::Direct, // <-- L4 divergence captured as data
        },
        Intent::FileClose => OpSpec {
            reach_id: 3, // close(h)
            args: &[Arg::Sem(0)],
            structs: &[],
            ret: Ret::Direct,
        },
        Intent::WriteStdout => OpSpec {
            reach_id: 1, // write(1, buf, len) — fd 1 is a plain constant here
            args: &[Arg::Const(1), Arg::Sem(0), Arg::Sem(1)],
            structs: &[],
            ret: Ret::Direct,
        },
        Intent::SpawnWait => return None,
    })
}

// ---------- L3 probe artifacts: the layout-fact half of spawn, AS DATA ----------
// This demonstrates L3a: the STARTUPINFOA layout tablifies. What does NOT tablify is
// the orchestration around it (L3b) — see marshal.rs::spawn_boundary and RESULTS ⑤.
pub const STARTUPINFOA: StructSpec = StructSpec {
    size: 104,
    fields: &[FieldInit {
        off: 0,
        width: 4,
        // §1.3(b): the offset of cb is a NAME resolved by the host oracle, and its
        // value (the struct's own size) likewise. The table holds no bare 104/0.
        src: FieldSrc::Queried { struct_name: "STARTUPINFOA", field: "cb", value: 104 },
    }],
};
pub const PROCESS_INFORMATION: StructSpec = StructSpec {
    size: 24,
    fields: &[], // hProcess at offset 0 is an OUTPUT the OS fills — see L3b discussion
};
