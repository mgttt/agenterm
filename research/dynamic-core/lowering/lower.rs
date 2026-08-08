//! The minimal usable IR->native lowerer (Q3). THIS FILE IS X — the thing measured.
//!
//! It is a pure user of primitives ① (mem_alloc / mem_protect) and ② (jump): it walks
//! the IR bytecode, emits x86_64 machine code into RW memory, flips it to RX, and hands
//! control to it (passing the env-table pointer in rdi). No optimizer, no register
//! allocator, no IR sugar — a single emit pass plus one rel32 back-patch pass. Ugly on
//! purpose: Q3 measures MINIMAL bytes, not compiler quality (§1.4).
//!
//! ==========================================================================
//! For criterion ② the file is split into two clearly marked regions:
//!   [SHARED]  — ISA-independent: IR decode, control flow, label/patch tables,
//!               the call-lowering policy, and the vreg model. Reused by ANY ISA.
//!   [X86_64]  — target-specific machine-code encoders (`e_*`). This is the block
//!               that must be REWRITTEN (and re-shipped) for each new ISA.
//! ==========================================================================

use crate::abi::Kernel;
use crate::ir::*;

// vreg -> x86_64 physical register number (ModRM/REX encoding).
// vr0..vr3 = rbx,r12,r13,r14 (callee-saved: survive CALL). vr4..vr12 = caller-saved.
const VREG: [u8; 13] = [3, 12, 13, 14, 0, 1, 2, 6, 7, 8, 9, 10, 11];
const ENV: u8 = 15; // r15 holds the env-table pointer
const ARGR: [u8; 6] = [7, 6, 2, 1, 8, 9]; // sysv64 arg regs: rdi,rsi,rdx,rcx,r8,r9

const MAXLBL: usize = 16;
const MAXPAT: usize = 64;

// lbl/pat live in mem_alloc'd scratch, NOT on the stack: a large zero-initialized
// stack array would emit a `memset` call, and a flat copied-and-jumped blob (the
// out-of-kernel packaging) cannot satisfy that call — see RESULTS deviation note.
struct Asm {
    buf: *mut u8,
    pos: usize,
    lbl: *mut usize,        // [MAXLBL] label id -> byte offset (NONE = undefined)
    pat: *mut (usize, u8),  // [MAXPAT] (rel32 site offset, label id) to back-patch
    npat: usize,
}
const NONE: usize = usize::MAX;

