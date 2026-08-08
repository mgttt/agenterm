//! SHARED lowering: neutral IR -> x86-64, everything that is ABI-INDEPENDENT.
//!
//! This module lowers arithmetic, memory, and control flow identically for both
//! targets. The ONLY things it delegates to a `Target` are: which register carries
//! the incoming ctx pointer, how much outgoing-arg space a call needs, and how an
//! intent-call is emitted (arg placement + reach + target-specific constant args).
//! That delegation boundary is exactly the ABI/OS boundary the experiment probes.

use crate::asm::*;
use crate::ir::*;

/// Frame geometry. ctx at [rbp-8], a 4-byte out-param scratch at [rbp-16], IR value
/// slots from [rbp-24] downward. Outgoing call args live at [rsp+..].
pub struct Frame {
    pub frame_size: i32,
    pub takes_ctx: bool,
    pub n_vals: i32,
}
impl Frame {
    pub const CTX_DISP: i32 = -8;
    pub const OUTDW_DISP: i32 = -16;
    const VAL0: i32 = -24;
    pub const SCRATCH_N: i32 = 8; // lowerer-private temps (e.g. spawn's si/pi/cmdbuf)
    pub fn slot(&self, v: Val) -> i32 {
        Self::VAL0 - 8 * (v as i32)
    }
    /// lowerer-private scratch slot, below the IR value slots
    pub fn scratch(&self, i: i32) -> i32 {
        Self::VAL0 - 8 * (self.n_vals + i)
    }
}

/// A code-generation target = one ABI + one OS reach mechanism.
pub trait Target {
    /// register that holds the incoming ctx pointer at function entry
    fn ctx_reg(&self) -> u8;
    /// max outgoing-arg area (bytes) any single call in this module needs
    fn max_outgoing(&self, m: &Module) -> i32;
    /// emit a full intent call: place `argslots` per the ABI, inject any
    /// target-specific constant args, reach the callee, store the result to `dest`
    fn emit_call(&self, a: &mut Asm, f: &Frame, intent: Intent, argslots: &[i32], dest: i32);
    /// number of LOC that are target-specific (for criterion ⑤; filled by the file)
    fn name(&self) -> &'static str;
}

pub fn lower(m: &Module, t: &dyn Target) -> Vec<u8> {
    let n_vals = m.n_vals as i32;
    let max_out = t.max_outgoing(m);
    let mut frame_size = 24 + 8 * (n_vals + Frame::SCRATCH_N) + max_out;
    frame_size = (frame_size + 15) & !15; // 16-byte align
    let f = Frame { frame_size, takes_ctx: m.takes_ctx, n_vals };

    let mut a = Asm::new();
    // one label per block
    let block_labels: Vec<u32> = (0..m.blocks.len()).map(|_| a.new_label()).collect();

    // ---- prologue ----
    a.push(RBP);
    a.mov_rr(RBP, RSP);
    a.sub_rsp(frame_size);
    if m.takes_ctx {
        a.mov_mr(RBP, Frame::CTX_DISP, t.ctx_reg());
    }
    a.jmp(block_labels[m.entry as usize]);

    // ---- blocks ----
    for (bi, blk) in m.blocks.iter().enumerate() {
        a.bind(block_labels[bi]);
        for inst in &blk.insts {
            lower_inst(&mut a, &f, t, inst);
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
                // Both lowered as "return value in rax" so the JIT harness can read
                // it. A standalone build would replace Exit with a target exit call
                // (a handful of extra bytes) — noted in RESULTS.
                a.mov_rm(RAX, RBP, f.slot(*v));
                a.add_rsp(frame_size);
                a.pop(RBP);
                a.ret();
            }
        }
    }

    a.finish()
}

fn lower_inst(a: &mut Asm, f: &Frame, t: &dyn Target, inst: &Inst) {
    match inst {
        Inst::Set(d, op) => {
            lower_op(a, f, op); // result in RAX
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
            let slots: Vec<i32> = args.iter().map(|v| f.slot(*v)).collect();
            // NOTE: the intent for extern `id` is not stored here; the payload's
            // module carries it. The caller resolves it before emitting. We pass the
            // intent via a side channel: encode it by re-reading from the module.
            // To keep this function pure, the intent is looked up by the driver and
            // baked into a closure — but simpler: we stash intent in the high bits.
            // Instead, emit_call takes the intent; the driver passes a resolver.
            // (Handled in lower_call below.)
            lower_call(a, f, t, *id, &slots, f.slot(*d));
            let _ = id;
        }
    }
}

// The module's extern table is needed to map id->intent. We thread it via a
// thread-local set by `lower`. Simpler: store a reference. To avoid globals we
// re-lookup through a small registry set at call time.
use std::cell::RefCell;
thread_local! {
    static EXTERNS: RefCell<Vec<ExternDecl>> = RefCell::new(Vec::new());
}
pub fn set_externs(e: &[ExternDecl]) {
    EXTERNS.with(|c| *c.borrow_mut() = e.to_vec());
}
fn lower_call(a: &mut Asm, f: &Frame, t: &dyn Target, id: u32, slots: &[i32], dest: i32) {
    let intent = EXTERNS.with(|c| c.borrow()[id as usize].intent);
    t.emit_call(a, f, intent, slots, dest);
}

fn lower_op(a: &mut Asm, f: &Frame, op: &Op) {
    match op {
        Op::Const(x) => {
            if *x <= i32::MAX as u64 {
                a.mov_ri64(RAX, *x); // still 64-bit form; simplicity over 3 bytes
            } else {
                a.mov_ri64(RAX, *x);
            }
        }
        Op::Rodata(off) => {
            a.mov_rm(RAX, RBP, Frame::CTX_DISP); // rax = ctx
            a.mov_rm(RAX, RAX, 8); // rax = ctx[1] = rodata base
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
