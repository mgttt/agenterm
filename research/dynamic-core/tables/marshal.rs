//! Q7 — the fixed generic MARSHALLER (one mechanism, all intents, all targets).
//!
//! This file is CODE. Its size (③) and its invariance to intent/target count (②) are
//! the whole measurement. Discipline (spec §1.1): there is NO `match intent` here. An
//! intent's OS-interface content arrives entirely as an `OpSpec` from `table.rs`.
//!
//! The non-Call lowering (arith / mem / control / prologue) is copied VERBATIM from
//! Q1's `common.rs` so those bytes match Q1 exactly; only the Call path is replaced by
//! the table interpreter. That isolation is what lets ② attribute all growth to data.

use crate::asm::*;
use crate::ir::*;
use crate::table::*;

// ---- Frame geometry: identical to Q1 common.rs ----
pub struct Frame {
    pub frame_size: i32,
    pub takes_ctx: bool,
    pub n_vals: i32,
}
impl Frame {
    pub const CTX_DISP: i32 = -8;
    pub const OUTDW_DISP: i32 = -16;
    const VAL0: i32 = -24;
    pub const SCRATCH_N: i32 = 8;
    pub fn slot(&self, v: Val) -> i32 {
        Self::VAL0 - 8 * (v as i32)
    }
    pub fn scratch(&self, i: i32) -> i32 {
        Self::VAL0 - 8 * (self.n_vals + i)
    }
}

// ============================================================================
// THE GENERIC MARSHALLER — table interpreter. No per-intent, no per-target code.
// ============================================================================

/// Widest outgoing-arg area any single call in this module needs, computed from the
/// DATA (the arg lists) + the ABI descriptor. Not per-intent code: it walks the table.
fn max_outgoing(abi: &AbiDesc, m: &Module, tbl: fn(Intent) -> Option<OpSpec>) -> i32 {
    let mut widest = 0usize;
    let mut any = false;
    for e in &m.externs {
        if let Some(spec) = tbl(e.intent) {
            any = true;
            widest = widest.max(spec.args.len());
        }
    }
    if !any {
        return 0; // leaf function: no outgoing area, not even shadow
    }
    let nreg = abi.arg_regs.len();
    abi.shadow + 8 * widest.saturating_sub(nreg) as i32
}

/// Emit one native call entirely from `spec` (data). This is the ONLY place OS-
/// interface content is realized, and it is intent-agnostic and target-agnostic.
fn emit_intent(a: &mut Asm, f: &Frame, abi: &AbiDesc, spec: &OpSpec, sem: &[i32], dest: i32) {
    // 1. materialize any struct args into scratch (L3a layout-from-data).
    //    A struct pointer, once built, is read from a scratch slot as an ordinary arg.
    let mut struct_slots = [0i32; 4];
    for (j, st) in spec.structs.iter().enumerate() {
        struct_slots[j] = build_struct(a, f, abi, st, j);
    }

    // 2. if any arg is an out-param, zero the 4-byte scratch first.
    if spec.args.iter().any(|x| matches!(x, Arg::OutPtr)) {
        a.mov_mi32(RBP, Frame::OUTDW_DISP, 0);
    }

    // 3. place each native arg per the ABI descriptor (registers then spill).
    let nreg = abi.arg_regs.len();
    for (i, arg) in spec.args.iter().enumerate() {
        if i < nreg {
            load_arg(a, f, abi, arg, abi.arg_regs[i], sem, &struct_slots);
        } else {
            load_arg(a, f, abi, arg, RAX, sem, &struct_slots);
            a.mov_mr(RSP, abi.shadow + 8 * (i as i32 - nreg as i32), RAX);
        }
    }

    // 4. reach the callee — mechanism selected by DATA (abi.reach), not by intent.
    match abi.reach {
        ReachKind::Symbol => {
            a.mov_rm(R11, RBP, Frame::CTX_DISP); // r11 = ctx
            a.mov_rm(R11, R11, 0); // r11 = ctx[0] = &symbols
            a.mov_rm(RAX, R11, 8 * spec.reach_id as i32); // rax = symbols[reach_id]
            a.call_r(RAX);
        }
        ReachKind::Syscall => {
            a.mov_ri64(RAX, spec.reach_id);
            a.syscall();
        }
    }

    // 5. deliver the result to `dest` — again from DATA (spec.ret).
    match spec.ret {
        Ret::Direct => a.mov_mr(RBP, dest, RAX),
        Ret::OutParam { width } => {
            match width {
                4 => a.mov_rm32(RAX, RBP, Frame::OUTDW_DISP),
                _ => a.mov_rm(RAX, RBP, Frame::OUTDW_DISP),
            }
            a.mov_mr(RBP, dest, RAX);
        }
    }
}