// ==========================================================================
// [X86_64] target-specific encoders. Everything below `impl Asm` up to the
// SHARED banner is x86_64 machine-code emission — the per-ISA block.
// ==========================================================================
impl Asm {
    #[inline]
    fn b(&mut self, x: u8) {
        unsafe { *self.buf.add(self.pos) = x };
        self.pos += 1;
    }
    fn u32(&mut self, v: u32) {
        let e = v.to_le_bytes();
        for x in e {
            self.b(x);
        }
    }
    fn u64(&mut self, v: u64) {
        let e = v.to_le_bytes();
        for x in e {
            self.b(x);
        }
    }
    // REX prefix. w=64-bit, r=ext(reg), b=ext(rm/base).
    fn rex(&mut self, w: u8, r: u8, b: u8) {
        self.b(0x40 | (w << 3) | ((r >= 8) as u8) << 2 | ((b >= 8) as u8));
    }
    fn modrm(&mut self, md: u8, reg: u8, rm: u8) {
        self.b((md << 6) | ((reg & 7) << 3) | (rm & 7));
    }
    // reg,reg ALU with a single-byte opcode: op reg <- (reg, rm). (dest=reg, src=rm)
    fn rr(&mut self, op: u8, dst: u8, src: u8) {
        self.rex(1, dst, src);
        self.b(op);
        self.modrm(3, dst, src);
    }
    // memory operand [base + disp32], reg field = `reg`. Always uses SIB (index=none)
    // so ONE path encodes every base register (incl. rsp/rbp/r12/r13) — minimal.
    fn mem(&mut self, reg: u8, base: u8, disp: i32) {
        self.modrm(2, reg, 4); // mod=disp32, rm=100 => SIB follows
        self.b(0x20 | (base & 7)); // scale=0, index=100(none), base
        self.u32(disp as u32);
    }
    fn e_imm(&mut self, rd: u8, imm: i64) {
        self.rex(1, 0, rd);
        self.b(0xB8 + (rd & 7));
        self.u64(imm as u64);
    }
    fn e_mov(&mut self, rd: u8, rs: u8) {
        self.rr(0x8B, rd, rs);
    }
    fn e_alu(&mut self, op: u8, rd: u8, rs: u8) {
        self.rr(op, rd, rs);
    }
    fn e_mul(&mut self, rd: u8, rs: u8) {
        self.rex(1, rd, rs);
        self.b(0x0F);
        self.b(0xAF);
        self.modrm(3, rd, rs);
    }
    fn e_shift(&mut self, ext: u8, rd: u8, imm: u8) {
        self.rex(1, 0, rd);
        self.b(0xC1);
        self.modrm(3, ext, rd);
        self.b(imm);
    }
    fn e_ld64(&mut self, rd: u8, rb: u8, off: i32) {
        self.rex(1, rd, rb);
        self.b(0x8B);
        self.mem(rd, rb, off);
    }
    fn e_lde(&mut self, rd: u8, slot: u8) {
        self.rex(1, rd, ENV); // rd = qword [r15 + slot*8]
        self.b(0x8B);
        self.mem(rd, ENV, (slot as i32) * 8);
    }
    fn e_st64(&mut self, rb: u8, off: i32, rs: u8) {
        self.rex(1, rs, rb);
        self.b(0x89);
        self.mem(rs, rb, off);
    }
    fn e_ld8(&mut self, rd: u8, rb: u8, off: i32) {
        self.rex(1, rd, rb); // movzx r64, byte [mem]
        self.b(0x0F);
        self.b(0xB6);
        self.mem(rd, rb, off);
    }
    fn e_st8(&mut self, rb: u8, off: i32, rs: u8) {
        // byte store; REX (even bare 0x40) is required to reach sil/dil low bytes.
        self.b(0x40 | ((rs >= 8) as u8) << 2 | ((rb >= 8) as u8));
        self.b(0x88);
        self.mem(rs, rb, off);
    }
    fn e_cmp(&mut self, ra: u8, rb: u8) {
        self.rr(0x3B, ra, rb);
    }
    // emit a rel32 site (filled 0) and record it for back-patching to label L.
    fn site(&mut self, l: u8) {
        unsafe { *self.pat.add(self.npat) = (self.pos, l) };
        self.npat += 1;
        self.u32(0);
    }
    fn e_jmp(&mut self, l: u8) {
        self.b(0xE9);
        self.site(l);
    }
    fn e_jcc(&mut self, cc: u8, l: u8) {
        self.b(0x0F);
        self.b(cc);
        self.site(l);
    }
    fn e_push(&mut self, r: u8) {
        if r >= 8 {
            self.b(0x41);
        }
        self.b(0x50 + (r & 7));
    }
    // call qword [ENV + idx*8]
    fn e_call_env(&mut self, idx: u8) {
        self.b(0x41); // REX.B for r15 base
        self.b(0xFF);
        self.modrm(2, 2, 4); // /2, mod=disp32, SIB
        self.b(0x20 | (ENV & 7));
        self.u32((idx as u32) * 8);
    }
    fn e_prologue(&mut self) {
        // preserve callee-saved regs the vreg map uses; land rsp 16-aligned for CALLs;
        // move env pointer (rdi) into r15.
        for r in [3u8, 12, 13, 14, 15] {
            self.e_push(r);
        }
        self.b(0x49); // mov r15, rdi
        self.b(0x89);
        self.b(0xFF);
    }
    fn e_halt(&mut self) {
        self.b(0x0F);
        self.b(0x0B); // ud2
    }
    fn patch(&mut self) {
        for i in 0..self.npat {
            let (site, l) = unsafe { *self.pat.add(i) };
            let target = unsafe { *self.lbl.add(l as usize) };
            let rel = (target as isize - (site as isize + 4)) as i32;
            let e = rel.to_le_bytes();
            unsafe {
                for (j, &x) in e.iter().enumerate() {
                    *self.buf.add(site + j) = x;
                }
            }
        }
    }
}

// ==========================================================================
// [SHARED] ISA-independent driver: IR decode + control flow + call policy.
// To retarget to another ISA, only the [X86_64] block above changes; this
// decode/dispatch skeleton is reused unchanged (criterion ②).
// ==========================================================================
// Unchecked table lookups (indices are always in range for well-formed IR) — avoids
// bounds-check panics, which would drag in core::fmt (and MSVC EH) and inflate X.
#[inline]
fn vreg(i: u8) -> u8 {
    unsafe { *VREG.as_ptr().add(i as usize) }
}
#[inline]
fn argr(i: u8) -> u8 {
    unsafe { *ARGR.as_ptr().add(i as usize) }
}

struct Rd<'a> {
    ir: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn u8(&mut self) -> u8 {
        let v = unsafe { *self.ir.as_ptr().add(self.p) };
        self.p += 1;
        v
    }
    fn i32(&mut self) -> i32 {
        let v = unsafe { core::ptr::read_unaligned(self.ir.as_ptr().add(self.p) as *const i32) };
        self.p += 4;
        v
    }
    fn i64(&mut self) -> i64 {
        let v = unsafe { core::ptr::read_unaligned(self.ir.as_ptr().add(self.p) as *const i64) };
        self.p += 8;
        v
    }
}

