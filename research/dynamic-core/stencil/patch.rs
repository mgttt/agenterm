//! Q10 APPLIER — THIS FILE IS X_new (the runtime "lowerer" under copy-and-patch).
//!
//! There is NO instruction encoder and NO register allocator here. For each IR
//! instruction it: (1) selects the pre-built stencil (machine code produced at BUILD
//! time by rustc -O2, see stencils.rs), (2) `memcpy`s its bytes into the code buffer,
//! (3) applies the recorded PC32 holes by writing `target - (site+4)`. That is the
//! whole "code generator". VM state lives in a MEMORY register file, so the vreg id IS
//! its slot — no physical-register mapping, nothing carried across stencils.
//!
//! The ONE thing copy-and-patch cannot express is control flow: a compiled stencil
//! cannot leave CPU flags live across its boundary. So JMP/Jcc/LABEL are emitted here
//! as a few fixed bytes + a rel32 back-patch (the residual encoder — measured in ①).

use crate::abi::Kernel;
use crate::ir::*;
use crate::stencils_gen::*;

const RF_SLOTS: usize = 24; // 13 vregs + scratch; slot 13 = discard sink
const DISCARD: usize = 13;
const CODE_CAP: usize = 16384; // 4 pages, RX
const NONE: usize = usize::MAX;
const MAXLBL: usize = 16;
const MAXPAT: usize = 256;

struct Ctx {
    rd: usize,
    rs: usize,
    rb: usize,
    slot: usize,      // env slot (LDE) / call idx
    args: [usize; 4], // call arg slots
    imm_slot: usize,  // const-pool address holding an imm/count
    off_slot: usize,  // const-pool address holding a mem offset
}

struct Emit {
    code: *mut u8,
    pos: usize,
    rf: usize,
    env: usize,
    pool: *mut u8,
    ppos: usize,
    lbl: *mut usize,
    pat: *mut (usize, u8),
    npat: usize,
}