fn load_arg(a: &mut Asm, _f: &Frame, _abi: &AbiDesc, arg: &Arg, reg: u8, sem: &[i32], structs: &[i32; 4]) {
    match arg {
        // Sem(k) = the k-th SEMANTIC arg of this call; its slot is sem[k].
        Arg::Sem(k) => a.mov_rm(reg, RBP, sem[*k]),
        Arg::Const(x) => a.mov_ri64(reg, *x),
        Arg::OutPtr => a.lea(reg, RBP, Frame::OUTDW_DISP),
        Arg::CtxWord(idx) => {
            a.mov_rm(reg, RBP, Frame::CTX_DISP);
            a.mov_rm(reg, reg, 8 * *idx as i32);
        }
        Arg::StructPtr(j) => a.mov_rm(reg, RBP, structs[*j]),
    }
}

/// Build a struct into a scratch slot from a `StructSpec` (DATA). Returns the slot
/// holding the struct pointer. This is L3a: the layout facts are consumed from data.
/// It calls Alloc through the SAME table (recursion into the marshaller), so no new
/// mechanism. NOTE what is *absent*: this only writes CONST/queried fields. A field
/// that must hold a runtime pointer or a copied byte-string is unreachable here — the
/// boundary (⑤).
fn build_struct(a: &mut Asm, f: &Frame, abi: &AbiDesc, st: &StructSpec, j: usize) -> i32 {
    let slot = f.scratch(j as i32);
    // p = Alloc(size)  — reuse the Alloc row of whichever table this target uses.
    let alloc = alloc_spec(abi);
    // inline a minimal single-arg Alloc call (size is a constant here).
    let nreg = abi.arg_regs.len();
    for (i, arg) in alloc.args.iter().enumerate() {
        let a2 = match arg {
            Arg::Sem(_) => Arg::Const(st.size), // the payload's size arg -> our const
            other => *other,
        };
        if i < nreg {
            load_arg(a, f, abi, &a2, abi.arg_regs[i], &[], &[0; 4]);
        } else {
            load_arg(a, f, abi, &a2, RAX, &[], &[0; 4]);
            a.mov_mr(RSP, abi.shadow + 8 * (i as i32 - nreg as i32), RAX);
        }
    }
    match abi.reach {
        ReachKind::Symbol => {
            a.mov_rm(R11, RBP, Frame::CTX_DISP);
            a.mov_rm(R11, R11, 0);
            a.mov_rm(RAX, R11, 8 * alloc.reach_id as i32);
            a.call_r(RAX);
        }
        ReachKind::Syscall => {
            a.mov_ri64(RAX, alloc.reach_id);
            a.syscall();
        }
    }
    a.mov_mr(RBP, slot, RAX); // slot = base
    // write each field (CONST/queried only)
    for fld in st.fields {
        let v = match fld.src {
            FieldSrc::Const(v) => v,
            FieldSrc::Queried { value, .. } => value, // oracle-resolved at bind time
        };
        a.mov_rm(RAX, RBP, slot);
        match fld.width {
            4 => a.mov_mi32(RAX, fld.off as i32, v as i32),
            _ => a.mov_mi32(RAX, fld.off as i32, v as i32),
        }
    }
    slot
}

fn alloc_spec(abi: &AbiDesc) -> OpSpec {
    (abi.table)(Intent::Alloc).expect("Alloc must be in every target table")
}

// ============================================================================
// Non-Call lowering: copied VERBATIM from Q1 common.rs (arith/mem/control).
// ============================================================================

