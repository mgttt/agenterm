//! Canonical payloads — ported from `research/dynamic-core/ir/payloads/payloads.rs`
//! (the three original Q1/Q9 payloads) and `research/dynamic-core/assembled/
//! extra_payloads.rs` (Q22's `filewrite_demo`/`bad_ir_demo`), plus one new
//! negative payload for THIS crate's F1 fix
//! (`intent_arity_mismatch_demo`). Semantics are reused verbatim where a
//! payload already existed; only representation details changed (`Module`
//! here has no `takes_ctx` field — dropped the same way the logic pack's IR
//! dropped it, since no execution path in this crate's `eval_core.rs` ever
//! reads it either).
//!
//! `pub` (not `pub(crate)`) so `tests/*.rs` integration tests can build the
//! exact same real payloads the design doc names by name
//! (`pure_compute`/`read_hash_print`/`spawn_echo`).

#![allow(dead_code)]

use crate::ir::*;

// FNV-1a/64 constants — ALGORITHM constants, not layout numbers.
const FNV_BASIS: u64 = 14695981039346656037;
const FNV_PRIME: u64 = 1099511628211;
const READ_CAP: u64 = 65536;
// pure_compute reuses the track's exact mix constant (its exit code is 163).
const PURE_BASIS: u64 = 1469598103934665603;

/// Payload ① — pure computation, returns low byte of a fixed mix. No OS calls
/// at all: if this is not neutral (needs no intent to run), the thesis dies.
pub fn pure_compute() -> Module {
    let mut b = Builder::new();
    let acc = b.konst(PURE_BASIS);
    let i = b.konst(0);
    let one = b.konst(1);
    let prime = b.konst(FNV_PRIME);
    let limit = b.konst(1_000_000);
    b.term(Term::Br(1)); // -> loop

    // block 1: loop body
    b.assign(acc, Op::Mul(acc, prime));
    b.assign(acc, Op::Add(acc, i));
    b.assign(i, Op::Add(i, one));
    let d = b.set(Op::Sub(limit, i));
    b.term(Term::BrCond(d, 1, 2)); // d != 0 -> loop, else -> done

    // block 2: done
    let mask = b.konst(0xff);
    let r = b.set(Op::And(acc, mask));
    b.term(Term::Exit(r));

    b.finish("pure_compute", 0)
}

/// Payload ② — read "input.txt" (in CWD at run time), FNV-1a/64 hash it,
/// print 16 hex digits + newline. Exercises Alloc/FileOpen/FileRead/
/// FileClose/WriteStdout.
pub fn read_hash_print() -> Module {
    let mut b = Builder::new();
    let path_off = b.rodata(b"input.txt\0");
    let hex_off = b.rodata(b"0123456789abcdef");

    let cap = b.konst(READ_CAP);
    let buf = b.call(Intent::Alloc, vec![cap]);
    let path = b.set(Op::Rodata(path_off));
    let h = b.call(Intent::FileOpen, vec![path]);
    let n = b.call(Intent::FileRead, vec![h, buf, cap]);
    let _ = b.call(Intent::FileClose, vec![h]);

    // hash: hh = basis; idx = 0; while idx < n { hh ^= buf[idx]; hh *= prime; idx++ }
    let hh = b.konst(FNV_BASIS);
    let prime = b.konst(FNV_PRIME);
    let idx = b.konst(0);
    let one = b.konst(1);
    b.term(Term::Br(1));

    // block 1: hash loop guard
    let cont = b.set(Op::Ult(idx, n));
    b.term(Term::BrCond(cont, 2, 3));

    // block 2: hash body
    let addr = b.set(Op::Add(buf, idx));
    let byte = b.set(Op::Load8(addr));
    b.assign(hh, Op::Xor(hh, byte));
    b.assign(hh, Op::Mul(hh, prime));
    b.assign(idx, Op::Add(idx, one));
    b.term(Term::Br(1));

    // block 3: format 16 hex digits (right to left) + newline, then write
    let c17 = b.konst(17);
    let out = b.call(Intent::Alloc, vec![c17]);
    let c16 = b.konst(16);
    let nl_pos = b.set(Op::Add(out, c16));
    let nl = b.konst(0x0a);
    b.store8(nl_pos, nl);
    let c15 = b.konst(15);
    let pos = b.set(Op::Add(out, c15));
    let c = b.konst(16);
    let hextab = b.set(Op::Rodata(hex_off));
    let fmask = b.konst(0xf);
    b.term(Term::Br(4));

    // block 4: hex loop (writes 16 chars)
    let nib = b.set(Op::And(hh, fmask));
    let chaddr = b.set(Op::Add(hextab, nib));
    let ch = b.set(Op::Load8(chaddr));
    b.store8(pos, ch);
    b.assign(hh, Op::Shr(hh, 4));
    b.assign(pos, Op::Sub(pos, one));
    b.assign(c, Op::Sub(c, one));
    b.term(Term::BrCond(c, 4, 5));

    // block 5: write + exit
    let seventeen = b.konst(17);
    let _ = b.call(Intent::WriteStdout, vec![out, seventeen]);
    let zero = b.konst(0);
    b.term(Term::Exit(zero));

    b.finish("read_hash_print", 0)
}

