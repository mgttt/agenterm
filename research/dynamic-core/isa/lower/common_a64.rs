//! PER-ISA generic lowering for AArch64: neutral IR -> A64, everything that is
//! ABI-INDEPENDENT *within this ISA*. The analogue of Q1's `common.rs`.
//!
//! This is the honest three-way split in action. Q1's `common.rs` looked "shared"
//! but was written against x86-64 method names (add_rr, mov_rm, ...). Instruction
//! SELECTION is ISA-specific, so a second ISA needs its own copy of this file: the
//! same *structure* (walk blocks, dispatch ops, spill temps to a frame) but a
//! different instruction stream. What it delegates to a `Target` is identical in
//! shape to Q1: ctx register, outgoing-arg area size, and how an intent-call is
//! emitted. That delegation boundary is the ABI/OS boundary.

use crate::a64::*;
use crate::ir::*;

// Two caller-saved scratch registers do all the work (analogue of RAX/RCX).
const ACC: u8 = X9;
const TMP: u8 = X10;

/// Frame geometry (AArch64: all offsets POSITIVE from SP, unlike x86's negative-from-RBP).
/// [sp, 0..max_out) outgoing args | ctx | saved LR | out-dword scratch | IR val slots | private scratch
pub struct Frame {
    pub frame_size: i32,
    pub takes_ctx: bool,
    pub n_vals: i32,
    pub max_out: i32,
}
impl Frame {
    pub const SCRATCH_N: i32 = 8;
    pub fn ctx_disp(&self) -> i32 { self.max_out }
    pub fn lr_disp(&self) -> i32 { self.max_out + 8 }
    pub fn outdw_disp(&self) -> i32 { self.max_out + 16 }
    fn val0(&self) -> i32 { self.max_out + 24 }
    pub fn slot(&self, v: Val) -> i32 { self.val0() + 8 * (v as i32) }
    pub fn scratch(&self, i: i32) -> i32 { self.val0() + 8 * (self.n_vals + i) }
}

/// A code-generation target = one ABI + one OS reach mechanism, on AArch64.
pub trait Target {
    fn name(&self) -> &'static str;
    fn ctx_reg(&self) -> u8;
    fn max_outgoing(&self, m: &Module) -> i32;
    fn emit_call(&self, a: &mut A64, f: &Frame, intent: Intent, argslots: &[i32], dest: i32);
}

/// One native-call argument. The lowerer injects Imm (target constant), Slot (an IR
/// value on the frame), or OutPtr (address of the out-dword scratch).
pub enum CallArg {
    Imm(u64),
    Slot(i32),
    OutPtr,
}

/// PER-ISA (shared across BOTH aarch64 OS targets): AAPCS64 argument placement.
/// The first `nreg` args go in x0..x(nreg-1); the rest spill onto the stack at
/// [sp, #0], [sp, #8], ... (no shadow space). On AArch64 this same routine serves
/// both Linux and Windows because they share AAPCS64 — unlike x86 where SysV and
/// Win64 needed different placement. That is the ISA-axis finding: ABI placement is
/// per-ISA here, not per-target.
pub fn place_args(a: &mut A64, f: &Frame, args: &[CallArg], nreg: usize) {
    for (i, arg) in args.iter().enumerate() {
        let reg = if i < nreg { i as u8 } else { ACC };
        match arg {
            CallArg::Imm(x) => a.mov_imm(reg, *x),
            CallArg::Slot(d) => a.ldr(reg, SP, *d),
            CallArg::OutPtr => a.add_imm(reg, SP, f.outdw_disp() as u32),
        }
        if i >= nreg {
            a.str(reg, SP, 8 * (i as i32 - nreg as i32));
        }
    }
}

pub fn lower(m: &Module, t: &dyn Target) -> Vec<u8> {
    let n_vals = m.n_vals as i32;
    let max_out = t.max_outgoing(m);
    let mut frame_size = max_out + 24 + 8 * (n_vals + Frame::SCRATCH_N);
    frame_size = (frame_size + 15) & !15;
    let f = Frame { frame_size, takes_ctx: m.takes_ctx, n_vals, max_out };

    let mut a = A64::new();
    let block_labels: Vec<u32> = (0..m.blocks.len()).map(|_| a.new_label()).collect();

    // ---- prologue ----
    a.sub_imm(SP, SP, frame_size as u32);
    a.str(X30, SP, f.lr_disp()); // save LR (any intent-call uses BLR which clobbers it)
    if m.takes_ctx {
        a.str(t.ctx_reg(), SP, f.ctx_disp());
    }
    a.b(block_labels[m.entry as usize]);

    // ---- blocks ----
    for (bi, blk) in m.blocks.iter().enumerate() {
        a.bind(block_labels[bi]);
        for inst in &blk.insts {
            lower_inst(&mut a, &f, t, inst);
        }
        match &blk.term {
            Term::Br(x) => a.b(block_labels[*x as usize]),
            Term::BrCond(c, nz, z) => {
                a.ldr(ACC, SP, f.slot(*c));
                a.cbnz(ACC, block_labels[*nz as usize]);
                a.b(block_labels[*z as usize]);
            }
            Term::Ret(v) | Term::Exit(v) => {
                a.ldr(X0, SP, f.slot(*v)); // AArch64 returns in x0
                a.ldr(X30, SP, f.lr_disp());
                a.add_imm(SP, SP, frame_size as u32);
                a.ret();
            }
        }
    }
    a.finish()
}

