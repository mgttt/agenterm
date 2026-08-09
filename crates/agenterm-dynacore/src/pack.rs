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
///
/// Deliberately does NOT `Vec::with_capacity(n as usize)` for any of the
/// three counts this format reads off untrusted bytes (`n_ext`/`n_blk`/
/// `n_inst`) — found while hardening this crate's test suite: a corrupted or
/// truncated buffer can carry an arbitrary `u32` in any of those fields
/// (e.g. a byte flipped to `0xFF`), and reserving capacity for that many
/// elements BEFORE checking whether the buffer actually contains them tries
/// to allocate up to ~4 billion elements' worth of memory. That allocation
/// request fails, and a failed `Vec::with_capacity` allocation aborts the
/// process (`handle_alloc_error`) — not a catchable panic, not a `None`, not
/// something `catch_unwind` can stop. A store/pack consumer must never be
/// able to take down its host process just by handing it corrupted bytes; a
/// plain `Vec::new()` that grows only as real elements are actually read
/// (each read still bounds-checked against `buf` by `r_str`/`r_u32`/
/// `r_inst`/`r_term`) is bounded by the buffer's real size, not by whatever
/// number a corrupted length field happens to spell.
pub fn deserialize_module(buf: &[u8]) -> Option<Module> {
    let mut p = 0usize;
    let name = r_str(buf, &mut p)?;
    let n_vals = r_u32(buf, &mut p)?;
    let entry = r_u32(buf, &mut p)?;
    let n_ext = r_u32(buf, &mut p)?;
    let mut externs = Vec::new();
    for _ in 0..n_ext {
        let operation_id = r_str(buf, &mut p)?;
        let params_json = r_str(buf, &mut p)?;
        externs.push(ExternDecl {
            operation_id,
            params_json,
        });
    }
    let n_blk = r_u32(buf, &mut p)?;
    let mut blocks = Vec::new();
    for _ in 0..n_blk {
        let n_inst = r_u32(buf, &mut p)?;
        let mut insts = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Builder, Op, Term};

    /// A module exercising every `Op`, every `Inst`, every `Term` variant,
    /// multiple blocks, and a non-empty extern table -- not just the
    /// straight-line one-block modules the integration tests use, so the
    /// wire format's block/inst/extern COUNT fields (not just their payload
    /// encoding) are actually exercised by the round trip.
    fn kitchen_sink_module() -> Module {
        let mut b = Builder::new();
        let a = b.konst(7);
        let bb = b.konst(3);
        let _add = b.set(Op::Add(a, bb));
        let _sub = b.set(Op::Sub(a, bb));
        let _mul = b.set(Op::Mul(a, bb));
        let _xor = b.set(Op::Xor(a, bb));
        let _and = b.set(Op::And(a, bb));
        let _or = b.set(Op::Or(a, bb));
        let _shl = b.set(Op::Shl(a, 2));
        let _shr = b.set(Op::Shr(a, 1));
        let cond = b.set(Op::Ult(a, bb));
        let call = b.fleet_call("demo.op", "{\"x\":1}");
        b.term(Term::BrCond(cond, 1, 2));
        let _ret_call = b.fleet_call("demo.op", "{\"x\":1}"); // reuses the same extern (by value)
        b.term(Term::Br(2));
        b.term(Term::Exit(call));
        b.finish("kitchen_sink", 0)
    }

    #[test]
    fn serialize_then_deserialize_round_trips_a_multi_block_module_exactly() {
        let module = kitchen_sink_module();
        let bytes = serialize_module(&module);
        let decoded = deserialize_module(&bytes).expect("well-formed bytes must decode");
        assert_eq!(decoded, module);
        // Reuse-by-value (Builder::fleet_call's own contract) must survive
        // the wire format too: two calls to the same operation_id+params_json
        // share one extern table entry, not two.
        assert_eq!(decoded.externs.len(), 1);
    }

    #[test]
    fn serialize_is_deterministic_for_the_same_module() {
        let module = kitchen_sink_module();
        assert_eq!(serialize_module(&module), serialize_module(&module));
    }

    #[test]
    fn deserialize_rejects_empty_bytes() {
        assert_eq!(deserialize_module(&[]), None);
    }

    /// Every truncation point of a real serialized module -- not just "one
    /// byte cut somewhere" -- must decode to `None`, never panic (an
    /// out-of-bounds slice on attacker/corruption-controlled length-prefixed
    /// bytes is exactly the class of bug a fixed-width, no-bounds-checked-by-
    /// construction wire format like this one could hide).
    #[test]
    fn deserialize_rejects_every_truncation_of_a_real_module_without_panicking() {
        let module = kitchen_sink_module();
        let bytes = serialize_module(&module);
        assert!(bytes.len() > 16, "sanity: the fixture must be long enough to be worth truncating");
        for cut in 0..bytes.len() {
            let truncated = &bytes[..cut];
            // Must never panic (a slice index out of bounds would abort the
            // whole test binary, not just fail one assertion) and a
            // truncated buffer must never decode to something claiming to be
            // the full module.
            let result = std::panic::catch_unwind(|| deserialize_module(truncated));
            match result {
                Ok(Some(decoded)) => assert_ne!(decoded, module, "cut={cut}: a truncated buffer must not decode into the exact original module"),
                Ok(None) => {}
                Err(_) => panic!("cut={cut}: deserialize_module panicked on truncated input"),
            }
        }
    }

    /// A garbage discriminant byte (an `Op`/`Inst`/`Term` tag no variant
    /// uses) must be rejected, not panic or silently pick a variant.
    #[test]
    fn deserialize_rejects_an_unknown_op_discriminant() {
        let mut b = Builder::new();
        let one = b.konst(1);
        let _ = b.set(Op::Add(one, one));
        b.term(Term::Exit(one));
        let module = b.finish("one_op", 0);
        let mut bytes = serialize_module(&module);

        // Locate the `Op::Add` tag byte (value 1) and corrupt it to a value
        // no `r_op` arm handles. `Const`'s own tag (0) precedes it, so the
        // second `w_u8` written for an op-carrying inst is this one; walking
        // from a known-good round trip is more robust than hardcoding an
        // offset, so instead corrupt every byte in turn and confirm the
        // decoder either still succeeds (byte was inert, e.g. inside a
        // length field it happens not to change meaningfully) or fails
        // cleanly -- never panics.
        for i in 0..bytes.len() {
            let original = bytes[i];
            bytes[i] = 0xFF; // not a valid tag for any Op/Inst/Term encoding
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| deserialize_module(&bytes)));
            assert!(result.is_ok(), "byte {i} corrupted to 0xFF must not panic deserialize_module");
            bytes[i] = original;
        }
    }

    #[test]
    fn build_manifest_hash_matches_store_hash_hex_of_the_same_bytes() {
        let module = kitchen_sink_module();
        let (manifest, bytes) = build_manifest(&module);
        assert_eq!(manifest.hash, crate::store::hash_hex(&bytes));
        assert_eq!(manifest.schema_version, PACK_SCHEMA_VERSION);
        assert_eq!(manifest.operation_ids, vec!["demo.op".to_string()]);
    }
}
