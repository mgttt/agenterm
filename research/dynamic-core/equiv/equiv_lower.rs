//! `mod common` for the Q4 equivalence experiment.
//!
//! This is Q1's `ir/lower/common.rs` with **region-boundary recording added** and
//! NOTHING else changed. The `Frame`/`Target`/`set_externs` definitions and the entire
//! emit logic (`lower_inst`/`lower_op`/`bin`) are reproduced VERBATIM so the emitted
//! bytes are byte-for-byte identical to Q1 (spec §1.2: this experiment must not change
//! the lowering products). The reused `ir/lower/{sysv64,win64}.rs` import
//! `crate::common::{Frame, Target}`, which resolve to the definitions here.
//!
//! Why a copy and not a `#[path]` re-use of Q1's file: Q1's `lower_inst`/`lower_op`
//! are private, so region boundaries (prologue / each intent-call / terminators)
//! cannot be observed by driving Q1's `lower` from outside. The copy adds a region
//! sink; it emits no extra bytes (spec §5).

use crate::asm::*;
use crate::ir::*;

// ---------------- Frame + Target: VERBATIM from Q1 common.rs ----------------

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

pub trait Target {
    fn ctx_reg(&self) -> u8;
    fn max_outgoing(&self, m: &Module) -> i32;
    fn emit_call(&self, a: &mut Asm, f: &Frame, intent: Intent, argslots: &[i32], dest: i32);
    fn name(&self) -> &'static str;
}

use std::cell::RefCell;
thread_local! {
    static EXTERNS: RefCell<Vec<ExternDecl>> = RefCell::new(Vec::new());
}
pub fn set_externs(e: &[ExternDecl]) {
    EXTERNS.with(|c| *c.borrow_mut() = e.to_vec());
}
fn intent_of(id: u32) -> Intent {
    EXTERNS.with(|c| c.borrow()[id as usize].intent)
}

// ---------------- Region model (NEW; the only addition) ----------------

/// Why a byte span of the emitted image is what it is. The whole point of Q4 is that
/// only some kinds can carry a *structural* equivalence guard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RegionKind {
    /// straight-line ABI/OS-independent code. MUST be byte-identical across targets.
    Neutral,
    /// `sub rsp,N` / `add rsp,N`. Same opcode; the immediate `N` (frame size) is an
    /// ABI fact (Win64 reserves shadow+spill, SysV reserves 0). Mechanics divergence.
    Frame,
    /// prologue store of the incoming ctx pointer. Same opcode; the source register is
    /// ABI-mandated (Win64 rcx vs SysV rdi). Mechanics divergence.
    CtxReg,
    /// a control transfer (jmp/jcc). Same opcode + same target block; the rel32 bytes
    /// differ only because upstream region lengths differ (jump-offset drift).
    Control(u32),
    /// an intent call: the OS-interface content. NO neutral form; the real leak.
    Intent(Intent),
}

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub kind: RegionKind,
    pub start: usize,
    pub end: usize,
}
impl Region {
    pub fn len(&self) -> usize {
        self.end - self.start
    }
}

struct Rec {
    regions: Vec<Region>,
    mark: usize,
}
impl Rec {
    fn new() -> Self {
        Rec { regions: Vec::new(), mark: 0 }
    }
    /// close a Neutral span up to the current code length
    fn flush_neutral(&mut self, a: &Asm) {
        let end = a.code.len();
        if end > self.mark {
            self.regions.push(Region { kind: RegionKind::Neutral, start: self.mark, end });
        }
        self.mark = end;
    }
    /// after emitting a special instruction, record its span as `kind`
    fn special(&mut self, a: &Asm, kind: RegionKind) {
        let end = a.code.len();
        self.regions.push(Region { kind, start: self.mark, end });
        self.mark = end;
    }
}

// ---------------- instrumented lower (the driver loop, region-aware) ----------------

pub fn lower_regions(m: &Module, t: &dyn Target) -> (Vec<u8>, Vec<Region>) {
    let n_vals = m.n_vals as i32;
    let max_out = t.max_outgoing(m);
    let mut frame_size = 24 + 8 * (n_vals + Frame::SCRATCH_N) + max_out;
    frame_size = (frame_size + 15) & !15;
    let f = Frame { frame_size, takes_ctx: m.takes_ctx, n_vals };

    let mut a = Asm::new();
    let mut r = Rec::new();
    let block_labels: Vec<u32> = (0..m.blocks.len()).map(|_| a.new_label()).collect();

    // ---- prologue ----
    a.push(RBP);
    a.mov_rr(RBP, RSP);
    r.flush_neutral(&a); // push+mov are neutral
    a.sub_rsp(frame_size);
    r.special(&a, RegionKind::Frame);
    if m.takes_ctx {
        a.mov_mr(RBP, Frame::CTX_DISP, t.ctx_reg());
        r.special(&a, RegionKind::CtxReg);
    }
    a.jmp(block_labels[m.entry as usize]);
    r.special(&a, RegionKind::Control(block_labels[m.entry as usize]));

    // ---- blocks ----
    for (bi, blk) in m.blocks.iter().enumerate() {
        a.bind(block_labels[bi]);
        for inst in &blk.insts {
            match inst {
                Inst::Call(d, id, args) => {
                    r.flush_neutral(&a);
                    let slots: Vec<i32> = args.iter().map(|v| f.slot(*v)).collect();
                    let intent = intent_of(*id);
                    t.emit_call(&mut a, &f, intent, &slots, f.slot(*d));
                    r.special(&a, RegionKind::Intent(intent));
                }
                other => lower_inst(&mut a, &f, other),
            }
        }
        match &blk.term {
            Term::Br(x) => {
                r.flush_neutral(&a);
                a.jmp(block_labels[*x as usize]);
                r.special(&a, RegionKind::Control(block_labels[*x as usize]));
            }
            Term::BrCond(c, nz, z) => {
                a.mov_rm(RAX, RBP, f.slot(*c));
                a.test_rr(RAX, RAX);
                r.flush_neutral(&a); // the load+test are neutral
                a.jnz(block_labels[*nz as usize]);
                r.special(&a, RegionKind::Control(block_labels[*nz as usize]));
                a.jmp(block_labels[*z as usize]);
                r.special(&a, RegionKind::Control(block_labels[*z as usize]));
            }
            Term::Ret(v) | Term::Exit(v) => {
                a.mov_rm(RAX, RBP, f.slot(*v));
                r.flush_neutral(&a); // the load is neutral
                a.add_rsp(frame_size);
                r.special(&a, RegionKind::Frame);
                a.pop(RBP);
                a.ret();
                r.flush_neutral(&a); // pop+ret are neutral
            }
        }
    }

    let code = a.finish();
    // finish() patched rel32s in place; region spans are unchanged.
    (code, r.regions)
}

// ---------------- emit logic: VERBATIM from Q1 common.rs ----------------

fn lower_inst(a: &mut Asm, f: &Frame, inst: &Inst) {
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
        Inst::Call(..) => unreachable!("calls handled in lower_regions"),
    }
}

fn lower_op(a: &mut Asm, f: &Frame, op: &Op) {
    match op {
        Op::Const(x) => {
            a.mov_ri64(RAX, *x);
        }
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