fn lower_inst(a: &mut A64, f: &Frame, t: &dyn Target, inst: &Inst) {
    match inst {
        Inst::Set(d, op) => {
            lower_op(a, f, op); // result in ACC
            a.str(ACC, SP, f.slot(*d));
        }
        Inst::Store8(addr, v) => {
            a.ldr(ACC, SP, f.slot(*addr));
            a.ldr(TMP, SP, f.slot(*v));
            a.strb(TMP, ACC, 0);
        }
        Inst::StoreW(addr, v) => {
            a.ldr(ACC, SP, f.slot(*addr));
            a.ldr(TMP, SP, f.slot(*v));
            a.str(TMP, ACC, 0);
        }
        Inst::Call(_d, id, args) => {
            let slots: Vec<i32> = args.iter().map(|v| f.slot(*v)).collect();
            lower_call(a, f, t, *id, &slots, f.slot(*_d));
        }
    }
}

use std::cell::RefCell;
thread_local! {
    static EXTERNS: RefCell<Vec<ExternDecl>> = RefCell::new(Vec::new());
}
pub fn set_externs(e: &[ExternDecl]) {
    EXTERNS.with(|c| *c.borrow_mut() = e.to_vec());
}
fn lower_call(a: &mut A64, f: &Frame, t: &dyn Target, id: u32, slots: &[i32], dest: i32) {
    let intent = EXTERNS.with(|c| c.borrow()[id as usize].intent);
    t.emit_call(a, f, intent, slots, dest);
}

fn lower_op(a: &mut A64, f: &Frame, op: &Op) {
    match op {
        Op::Const(x) => a.mov_imm(ACC, *x),
        Op::Rodata(off) => {
            a.ldr(ACC, SP, f.ctx_disp()); // acc = ctx
            a.ldr(ACC, ACC, 8); // acc = ctx[1] = rodata base
            if *off != 0 {
                a.add_imm(ACC, ACC, *off);
            }
        }
        Op::Add(x, y) => bin(a, f, *x, *y, |a| a.add(ACC, ACC, TMP)),
        Op::Sub(x, y) => bin(a, f, *x, *y, |a| a.sub(ACC, ACC, TMP)),
        Op::Mul(x, y) => bin(a, f, *x, *y, |a| a.mul(ACC, ACC, TMP)),
        Op::Xor(x, y) => bin(a, f, *x, *y, |a| a.eor(ACC, ACC, TMP)),
        Op::And(x, y) => bin(a, f, *x, *y, |a| a.and(ACC, ACC, TMP)),
        Op::Or(x, y) => bin(a, f, *x, *y, |a| a.orr(ACC, ACC, TMP)),
        Op::Ult(x, y) => {
            a.ldr(ACC, SP, f.slot(*x));
            a.ldr(TMP, SP, f.slot(*y));
            a.cmp(ACC, TMP);
            a.cset_lo(ACC);
        }
        Op::Shl(x, s) => {
            a.ldr(ACC, SP, f.slot(*x));
            a.lsl_imm(ACC, ACC, *s);
        }
        Op::Shr(x, s) => {
            a.ldr(ACC, SP, f.slot(*x));
            a.lsr_imm(ACC, ACC, *s);
        }
        Op::Load8(x) => {
            a.ldr(ACC, SP, f.slot(*x));
            a.ldrb(ACC, ACC, 0);
        }
        Op::LoadW(x) => {
            a.ldr(ACC, SP, f.slot(*x));
            a.ldr(ACC, ACC, 0);
        }
    }
}

fn bin(a: &mut A64, f: &Frame, x: Val, y: Val, op: impl Fn(&mut A64)) {
    a.ldr(ACC, SP, f.slot(x));
    a.ldr(TMP, SP, f.slot(y));
    op(a);
}
