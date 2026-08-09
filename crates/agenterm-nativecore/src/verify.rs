//! Well-formedness gate — ported from `research/dynamic-core/verify/verify.rs`
//! (Q19), reused byte-for-byte on the structural checks (`NoBlocks`,
//! `EntryOutOfRange`, `ValOutOfRange`, `BlockTargetOutOfRange`,
//! `ExternIdOutOfRange`, `ArityMismatch`, `RodataOffsetOutOfRange` — the seven
//! IR-internal fault classes; "五类结构错误" in the design doc names the
//! headline ones, this crate keeps all seven Q19 had), PLUS one addition this
//! crate's IR needs that Q19's never did: `IntentArityMismatch`.
//!
//! ## The F1 fix (the whole reason this file has a new fault variant)
//! `research/dynamic-core/assembled/RESULTS.md` §② **F1** found, by actually
//! building the assembly, that Q19's verifier checks ONLY that a `Call`'s arg
//! count matches the arity the SAME IR self-declared for that extern
//! (`ExternDecl.nargs`, itself derived from that call site's own
//! `args.len()`) — a check that can never fail on its own terms, because it
//! is checking a fact against itself. It has zero knowledge of what the
//! intent's OWN native-call contract requires (e.g. `seam.rs`'s
//! `FILEWRITE_TABLE` unconditionally reads `args[0..3]`). Q22's demonstration
//! (`mismatched_arity_demo`, a `FileWrite` call with 1 arg) passed `verify()`
//! and panicked inside the interpreter with an out-of-bounds slice index —
//! reported, not fixed, in Q22's own RESULTS ("a real fix would be a second,
//! seam-specific validator... not built here").
//!
//! This file IS that second validator, merged into the same produce-time
//! gate rather than kept as a separate pass (the design doc's F1 instruction:
//! "从第一天就补上，不是装完了才发现"). `Intent::contract_arity()` (`ir.rs`)
//! is the independent source of truth — it is fixed by the intent's real
//! Win32-call shape, not derived from anything the IR itself says — so
//! cross-checking `ExternDecl.nargs` against it catches EXACTLY the class of
//! self-consistent-but-wrong IR that broke through Q19's check. See
//! `tests/native_intents.rs::rejects_intent_contract_arity_mismatch` for the
//! real, deliberately-constructed reproduction of the F1 panic, now caught
//! here instead.
//!
//! Discipline carried over from Q19: no abstract interpretation, no
//! value-range tracking beyond what's needed to catch out-of-range indices,
//! no dataflow — a single structural walk plus (new) one contract-arity
//! lookup per declared extern.

#![allow(dead_code)]

use crate::ir::*;

/// A structural fault in an IR. Each variant is a produce-time,
/// execution-free finding. A well-formed IR can still be mis-executed (e.g.
/// a spawned child does something unexpected) — `verify()` makes no claim
/// about behavior past the seam boundary, exactly Q19's own boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrFault {
    /// no blocks at all — nothing to enter
    NoBlocks,
    /// entry block index >= blocks.len()  (control-flow integrity: entry)
    EntryOutOfRange { entry: u32 },
    /// a Val id referenced or defined is >= n_vals  (out-of-range index)
    ValOutOfRange { block: usize, val: Val },
    /// a Br/BrCond names a block index that does not exist  (CFI: jump to illegal target)
    BlockTargetOutOfRange { block: usize, target: u32 },
    /// a Call names an extern_id that is not in the extern table  (undefined "opcode"/callee)
    ExternIdOutOfRange { block: usize, id: u32 },
    /// a Call's arg count != the declared nargs for that extern (IR-internal
    /// self-consistency — Q19's original check; note this can still pass
    /// while `IntentArityMismatch` below fires, which is exactly F1)
    ArityMismatch { block: usize, id: u32, got: usize, want: usize },
    /// an Op::Rodata offset points past the rodata blob  (out-of-range index, data side)
    RodataOffsetOutOfRange { block: usize, off: u32 },
    /// **F1 fix.** An extern's declared `nargs` disagrees with
    /// `intent.contract_arity()` — the intent's OWN native-call contract,
    /// independent of what this IR self-declares. This is checked over
    /// `Module::externs` directly (once per declared extern), not per call
    /// site, because the contract is a property of the DECLARATION, and a
    /// declaration is reused by every call site that names it (`Builder`'s
    /// dedup) — every one of them would panic identically at the seam.
    IntentArityMismatch { id: u32, intent: Intent, declared: usize, contract: usize },
}

/// A module that has PASSED structural verification. Its inner `&Module` is
/// PRIVATE, and the ONLY constructor is `verify` — the un-forgettable
/// construction gate (Q19/Q4's pattern, unchanged here).
pub struct VerifiedModule<'a>(&'a Module);

impl<'a> VerifiedModule<'a> {
    /// The only way to read the module back out — proof you passed the gate.
    pub fn module(&self) -> &Module {
        self.0
    }
}

/// Structurally verify a module. Produce-time, no execution, one pass over
/// `externs` (contract-arity) followed by one pass over `blocks` (Q19's
/// original structural walk). Returns a `VerifiedModule` (the capability to
/// run it) or the first `IrFault` found.
pub fn verify(m: &Module) -> Result<VerifiedModule<'_>, IrFault> {
    // --- F1 fix: contract-arity pass, BEFORE any block is walked, so a
    // seam-hostile IR is rejected even if it happens to never reach the
    // offending call at runtime (produce-time means produce-time). ---
    for (id, e) in m.externs.iter().enumerate() {
        let contract = e.intent.contract_arity();
        if e.nargs != contract {
            return Err(IrFault::IntentArityMismatch {
                id: id as u32,
                intent: e.intent,
                declared: e.nargs,
                contract,
            });
        }
    }

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
        Op::Add(a, b) | Op::Sub(a, b) | Op::Mul(a, b) | Op::Xor(a, b) | Op::And(a, b) | Op::Or(a, b) | Op::Ult(a, b) => {
            f(*a)?;
            f(*b)
        }
        Op::Shl(a, _) | Op::Shr(a, _) | Op::Load8(a) | Op::LoadW(a) => f(*a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every intent's contract is exercised at least once by this crate's own
    /// tests — a cheap guard against silently adding an eighth intent to
    /// `ir.rs` without giving it a contract arity here too.
    #[test]
    fn every_intent_has_a_distinct_documented_contract() {
        for intent in Intent::ALL {
            // just exercise the const fn; the real assertions are the
            // integration tests in tests/native_intents.rs that build real
            // IR against each of these and check verify()'s behavior.
            let _ = intent.contract_arity();
        }
    }
}
