//! Q17 — the A↔B channel wire format. **THE NEW SEAM introduced by recursion.**
//!
//! Recursion's claim: instead of core A spawning a foreign binary (cmd.exe, opaque —
//! Q15's L1–L5 wall), A spawns core B = another instance of our own interpreter, and
//! hands it a neutral IR module to run. For that to happen the sub-task must cross the
//! process boundary AS BYTES. This codec is those bytes. It is the seam that recursion
//! *moves the wall to* — measured in RESULTS §②.
//!
//! Two directions:
//!   * downlink  (A→B):  `ser_module` / `de_module`  — the sub-task, as an IR VALUE.
//!                       Contrast Q15 L3: the cmd.exe command was NOT an IR value; it lived
//!                       in the seam BELOW the IR. Here the whole sub-task is inspectable
//!                       bytes A holds before launch.
//!   * uplink    (B→A):  `ser_report` / `de_report`  — B's structured observation of what
//!                       it did (per-intent trace + captured stdout + result).
//!
//! Discipline notes (for §②):
//!   * The MECHANISM is fixed — it does not grow with the number of intents or targets.
//!   * The per-element VOCABULARY (op tags 0..=12, intent tags 0..=5) grows O(ir-surface),
//!     but each tag is a NEUTRAL byte SHARED by both cores — not target-specific and not
//!     disjoint like Q1's L1 (win64 strings vs sysv syscall numbers).

use crate::ir::*;
use std::convert::TryInto;

// ---- little-endian primitives ----
fn w32(o: &mut Vec<u8>, x: u32) { o.extend_from_slice(&x.to_le_bytes()); }
fn w64(o: &mut Vec<u8>, x: u64) { o.extend_from_slice(&x.to_le_bytes()); }

pub struct Rd<'a> { b: &'a [u8], p: usize }
impl<'a> Rd<'a> {
    pub fn new(b: &'a [u8]) -> Self { Rd { b, p: 0 } }
    fn u8(&mut self) -> u8 { let v = self.b[self.p]; self.p += 1; v }
    fn u32(&mut self) -> u32 { let v = u32::from_le_bytes(self.b[self.p..self.p + 4].try_into().unwrap()); self.p += 4; v }
    fn u64(&mut self) -> u64 { let v = u64::from_le_bytes(self.b[self.p..self.p + 8].try_into().unwrap()); self.p += 8; v }
    fn bytes(&mut self, n: usize) -> Vec<u8> { let v = self.b[self.p..self.p + n].to_vec(); self.p += n; v }
}

// ---- intent <-> neutral tag (this vocabulary grows O(intents); each entry is 1 byte, shared) ----
pub fn intent_tag(i: Intent) -> u8 {
    match i {
        Intent::Alloc => 0, Intent::FileOpen => 1, Intent::FileRead => 2,
        Intent::FileClose => 3, Intent::WriteStdout => 4, Intent::SpawnWait => 5,
    }
}
pub fn intent_of(t: u8) -> Intent {
    match t {
        0 => Intent::Alloc, 1 => Intent::FileOpen, 2 => Intent::FileRead,
        3 => Intent::FileClose, 4 => Intent::WriteStdout, 5 => Intent::SpawnWait,
        _ => panic!("bad intent tag {t}"),
    }
}
pub fn intent_name(i: Intent) -> &'static str {
    match i {
        Intent::Alloc => "Alloc", Intent::FileOpen => "FileOpen", Intent::FileRead => "FileRead",
        Intent::FileClose => "FileClose", Intent::WriteStdout => "WriteStdout", Intent::SpawnWait => "SpawnWait",
    }
}

// ---- op (value-producing) — tag grows O(op-surface); each is a neutral byte ----
fn ser_op(o: &mut Vec<u8>, op: &Op) {
    match op {
        Op::Const(x) => { o.push(0); w64(o, *x); }
        Op::Rodata(f) => { o.push(1); w32(o, *f); }
        Op::Add(a, b) => { o.push(2); w32(o, *a); w32(o, *b); }
        Op::Sub(a, b) => { o.push(3); w32(o, *a); w32(o, *b); }
        Op::Mul(a, b) => { o.push(4); w32(o, *a); w32(o, *b); }
        Op::Xor(a, b) => { o.push(5); w32(o, *a); w32(o, *b); }
        Op::And(a, b) => { o.push(6); w32(o, *a); w32(o, *b); }
        Op::Or(a, b) => { o.push(7); w32(o, *a); w32(o, *b); }
        Op::Shl(a, s) => { o.push(8); w32(o, *a); o.push(*s); }
        Op::Shr(a, s) => { o.push(9); w32(o, *a); o.push(*s); }
        Op::Ult(a, b) => { o.push(10); w32(o, *a); w32(o, *b); }
        Op::Load8(a) => { o.push(11); w32(o, *a); }
        Op::LoadW(a) => { o.push(12); w32(o, *a); }
    }
}
fn de_op(r: &mut Rd) -> Op {
    match r.u8() {
        0 => Op::Const(r.u64()),
        1 => Op::Rodata(r.u32()),
        2 => Op::Add(r.u32(), r.u32()),
        3 => Op::Sub(r.u32(), r.u32()),
        4 => Op::Mul(r.u32(), r.u32()),
        5 => Op::Xor(r.u32(), r.u32()),
        6 => Op::And(r.u32(), r.u32()),
        7 => Op::Or(r.u32(), r.u32()),
        8 => Op::Shl(r.u32(), r.u8()),
        9 => Op::Shr(r.u32(), r.u8()),
        10 => Op::Ult(r.u32(), r.u32()),
        11 => Op::Load8(r.u32()),
        12 => Op::LoadW(r.u32()),
        t => panic!("bad op tag {t}"),
    }
}

