//! Neutral IR — ported from `research/dynamic-core/assembled/ir.rs` (Q1/Q22).
//!
//! Q22's IR let a payload address raw host memory (`Op::Rodata`,
//! `Inst::Store8`/`StoreW`, `Op::Load8`/`LoadW`) because its native-call
//! `Intent`s (`Alloc`/`FileOpen`/`FileRead`/`FileClose`/`WriteStdout`/
//! `SpawnWait`/`FileWrite`) needed pointers into that memory, and each one
//! bound to Win32 APIs directly (see `research/dynamic-core/assembled/seam.rs`).
//! dynacore v1's only host-call primitive is `fleet_call(operation_id,
//! params_json)` (`plan/design-dynacore-logic-pack.md` §2) — both strings
//! are pinned per call site at pack-build time (see `ExternDecl`), so a
//! payload never needs a raw pointer into host memory. The raw-memory ops
//! and the seven Win32-bound intents are cut with that requirement, not kept
//! around as dead surface.
//!
//! Everything else — the SSA/virtual-register word model, wrapping u64
//! arithmetic, and the block/terminator control-flow shape — is Q22-verbatim
//! (same zero-ISA-token discipline Q9 measured, criterion ④).

#![allow(dead_code)]

pub type Val = u32;

/// A value-producing operation. Every variant yields one 64-bit word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// A literal word.
    Const(u64),
    Add(Val, Val),
    Sub(Val, Val),
    Mul(Val, Val),
    Xor(Val, Val),
    And(Val, Val),
    Or(Val, Val),
    Shl(Val, u8),
    Shr(Val, u8),
    /// unsigned less-than: 1 if a < b else 0.
    Ult(Val, Val),
}

/// A statement inside a basic block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inst {
    /// dest = op
    Set(Val, Op),
    /// dest = fleet_call(externs\[id\].operation_id, externs\[id\].params_json).
    /// dest is 1 on `Ok`, 0 on `Err` — directly usable by `Term::BrCond` so a
    /// pack can branch on whether the host call it just made succeeded.
    FleetCall(Val, u32),
}

/// A block terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    Br(u32),
    /// if `cond` != 0 goto `nz` else goto `z`
    BrCond(Val, u32, u32),
    Ret(Val),
    Exit(Val),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

/// One `fleet_call` site's fully-resolved target. Both fields are pinned at
/// pack-build time — dynacore v1 has no expression language for assembling
/// `params_json` from runtime `Val`s (see the design doc's non-goals); a
/// pack that needs different params on different branches declares one
/// extern per distinct call and picks the right `FleetCall` id at each site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternDecl {
    /// `OPERATION_CATALOG` entry id (e.g. `"tabs.list"`), not the
    /// `script_surface` dotted path.
    pub operation_id: String,
    /// A JSON object literal, e.g. `"{}"` or `"{\"tab_id\":\"@1\"}"`.
    pub params_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub name: String,
    pub n_vals: u32,
    pub blocks: Vec<Block>,
    pub entry: u32,
    pub externs: Vec<ExternDecl>,
}

/// Small builder to keep pack sources readable (mirrors Q1/Q22's `Builder`).
pub struct Builder {
    next: u32,
    blocks: Vec<Block>,
    cur: Vec<Inst>,
    externs: Vec<ExternDecl>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        Builder {
            next: 0,
            blocks: Vec::new(),
            cur: Vec::new(),
            externs: Vec::new(),
        }
    }

    pub fn v(&mut self) -> Val {
        let n = self.next;
        self.next += 1;
        n
    }

    pub fn set(&mut self, op: Op) -> Val {
        let d = self.v();
        self.cur.push(Inst::Set(d, op));
        d
    }

    pub fn konst(&mut self, x: u64) -> Val {
        self.set(Op::Const(x))
    }

    /// reassign an existing val (virtual register, not SSA — needed for loops)
    pub fn assign(&mut self, dest: Val, op: Op) {
        self.cur.push(Inst::Set(dest, op));
    }

    /// Declare (or reuse an identical existing) `fleet_call` extern and emit
    /// a call to it. Reuse is by-value (same operation_id + params_json),
    /// mirroring Q7's marginal-cost finding: repeating a call site costs no
    /// new extern declaration.
    pub fn fleet_call(&mut self, operation_id: impl Into<String>, params_json: impl Into<String>) -> Val {
        let id = self.decl(operation_id.into(), params_json.into());
        let d = self.v();
        self.cur.push(Inst::FleetCall(d, id));
        d
    }

    fn decl(&mut self, operation_id: String, params_json: String) -> u32 {
        for (i, e) in self.externs.iter().enumerate() {
            if e.operation_id == operation_id && e.params_json == params_json {
                return i as u32;
            }
        }
        self.externs.push(ExternDecl {
            operation_id,
            params_json,
        });
        (self.externs.len() - 1) as u32
    }

    /// end the current block with a terminator, opening a fresh block
    pub fn term(&mut self, t: Term) {
        let insts = std::mem::take(&mut self.cur);
        self.blocks.push(Block { insts, term: t });
    }

    pub fn finish(self, name: impl Into<String>, entry: u32) -> Module {
        Module {
            name: name.into(),
            n_vals: self.next,
            blocks: self.blocks,
            entry,
            externs: self.externs,
        }
    }
}
