//! A tiny byte-emitting helper this crate's own tests/examples use to
//! hand-encode guest programs (same technique the reference prototype
//! `guest_interp.rs` used: write real opcode bytes, patch relative
//! displacements once label positions are known) without doing that
//! displacement arithmetic by hand at every call site. Not a real
//! assembler -- there is no mnemonic parser, just raw byte pushes plus
//! label/patch bookkeeping. Public (not `#[cfg(test)]`-gated) so both
//! `tests/*.rs` and `examples/*.rs` (separate compilation units) can use it.

pub struct Emitter {
    pub buf: Vec<u8>,
}

impl Default for Emitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Emitter {
    pub fn new() -> Self {
        Emitter { buf: Vec::new() }
    }

    /// Current position -- use to record a label before emitting its target
    /// instruction.
    pub fn here(&self) -> usize {
        self.buf.len()
    }

    pub fn b(&mut self, byte: u8) -> &mut Self {
        self.buf.push(byte);
        self
    }

    pub fn bytes(&mut self, bs: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bs);
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// Emit a 1-byte placeholder (for a `rel8` jcc/jmp displacement),
    /// returning its offset for a later `patch_i8`.
    pub fn i8_hole(&mut self) -> usize {
        let at = self.buf.len();
        self.buf.push(0);
        at
    }

    /// Emit a 4-byte placeholder (for a `rel32` jmp/call, or a RIP-relative
    /// `lea`/`mov` disp32), returning its offset for a later `patch_i32`.
    pub fn i32_hole(&mut self) -> usize {
        let at = self.buf.len();
        self.buf.extend_from_slice(&[0; 4]);
        at
    }

    /// Patch an `i8_hole` so it encodes the real x86 `rel8` semantics:
    /// signed displacement from the address of the byte AFTER the 1-byte
    /// field (`at + 1`) to `target`.
    pub fn patch_i8(&mut self, at: usize, target: usize) {
        let end = at + 1;
        let rel = target as i64 - end as i64;
        assert!((-128..=127).contains(&rel), "rel8 out of range: {rel}");
        self.buf[at] = rel as i8 as u8;
    }

    /// Patch an `i32_hole` so it encodes the real x86 `rel32`/RIP-relative
    /// `disp32` semantics: signed displacement from the address of the byte
    /// AFTER the 4-byte field (`at + 4`) to `target`. Both `jmp/call rel32`
    /// and RIP-relative `lea`/`mov` operands use exactly this arithmetic --
    /// the whole point of `decode::resolve_addr` (see its header) is that
    /// this must be the address of the NEXT instruction, not the current
    /// one.
    pub fn patch_i32(&mut self, at: usize, target: usize) {
        let end = at + 4;
        let rel = target as i64 - end as i64;
        assert!(i32::try_from(rel).is_ok(), "rel32 out of range: {rel}");
        self.buf[at..at + 4].copy_from_slice(&(rel as i32).to_le_bytes());
    }
}
