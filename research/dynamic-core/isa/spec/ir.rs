//! The candidate NEUTRAL IR.
//!
//! Design goal (spec §1): encode NOTHING about ABI or layout. This IR may assume
//! only one target fact — pointer width — and nothing else. Concretely it MUST NOT
//! contain:
//!   * which register / stack slot an argument goes in, shadow space, red zone;
//!   * any concrete sizeof / alignment / struct-field-offset number;
//!   * a calling-convention name (sysv64 / win64 / cdecl / ...).
//!
//! Shape chosen: a small typed, three-address (register/SSA-lite) IR. NOT a stack
//! machine (spec §6 excludes bytecode/JVM routes — a stack machine grows a runtime),
//! NOT LLVM IR (spec §6 — the reference anti-pattern).
//!
//! There is exactly ONE value type: `Word` — a machine word, pointer-width. We do
//! NOT give the IR i8/i16/i32/i64 value types, because a value's byte width is a
//! layout fact. Memory is addressed byte-granularly through explicit Load/Store ops
//! whose element width is named by the OP, not by a type carried on the value.
//!
//! Calls are described ONLY by a semantic signature: an ordered arg list (each a
//! Word) and a return (a Word). The IR never says how args are placed — the lowerer
//! decides. The callee is named by an abstract `extern_id` into the module's extern
//! table; the extern table entries carry an OS-neutral *intent*, never a symbol
//! string or a syscall number (those are the lowerer's business — and, as it turns
//! out, the leak — see RESULTS §②).

#![allow(dead_code)]

pub type Val = u32; // an SSA temp id, local to a function

/// A value-producing operation. Every variant yields one Word.
#[derive(Clone, Debug)]
pub enum Op {
    /// A literal word. Discipline (spec §1.2): this must be an ALGORITHM constant
    /// (e.g. an FNV prime, an ASCII code), NEVER a sizeof/offset. Layout numbers are
    /// banned from the IR; if one is needed, that is a leak, recorded not smuggled.
    Const(u64),
    /// Address of byte `off` within the module's read-only data blob. A data address
    /// is neutral (it is not a struct offset — rodata is an opaque byte array).
    Rodata(u32),

    Add(Val, Val),
    Sub(Val, Val),
    Mul(Val, Val),
    Xor(Val, Val),
    And(Val, Val),
    Or(Val, Val),
    Shl(Val, u8),
    Shr(Val, u8),
    /// unsigned less-than: 1 if a < b else 0 (ISA-level compare; not ABI/layout)
    Ult(Val, Val),

    /// zero-extend the byte at [addr]
    Load8(Val),
    /// pointer-width load at [addr]
    LoadW(Val),
}

/// A statement (effect or value binding) inside a basic block.
#[derive(Clone, Debug)]
pub enum Inst {
    /// dest = op
    Set(Val, Op),
    /// store low byte of `v` at [addr]
    Store8(Val, Val),
    /// store word `v` at [addr]
    StoreW(Val, Val),
    /// dest = call extern#id(args...)   (semantic-signature call; ABI deferred)
    Call(Val, u32, Vec<Val>),
}

/// A block terminator.
#[derive(Clone, Debug)]
pub enum Term {
    Br(u32),
    /// if `cond` != 0 goto `nz` else goto `z`
    BrCond(Val, u32, u32),
    /// return `v` in the value register (ABI-neutral: both SysV and Win64 return
    /// integers in the same register — that is the one place the two ABIs agree,
    /// and the IR is allowed to rely on nothing beyond it)
    Ret(Val),
    /// terminate the payload with exit code `v`. Lowered per target (Linux exit
    /// syscall vs Win64 ExitProcess); the JIT harness lowers it to `Ret` so pure
    /// compute can be executed and its result read.
    Exit(Val),
}

#[derive(Clone, Debug)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

/// An OS-neutral operation the payload needs from the host. It carries a semantic
/// intent and an arg/return arity — NOT a symbol, NOT a syscall number, NOT a
/// struct. Binding an intent to target specifics is the lowerer's job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// allocate `n` writable bytes, zero-filled; returns base pointer. (args: [n])
    Alloc,
    /// open a file named by the NUL-terminated path at args[0], for reading; returns
    /// an opaque handle word. (args: [path_ptr])
    FileOpen,
    /// read up to args[2] bytes from handle args[0] into buffer args[1]; returns the
    /// count actually read. (args: [handle, buf, cap])
    FileRead,
    /// close handle args[0]. (args: [handle])
    FileClose,
    /// write args[1] bytes from args[0] to standard output. (args: [buf, len])
    WriteStdout,
    /// spawn a fixed child, wait for it, return its exit code. (args: [])
    /// NOTE: no neutral finer grain exists — see RESULTS §② leak L3.
    SpawnWait,
}

#[derive(Clone, Copy, Debug)]
pub struct ExternDecl {
    pub intent: Intent,
    pub nargs: usize,
}

pub struct Module {
    pub name: &'static str,
    pub n_vals: u32,
    pub blocks: Vec<Block>,
    pub entry: u32,
    /// does the entry receive a runtime context pointer? (pure compute does not,
    /// so its two lowerings are byte-identical and both can be executed)
    pub takes_ctx: bool,
    pub rodata: Vec<u8>,
    pub externs: Vec<ExternDecl>,
}

/// Small builder to keep the payloads readable.
pub struct Builder {
    next: u32,
    blocks: Vec<Block>,
    cur: Vec<Inst>,
    rodata: Vec<u8>,
    externs: Vec<ExternDecl>,
}

impl Builder {
    pub fn new() -> Self {
        Builder { next: 0, blocks: Vec::new(), cur: Vec::new(), rodata: Vec::new(), externs: Vec::new() }
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
    pub fn konst(&mut self, x: u64) -> Val { self.set(Op::Const(x)) }
    /// reassign an existing val (virtual register, not SSA — needed for loops)
    pub fn assign(&mut self, dest: Val, op: Op) {
        self.cur.push(Inst::Set(dest, op));
    }
    pub fn store8(&mut self, addr: Val, v: Val) { self.cur.push(Inst::Store8(addr, v)); }
    pub fn storew(&mut self, addr: Val, v: Val) { self.cur.push(Inst::StoreW(addr, v)); }
    pub fn call(&mut self, intent: Intent, args: Vec<Val>) -> Val {
        let id = self.decl(intent, args.len());
        let d = self.v();
        self.cur.push(Inst::Call(d, id, args));
        d
    }
    fn decl(&mut self, intent: Intent, nargs: usize) -> u32 {
        for (i, e) in self.externs.iter().enumerate() {
            if e.intent == intent {
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
    pub fn finish(self, name: &'static str, takes_ctx: bool, entry: u32) -> Module {
        Module {
            name,
            n_vals: self.next,
            blocks: self.blocks,
            entry,
            takes_ctx,
            rodata: self.rodata,
            externs: self.externs,
        }
    }
}
