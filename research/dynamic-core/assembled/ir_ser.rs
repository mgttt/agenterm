//! Q22 (assembled) — NEW: a minimal serializer/deserializer for `ir::Module`, so the
//! IR can be written to and read back from the content-addressed store (`store.rs`) as
//! bytes on disk, not passed around as an in-process Rust value. This is new code (no
//! prior Q built a byte format for the IR — Q9/Q19/Q21 all consumed an in-memory
//! `Module` built by `payloads.rs` in the SAME process). It is deliberately tiny and
//! format-naive (fixed-width fields, no varint/compression) — the property under test
//! is "loaded from a store", not "a good wire format".

#![allow(dead_code)]

use crate::ir::*;

pub fn serialize(m: &Module) -> Vec<u8> {
    let mut out = Vec::new();
    w_str(&mut out, m.name);
    w_u32(&mut out, m.n_vals);
    w_u32(&mut out, m.entry);
    out.push(if m.takes_ctx { 1 } else { 0 });
    w_u32(&mut out, m.rodata.len() as u32);
    out.extend_from_slice(&m.rodata);
    w_u32(&mut out, m.externs.len() as u32);
    for e in &m.externs {
        out.push(intent_tag(e.intent));
        w_u32(&mut out, e.nargs as u32);
    }
    w_u32(&mut out, m.blocks.len() as u32);
    for b in &m.blocks {
        w_u32(&mut out, b.insts.len() as u32);
        for inst in &b.insts {
            w_inst(&mut out, inst);
        }
        w_term(&mut out, &b.term);
    }
    out
}

/// Returns None on any structural read failure (truncated/garbage bytes) — a SEPARATE
/// failure mode from `verify::verify` (which checks a successfully-decoded IR is
/// well-formed). This function only turns bytes back into a `Module` value.
pub fn deserialize(buf: &[u8]) -> Option<Module> {
    let mut p = 0usize;
    let name = r_str(buf, &mut p)?;
    let n_vals = r_u32(buf, &mut p)?;
    let entry = r_u32(buf, &mut p)?;
    let takes_ctx = *buf.get(p)? != 0;
    p += 1;
    let rlen = r_u32(buf, &mut p)? as usize;
    let rodata = buf.get(p..p + rlen)?.to_vec();
    p += rlen;
    let n_ext = r_u32(buf, &mut p)?;
    let mut externs = Vec::with_capacity(n_ext as usize);
    for _ in 0..n_ext {
        let intent = tag_intent(*buf.get(p)?)?;
        p += 1;
        let nargs = r_u32(buf, &mut p)? as usize;
        externs.push(ExternDecl { intent, nargs });
    }
    let n_blk = r_u32(buf, &mut p)?;
    let mut blocks = Vec::with_capacity(n_blk as usize);
    for _ in 0..n_blk {
        let n_inst = r_u32(buf, &mut p)?;
        let mut insts = Vec::with_capacity(n_inst as usize);
        for _ in 0..n_inst {
            insts.push(r_inst(buf, &mut p)?);
        }
        let term = r_term(buf, &mut p)?;
        blocks.push(Block { insts, term });
    }
    Some(Module {
        name: Box::leak(name.into_boxed_str()),
        n_vals,
        blocks,
        entry,
        takes_ctx,
        rodata,
        externs,
    })
}

// ---- primitive writers/readers ----
fn w_u8(out: &mut Vec<u8>, v: u8) { out.push(v); }
fn w_u32(out: &mut Vec<u8>, v: u32) { out.extend_from_slice(&v.to_le_bytes()); }
fn w_u64(out: &mut Vec<u8>, v: u64) { out.extend_from_slice(&v.to_le_bytes()); }
fn w_str(out: &mut Vec<u8>, s: &str) { w_u32(out, s.len() as u32); out.extend_from_slice(s.as_bytes()); }

fn r_u32(buf: &[u8], p: &mut usize) -> Option<u32> {
    let b: [u8; 4] = buf.get(*p..*p + 4)?.try_into().ok()?;
    *p += 4;
    Some(u32::from_le_bytes(b))
}
fn r_u64(buf: &[u8], p: &mut usize) -> Option<u64> {
    let b: [u8; 8] = buf.get(*p..*p + 8)?.try_into().ok()?;
    *p += 8;
    Some(u64::from_le_bytes(b))
}
fn r_str(buf: &[u8], p: &mut usize) -> Option<String> {
    let len = r_u32(buf, p)? as usize;
    let s = buf.get(*p..*p + len)?;
    *p += len;
    String::from_utf8(s.to_vec()).ok()
}

fn intent_tag(i: Intent) -> u8 {
    match i {
        Intent::Alloc => 0,
        Intent::FileOpen => 1,
        Intent::FileRead => 2,
        Intent::FileClose => 3,
        Intent::WriteStdout => 4,
        Intent::SpawnWait => 5,
        Intent::FileWrite => 6,
    }
}
fn tag_intent(t: u8) -> Option<Intent> {
    Some(match t {
        0 => Intent::Alloc,
        1 => Intent::FileOpen,
        2 => Intent::FileRead,
        3 => Intent::FileClose,
        4 => Intent::WriteStdout,
        5 => Intent::SpawnWait,
        6 => Intent::FileWrite,
        _ => return None,
    })
}

