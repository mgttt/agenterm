//! Minimal x86-64 machine-code encoder — SHARED by both lowerers.
//!
//! This is deliberately ISA-only: it knows how to encode x86-64 instructions and
//! nothing about ABI (no arg registers, no shadow space, no red zone). The ABI
//! decisions live entirely in `lower/sysv64.rs` and `lower/win64.rs`. That split
//! is the whole point of the experiment: the ISA is shared, the ABI is deferred.
//!
//! Register numbering is the standard x86-64 encoding order.

#![allow(dead_code)]

pub const RAX: u8 = 0;
pub const RCX: u8 = 1;
pub const RDX: u8 = 2;
pub const RBX: u8 = 3;
pub const RSP: u8 = 4;
pub const RBP: u8 = 5;
pub const RSI: u8 = 6;
pub const RDI: u8 = 7;
pub const R8: u8 = 8;
pub const R9: u8 = 9;
pub const R10: u8 = 10;
pub const R11: u8 = 11;

pub struct Asm {
    pub code: Vec<u8>,
    /// pending jump fixups: (position of the rel32 field, target label id)
    fixups: Vec<(usize, u32)>,
    /// resolved label -> code offset
    labels: Vec<Option<usize>>,
}

impl Asm {
    pub fn new() -> Self {
        Asm { code: Vec::new(), fixups: Vec::new(), labels: Vec::new() }
    }

    pub fn new_label(&mut self) -> u32 {
        self.labels.push(None);
        (self.labels.len() - 1) as u32
    }
    pub fn bind(&mut self, l: u32) {
        self.labels[l as usize] = Some(self.code.len());
    }