pub fn lower(m: &Module, abi: &AbiDesc) -> Vec<u8> {
    let n_vals = m.n_vals as i32;
    let tbl = abi.table;
    let max_out = max_outgoing(abi, m, tbl);
    let mut frame_size = 24 + 8 * (n_vals + Frame::SCRATCH_N) + max_out;
    frame_size = (frame_size + 15) & !15;
    let f = Frame { frame_size, takes_ctx: m.takes_ctx, n_vals };

    let mut a = Asm::new();
    let block_labels: Vec<u32> = (0..m.blocks.len()).map(|_| a.new_label()).collect();

    a.push(RBP);
    a.mov_rr(RBP, RSP);
    a.sub_rsp(frame_size);
    if m.takes_ctx {
        a.mov_mr(RBP, Frame::CTX_DISP, abi.ctx_reg);
    }
    a.jmp(block_labels[m.entry as usize]);

    for (bi, blk) in m.blocks.iter().enumerate() {
        a.bind(block_labels[bi]);
        for inst in &blk.insts {
            lower_inst(&mut a, &f, abi, m, inst);
        }
        match &blk.term {
            Term::Br(x) => a.jmp(block_labels[*x as usize]),
            Term::BrCond(c, nz, z) => {
                a.mov_rm(RAX, RBP, f.slot(*c));
                a.test_rr(RAX, RAX);
                a.jnz(block_labels[*nz as usize]);
                a.jmp(block_labels[*z as usize]);
            }
            Term::Ret(v) | Term::Exit(v) => {
                a.mov_rm(RAX, RBP, f.slot(*v));
                a.add_rsp(frame_size);
                a.pop(RBP);
                a.ret();
            }
        }
    }
    a.finish()
}

fn lower_inst(a: &mut Asm, f: &Frame, abi: &AbiDesc, m: &Module, inst: &Inst) {
    match inst {
        Inst::Set(d, op) => {
            lower_op(a, f, op);
            a.mov_mr(RBP, f.slot(*d), RAX);
        }
        Inst::Store8(addr, v) => {
            a.mov_rm(RAX, RBP, f.slot(*addr));
            a.mov_rm(RCX, RBP, f.slot(*v));
            a.mov_mr8(RAX, 0, RCX);
        }
        Inst::StoreW(addr, v) => {
            a.mov_rm(RAX, RBP, f.slot(*addr));
            a.mov_rm(RCX, RBP, f.slot(*v));
            a.mov_mr(RAX, 0, RCX);
        }
        Inst::Call(d, id, args) => {
            let intent = m.externs[*id as usize].intent;
            let tbl = abi.table;
            let spec = tbl(intent).unwrap_or_else(|| {
                panic!(
                    "intent {:?} has no table row for target {} — NOT data-driven (⑤)",
                    intent, abi.name
                )
            });
            let slots: Vec<i32> = args.iter().map(|v| f.slot(*v)).collect();
            emit_intent(a, f, abi, &spec, &slots, f.slot(*d));
        }
    }
}

fn lower_op(a: &mut Asm, f: &Frame, op: &Op) {
    match op {
        Op::Const(x) => a.mov_ri64(RAX, *x),
        Op::Rodata(off) => {
            a.mov_rm(RAX, RBP, Frame::CTX_DISP);
            a.mov_rm(RAX, RAX, 8);
            if *off != 0 {
                a.add_ri(RAX, *off as i32);
            }
        }
        Op::Add(x, y) => bin(a, f, *x, *y, |a| a.add_rr(RAX, RCX)),
        Op::Sub(x, y) => bin(a, f, *x, *y, |a| a.sub_rr(RAX, RCX)),
        Op::Mul(x, y) => bin(a, f, *x, *y, |a| a.imul_rr(RAX, RCX)),
        Op::Xor(x, y) => bin(a, f, *x, *y, |a| a.xor_rr(RAX, RCX)),
        Op::And(x, y) => bin(a, f, *x, *y, |a| a.and_rr(RAX, RCX)),
        Op::Or(x, y) => bin(a, f, *x, *y, |a| a.or_rr(RAX, RCX)),
        Op::Ult(x, y) => {
            a.mov_rm(RAX, RBP, f.slot(*x));
            a.mov_rm(RCX, RBP, f.slot(*y));
            a.cmp_rr(RAX, RCX);
            a.setb_eax();
        }
        Op::Shl(x, s) => {
            a.mov_rm(RAX, RBP, f.slot(*x));
            a.shl_ri(RAX, *s);
        }
        Op::Shr(x, s) => {
            a.mov_rm(RAX, RBP, f.slot(*x));
            a.shr_ri(RAX, *s);
        }
        Op::Load8(x) => {
            a.mov_rm(RAX, RBP, f.slot(*x));
            a.movzx8(RAX, RAX, 0);
        }
        Op::LoadW(x) => {
            a.mov_rm(RAX, RBP, f.slot(*x));
            a.mov_rm(RAX, RAX, 0);
        }
    }
}
fn bin(a: &mut Asm, f: &Frame, x: Val, y: Val, op: impl Fn(&mut Asm)) {
    a.mov_rm(RAX, RBP, f.slot(x));
    a.mov_rm(RCX, RBP, f.slot(y));
    op(a);
}