fn emit(a: &mut Asm, ir: &[u8]) {
    a.e_prologue();
    let mut r = Rd { ir, p: 0 };
    // NOTE: dispatch is an explicit if/else-if chain, NOT a `match`. A dense `match`
    // on contiguous opcodes compiles to an absolute jump table in .rodata; the
    // out-of-kernel flat blob is copied+jumped with NO relocations applied, so that
    // table's absolute entries are wrong and emission corrupts. This is a real cost
    // the minimal (non-relocating) loader imposes on the lowerer. See RESULTS.
    while r.p < ir.len() {
        let op = r.u8();
        if op == OP_IMM {
            let d = vreg(r.u8());
            let imm = r.i64();
            a.e_imm(d, imm);
        } else if op == OP_MOV {
            let d = vreg(r.u8());
            a.e_mov(d, vreg(r.u8()));
        } else if op == OP_MUL {
            let d = vreg(r.u8());
            a.e_mul(d, vreg(r.u8()));
        } else if op == OP_ADD {
            let d = vreg(r.u8());
            a.e_alu(0x03, d, vreg(r.u8()));
        } else if op == OP_SUB {
            let d = vreg(r.u8());
            a.e_alu(0x2B, d, vreg(r.u8()));
        } else if op == OP_AND {
            let d = vreg(r.u8());
            a.e_alu(0x23, d, vreg(r.u8()));
        } else if op == OP_OR {
            let d = vreg(r.u8());
            a.e_alu(0x0B, d, vreg(r.u8()));
        } else if op == OP_XOR {
            let d = vreg(r.u8());
            a.e_alu(0x33, d, vreg(r.u8()));
        } else if op == OP_SHL {
            let d = vreg(r.u8());
            a.e_shift(4, d, r.u8());
        } else if op == OP_SHR {
            let d = vreg(r.u8());
            a.e_shift(5, d, r.u8());
        } else if op == OP_LD8 {
            let d = vreg(r.u8());
            let b = vreg(r.u8());
            a.e_ld8(d, b, r.i32());
        } else if op == OP_LD64 {
            let d = vreg(r.u8());
            let b = vreg(r.u8());
            a.e_ld64(d, b, r.i32());
        } else if op == OP_LDE {
            let d = vreg(r.u8());
            a.e_lde(d, r.u8());
        } else if op == OP_ST8 {
            let b = vreg(r.u8());
            let off = r.i32();
            a.e_st8(b, off, vreg(r.u8()));
        } else if op == OP_ST64 {
            let b = vreg(r.u8());
            let off = r.i32();
            a.e_st64(b, off, vreg(r.u8()));
        } else if op == OP_JMP {
            a.e_jmp(r.u8());
        } else if op == OP_JB {
            let ra = vreg(r.u8());
            let rb = vreg(r.u8());
            a.e_cmp(ra, rb);
            a.e_jcc(0x82, r.u8());
        } else if op == OP_JAE {
            let ra = vreg(r.u8());
            let rb = vreg(r.u8());
            a.e_cmp(ra, rb);
            a.e_jcc(0x83, r.u8());
        } else if op == OP_JE {
            let ra = vreg(r.u8());
            let rb = vreg(r.u8());
            a.e_cmp(ra, rb);
            a.e_jcc(0x84, r.u8());
        } else if op == OP_JNE {
            let ra = vreg(r.u8());
            let rb = vreg(r.u8());
            a.e_cmp(ra, rb);
            a.e_jcc(0x85, r.u8());
        } else if op == OP_LABEL {
            let l = r.u8();
            unsafe { *a.lbl.add(l as usize) = a.pos };
        } else if op == OP_CALL {
            let rd = r.u8();
            let idx = r.u8();
            let argc = r.u8();
            // Move args into sysv64 arg regs. IR guarantees args come from the
            // callee-saved vreg bank (vr0..vr3 = rbx/r12/r13/r14), which never collide
            // with the arg regs (rdi/rsi/rdx/rcx/r8/r9) — plain sequential moves suffice.
            let mut i = 0u8;
            while i < argc {
                a.e_mov(argr(i), vreg(r.u8()));
                i += 1;
            }
            a.e_call_env(idx);
            if rd != RD_DISCARD {
                a.e_mov(vreg(rd), 0); // result (rax=0) -> rd
            }
        } else {
            a.e_halt();
        }
    }
    a.patch();
}

// Diagnostic only (cfg-gated so it never enters X): emit without jumping, return the
// emitted byte length.
// Build an Asm with code buffer + scratch tables, all from primitive ① (no stack
// arrays -> no memset dependency). lbl is filled with NONE by an explicit loop.
fn new_asm(k: &Kernel, code: *mut u8) -> Asm {
    let lbl = (k.mem_alloc)(MAXLBL * 8) as *mut usize;
    let pat = (k.mem_alloc)(MAXPAT * 16) as *mut (usize, u8);
    let mut i = 0;
    while i < MAXLBL {
        unsafe { *lbl.add(i) = NONE };
        i += 1;
    }
    Asm { buf: code, pos: 0, lbl, pat, npat: 0 }
}

/// Lower `ir` to native code using primitive ① for memory, then enter it (②) with
/// the env-table pointer in rdi. Never returns: the lowered payload exits the process.
pub fn lower_and_run(k: &Kernel, ir: &[u8], env: *const usize) -> ! {
    let cap = 8192usize;
    let code = (k.mem_alloc)(cap);
    let mut a = new_asm(k, code);
    emit(&mut a, ir);
    (k.mem_protect)(code, cap, true); // ① RW -> RX
    let entry: extern "sysv64" fn(*const usize) -> ! = unsafe { core::mem::transmute(code) };
    entry(env) // ② jump
}