    fn rex(&mut self, w: bool, r: u8, x: u8, b: u8) {
        let byte = 0x40
            | ((w as u8) << 3)
            | (((r >= 8) as u8) << 2)
            | (((x >= 8) as u8) << 1)
            | ((b >= 8) as u8);
        self.code.push(byte);
    }
    fn modrm(&mut self, md: u8, reg: u8, rm: u8) {
        self.code.push((md << 6) | ((reg & 7) << 3) | (rm & 7));
    }
    fn imm32(&mut self, v: i32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    /// Encode a memory operand [base + disp] with a given reg field. Always disp32.
    fn mem(&mut self, reg: u8, base: u8, disp: i32) {
        // mod=10 (disp32). base rsp/r12 needs a SIB byte.
        self.modrm(0b10, reg, base);
        if (base & 7) == RSP {
            // SIB: scale=0, index=100 (none), base=rsp
            self.code.push((0 << 6) | (0b100 << 3) | RSP);
        }
        self.imm32(disp);
    }

    // ---- moves ----
    pub fn mov_ri64(&mut self, r: u8, imm: u64) {
        self.rex(true, 0, 0, r);
        self.code.push(0xB8 + (r & 7));
        self.code.extend_from_slice(&imm.to_le_bytes());
    }
    /// r = [base+disp]  (64-bit)
    pub fn mov_rm(&mut self, r: u8, base: u8, disp: i32) {
        self.rex(true, r, 0, base);
        self.code.push(0x8B);
        self.mem(r, base, disp);
    }
    /// [base+disp] = r  (64-bit)
    pub fn mov_mr(&mut self, base: u8, disp: i32, r: u8) {
        self.rex(true, r, 0, base);
        self.code.push(0x89);
        self.mem(r, base, disp);
    }
    /// r = r2 (64-bit)
    pub fn mov_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, src, 0, dst);
        self.code.push(0x89);
        self.modrm(0b11, src, dst);
    }
    /// r = zero-extend byte [base+disp]
    pub fn movzx8(&mut self, r: u8, base: u8, disp: i32) {
        self.rex(true, r, 0, base);
        self.code.push(0x0F);
        self.code.push(0xB6);
        self.mem(r, base, disp);
    }
    /// [base+disp] = low byte of r
    pub fn mov_mr8(&mut self, base: u8, disp: i32, r: u8) {
        // 88 /r (no REX.W). Emit REX only if extended regs used.
        if r >= 8 || base >= 8 {
            self.rex(false, r, 0, base);
        }
        self.code.push(0x88);
        self.mem(r, base, disp);
    }
    /// r = zero-extend 32-bit [base+disp]  (32-bit load auto-zero-extends to 64)
    pub fn mov_rm32(&mut self, r: u8, base: u8, disp: i32) {
        if r >= 8 || base >= 8 {
            self.rex(false, r, 0, base);
        }
        self.code.push(0x8B);
        self.mem(r, base, disp);
    }
    /// [base+disp] = low 32 bits of r
    pub fn mov_mr32(&mut self, base: u8, disp: i32, r: u8) {
        if r >= 8 || base >= 8 {
            self.rex(false, r, 0, base);
        }
        self.code.push(0x89);
        self.mem(r, base, disp);
    }

    /// r = &[base+disp]   (lea r64, [base+disp])
    pub fn lea(&mut self, r: u8, base: u8, disp: i32) {
        self.rex(true, r, 0, base);
        self.code.push(0x8D);
        self.mem(r, base, disp);
    }
    /// [base+disp] = imm32   (32-bit store)
    pub fn mov_mi32(&mut self, base: u8, disp: i32, imm: i32) {
        if base >= 8 { self.rex(false, 0, 0, base); }
        self.code.push(0xC7);
        self.mem(0, base, disp);
        self.imm32(imm);
    }
    /// [base+disp] = imm8   (byte store)
    pub fn mov_mi8(&mut self, base: u8, disp: i32, imm: u8) {
        if base >= 8 { self.rex(false, 0, 0, base); }
        self.code.push(0xC6);
        self.mem(0, base, disp);
        self.code.push(imm);
    }

    // ---- ALU reg,reg (64-bit) ----
    fn alu_rr(&mut self, op: u8, dst: u8, src: u8) {
        self.rex(true, src, 0, dst);
        self.code.push(op);
        self.modrm(0b11, src, dst);
    }
    pub fn add_rr(&mut self, d: u8, s: u8) { self.alu_rr(0x01, d, s); }
    pub fn sub_rr(&mut self, d: u8, s: u8) { self.alu_rr(0x29, d, s); }
    pub fn xor_rr(&mut self, d: u8, s: u8) { self.alu_rr(0x31, d, s); }
    pub fn and_rr(&mut self, d: u8, s: u8) { self.alu_rr(0x21, d, s); }
    pub fn or_rr(&mut self, d: u8, s: u8) { self.alu_rr(0x09, d, s); }
    pub fn cmp_rr(&mut self, a: u8, b: u8) { self.alu_rr(0x39, a, b); }
    pub fn test_rr(&mut self, a: u8, b: u8) { self.alu_rr(0x85, a, b); }

    /// dst = dst * src  (imul r64, r/m64)
    pub fn imul_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, dst, 0, src);
        self.code.push(0x0F);
        self.code.push(0xAF);
        self.modrm(0b11, dst, src);
    }

    // ---- ALU reg,imm32 (64-bit, sign-extended) ----
    fn alu_ri(&mut self, ext: u8, r: u8, imm: i32) {
        self.rex(true, 0, 0, r);
        self.code.push(0x81);
        self.modrm(0b11, ext, r);
        self.imm32(imm);
    }
    pub fn add_ri(&mut self, r: u8, imm: i32) { self.alu_ri(0, r, imm); }
    pub fn or_ri(&mut self, r: u8, imm: i32) { self.alu_ri(1, r, imm); }
    pub fn and_ri(&mut self, r: u8, imm: i32) { self.alu_ri(4, r, imm); }
    pub fn sub_ri(&mut self, r: u8, imm: i32) { self.alu_ri(5, r, imm); }
    pub fn cmp_ri(&mut self, r: u8, imm: i32) { self.alu_ri(7, r, imm); }

    // ---- shifts by imm8 ----
    pub fn shl_ri(&mut self, r: u8, imm: u8) {
        self.rex(true, 0, 0, r);
        self.code.push(0xC1);
        self.modrm(0b11, 4, r);
        self.code.push(imm);
    }
    pub fn shr_ri(&mut self, r: u8, imm: u8) {
        self.rex(true, 0, 0, r);
        self.code.push(0xC1);
        self.modrm(0b11, 5, r);
        self.code.push(imm);
    }

    // ---- stack ----
    pub fn push(&mut self, r: u8) {
        if r >= 8 { self.code.push(0x41); }
        self.code.push(0x50 + (r & 7));
    }
    pub fn pop(&mut self, r: u8) {
        if r >= 8 { self.code.push(0x41); }
        self.code.push(0x58 + (r & 7));
    }
    pub fn sub_rsp(&mut self, imm: i32) { self.sub_ri(RSP, imm); }
    pub fn add_rsp(&mut self, imm: i32) { self.add_ri(RSP, imm); }

    // ---- control flow ----
    pub fn call_r(&mut self, r: u8) {
        if r >= 8 { self.rex(false, 0, 0, r); }
        self.code.push(0xFF);
        self.modrm(0b11, 2, r);
    }
    pub fn ret(&mut self) { self.code.push(0xC3); }
    /// setb al ; movzx eax, al  (rax = 1 if CF else 0)  — used for unsigned-less-than
    pub fn setb_eax(&mut self) {
        self.code.extend_from_slice(&[0x0F, 0x92, 0xC0]); // setb al
        self.code.extend_from_slice(&[0x0F, 0xB6, 0xC0]); // movzx eax, al
    }
    pub fn syscall(&mut self) { self.code.push(0x0F); self.code.push(0x05); }

    pub fn jmp(&mut self, l: u32) {
        self.code.push(0xE9);
        let pos = self.code.len();
        self.imm32(0);
        self.fixups.push((pos, l));
    }
    fn jcc(&mut self, cc: u8, l: u32) {
        self.code.push(0x0F);
        self.code.push(0x80 + cc);
        let pos = self.code.len();
        self.imm32(0);
        self.fixups.push((pos, l));
    }
    pub fn jnz(&mut self, l: u32) { self.jcc(0x5, l); }
    pub fn jz(&mut self, l: u32) { self.jcc(0x4, l); }
    pub fn jb(&mut self, l: u32) { self.jcc(0x2, l); }

    /// Resolve all rel32 fixups. Call once, after all code + binds are emitted.
    pub fn finish(mut self) -> Vec<u8> {
        for (pos, l) in self.fixups.drain(..) {
            let target = self.labels[l as usize].expect("unbound label");
            let rel = (target as i64) - (pos as i64 + 4);
            let bytes = (rel as i32).to_le_bytes();
            self.code[pos..pos + 4].copy_from_slice(&bytes);
        }
        self.code
    }
}