fn ser_inst(o: &mut Vec<u8>, i: &Inst) {
    match i {
        Inst::Set(d, op) => { o.push(0); w32(o, *d); ser_op(o, op); }
        Inst::Store8(a, v) => { o.push(1); w32(o, *a); w32(o, *v); }
        Inst::StoreW(a, v) => { o.push(2); w32(o, *a); w32(o, *v); }
        Inst::Call(d, id, args) => {
            o.push(3); w32(o, *d); w32(o, *id); w32(o, args.len() as u32);
            for a in args { w32(o, *a); }
        }
    }
}
fn de_inst(r: &mut Rd) -> Inst {
    match r.u8() {
        0 => Inst::Set(r.u32(), de_op(r)),
        1 => Inst::Store8(r.u32(), r.u32()),
        2 => Inst::StoreW(r.u32(), r.u32()),
        3 => {
            let d = r.u32(); let id = r.u32(); let n = r.u32() as usize;
            let args = (0..n).map(|_| r.u32()).collect();
            Inst::Call(d, id, args)
        }
        t => panic!("bad inst tag {t}"),
    }
}

fn ser_term(o: &mut Vec<u8>, t: &Term) {
    match t {
        Term::Br(x) => { o.push(0); w32(o, *x); }
        Term::BrCond(c, nz, z) => { o.push(1); w32(o, *c); w32(o, *nz); w32(o, *z); }
        Term::Ret(v) => { o.push(2); w32(o, *v); }
        Term::Exit(v) => { o.push(3); w32(o, *v); }
    }
}
fn de_term(r: &mut Rd) -> Term {
    match r.u8() {
        0 => Term::Br(r.u32()),
        1 => Term::BrCond(r.u32(), r.u32(), r.u32()),
        2 => Term::Ret(r.u32()),
        3 => Term::Exit(r.u32()),
        t => panic!("bad term tag {t}"),
    }
}

/// downlink: the sub-task, as bytes A holds and can inspect/verify before launch.
pub fn ser_module(m: &Module) -> Vec<u8> {
    let mut o = Vec::new();
    w32(&mut o, m.n_vals);
    w32(&mut o, m.entry);
    o.push(m.takes_ctx as u8);
    w32(&mut o, m.rodata.len() as u32);
    o.extend_from_slice(&m.rodata);
    w32(&mut o, m.externs.len() as u32);
    for e in &m.externs { o.push(intent_tag(e.intent)); w32(&mut o, e.nargs as u32); }
    w32(&mut o, m.blocks.len() as u32);
    for blk in &m.blocks {
        w32(&mut o, blk.insts.len() as u32);
        for i in &blk.insts { ser_inst(&mut o, i); }
        ser_term(&mut o, &blk.term);
    }
    o
}
pub fn de_module(b: &[u8]) -> Module {
    let mut r = Rd::new(b);
    let n_vals = r.u32();
    let entry = r.u32();
    let takes_ctx = r.u8() != 0;
    let rlen = r.u32() as usize;
    let rodata = r.bytes(rlen);
    let ne = r.u32() as usize;
    let mut externs = Vec::with_capacity(ne);
    for _ in 0..ne { externs.push(ExternDecl { intent: intent_of(r.u8()), nargs: r.u32() as usize }); }
    let nb = r.u32() as usize;
    let mut blocks = Vec::with_capacity(nb);
    for _ in 0..nb {
        let ni = r.u32() as usize;
        let mut insts = Vec::with_capacity(ni);
        for _ in 0..ni { insts.push(de_inst(&mut r)); }
        let term = de_term(&mut r);
        blocks.push(Block { insts, term });
    }
    Module { name: "recv", n_vals, blocks, entry, takes_ctx, rodata, externs }
}

// ---- uplink: B's structured observation (per-intent trace + captured stdout + result) ----
pub struct TraceEntry { pub intent: Intent, pub args: Vec<u64>, pub ret: u64 }
pub struct Report { pub result: u64, pub capture: Vec<u8>, pub trace: Vec<TraceEntry>, pub denied: Option<Intent> }

pub fn ser_report(rep: &Report) -> Vec<u8> {
    let mut o = Vec::new();
    w64(&mut o, rep.result);
    match rep.denied { Some(i) => { o.push(1); o.push(intent_tag(i)); } None => o.push(0) }
    w32(&mut o, rep.capture.len() as u32);
    o.extend_from_slice(&rep.capture);
    w32(&mut o, rep.trace.len() as u32);
    for e in &rep.trace {
        o.push(intent_tag(e.intent));
        w32(&mut o, e.args.len() as u32);
        for a in &e.args { w64(&mut o, *a); }
        w64(&mut o, e.ret);
    }
    o
}
pub fn de_report(b: &[u8]) -> Report {
    let mut r = Rd::new(b);
    let result = r.u64();
    let denied = if r.u8() != 0 { Some(intent_of(r.u8())) } else { None };
    let clen = r.u32() as usize;
    let capture = r.bytes(clen);
    let nt = r.u32() as usize;
    let mut trace = Vec::with_capacity(nt);
    for _ in 0..nt {
        let intent = intent_of(r.u8());
        let na = r.u32() as usize;
        let args = (0..na).map(|_| r.u64()).collect();
        let ret = r.u64();
        trace.push(TraceEntry { intent, args, ret });
    }
    Report { result, capture, trace, denied }
}
