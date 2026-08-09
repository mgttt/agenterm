//! Q22 (assembled) — NEW payload code (payload-level, not engine-level — the same
//! kind of file `ir/payloads/payloads.rs` already is; adding a payload has always cost
//! payload-authoring LOC in this track, that is not part of the "marginal engine cost"
//! claim being measured for `FileWrite`).
//!
//! `filewrite_demo` exercises the new `Intent::FileWrite`. `bad_ir_demo` is a
//! deliberately malformed `Module`, built by hand (not through `Builder`, which cannot
//! express an out-of-range extern id) to feed `verify::verify` a negative probe loaded
//! FROM THE STORE, mirroring Q19's P2 probe (`ExternIdOutOfRange`).

use crate::ir::*;

/// Writes `n` bytes (the fixed string below) to `dc_assembled_filewrite_out.txt` in the
/// CWD, returns the byte count WriteFile reported. Exercises FileOpen-shaped path/const
/// injection (L2) and the NEW FileWrite intent end to end.
pub fn filewrite_demo() -> Module {
    let mut b = Builder::new();
    let path_off = b.rodata(b"dc_assembled_filewrite_out.txt\0");
    let payload_off = b.rodata(b"hello from Q22 FileWrite\n"); // 26 bytes
    let path = b.set(Op::Rodata(path_off));
    let buf = b.set(Op::Rodata(payload_off));
    let len = b.konst(25); // "hello from Q22 FileWrite\n" is 25 bytes
    let n = b.call(Intent::FileWrite, vec![path, buf, len]);
    b.term(Term::Exit(n));
    b.finish("filewrite_demo", false, 0)
}

/// Well-formed BY `verify::verify`'s OWN RULES (the extern's declared `nargs` and the
/// call site's arg count agree — both are 1, because `Builder::call` derives `nargs`
/// FROM the call site) — but it calls `FileWrite` with only 1 of the 3 args the seam
/// table (`seam.rs::FILEWRITE_TABLE`, `input_slots: &[0,1,2]`) assumes exist. `verify`
/// has no notion of the seam table's own arity contract, so this IR sails through the
/// well-formedness gate and only fails inside the interpreter (out-of-bounds slice
/// index) — the cross-part gap named in RESULTS.md ②.
pub fn mismatched_arity_demo() -> Module {
    let mut b = Builder::new();
    let path_off = b.rodata(b"dc_assembled_mismatch.txt\0");
    let path = b.set(Op::Rodata(path_off));
    let n = b.call(Intent::FileWrite, vec![path]); // 1 arg; FileWrite's seam table wants 3
    b.term(Term::Exit(n));
    b.finish("mismatched_arity_demo", false, 0)
}

/// Structurally malformed IR: a `Call` naming extern id 99 while the extern table is
/// EMPTY. `verify::verify` must reject this with `ExternIdOutOfRange` before any
/// execution — the negative probe for criterion ②.
pub fn bad_ir_demo() -> Module {
    Module {
        name: "bad_ir_demo",
        n_vals: 1,
        blocks: vec![Block {
            insts: vec![Inst::Call(0, 99, vec![])],
            term: Term::Exit(0),
        }],
        entry: 0,
        takes_ctx: false,
        rodata: vec![],
        externs: vec![],
    }
}
