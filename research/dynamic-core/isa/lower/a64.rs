//! Minimal AArch64 (ARMv8-A) machine-code encoder — the PER-ISA analogue of the
//! x86-64 `asm.rs` from Q1. Written clean-room from the published ARM Architecture
//! Reference Manual A64 encodings; every instruction word below is cross-checked
//! against LLVM ground truth (see `main.rs::validate_encoder`).
//!
//! Deliberately ISA-only: it knows how to encode A64 instructions and NOTHING about
//! ABI (no arg registers, no shadow space). The ABI/OS decisions live entirely in
//! `a64_linux.rs` / `a64_win.rs`. Same split as Q1 — the ISA is one bucket, the ABI
//! another. This file is what a *second ISA* costs at the encoder layer.
//!
//! A64 is fixed-width: every instruction is exactly 4 bytes, stored little-endian.

#![allow(dead_code)]

// General registers. x31 encodes SP in address/ALU-with-sp contexts and XZR
// (the zero register) in ALU/compare contexts — the two never collide in one field.
pub const X0: u8 = 0;
pub const X1: u8 = 1;
pub const X2: u8 = 2;
pub const X3: u8 = 3;
pub const X4: u8 = 4;
pub const X5: u8 = 5;
pub const X6: u8 = 6;
pub const X7: u8 = 7;
pub const X8: u8 = 8;
pub const X9: u8 = 9;   // scratch "accumulator" (analogue of x86 RAX in the lowering)
pub const X10: u8 = 10; // scratch second operand (analogue of RCX)
pub const X11: u8 = 11; // reach scratch (analogue of R11)
pub const X30: u8 = 30; // LR
pub const SP: u8 = 31;
pub const XZR: u8 = 31;

pub struct A64 {
    pub code: Vec<u8>,
    /// pending branch fixups: (word position, target label, kind)
    fixups: Vec<(usize, u32, BrKind)>,
    labels: Vec<Option<usize>>,
}

#[derive(Clone, Copy)]
enum BrKind {
    B,        // imm26 unconditional
    Cbnz(u8), // imm19 + Rt
    Cbz(u8),  // imm19 + Rt
}

impl A64 {
    pub fn new() -> Self {
        A64 { code: Vec::new(), fixups: Vec::new(), labels: Vec::new() }
    }
    fn w(&mut self, word: u32) {
        self.code.extend_from_slice(&word.to_le_bytes());
    }
    pub fn new_label(&mut self) -> u32 {
        self.labels.push(None);
        (self.labels.len() - 1) as u32
    }
    pub fn bind(&mut self, l: u32) {
        self.labels[l as usize] = Some(self.code.len());
    }

    // ---- immediates: materialise a 64-bit constant with movz + movk chain ----
    /// xd = imm (1..4 instructions)
    pub fn mov_imm(&mut self, rd: u8, imm: u64) {
        // movz xd, #(imm[0..16])
        self.w(0xD280_0000 | (((imm & 0xffff) as u32) << 5) | rd as u32);
        for hw in 1..4u32 {
            let part = ((imm >> (16 * hw)) & 0xffff) as u32;
            if part != 0 {
                // movk xd, #part, lsl #(16*hw)
                self.w(0xF280_0000 | (hw << 21) | (part << 5) | rd as u32);
            }
        }
    }

    // ---- register move ----  mov xd, xn  == orr xd, xzr, xn
    pub fn mov_rr(&mut self, rd: u8, rn: u8) {
        self.w(0xAA00_0000 | ((rn as u32) << 16) | ((XZR as u32) << 5) | rd as u32);
    }

    // ---- ALU reg,reg (64-bit) ----
    fn alu3(&mut self, base: u32, rd: u8, rn: u8, rm: u8) {
        self.w(base | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32);
    }
    pub fn add(&mut self, rd: u8, rn: u8, rm: u8) { self.alu3(0x8B00_0000, rd, rn, rm); }
    pub fn sub(&mut self, rd: u8, rn: u8, rm: u8) { self.alu3(0xCB00_0000, rd, rn, rm); }
    pub fn and(&mut self, rd: u8, rn: u8, rm: u8) { self.alu3(0x8A00_0000, rd, rn, rm); }
    pub fn orr(&mut self, rd: u8, rn: u8, rm: u8) { self.alu3(0xAA00_0000, rd, rn, rm); }
    pub fn eor(&mut self, rd: u8, rn: u8, rm: u8) { self.alu3(0xCA00_0000, rd, rn, rm); }
    /// xd = xn * xm  (madd xd, xn, xm, xzr)
    pub fn mul(&mut self, rd: u8, rn: u8, rm: u8) {
        self.w(0x9B00_0000 | ((rm as u32) << 16) | ((XZR as u32) << 10) | ((rn as u32) << 5) | rd as u32);
    }
    /// cmp xn, xm  (subs xzr, xn, xm)
    pub fn cmp(&mut self, rn: u8, rm: u8) {
        self.w(0xEB00_0000 | ((rm as u32) << 16) | ((rn as u32) << 5) | XZR as u32);
    }

    // ---- ALU reg,imm12 (64-bit) ----  add/sub xd, xn, #imm  (imm 0..4095)
    pub fn add_imm(&mut self, rd: u8, rn: u8, imm: u32) {
        self.w(0x9100_0000 | ((imm & 0xfff) << 10) | ((rn as u32) << 5) | rd as u32);
    }
    pub fn sub_imm(&mut self, rd: u8, rn: u8, imm: u32) {
        self.w(0xD100_0000 | ((imm & 0xfff) << 10) | ((rn as u32) << 5) | rd as u32);
    }

