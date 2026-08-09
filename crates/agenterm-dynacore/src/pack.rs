//! Pack manifest and IR wire format.
//!
//! The manifest is the "目标操作依赖清单、hash、schema 版本" the design doc
//! asks for: what a loader holds to locate a pack's bytes in `store::Store`
//! (by hash — see `store.rs`'s header for why no name->hash step exists) and
//! to audit which `fleet.*` operations it declares before ever
//! deserializing, verifying, or running it.
//!
//! `serialize_module`/`deserialize_module` are this crate's IR wire format —
//! the content that gets hashed and stored. Ported/shrunk from
//! `research/dynamic-core/assembled/ir_ser.rs` (Q22, itself new code with no
//! prior-Q precedent — Q9/Q19/Q21 all consumed an in-memory `Module` in the
//! same process). Shrunk here because this crate's IR has no rodata and no
//! raw-memory ops; the format is still deliberately naive (fixed-width
//! fields, no varint/compression) — the property under test is "loaded from
//! a store", not "a good wire format".

use crate::ir::{Block, ExternDecl, Inst, Module, Op, Term};
use crate::store::{Store, hash_hex};

pub const PACK_SCHEMA_VERSION: u32 = 1;

/// What a loader holds before it ever touches pack bytes: the content hash
/// to fetch from `store::Store`, and the `operation_id`s the pack declares
/// (mirrors `Module::externs`) so dependencies can be audited without first
/// deserializing and verifying.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PackManifest {
    pub schema_version: u32,
    pub hash: String,
    pub operation_ids: Vec<String>,
}

/// Serialize `m` to bytes and derive the manifest that describes it —
/// content-addressing happens here (build time), not at load time.
pub fn build_manifest(m: &Module) -> (PackManifest, Vec<u8>) {
    let bytes = serialize_module(m);
    let hash = hash_hex(&bytes);
    let operation_ids = m.externs.iter().map(|e| e.operation_id.clone()).collect();
    (
        PackManifest {
            schema_version: PACK_SCHEMA_VERSION,
            hash,
            operation_ids,
        },
        bytes,
    )
}

/// Serialize `m`, write it into `store` under its content hash, and return
/// the manifest a loader should hold onto (build-time step — the design
/// doc's "构造式内容寻址：源内容→hash→存进 store").
pub fn pack(store: &Store, m: &Module) -> std::io::Result<PackManifest> {
    let (manifest, bytes) = build_manifest(m);
    let stored_hash = store.put(&bytes)?;
    debug_assert_eq!(stored_hash, manifest.hash, "store.put must reproduce build_manifest's hash");
    Ok(manifest)
}

/// Fetch `manifest.hash` from `store` and deserialize it back into a
/// `Module` (run-time step). This does NOT verify well-formedness — call
/// `verify::verify` on the result before `eval_core::run`.
pub fn load(store: &Store, manifest: &PackManifest) -> Result<Module, String> {
    let bytes = store
        .get(&manifest.hash)
        .ok_or_else(|| format!("hash {} not found in store (or content mismatch)", manifest.hash))?;
    deserialize_module(&bytes).ok_or_else(|| format!("hash {}: deserialize failed (corrupt bytes)", manifest.hash))
}

// ============================================================================
// wire format
// ============================================================================

