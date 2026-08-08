//! The three payloads from the prior round, re-expressed in the neutral IR.
//! Semantics are REUSED verbatim (spec §2 "复用上一轮那三个，不要新造"); only the
//! representation changed (Rust source -> neutral IR).
//!
//! What to watch:
//!   * `pure_compute`  — no externs, no ctx. If this is not neutral the thesis dies.
//!   * `read_hash_print` — real OS calls with different arities (FileOpen etc.);
//!     exercises ABI-divergent arg placement (CreateFileA=7 args spills to the stack
//!     differently under each ABI). Mostly neutral IR + a few intent calls.
//!   * `spawn_echo` — the known hard bone. The whole OS operation collapses to ONE
//!     coarse intent because its natural operands are STRUCTS, which the IR is
//!     forbidden to describe. See RESULTS §② leak L3.

use crate::ir::*;

// FNV-1a/64 constants — these are ALGORITHM constants, not layout numbers.
const FNV_BASIS: u64 = 14695981039346656037;
const FNV_PRIME: u64 = 1099511628211;
const READ_CAP: u64 = 65536;
// pure_compute reuses the prior round's exact mix constant (its exit code is 163).
const PURE_BASIS: u64 = 1469598103934665603;

/// Payload ① — pure computation, returns low byte of a fixed mix. No OS, no ctx.
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

    b.finish("pure_compute", false, 0)
}

/// Payload ② — read "input.txt", FNV-1a/64 hash it, print 16 hex digits + newline.
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
    let cont = b.set(Op::Ult(idx, n)); // idx < n ?
    b.term(Term::BrCond(cont, 2, 3)); // in-range -> body, else -> format

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
    // out[16] = '\n'
    let c16 = b.konst(16);
    let nl_pos = b.set(Op::Add(out, c16));
    let nl = b.konst(0x0a);
    b.store8(nl_pos, nl);
    // pos = out + 15 ; c = 16 ; hextab = rodata(hex_off)
    let c15 = b.konst(15);
    let pos = b.set(Op::Add(out, c15));
    let c = b.konst(16);
    let hextab = b.set(Op::Rodata(hex_off));
    let fmask = b.konst(0xf);
    b.term(Term::Br(4));

    // block 4: hex loop  (writes 16 chars)
    let nib = b.set(Op::And(hh, fmask));
    let chaddr = b.set(Op::Add(hextab, nib));
    let ch = b.set(Op::Load8(chaddr));
    b.store8(pos, ch);
    b.assign(hh, Op::Shr(hh, 4));
    b.assign(pos, Op::Sub(pos, one));
    b.assign(c, Op::Sub(c, one));
    b.term(Term::BrCond(c, 4, 5)); // c != 0 -> loop, else done

    // block 5: write + exit
    let seventeen = b.konst(17);
    let _ = b.call(Intent::WriteStdout, vec![out, seventeen]);
    let zero = b.konst(0);
    b.term(Term::Exit(zero));

    b.finish("read_hash_print", true, 0)
}

/// Payload ③ — spawn a fixed child, wait, print "exit=NN", exit with that code.
pub fn spawn_echo() -> Module {
    let mut b = Builder::new();
    let template = b.rodata(b"exit=00\n"); // 8 bytes; digits patched at runtime

    let code = b.call(Intent::SpawnWait, vec![]);

    // out = Alloc(8); copy the 8-byte template in
    let c8a = b.konst(8);
    let out = b.call(Intent::Alloc, vec![c8a]);
    let tmpl = b.set(Op::Rodata(template));
    // copy 8 bytes: i=0; while i<8 { out[i]=tmpl[i]; i++ }
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

    // block 3: two decimal digits of `code` (code < 100). tens = code/10 via loop.
    let tens = b.konst(0);
    let zero0 = b.konst(0);
    let tmp = b.set(Op::Add(code, zero0)); // tmp = code (copy)
    let ten = b.konst(10);
    b.term(Term::Br(4));
    // block 4: while !(tmp < 10) { tmp -= 10; tens++ }
    let lt = b.set(Op::Ult(tmp, ten));
    b.term(Term::BrCond(lt, 6, 5)); // tmp<10 -> emit digits, else keep subtracting
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

    b.finish("spawn_echo", true, 0)
}
