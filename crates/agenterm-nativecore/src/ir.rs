//! Neutral IR — ported from `research/dynamic-core/assembled/ir.rs` (Q1/Q22),
//! this time WITHOUT the cut logic-pack made. `plan/design-dynacore-logic-pack.md`
//! kept only `fleet_call` and removed raw-memory access + the seven Win32-bound
//! `Intent`s; `plan/design-dynacore-native-core.md` (this crate) is the other
//! half — it keeps `Op::Rodata`/`Inst::Store8`/`StoreW`/`Op::Load8`/`LoadW` and
//! all seven native intents (`Alloc`/`FileOpen`/`FileRead`/`FileClose`/
//! `WriteStdout`/`SpawnWait`/`FileWrite`) because those intents genuinely need
//! raw pointers into host memory (a file-read buffer, a rodata string for a
//! path) — a IR that only carries `operation_id`/`params_json` strings (the
//! logic pack's shape) cannot express "read N bytes into a buffer I then hash
//! byte-by-byte". The two IRs are intentionally NOT unified (design doc §4).
//!
//! One addition beyond Q22's `ir.rs`: `Intent::contract_arity` — each intent's
//! OWN declared native-call arity, independent of what any particular IR
//! happens to self-declare in its `ExternDecl.nargs`. This is the fix for
//! `research/dynamic-core/assembled/RESULTS.md` §② **F1**: Q22's verifier only
//! checked that a call site's arg count matched the arity the IR itself
//! declared for that extern — an IR-internal-only consistency check with no
//! knowledge of what the intent's real Win32-call contract requires. A
//! self-consistent-but-wrong IR (e.g. `FileWrite` called with 1 arg, when its
//! own `ExternDecl.nargs` is honestly derived from that same 1-arg call site)
//! sailed through `verify()` in Q22 and only panicked inside the interpreter,
//! reading out of bounds on `seam.rs`'s argument-slot tables. `verify.rs` in
//! this crate cross-checks EVERY extern's declared `nargs` against
//! `contract_arity()`, closing that gap at produce time (see `verify.rs`'s
//! header and the `IntentArityMismatch` fault).

#![allow(dead_code)]

pub type Val = u32; // an SSA/virtual temp id, local to a function

/// A value-producing operation. Every variant yields one 64-bit word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// A literal word. Discipline (inherited from the research track): this
    /// must be an ALGORITHM constant (an FNV prime, an ASCII code), never a
    /// sizeof/offset — those are `declare.rs`'s job, and go through baked
    /// (and self-checked, see `declare.rs`) constants at the seam layer, not
    /// into payload IR as `Op::Const`.
    Const(u64),
    /// Address of byte `off` within the module's read-only data blob. A data
    /// address is neutral (rodata is an opaque byte array, not a struct).
    Rodata(u32),

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

    /// zero-extend the byte at [addr]
    Load8(Val),
    /// pointer-width load at [addr]
    LoadW(Val),
}

/// A statement (effect or value binding) inside a basic block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inst {
    /// dest = op
    Set(Val, Op),
    /// store low byte of `v` at [addr]
    Store8(Val, Val),
    /// store word `v` at [addr]
    StoreW(Val, Val),
    /// dest = call extern#id(args...)  (semantic-signature call; ABI is the
    /// seam's problem, not the IR's — see `seam.rs`)
    Call(Val, u32, Vec<Val>),
}

/// A block terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    Br(u32),
    /// if `cond` != 0 goto `nz` else goto `z`
    BrCond(Val, u32, u32),
    /// return `v` (ABI-neutral: SysV and Win64 agree on integer returns)
    Ret(Val),
    /// terminate the payload with exit code `v`.
    Exit(Val),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