// ============================================================================
// ④ — schema validator over the DATA. Checks the table is well-formed. This is a
// STRUCTURAL check that Q1's hand-written code could not receive. It reports both
// what it CAN check and — honestly — what it cannot (the naming truth of L1).
// ============================================================================

pub struct Report {
    pub checked: usize,
    pub errors: Vec<String>,
}

pub fn validate(abi: &AbiDesc, intents: &[(Intent, usize)]) -> Report {
    let tbl = abi.table;
    let mut r = Report { checked: 0, errors: Vec::new() };
    for &(intent, sem_arity) in intents {
        let spec = match tbl(intent) {
            Some(s) => s,
            None => continue,
        };
        r.checked += 1;
        // L1: symbol reach must index a bound symbol; syscall# is unconstrainable.
        if abi.reach == ReachKind::Symbol && spec.reach_id as usize >= WIN_SYMBOLS.len() {
            r.errors.push(format!("{intent:?}: reach_id {} out of symbol table", spec.reach_id));
        }
        // L2/L4: every arg must reference something in range.
        for (i, arg) in spec.args.iter().enumerate() {
            match arg {
                Arg::Sem(k) if *k >= sem_arity => {
                    r.errors.push(format!("{intent:?}: arg{i} Sem({k}) >= arity {sem_arity}"));
                }
                Arg::CtxWord(idx) if *idx >= 3 => {
                    r.errors.push(format!("{intent:?}: arg{i} CtxWord({idx}) out of ctx"));
                }
                Arg::StructPtr(j) if *j >= spec.structs.len() => {
                    r.errors.push(format!("{intent:?}: arg{i} StructPtr({j}) out of structs"));
                }
                _ => {}
            }
        }
        // L4: out-param width must be 4 or 8.
        if let Ret::OutParam { width } = spec.ret {
            if width != 4 && width != 8 {
                r.errors.push(format!("{intent:?}: out-param width {width} not in {{4,8}}"));
            }
        }
        // L3a: struct field offsets must fit inside the struct.
        for st in spec.structs {
            for fld in st.fields {
                if fld.off + fld.width as u64 > st.size {
                    r.errors.push(format!("{intent:?}: field at {} exceeds struct size {}", fld.off, st.size));
                }
            }
        }
    }
    r
}

// ============================================================================
// ⑤ — the boundary. spawn_boundary() does NOT emit code; it classifies each fact of
// SpawnWait as {tablable | needs-query-channel | not-data}, so the RESULTS list is
// grounded in the actual operation, not asserted.
// ============================================================================

pub struct Fact {
    pub name: &'static str,
    pub verdict: &'static str, // "data" | "query" | "code"
    pub why: &'static str,
}

pub fn spawn_boundary() -> Vec<Fact> {
    vec![
        Fact { name: "STARTUPINFOA.cb size/offset", verdict: "query",
               why: "L3a: a bounded field-offset relocation; CO-RE-style host oracle answers it (table.rs STARTUPINFOA)" },
        Fact { name: "PROCESS_INFORMATION layout", verdict: "query",
               why: "L3a: field offsets tablify the same way" },
        Fact { name: "command-line byte string 'cmd.exe /c exit 7'", verdict: "data",
               why: "a rodata blob — already data (payload rodata)" },
        Fact { name: "extract hProcess from PROCESS_INFORMATION output", verdict: "code",
               why: "L3b: a runtime pointer read whose result feeds the NEXT call — dataflow between calls, not a single-call arg recipe" },
        Fact { name: "sequence CreateProcess -> Wait -> GetExitCode -> Close", verdict: "code",
               why: "L3b: multi-call ORCHESTRATION with data dependencies. Making it data = a call-sequencing bytecode = the IDL slide (spec §1.2)" },
        Fact { name: "SysV fork() then BRANCH on pid; child execve, parent wait4", verdict: "code",
               why: "L3b: two divergent control-flow paths. No flat table expresses a conditional branch; this is the hardest wall" },
        Fact { name: "SysV child argv[]/envp[] pointer-array construction", verdict: "code",
               why: "L3b: building an array of pointers-into-a-buffer at runtime — pointer arithmetic, not constant fields" },
        Fact { name: "error/sentinel handling (L5): (HANDLE)-1, negative return", verdict: "code",
               why: "recording the sentinel value is data; ACTING on it needs a branch = control flow (Q4 judged L5 undecidable)" },
    ]
}