fn w_op(out: &mut Vec<u8>, op: &Op) {
    match op {
        Op::Const(x) => { w_u8(out, 0); w_u64(out, *x); }
        Op::Rodata(o) => { w_u8(out, 1); w_u32(out, *o); }
        Op::Add(a, b) => { w_u8(out, 2); w_u32(out, *a); w_u32(out, *b); }
        Op::Sub(a, b) => { w_u8(out, 3); w_u32(out, *a); w_u32(out, *b); }
        Op::Mul(a, b) => { w_u8(out, 4); w_u32(out, *a); w_u32(out, *b); }
        Op::Xor(a, b) => { w_u8(out, 5); w_u32(out, *a); w_u32(out, *b); }
        Op::And(a, b) => { w_u8(out, 6); w_u32(out, *a); w_u32(out, *b); }
        Op::Or(a, b) => { w_u8(out, 7); w_u32(out, *a); w_u32(out, *b); }
        Op::Shl(a, s) => { w_u8(out, 8); w_u32(out, *a); w_u8(out, *s); }
        Op::Shr(a, s) => { w_u8(out, 9); w_u32(out, *a); w_u8(out, *s); }
        Op::Ult(a, b) => { w_u8(out, 10); w_u32(out, *a); w_u32(out, *b); }
        Op::Load8(a) => { w_u8(out, 11); w_u32(out, *a); }
        Op::LoadW(a) => { w_u8(out, 12); w_u32(out, *a); }
    }
}
fn r_op(buf: &[u8], p: &mut usize) -> Option<Op> {
    let tag = *buf.get(*p)?;
    *p += 1;
    Some(match tag {
        0 => Op::Const(r_u64(buf, p)?),
        1 => Op::Rodata(r_u32(buf, p)?),
        2 => Op::Add(r_u32(buf, p)?, r_u32(buf, p)?),
        3 => Op::Sub(r_u32(buf, p)?, r_u32(buf, p)?),
        4 => Op::Mul(r_u32(buf, p)?, r_u32(buf, p)?),
        5 => Op::Xor(r_u32(buf, p)?, r_u32(buf, p)?),
        6 => Op::And(r_u32(buf, p)?, r_u32(buf, p)?),
        7 => Op::Or(r_u32(buf, p)?, r_u32(buf, p)?),
        8 => Op::Shl(r_u32(buf, p)?, { let v = *buf.get(*p)?; *p += 1; v }),
        9 => Op::Shr(r_u32(buf, p)?, { let v = *buf.get(*p)?; *p += 1; v }),
        10 => Op::Ult(r_u32(buf, p)?, r_u32(buf, p)?),
        11 => Op::Load8(r_u32(buf, p)?),
        12 => Op::LoadW(r_u32(buf, p)?),
        _ => return None,
    })
}

fn w_inst(out: &mut Vec<u8>, inst: &Inst) {
    match inst {
        Inst::Set(d, op) => { w_u8(out, 0); w_u32(out, *d); w_op(out, op); }
        Inst::Store8(a, v) => { w_u8(out, 1); w_u32(out, *a); w_u32(out, *v); }
        Inst::StoreW(a, v) => { w_u8(out, 2); w_u32(out, *a); w_u32(out, *v); }
        Inst::Call(d, id, args) => {
            w_u8(out, 3); w_u32(out, *d); w_u32(out, *id);
            w_u32(out, args.len() as u32);
            for a in args { w_u32(out, *a); }
        }
    }
}
fn r_inst(buf: &[u8], p: &mut usize) -> Option<Inst> {
    let tag = *buf.get(*p)?;
    *p += 1;
    Some(match tag {
        0 => Inst::Set(r_u32(buf, p)?, r_op(buf, p)?),
        1 => Inst::Store8(r_u32(buf, p)?, r_u32(buf, p)?),
        2 => Inst::StoreW(r_u32(buf, p)?, r_u32(buf, p)?),
        3 => {
            let d = r_u32(buf, p)?;
            let id = r_u32(buf, p)?;
            let n = r_u32(buf, p)?;
            let mut args = Vec::with_capacity(n as usize);
            for _ in 0..n { args.push(r_u32(buf, p)?); }
            Inst::Call(d, id, args)
        }
        _ => return None,
    })
}

fn w_term(out: &mut Vec<u8>, t: &Term) {
    match t {
        Term::Br(x) => { w_u8(out, 0); w_u32(out, *x); }
        Term::BrCond(c, nz, z) => { w_u8(out, 1); w_u32(out, *c); w_u32(out, *nz); w_u32(out, *z); }
        Term::Ret(v) => { w_u8(out, 2); w_u32(out, *v); }
        Term::Exit(v) => { w_u8(out, 3); w_u32(out, *v); }
    }
}
fn r_term(buf: &[u8], p: &mut usize) -> Option<Term> {
    let tag = *buf.get(*p)?;
    *p += 1;
    Some(match tag {
        0 => Term::Br(r_u32(buf, p)?),
        1 => Term::BrCond(r_u32(buf, p)?, r_u32(buf, p)?, r_u32(buf, p)?),
        2 => Term::Ret(r_u32(buf, p)?),
        3 => Term::Exit(r_u32(buf, p)?),
        _ => return None,
    })
}