pub fn serialize_module(m: &Module) -> Vec<u8> {
    let mut out = Vec::new();
    w_str(&mut out, &m.name);
    w_u32(&mut out, m.n_vals);
    w_u32(&mut out, m.entry);
    w_u32(&mut out, m.externs.len() as u32);
    for e in &m.externs {
        w_str(&mut out, &e.operation_id);
        w_str(&mut out, &e.params_json);
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

/// Returns `None` on any structural read failure (truncated/garbage bytes) —
/// a SEPARATE failure mode from `verify::verify` (which checks a
/// successfully-decoded `Module` is well-formed). This function only turns
/// bytes back into a `Module` value.
pub fn deserialize_module(buf: &[u8]) -> Option<Module> {
    let mut p = 0usize;
    let name = r_str(buf, &mut p)?;
    let n_vals = r_u32(buf, &mut p)?;
    let entry = r_u32(buf, &mut p)?;
    let n_ext = r_u32(buf, &mut p)?;
    let mut externs = Vec::with_capacity(n_ext as usize);
    for _ in 0..n_ext {
        let operation_id = r_str(buf, &mut p)?;
        let params_json = r_str(buf, &mut p)?;
        externs.push(ExternDecl {
            operation_id,
            params_json,
        });
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
        name,
        n_vals,
        blocks,
        entry,
        externs,
    })
}

fn w_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}
fn w_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn w_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn w_str(out: &mut Vec<u8>, s: &str) {
    w_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

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

fn w_op(out: &mut Vec<u8>, op: &Op) {
    match op {
        Op::Const(x) => {
            w_u8(out, 0);
            w_u64(out, *x);
        }
        Op::Add(a, b) => {
            w_u8(out, 1);
            w_u32(out, *a);
            w_u32(out, *b);
        }
        Op::Sub(a, b) => {
            w_u8(out, 2);
            w_u32(out, *a);
            w_u32(out, *b);
        }
        Op::Mul(a, b) => {
            w_u8(out, 3);
            w_u32(out, *a);
            w_u32(out, *b);
        }
        Op::Xor(a, b) => {
            w_u8(out, 4);
            w_u32(out, *a);
            w_u32(out, *b);
        }
        Op::And(a, b) => {
            w_u8(out, 5);
            w_u32(out, *a);
            w_u32(out, *b);
        }
        Op::Or(a, b) => {
            w_u8(out, 6);
            w_u32(out, *a);
            w_u32(out, *b);
        }
        Op::Shl(a, s) => {
            w_u8(out, 7);
            w_u32(out, *a);
            w_u8(out, *s);
        }
        Op::Shr(a, s) => {
            w_u8(out, 8);
            w_u32(out, *a);
            w_u8(out, *s);
        }
        Op::Ult(a, b) => {
            w_u8(out, 9);
            w_u32(out, *a);
            w_u32(out, *b);
        }
    }
}
fn r_op(buf: &[u8], p: &mut usize) -> Option<Op> {
    let tag = *buf.get(*p)?;
    *p += 1;
    Some(match tag {
        0 => Op::Const(r_u64(buf, p)?),
        1 => Op::Add(r_u32(buf, p)?, r_u32(buf, p)?),
        2 => Op::Sub(r_u32(buf, p)?, r_u32(buf, p)?),
        3 => Op::Mul(r_u32(buf, p)?, r_u32(buf, p)?),
        4 => Op::Xor(r_u32(buf, p)?, r_u32(buf, p)?),
        5 => Op::And(r_u32(buf, p)?, r_u32(buf, p)?),
        6 => Op::Or(r_u32(buf, p)?, r_u32(buf, p)?),
        7 => Op::Shl(r_u32(buf, p)?, {
            let v = *buf.get(*p)?;
            *p += 1;
            v
        }),
        8 => Op::Shr(r_u32(buf, p)?, {
            let v = *buf.get(*p)?;
            *p += 1;
            v
        }),
        9 => Op::Ult(r_u32(buf, p)?, r_u32(buf, p)?),
        _ => return None,
    })
}

fn w_inst(out: &mut Vec<u8>, inst: &Inst) {
    match inst {
        Inst::Set(d, op) => {
            w_u8(out, 0);
            w_u32(out, *d);
            w_op(out, op);
        }
        Inst::FleetCall(d, id) => {
            w_u8(out, 1);
            w_u32(out, *d);
            w_u32(out, *id);
        }
    }
}
fn r_inst(buf: &[u8], p: &mut usize) -> Option<Inst> {
    let tag = *buf.get(*p)?;
    *p += 1;
    Some(match tag {
        0 => Inst::Set(r_u32(buf, p)?, r_op(buf, p)?),
        1 => Inst::FleetCall(r_u32(buf, p)?, r_u32(buf, p)?),
        _ => return None,
    })
}

fn w_term(out: &mut Vec<u8>, t: &Term) {
    match t {
        Term::Br(x) => {
            w_u8(out, 0);
            w_u32(out, *x);
        }
        Term::BrCond(c, nz, z) => {
            w_u8(out, 1);
            w_u32(out, *c);
            w_u32(out, *nz);
            w_u32(out, *z);
        }
        Term::Ret(v) => {
            w_u8(out, 2);
            w_u32(out, *v);
        }
        Term::Exit(v) => {
            w_u8(out, 3);
            w_u32(out, *v);
        }
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