/// Payload ③ — spawn a fixed child, wait, print "exit=NN", exit with that
/// code. Exercises Alloc/WriteStdout/SpawnWait.
pub fn spawn_echo() -> Module {
    let mut b = Builder::new();
    let template = b.rodata(b"exit=00\n"); // 8 bytes; digits patched at runtime

    let code = b.call(Intent::SpawnWait, vec![]);

    let c8a = b.konst(8);
    let out = b.call(Intent::Alloc, vec![c8a]);
    let tmpl = b.set(Op::Rodata(template));
    let ci = b.konst(0);
    let one = b.konst(1);
    let eight = b.konst(8);
    b.term(Term::Br(1));
    // block 1: copy guard
    let ccont = b.set(Op::Ult(ci, eight));
    b.term(Term::BrCond(ccont, 2, 3));
    // block 2: copy body
    let sa = b.set(Op::Add(tmpl, ci));
    let sb = b.set(Op::Load8(sa));
    let da = b.set(Op::Add(out, ci));
    b.store8(da, sb);
    b.assign(ci, Op::Add(ci, one));
    b.term(Term::Br(1));

    // block 3: two decimal digits of `code` (code < 100)
    let tens = b.konst(0);
    let zero0 = b.konst(0);
    let tmp = b.set(Op::Add(code, zero0));
    let ten = b.konst(10);
    b.term(Term::Br(4));
    // block 4: while !(tmp < 10) { tmp -= 10; tens++ }
    let lt = b.set(Op::Ult(tmp, ten));
    b.term(Term::BrCond(lt, 6, 5));
    // block 5: subtract
    b.assign(tmp, Op::Sub(tmp, ten));
    b.assign(tens, Op::Add(tens, one));
    b.term(Term::Br(4));

    // block 6: patch out[5]='0'+tens, out[6]='0'+tmp ; write 8 ; exit code
    let zc = b.konst(0x30);
    let tens_ch = b.set(Op::Add(zc, tens));
    let c5 = b.konst(5);
    let p5 = b.set(Op::Add(out, c5));
    b.store8(p5, tens_ch);
    let ones_ch = b.set(Op::Add(zc, tmp));
    let c6 = b.konst(6);
    let p6 = b.set(Op::Add(out, c6));
    b.store8(p6, ones_ch);
    let len8 = b.konst(8);
    let _ = b.call(Intent::WriteStdout, vec![out, len8]);
    b.term(Term::Exit(code));

    b.finish("spawn_echo", 0)
}

/// Payload ④ — the seventh intent, `FileWrite`, not exercised by any of the
/// three above: create/truncate a file, write a fixed string, return the
/// byte count. `out_filename` is a caller-chosen relative path so different
/// tests running concurrently in different CWDs don't collide.
pub fn filewrite_demo(out_filename: &str) -> Module {
    let mut b = Builder::new();
    let mut path_bytes = out_filename.as_bytes().to_vec();
    path_bytes.push(0);
    let path_off = b.rodata(&path_bytes);
    let payload_off = b.rodata(b"hello from nativecore\n"); // 22 bytes
    let path = b.set(Op::Rodata(path_off));
    let buf = b.set(Op::Rodata(payload_off));
    let len = b.konst(22);
    let n = b.call(Intent::FileWrite, vec![path, buf, len]);
    b.term(Term::Exit(n));
    b.finish("filewrite_demo", 0)
}

/// Structurally malformed IR (Q19-style negative probe): a `Call` naming
/// extern id 99 while the extern table is EMPTY. `verify::verify` must
/// reject this with `ExternIdOutOfRange` before any execution.
pub fn bad_ir_demo() -> Module {
    Module {
        name: "bad_ir_demo".to_string(),
        n_vals: 1,
        blocks: vec![Block { insts: vec![Inst::Call(0, 99, vec![])], term: Term::Exit(0) }],
        entry: 0,
        rodata: vec![],
        externs: vec![],
        registry_externs: vec![],
    }
}

/// **F1 reproduction.** Well-formed BY Q19's ORIGINAL rule (the extern's
/// declared `nargs` and the call site's own arg count agree — both are 1,
/// because `Builder::call_raw_arity` is told to declare exactly 1) but
/// `SpawnWait`'s real native-call contract is 0 args
/// (`Intent::contract_arity`). Under Q22's verifier alone this passed and
/// only panicked inside the interpreter/seam; under THIS crate's
/// `verify::verify` it must be rejected with `IrFault::IntentArityMismatch`
/// — the direct acceptance test for design doc §5 item 3(b).
pub fn spawnwait_arity_mismatch_demo() -> Module {
    let mut b = Builder::new();
    let one = b.konst(1);
    let _ = b.call_raw_arity(Intent::SpawnWait, vec![one], 1);
    b.term(Term::Exit(one));
    b.finish("spawnwait_arity_mismatch_demo", 0)
}

/// Second F1 reproduction, different intent shape: `FileWrite`'s real
/// contract is 3 args; this IR self-consistently declares (and calls) it
/// with only 1. This is the exact case
/// `research/dynamic-core/assembled/extra_payloads.rs::mismatched_arity_demo`
/// used to demonstrate the panic Q22 left open — reproduced here as a
/// `verify()`-time rejection instead.
pub fn filewrite_arity_mismatch_demo() -> Module {
    let mut b = Builder::new();
    let path_off = b.rodata(b"dc_native_mismatch.txt\0");
    let path = b.set(Op::Rodata(path_off));
    let _ = b.call_raw_arity(Intent::FileWrite, vec![path], 1);
    b.term(Term::Exit(path));
    b.finish("filewrite_arity_mismatch_demo", 0)
}