/// An OS-neutral operation the payload needs from the host. Carries a
/// semantic intent, not a symbol/syscall number/struct — binding to target
/// specifics is `seam.rs`'s job. Exactly the seven Q22 verified real-machine;
/// v1 does not add an eighth (design doc §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Intent {
    /// allocate `n` writable bytes, zero-filled; returns base pointer. (args: [n])
    Alloc,
    /// open a file named by the NUL-terminated path at args[0], for reading;
    /// returns an opaque handle word. (args: [path_ptr])
    FileOpen,
    /// read up to args[2] bytes from handle args[0] into buffer args[1];
    /// returns the count actually read. (args: [handle, buf, cap])
    FileRead,
    /// close handle args[0]. (args: [handle])
    FileClose,
    /// write args[1] bytes from args[0] to standard output. (args: [buf, len])
    WriteStdout,
    /// spawn a fixed child, wait for it, return its exit code. (args: [])
    SpawnWait,
    /// create/truncate the file named by the NUL-terminated path at args[0],
    /// write args[2] bytes from buffer args[1] into it, close it; returns
    /// the byte count actually written. (args: [path_ptr, buf, len])
    FileWrite,
}

impl Intent {
    /// This intent's OWN native-call contract arity — fixed by the Win32 API
    /// shape it binds to (see each variant's doc comment above and
    /// `seam.rs`'s per-intent `Mechanism`), NOT read from any particular IR.
    /// `verify()` checks every `ExternDecl` against this, independent of
    /// whether the IR that declared it is internally self-consistent — the
    /// direct fix for F1 (see this module's header).
    pub const fn contract_arity(self) -> usize {
        match self {
            Intent::Alloc => 1,
            Intent::FileOpen => 1,
            Intent::FileRead => 3,
            Intent::FileClose => 1,
            Intent::WriteStdout => 2,
            Intent::SpawnWait => 0,
            Intent::FileWrite => 3,
        }
    }

    /// All seven intents, for iteration (verify.rs's docs / tests use this to
    /// assert every intent's contract is exercised, not just declared).
    pub const ALL: [Intent; 7] = [
        Intent::Alloc,
        Intent::FileOpen,
        Intent::FileRead,
        Intent::FileClose,
        Intent::WriteStdout,
        Intent::SpawnWait,
        Intent::FileWrite,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternDecl {
    pub intent: Intent,
    pub nargs: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub name: String,
    pub n_vals: u32,
    pub blocks: Vec<Block>,
    pub entry: u32,
    pub rodata: Vec<u8>,
    pub externs: Vec<ExternDecl>,
}

/// Small builder to keep payloads readable (Q1/Q22-style).
pub struct Builder {
    next: u32,
    blocks: Vec<Block>,
    cur: Vec<Inst>,
    rodata: Vec<u8>,
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
            rodata: Vec::new(),
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
    pub fn store8(&mut self, addr: Val, v: Val) {
        self.cur.push(Inst::Store8(addr, v));
    }
    pub fn storew(&mut self, addr: Val, v: Val) {
        self.cur.push(Inst::StoreW(addr, v));
    }
    /// Declare (or reuse an identical existing) extern and emit a call.
    /// `nargs` is honestly derived from `args.len()` here — exactly the
    /// self-consistency Q19/F1's IR-internal check relies on; `verify()`
    /// additionally cross-checks this against `intent.contract_arity()`.
    pub fn call(&mut self, intent: Intent, args: Vec<Val>) -> Val {
        let id = self.decl(intent, args.len());
        let d = self.v();
        self.cur.push(Inst::Call(d, id, args));
        d
    }
    /// Like `call`, but deliberately lets a test pass a `nargs` that
    /// disagrees with `args.len()` (or with the intent's real contract) —
    /// this is the ONLY way to build the deliberately-malformed IR the
    /// acceptance tests need (`Builder::call` above cannot express it, same
    /// as Q22's honesty note in `extra_payloads.rs`).
    pub fn call_raw_arity(&mut self, intent: Intent, args: Vec<Val>, declared_nargs: usize) -> Val {
        let id = self.decl(intent, declared_nargs);
        let d = self.v();
        self.cur.push(Inst::Call(d, id, args));
        d
    }
    fn decl(&mut self, intent: Intent, nargs: usize) -> u32 {
        for (i, e) in self.externs.iter().enumerate() {
            if e.intent == intent && e.nargs == nargs {
                return i as u32;
            }
        }
        self.externs.push(ExternDecl { intent, nargs });
        (self.externs.len() - 1) as u32
    }
    /// append raw bytes to rodata, return their starting offset
    pub fn rodata(&mut self, bytes: &[u8]) -> u32 {
        let off = self.rodata.len() as u32;
        self.rodata.extend_from_slice(bytes);
        off
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
            rodata: self.rodata,
            externs: self.externs,
        }
    }
}