    // ---- shifts by immediate (64-bit), UBFM aliases ----
    pub fn lsl_imm(&mut self, rd: u8, rn: u8, sh: u8) {
        let immr = ((64 - sh as u32) & 63) << 16;
        let imms = ((63 - sh as u32) & 63) << 10;
        self.w(0xD340_0000 | immr | imms | ((rn as u32) << 5) | rd as u32);
    }
    pub fn lsr_imm(&mut self, rd: u8, rn: u8, sh: u8) {
        let immr = (sh as u32 & 63) << 16;
        let imms = 63u32 << 10;
        self.w(0xD340_0000 | immr | imms | ((rn as u32) << 5) | rd as u32);
    }

    // ---- cset xd, cc   (unsigned lower / carry-clear) == csinc xd,xzr,xzr,cs ----
    /// set xd = 1 if the last cmp was unsigned-lower (a<b), else 0
    pub fn cset_lo(&mut self, rd: u8) {
        // csinc rd, xzr, xzr, cond=CS(0b0010)  [inverted encoding of the LO test]
        self.w(0x9A80_0400 | ((XZR as u32) << 16) | (0x2 << 12) | ((XZR as u32) << 5) | rd as u32);
    }

    // ---- loads / stores (unsigned scaled offset) ----
    /// xt = [xn + #off]   (64-bit; off must be a multiple of 8, 0..32760)
    pub fn ldr(&mut self, rt: u8, rn: u8, off: i32) {
        let imm12 = ((off as u32) / 8) & 0xfff;
        self.w(0xF940_0000 | (imm12 << 10) | ((rn as u32) << 5) | rt as u32);
    }
    /// [xn + #off] = xt   (64-bit)
    pub fn str(&mut self, rt: u8, rn: u8, off: i32) {
        let imm12 = ((off as u32) / 8) & 0xfff;
        self.w(0xF900_0000 | (imm12 << 10) | ((rn as u32) << 5) | rt as u32);
    }
    /// wt = [xn + #off]   (32-bit load, zero-extends to 64; off multiple of 4)
    pub fn ldr_w(&mut self, rt: u8, rn: u8, off: i32) {
        let imm12 = ((off as u32) / 4) & 0xfff;
        self.w(0xB940_0000 | (imm12 << 10) | ((rn as u32) << 5) | rt as u32);
    }
    /// [xn + #off] = wt   (32-bit store)
    pub fn str_w(&mut self, rt: u8, rn: u8, off: i32) {
        let imm12 = ((off as u32) / 4) & 0xfff;
        self.w(0xB900_0000 | (imm12 << 10) | ((rn as u32) << 5) | rt as u32);
    }
    /// wt = zero-extend byte [xn + #off]
    pub fn ldrb(&mut self, rt: u8, rn: u8, off: i32) {
        let imm12 = (off as u32) & 0xfff;
        self.w(0x3940_0000 | (imm12 << 10) | ((rn as u32) << 5) | rt as u32);
    }
    /// [xn + #off] = low byte of wt
    pub fn strb(&mut self, rt: u8, rn: u8, off: i32) {
        let imm12 = (off as u32) & 0xfff;
        self.w(0x3900_0000 | (imm12 << 10) | ((rn as u32) << 5) | rt as u32);
    }

    // ---- control flow ----
    pub fn blr(&mut self, rn: u8) {
        self.w(0xD63F_0000 | ((rn as u32) << 5));
    }
    pub fn ret(&mut self) {
        self.w(0xD65F_0000 | ((X30 as u32) << 5));
    }
    pub fn svc0(&mut self) {
        self.w(0xD400_0001);
    }
    pub fn b(&mut self, l: u32) {
        let pos = self.code.len();
        self.w(0x1400_0000);
        self.fixups.push((pos, l, BrKind::B));
    }
    pub fn cbnz(&mut self, rt: u8, l: u32) {
        let pos = self.code.len();
        self.w(0xB500_0000 | rt as u32);
        self.fixups.push((pos, l, BrKind::Cbnz(rt)));
    }
    pub fn cbz(&mut self, rt: u8, l: u32) {
        let pos = self.code.len();
        self.w(0xB400_0000 | rt as u32);
        self.fixups.push((pos, l, BrKind::Cbz(rt)));
    }

    pub fn finish(mut self) -> Vec<u8> {
        let fixups = std::mem::take(&mut self.fixups);
        for (pos, l, kind) in fixups {
            let target = self.labels[l as usize].expect("unbound label");
            let rel = (target as i64) - (pos as i64); // byte delta; A64 branches are PC-relative to the instr
            let words = (rel / 4) as i32;
            let word = match kind {
                BrKind::B => 0x1400_0000 | ((words as u32) & 0x03ff_ffff),
                BrKind::Cbnz(rt) => 0xB500_0000 | (((words as u32) & 0x7ffff) << 5) | rt as u32,
                BrKind::Cbz(rt) => 0xB400_0000 | (((words as u32) & 0x7ffff) << 5) | rt as u32,
            };
            self.code[pos..pos + 4].copy_from_slice(&word.to_le_bytes());
        }
        self.code
    }
}