impl Emit {
    #[inline]
    fn b(&mut self, x: u8) {
        unsafe { *self.code.add(self.pos) = x };
        self.pos += 1;
    }
    fn wr32(&mut self, at: usize, v: i32) {
        let mut i = 0;
        while i < 4 {
            unsafe { *self.code.add(at + i) = (v >> (i * 8)) as u8 };
            i += 1;
        }
    }
    fn d32(&mut self, v: i32) {
        let mut i = 0;
        while i < 4 {
            self.b((v >> (i * 8)) as u8);
            i += 1;
        }
    }
    // push an 8-byte value into the constant pool, return its absolute address.
    fn pool_push(&mut self, v: u64) -> usize {
        let at = self.ppos;
        let mut i = 0;
        while i < 8 {
            unsafe { *self.pool.add(at + i) = (v >> (i * 8)) as u8 };
            i += 1;
        }
        self.ppos += 8;
        self.pool as usize + at
    }
    // resolve a hole kind to its absolute target address for this instruction.
    fn target(&self, kind: u8, c: &Ctx) -> usize {
        if kind == 0 {
            self.rf + c.rd * 8
        } else if kind == 1 {
            self.rf + c.rs * 8
        } else if kind == 2 {
            self.rf + c.rb * 8
        } else if kind == 3 {
            c.imm_slot
        } else if kind == 4 {
            c.off_slot
        } else if kind == 5 {
            self.env + c.slot * 8
        } else if kind == 6 {
            self.rf + c.args[0] * 8
        } else if kind == 7 {
            self.rf + c.args[1] * 8
        } else if kind == 8 {
            self.rf + c.args[2] * 8
        } else {
            self.rf + c.args[3] * 8
        }
    }
    // memcpy a stencil body and apply its PC32 holes for this instruction.
    fn place(&mut self, s: &Stencil, c: &Ctx) {
        let base = self.pos;
        let mut i = 0;
        while i < s.code.len() {
            self.b(unsafe { *s.code.as_ptr().add(i) });
            i += 1;
        }
        let mut h = 0;
        while h < s.holes.len() {
            let hole = unsafe { *s.holes.as_ptr().add(h) };
            let site = base + hole.off as usize;
            let t = self.target(hole.kind, c) as isize;
            let rel = t - (self.code as isize + site as isize + 4);
            self.wr32(site, rel as i32);
            h += 1;
        }
    }
    // emit a rip-relative memory access opcode `pfx...` whose disp32 targets abs addr T.
    // Caller has already emitted the ModRM (rip-relative /05) up to the disp field.
    fn riprel(&mut self, t: usize) {
        // disp is relative to the address AFTER the 4-byte disp field.
        let rel = t as isize - (self.code as isize + self.pos as isize + 4);
        self.d32(rel as i32);
    }
    // control flow: record a rel32 jump site to be patched to label L.
    fn jsite(&mut self, l: u8) {
        unsafe { *self.pat.add(self.npat) = (self.pos, l) };
        self.npat += 1;
        self.d32(0);
    }
    fn patch(&mut self) {
        let mut i = 0;
        while i < self.npat {
            let (site, l) = unsafe { *self.pat.add(i) };
            let target = unsafe { *self.lbl.add(l as usize) };
            let rel = target as isize - (site as isize + 4);
            self.wr32(site, rel as i32);
            i += 1;
        }
    }
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

fn slot(rd: u8) -> usize {
    if rd == RD_DISCARD {
        DISCARD
    } else {
        rd as usize
    }
}

fn emit(e: &mut Emit, ir: &[u8]) {
    let mut r = Rd { ir, p: 0 };
    let z = Ctx { rd: 0, rs: 0, rb: 0, slot: 0, args: [0; 4], imm_slot: 0, off_slot: 0 };
    while r.p < ir.len() {
        let op = r.u8();
        if op == OP_IMM {
            let rd = slot(r.u8());
            let imm = r.i64() as u64;
            let mut c = Ctx { rd, ..z_copy(&z) };
            c.imm_slot = e.pool_push(imm);
            e.place(&ST_IMM, &c);
        } else if op == OP_MOV {
            let rd = slot(r.u8());
            let rs = slot(r.u8());
            e.place(&ST_MOV, &Ctx { rd, rs, ..z_copy(&z) });
        } else if op == OP_ADD {
            let rd = slot(r.u8());
            let rs = slot(r.u8());
            e.place(&ST_ADD, &Ctx { rd, rs, ..z_copy(&z) });
        } else if op == OP_SUB {
            let rd = slot(r.u8());
            let rs = slot(r.u8());
            e.place(&ST_SUB, &Ctx { rd, rs, ..z_copy(&z) });
        } else if op == OP_MUL {
            let rd = slot(r.u8());
            let rs = slot(r.u8());
            e.place(&ST_MUL, &Ctx { rd, rs, ..z_copy(&z) });
        } else if op == OP_AND {
            let rd = slot(r.u8());
            let rs = slot(r.u8());
            e.place(&ST_AND, &Ctx { rd, rs, ..z_copy(&z) });
        } else if op == OP_OR {
            let rd = slot(r.u8());
            let rs = slot(r.u8());
            e.place(&ST_OR, &Ctx { rd, rs, ..z_copy(&z) });
        } else if op == OP_XOR {
            let rd = slot(r.u8());
            let rs = slot(r.u8());
            e.place(&ST_XOR, &Ctx { rd, rs, ..z_copy(&z) });
        } else if op == OP_SHL {
            let rd = slot(r.u8());
            let cnt = r.u8() as u64;
            let mut c = Ctx { rd, ..z_copy(&z) };
            c.imm_slot = e.pool_push(cnt);
            e.place(&ST_SHL, &c);
        } else if op == OP_SHR {
            let rd = slot(r.u8());
            let cnt = r.u8() as u64;
            let mut c = Ctx { rd, ..z_copy(&z) };
            c.imm_slot = e.pool_push(cnt);
            e.place(&ST_SHR, &c);
        } else if op == OP_LD8 {
            let rd = slot(r.u8());
            let rb = slot(r.u8());
            let off = r.i32() as i64 as u64;
            let mut c = Ctx { rd, rb, ..z_copy(&z) };
            c.off_slot = e.pool_push(off);
            e.place(&ST_LD8, &c);
        } else if op == OP_LD64 {
            let rd = slot(r.u8());
            let rb = slot(r.u8());
            let off = r.i32() as i64 as u64;
            let mut c = Ctx { rd, rb, ..z_copy(&z) };
            c.off_slot = e.pool_push(off);
            e.place(&ST_LD64, &c);
        } else if op == OP_ST8 {
            let rb = slot(r.u8());
            let off = r.i32() as i64 as u64;
            let rs = slot(r.u8());
            let mut c = Ctx { rb, rs, ..z_copy(&z) };
            c.off_slot = e.pool_push(off);
            e.place(&ST_ST8, &c);
        } else if op == OP_ST64 {
            let rb = slot(r.u8());
            let off = r.i32() as i64 as u64;
            let rs = slot(r.u8());
            let mut c = Ctx { rb, rs, ..z_copy(&z) };
            c.off_slot = e.pool_push(off);
            e.place(&ST_ST64, &c);
        } else if op == OP_LDE {
            let rd = slot(r.u8());
            let s = r.u8() as usize;
            e.place(&ST_LDE, &Ctx { rd, slot: s, ..z_copy(&z) });
        } else if op == OP_CALL {
            let rd = slot(r.u8());
            let idx = r.u8() as usize;
            let argc = r.u8();
            let mut args = [0usize; 4];
            let mut i = 0u8;
            while i < argc && i < 4 {
                unsafe { *args.as_mut_ptr().add(i as usize) = slot(r.u8()) };
                i += 1;
            }
            let c = Ctx { rd, slot: idx, args, ..z_copy(&z) };
            if argc == 0 {
                e.place(&ST_CALL0, &c);
            } else if argc == 1 {
                e.place(&ST_CALL1, &c);
            } else if argc == 2 {
                e.place(&ST_CALL2, &c);
            } else if argc == 3 {
                e.place(&ST_CALL3, &c);
            } else {
                e.place(&ST_CALL4, &c);
            }
        } else if op == OP_LABEL {
            let l = r.u8();
            unsafe { *e.lbl.add(l as usize) = e.pos };
        } else if op == OP_JMP {
            let l = r.u8();
            e.b(0xE9);
            e.jsite(l);
        } else if op == OP_JB || op == OP_JAE || op == OP_JE || op == OP_JNE {
            // residual encoder: mov rax,[rip+&rf[ra]] ; cmp rax,[rip+&rf[rb]] ; Jcc L
            let ra = slot(r.u8());
            let rb = slot(r.u8());
            let l = r.u8();
            let cc = if op == OP_JB {
                0x82
            } else if op == OP_JAE {
                0x83
            } else if op == OP_JE {
                0x84
            } else {
                0x85
            };
            e.b(0x48);
            e.b(0x8B);
            e.b(0x05);
            e.riprel(e.rf + ra * 8); // mov rax,[rip+rf[ra]]
            e.b(0x48);
            e.b(0x3B);
            e.b(0x05);
            e.riprel(e.rf + rb * 8); // cmp rax,[rip+rf[rb]]
            e.b(0x0F);
            e.b(cc);
            e.jsite(l); // Jcc rel32 -> label
        } else {
            e.b(0x0F);
            e.b(0x0B); // ud2 safety
        }
    }
}

// Ctx is not Copy (keeps the struct honest); this clones the zero template cheaply.
#[inline]
fn z_copy(z: &Ctx) -> Ctx {
    Ctx {
        rd: z.rd,
        rs: z.rs,
        rb: z.rb,
        slot: z.slot,
        args: z.args,
        imm_slot: z.imm_slot,
        off_slot: z.off_slot,
    }
}

/// Lower `ir` to native code via copy-and-patch, then enter it. Never returns.
pub fn lower_and_run(k: &Kernel, ir: &[u8], env: *const usize) -> ! {
    // ONE arena so code / regfile / pool / env-copy are within PC32 (+-2GB) reach of
    // each other (the copy-and-patch placement constraint, §7.4). Only the code
    // sub-range is flipped to RX; regfile/pool/env-copy stay RW after it.
    const ARENA: usize = CODE_CAP + 0x4000;
    let arena = (k.mem_alloc)(ARENA);
    let code = arena;
    let rf = unsafe { arena.add(CODE_CAP) } as usize;
    let pool = unsafe { arena.add(CODE_CAP + RF_SLOTS * 8) };
    let envcopy = unsafe { arena.add(CODE_CAP + RF_SLOTS * 8 + 0x1000) } as *mut usize;
    let lbl = (k.mem_alloc)(MAXLBL * 8) as *mut usize;
    let pat = (k.mem_alloc)(MAXPAT * 16) as *mut (usize, u8);
    // copy env into the arena so rip-relative access to env slots is in range.
    let mut i = 0;
    while i < ENV_LEN {
        unsafe { *envcopy.add(i) = *env.add(i) };
        i += 1;
    }
    i = 0;
    while i < MAXLBL {
        unsafe { *lbl.add(i) = NONE };
        i += 1;
    }
    let mut e = Emit {
        code,
        pos: 0,
        rf,
        env: envcopy as usize,
        pool,
        ppos: 0,
        lbl,
        pat,
        npat: 0,
    };
    emit(&mut e, ir);
    e.patch();
    (k.mem_protect)(code, CODE_CAP, true); // ① RW -> RX (code sub-range only)
    let entry: extern "sysv64" fn() -> ! = unsafe { core::mem::transmute(code) };
    entry() // ② jump; the payload exits via an env CALL and never returns
}
