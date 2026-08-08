//! Q19 — a STRUCTURAL VERIFIER for the neutral IR (the interpreter path's produce-time gate).
//!
//! Q16's S2 found the track's one genuine non-coexistence: Q4's Tier-A guard verifies
//! **two codegen lowerings are behaviourally equivalent** (region-by-region byte compare),
//! and a pure interpreter emits **zero** lowerings, so on the ACG/iOS floor where the track
//! MANDATES interpretation there is nothing to cross-check — the produce-time structural axis
//! is (per S2) "absent".
//!
//! Q19 tests a candidate replacement: don't compare two OUTPUTS, verify the ONE thing an
//! interpreter has — the **IR itself**. The reference survey (§4.1b) names eBPF's verifier as
//! this model (load-time structural check on bytecode) — but it is 20,065 lines. This file is
//! the MINIMAL form of that idea for our IR, to answer: what does it verify, what can it not,
//! and does it patch S2 or just an adjacent hole?
//!
//! The property checked here is **well-formedness of one IR** — NOT equivalence of two
//! artifacts. That distinction is the whole experiment (criterion ②). A well-formed IR can
//! still be mis-executed; the verifier makes no claim about behaviour.
//!
//! Discipline: NO abstract interpretation, NO value-range tracking, NO type inference, NO
//! dataflow. A single structural walk. Anything more is the eBPF verifier, which criterion ④
//! prices as un-affordable. The line is drawn at "indices are in range and the graph is
//! walkable", deliberately BELOW eBPF's "runtime pointer values stay in bounds".

#![allow(dead_code)]

use crate::ir::*;

/// A structural fault in an IR. Each variant is a produce-time, execution-free finding.
#[derive(Debug, PartialEq)]
pub enum IrFault {
    /// no blocks at all — nothing to enter
    NoBlocks,
    /// entry block index >= blocks.len()  (control-flow integrity: entry)
    EntryOutOfRange { entry: u32 },
    /// a Val id referenced or defined is >= n_vals  (out-of-range index)
    ValOutOfRange { block: usize, val: Val },
    /// a Br/BrCond/entry names a block index that does not exist  (CFI: jump to illegal target)
    BlockTargetOutOfRange { block: usize, target: u32 },
    /// a Call names an extern_id that is not in the extern table  (undefined "opcode"/callee)
    ExternIdOutOfRange { block: usize, id: u32 },
    /// a Call's arg count != the declared nargs for that intent  (type/arity mismatch)
    ArityMismatch { block: usize, id: u32, got: usize, want: usize },
    /// an Op::Rodata offset points past the rodata blob  (out-of-range index, data side)
    RodataOffsetOutOfRange { block: usize, off: u32 },
}

/// A module that has PASSED structural verification. Its inner `&Module` is PRIVATE, and the
/// ONLY constructor is `verify`. Consumers that take `&VerifiedModule` therefore cannot obtain
/// one without the check having run — this is the un-forgettable **construction gate**, the
/// exact mechanism Q4's `VerifiedArtifact` uses. The difference is only in WHAT is guaranteed:
/// Q4's constructor proves "two lowerings congruent"; this one proves "one IR well-formed".
pub struct VerifiedModule<'a>(&'a Module);

impl<'a> VerifiedModule<'a> {
    /// The only way to read the module back out — proof you passed the gate.
    pub fn module(&self) -> &Module {
        self.0
    }
}

/// Structurally verify a module. Produce-time, no execution, one pass. Returns a
/// `VerifiedModule` (the capability to run it) or the first `IrFault` found.
pub fn verify(m: &Module) -> Result<VerifiedModule<'_>, IrFault> {
    let nblk = m.blocks.len();
    if nblk == 0 {
        return Err(IrFault::NoBlocks);
    }
    if m.entry as usize >= nblk {
        return Err(IrFault::EntryOutOfRange { entry: m.entry });
    }
    let nv = m.n_vals;
    for (b, blk) in m.blocks.iter().enumerate() {
        for inst in &blk.insts {
            match inst {
                Inst::Set(d, op) => {
                    chk(*d, b, nv)?;
                    each_read(op, |v| chk(v, b, nv))?;
                    if let Op::Rodata(off) = op {
                        // an offset EQUAL to len is a one-past-end address (allowed as a base
                        // that is never loaded); strictly greater is a structural fault.
                        if *off as usize > m.rodata.len() {
                            return Err(IrFault::RodataOffsetOutOfRange { block: b, off: *off });
                        }
                    }
                }
                Inst::Store8(a, v) | Inst::StoreW(a, v) => {
                    chk(*a, b, nv)?;
                    chk(*v, b, nv)?;
                }
                Inst::Call(d, id, args) => {
                    chk(*d, b, nv)?;
                    if *id as usize >= m.externs.len() {
                        return Err(IrFault::ExternIdOutOfRange { block: b, id: *id });
                    }
                    let want = m.externs[*id as usize].nargs;
                    if args.len() != want {
                        return Err(IrFault::ArityMismatch { block: b, id: *id, got: args.len(), want });
                    }
                    for v in args {
                        chk(*v, b, nv)?;
                    }
                }
            }
        }
        match &blk.term {
            Term::Br(t) => tgt(*t, b, nblk)?,
            Term::BrCond(c, nz, z) => {
                chk(*c, b, nv)?;
                tgt(*nz, b, nblk)?;
                tgt(*z, b, nblk)?;
            }
            Term::Ret(v) | Term::Exit(v) => chk(*v, b, nv)?,
        }
    }
    Ok(VerifiedModule(m))
}

#[inline]
fn chk(v: Val, b: usize, nv: u32) -> Result<(), IrFault> {
    if v >= nv {
        Err(IrFault::ValOutOfRange { block: b, val: v })
    } else {
        Ok(())
    }
}

#[inline]
fn tgt(t: u32, b: usize, nblk: usize) -> Result<(), IrFault> {
    if t as usize >= nblk {
        Err(IrFault::BlockTargetOutOfRange { block: b, target: t })
    } else {
        Ok(())
    }
}

/// Apply `f` to every Val an op reads. (Const/Rodata read no Vals.)
#[inline]
fn each_read(op: &Op, mut f: impl FnMut(Val) -> Result<(), IrFault>) -> Result<(), IrFault> {
    match op {
        Op::Const(_) | Op::Rodata(_) => Ok(()),
        Op::Add(a, b) | Op::Sub(a, b) | Op::Mul(a, b) | Op::Xor(a, b) | Op::And(a, b)
        | Op::Or(a, b) | Op::Ult(a, b) => {
            f(*a)?;
            f(*b)
        }
        Op::Shl(a, _) | Op::Shr(a, _) | Op::Load8(a) | Op::LoadW(a) => f(*a),
    }
}
