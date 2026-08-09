//! Guest CPU state: the sixteen general-purpose registers, `rip`, and the
//! four flags this interpreter's `jcc`/`cmp`/`test` subset actually needs.
//! Register numbering matches the real x86_64 encoding (ModRM/SIB reg
//! fields index straight into `regs`) so `decode.rs` never needs a
//! translation table.

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
#[allow(dead_code)] // named for ABI-table completeness (Linux syscall arg 5)
pub const R11: u8 = 11;

pub const REG_NAMES: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15",
];

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Flags {
    pub cf: bool,
    pub zf: bool,
    pub sf: bool,
    pub of: bool,
}

pub struct Cpu {
    pub regs: [u64; 16],
    /// Offset into the guest image byte buffer -- NOT a host address. Only
    /// `RipRel` address resolution and the top-level fetch loop ever read
    /// this as an index; every register value that represents a genuine
    /// memory address is a real host pointer (see `mem.rs`'s header).
    pub rip: usize,
    pub flags: Flags,
}

impl Cpu {
    pub fn new(entry: usize) -> Self {
        Cpu { regs: [0; 16], rip: entry, flags: Flags::default() }
    }

    #[inline]
    pub fn get(&self, idx: u8) -> u64 {
        self.regs[idx as usize]
    }

    #[inline]
    pub fn set32(&mut self, idx: u8, val: u32) {
        // x86_64 rule: a 32-bit write zero-extends into the full 64-bit register.
        self.regs[idx as usize] = u64::from(val);
    }

    #[inline]
    pub fn set64(&mut self, idx: u8, val: u64) {
        self.regs[idx as usize] = val;
    }

    #[inline]
    pub fn set_width(&mut self, idx: u8, val: u64, w64: bool) {
        if w64 {
            self.set64(idx, val);
        } else {
            self.set32(idx, val as u32);
        }
    }

    #[inline]
    pub fn get_width(&self, idx: u8, w64: bool) -> u64 {
        if w64 { self.regs[idx as usize] } else { self.regs[idx as usize] & 0xFFFF_FFFF }
    }
}
