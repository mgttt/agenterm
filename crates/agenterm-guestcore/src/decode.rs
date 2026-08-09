//! REX prefix + ModRM + SIB + disp8/32 (+ RIP-relative) decoding. This is the
//! addressing-mode core `interp.rs`'s opcode handlers all share -- one
//! decode routine, reused by every instruction that takes an r/m operand
//! (`mov`, the ALU ops, `lea`, `movzx`/`movsx`, `test`), same as a real
//! decoder table.

#[derive(Clone, Copy, Debug, Default)]
pub struct Rex {
    pub w: bool,
    pub r: bool,
    pub x: bool,
    pub b: bool,
}

impl Rex {
    pub fn parse(byte: u8) -> Rex {
        Rex { w: byte & 0x08 != 0, r: byte & 0x04 != 0, x: byte & 0x02 != 0, b: byte & 0x01 != 0 }
    }
}

/// A memory operand's address, in one of two forms depending on whether it
/// depends on where the CURRENT instruction ends (`RipRel`) or is fully
/// known the moment ModRM/SIB/disp are parsed (`Abs`). See `mem.rs`'s header
/// for why getting the `+base` on `RipRel` right is the one regression this
/// whole crate exists to guard.
#[derive(Clone, Copy, Debug)]
pub enum MemAddr {
    /// A fully resolved host address (base_reg [+ index_reg*scale] + disp).
    Abs(u64),
    /// `[rip + disp32]` -- resolve with `resolve_rip_rel` once the full
    /// instruction length (including any trailing immediate) is known.
    RipRel(i32),
}

#[derive(Clone, Copy, Debug)]
pub enum Rm {
    Reg(u8),
    Mem(MemAddr),
}

pub struct Modrm {
    pub rm: Rm,
    /// The ModRM `reg` field, extended by REX.R -- the second operand
    /// register (or opcode-extension digit, for the `/0`..`/7` group ops).
    pub reg: u8,
    /// Bytes consumed by ModRM (+ SIB, + disp) -- NOT including any trailing
    /// immediate the specific opcode may still need to read.
    pub len: usize,
}

/// Decode one ModRM(+SIB+disp) starting at `code[pos]`. `regs` is read (not
/// mutated) to resolve non-RIP-relative memory operands immediately, since
/// those don't depend on the final instruction length.
pub fn decode_modrm(code: &[u8], pos: usize, rex: Rex, regs: &[u64; 16]) -> Modrm {
    let modrm = code[pos];
    let md = modrm >> 6;
    let reg_field = (modrm >> 3) & 0x7;
    let rm_field = modrm & 0x7;
    let reg = reg_field | if rex.r { 0x8 } else { 0 };

    if md == 0b11 {
        let rm = rm_field | if rex.b { 0x8 } else { 0 };
        return Modrm { rm: Rm::Reg(rm), reg, len: 1 };
    }

    let mut p = pos + 1;
    let mem = if rm_field == 0b100 {
        // SIB byte follows.
        let sib = code[p];
        p += 1;
        let scale: u64 = 1u64 << (sib >> 6);
        let idx_field = (sib >> 3) & 0x7;
        let base_field = sib & 0x7;
        let has_index = !(idx_field == 0b100 && !rex.x); // raw idx==100 && no REX.X => "no index"
        let index_val = if has_index {
            let index_reg = idx_field | if rex.x { 0x8 } else { 0 };
            regs[index_reg as usize].wrapping_mul(scale)
        } else {
            0
        };
        let no_base = md == 0b00 && base_field == 0b101; // mod=00,base=101 => no base, disp32 only
        let base_val = if no_base { 0 } else { regs[(base_field | if rex.b { 0x8 } else { 0 }) as usize] };
        let disp: i64 = if no_base {
            let d = read_i32(code, p);
            p += 4;
            i64::from(d)
        } else {
            read_disp(code, &mut p, md)
        };
        MemAddr::Abs((base_val as i64).wrapping_add(index_val as i64).wrapping_add(disp) as u64)
    } else if md == 0b00 && rm_field == 0b101 {
        // RIP-relative: disp32, resolved later against this instruction's end.
        let d = read_i32(code, p);
        p += 4;
        MemAddr::RipRel(d)
    } else {
        let base_reg = rm_field | if rex.b { 0x8 } else { 0 };
        let base_val = regs[base_reg as usize];
        let disp = read_disp(code, &mut p, md);
        MemAddr::Abs((base_val as i64).wrapping_add(disp) as u64)
    };
    Modrm { rm: Rm::Mem(mem), reg, len: p - pos }
}

fn read_disp(code: &[u8], p: &mut usize, md: u8) -> i64 {
    match md {
        0b00 => 0,
        0b01 => {
            let d = code[*p] as i8;
            *p += 1;
            i64::from(d)
        }
        0b10 => {
            let d = read_i32(code, *p);
            *p += 4;
            i64::from(d)
        }
        _ => unreachable!("decode_modrm only calls read_disp for md in {{00,01,10}}"),
    }
}

/// Turn a decoded memory operand into a real host address. `mem_base` is the
/// guest image buffer's real base host pointer (`buf.as_ptr() as u64`);
/// `end_of_instruction` is the guest-image-relative offset of the byte right
/// after this whole instruction (including any trailing immediate) -- the
/// exact quantity a real CPU's `rip` holds while executing a RIP-relative
/// operand. THIS is the `+ base` the reference prototype (`guest_interp.rs`)
/// originally omitted; see `mem.rs`'s header.
pub fn resolve_addr(mem: MemAddr, mem_base: u64, end_of_instruction: usize) -> u64 {
    match mem {
        MemAddr::Abs(a) => a,
        MemAddr::RipRel(d) => (mem_base as i64 + end_of_instruction as i64 + i64::from(d)) as u64,
    }
}

pub fn read_i32(code: &[u8], p: usize) -> i32 {
    i32::from_le_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]])
}

pub fn read_u32(code: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([code[p], code[p + 1], code[p + 2], code[p + 3]])
}

pub fn read_u64(code: &[u8], p: usize) -> u64 {
    u64::from_le_bytes(code[p..p + 8].try_into().expect("8 bytes"))
}

pub fn read_i8(code: &[u8], p: usize) -> i8 {
    code[p] as i8
}
