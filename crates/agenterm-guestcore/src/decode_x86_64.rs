//! **Layer 1 of 3 (ISA layer) -- x86_64 only, zero knowledge of any guest OS
//! or host OS.** The fetch-decode-execute loop: one `step()` call decodes
//! and executes exactly one guest instruction. Everything reachable here is
//! real semantics of the real x86_64 ISA subset documented in the crate
//! README's opcode table -- there is no custom IR, no bytecode, nothing
//! invented; `step()` is the entire "CPU".
//!
//! This module does not know Linux syscall numbers exist, and does not know
//! Windows exists: when it decodes the guest `syscall` instruction (`0F
//! 05`), it does not read `rax`/`rdi`/etc. itself or interpret them in any
//! way -- it just advances `rip` past the instruction and reports
//! `StepOutcome::Syscall`, leaving the *current register file* for the
//! caller to interpret. That interpretation is `abi_linux_x86_64.rs`'s job
//! (layer 2) -- see that module's header. A future `decode_aarch64.rs`
//! (Phase 2, not built this round) would be a sibling of this file, not a
//! fork of it: same three-layer shape, different ISA-layer module.

use crate::cpu::{Cpu, Flags, RSP};
use crate::decode::{self, Modrm, Rex, Rm};
use crate::fault::GuestFault;
use crate::mem;

pub enum StepOutcome {
    /// One instruction executed; `cpu.rip` already points at the next one.
    Continue,
    /// `syscall` (`0F 05`) was decoded and `cpu.rip` advanced past it;
    /// `cpu.regs[RAX]` holds the syscall number and rdi/rsi/rdx/r10/r8/r9
    /// hold its arguments per the Linux x86_64 ABI. The caller (`lib.rs`'s
    /// run loop) is responsible for dispatching it via `syscall.rs`.
    Syscall,
}

#[derive(Clone, Copy)]
enum Eff {
    Reg(u8),
    Mem(u64), // resolved real host address
}

fn eff_of(rm: Rm, mem_base: u64, end_of_instr: usize) -> Eff {
    match rm {
        Rm::Reg(r) => Eff::Reg(r),
        Rm::Mem(m) => Eff::Mem(decode::resolve_addr(m, mem_base, end_of_instr)),
    }
}

fn read_eff(cpu: &Cpu, eff: Eff, w64: bool) -> u64 {
    match eff {
        Eff::Reg(r) => cpu.get_width(r, w64),
        Eff::Mem(a) => unsafe {
            if w64 {
                mem::read_u64(a)
            } else {
                u64::from(mem::read_u32(a))
            }
        },
    }
}

fn write_eff(cpu: &mut Cpu, eff: Eff, val: u64, w64: bool) {
    match eff {
        Eff::Reg(r) => cpu.set_width(r, val, w64),
        Eff::Mem(a) => unsafe {
            if w64 {
                mem::write_u64(a, val);
            } else {
                mem::write_u32(a, val as u32);
            }
        },
    }
}

fn read_eff8(cpu: &Cpu, eff: Eff) -> u8 {
    match eff {
        Eff::Reg(r) => cpu.get_width(r, true) as u8,
        Eff::Mem(a) => unsafe { mem::read_u8(a) },
    }
}

fn read_eff16(cpu: &Cpu, eff: Eff) -> u16 {
    match eff {
        Eff::Reg(r) => cpu.get_width(r, true) as u16,
        Eff::Mem(a) => unsafe { mem::read_u16(a) },
    }
}

fn width_mask(w64: bool) -> u64 {
    if w64 { u64::MAX } else { 0xFFFF_FFFF }
}

fn sign_bit(w64: bool) -> u64 {
    if w64 { 1u64 << 63 } else { 1u64 << 31 }
}

fn do_add(a: u64, b: u64, w64: bool) -> (u64, Flags) {
    let mask = width_mask(w64);
    let (aa, bb) = (a & mask, b & mask);
    let sum = u128::from(aa) + u128::from(bb);
    let result = (sum as u64) & mask;
    let cf = sum > u128::from(mask);
    let (sa, sb, sr) = (aa & sign_bit(w64) != 0, bb & sign_bit(w64) != 0, result & sign_bit(w64) != 0);
    let of = sa == sb && sr != sa;
    (result, Flags { cf, zf: result == 0, sf: sr, of })
}

fn do_sub(a: u64, b: u64, w64: bool) -> (u64, Flags) {
    let mask = width_mask(w64);
    let (aa, bb) = (a & mask, b & mask);
    let result = aa.wrapping_sub(bb) & mask;
    let cf = aa < bb;
    let (sa, sb, sr) = (aa & sign_bit(w64) != 0, bb & sign_bit(w64) != 0, result & sign_bit(w64) != 0);
    let of = sa != sb && sr != sa;
    (result, Flags { cf, zf: result == 0, sf: sr, of })
}

fn do_logic(result: u64, w64: bool) -> (u64, Flags) {
    let r = result & width_mask(w64);
    (r, Flags { cf: false, of: false, zf: r == 0, sf: r & sign_bit(w64) != 0 })
}

/// `digit` is the standard x86 ALU opcode-extension: 0=add 1=or 4=and 5=sub
/// 6=xor 7=cmp (2=adc, 3=sbb are out of scope -- callers reject those before
/// reaching here).
fn alu_apply(digit: u8, a: u64, b: u64, w64: bool) -> (u64, Flags) {
    match digit {
        0 => do_add(a, b, w64),
        1 => do_logic(a | b, w64),
        4 => do_logic(a & b, w64),
        5 | 7 => do_sub(a, b, w64), // 7 (cmp) computes the same flags, caller discards the result
        6 => do_logic(a ^ b, w64),
        _ => unreachable!("caller guards digit against {{2,3}} (adc/sbb, unimplemented)"),
    }
}

fn cond_holds(cc: u8, f: Flags) -> bool {
    match cc {
        0x0 => f.of,                    // JO
        0x1 => !f.of,                   // JNO
        0x2 => f.cf,                    // JB/JC/JNAE
        0x3 => !f.cf,                   // JAE/JNB/JNC
        0x4 => f.zf,                    // JE/JZ
        0x5 => !f.zf,                   // JNE/JNZ
        0x6 => f.cf || f.zf,            // JBE/JNA
        0x7 => !f.cf && !f.zf,          // JA/JNBE
        0x8 => f.sf,                    // JS
        0x9 => !f.sf,                   // JNS
        0xC => f.sf != f.of,            // JL/JNGE
        0xD => f.sf == f.of,            // JGE/JNL
        0xE => f.zf || (f.sf != f.of),  // JLE/JNG
        0xF => !f.zf && (f.sf == f.of), // JG/JNLE
        _ => false, // JP/JNP (0xA/0xB) -- parity flag not modeled, out of scope
    }
}

pub fn step(cpu: &mut Cpu, code: &[u8], mem_base: u64) -> Result<StepOutcome, GuestFault> {
    let start = cpu.rip;
    if start >= code.len() {
        return Err(GuestFault::RanOffEnd { rip: start });
    }
    let mut p = start;
    let mut rex = Rex::default();
    if (0x40..=0x4F).contains(&code[p]) {
        rex = Rex::parse(code[p]);
        p += 1;
    }
    if p >= code.len() {
        return Err(GuestFault::RanOffEnd { rip: p });
    }
    let op_pos = p;
    let op = code[p];
    p += 1;

    macro_rules! fault_here {
        ($byte:expr) => {
            return Err(GuestFault::UnimplementedOpcode { rip: op_pos, byte: $byte })
        };
    }

    match op {
        // ---- mov r32/r64, imm32/imm64 (B8+r .. BF+r) ----
        0xB8..=0xBF => {
            let reg = (op - 0xB8) | if rex.b { 0x8 } else { 0 };
            if rex.w {
                let imm = decode::read_u64(code, p);
                p += 8;
                cpu.set64(reg, imm);
            } else {
                let imm = decode::read_u32(code, p);
                p += 4;
                cpu.set32(reg, imm);
            }
            cpu.rip = p;
            Ok(StepOutcome::Continue)
        }
        // ---- C7 /0: mov rm, imm32 (sign-extended to 64 under REX.W) ----
        0xC7 => {
            let m = decode::decode_modrm(code, p, rex, &cpu.regs);
            if m.reg != 0 {
                fault_here!(op);
            }
            let imm_pos = p + m.len;
            let imm = decode::read_i32(code, imm_pos);
            let new_rip = imm_pos + 4;
            let val = if rex.w { imm as i64 as u64 } else { imm as u32 as u64 };
            write_eff(cpu, eff_of(m.rm, mem_base, new_rip), val, rex.w);
            cpu.rip = new_rip;
            Ok(StepOutcome::Continue)
        }
        // ---- 89 mov rm,r / 8B mov r,rm ----
        0x89 | 0x8B => {
            let m = decode::decode_modrm(code, p, rex, &cpu.regs);
            let new_rip = p + m.len;
            let eff = eff_of(m.rm, mem_base, new_rip);
            if op == 0x89 {
                let v = cpu.get_width(m.reg, rex.w);
                write_eff(cpu, eff, v, rex.w);
            } else {
                let v = read_eff(cpu, eff, rex.w);
                cpu.set_width(m.reg, v, rex.w);
            }
            cpu.rip = new_rip;
            Ok(StepOutcome::Continue)
        }
        // ---- 8D lea r, m ----
        0x8D => {
            let m = decode::decode_modrm(code, p, rex, &cpu.regs);
            let new_rip = p + m.len;
            let addr = match m.rm {
                Rm::Mem(mem_operand) => decode::resolve_addr(mem_operand, mem_base, new_rip),
                Rm::Reg(_) => fault_here!(op), // `lea reg, reg` is not a valid encoding
            };
            cpu.set_width(m.reg, addr, rex.w);
            cpu.rip = new_rip;
            Ok(StepOutcome::Continue)
        }
        // ---- 0F xx: two-byte opcodes ----
        0x0F => {
            if p >= code.len() {
                return Err(GuestFault::RanOffEnd { rip: p });
            }
            let op2 = code[p];
            p += 1;
            match op2 {
                0x05 => {
                    // syscall
                    cpu.rip = p;
                    Ok(StepOutcome::Syscall)
                }
                0xB6 | 0xB7 | 0xBE | 0xBF => {
                    // movzx/movsx r, rm8/rm16
                    let m = decode::decode_modrm(code, p, rex, &cpu.regs);
                    let new_rip = p + m.len;
                    let eff = eff_of(m.rm, mem_base, new_rip);
                    let val: u64 = match op2 {
                        0xB6 => u64::from(read_eff8(cpu, eff)),
                        0xB7 => u64::from(read_eff16(cpu, eff)),
                        0xBE => read_eff8(cpu, eff) as i8 as i64 as u64,
                        0xBF => read_eff16(cpu, eff) as i16 as i64 as u64,
                        _ => unreachable!(),
                    };
                    cpu.set_width(m.reg, val, rex.w);
                    cpu.rip = new_rip;
                    Ok(StepOutcome::Continue)
                }
                0x80..=0x8F => {
                    // jcc rel32
                    let cc = op2 & 0x0F;
                    let rel = decode::read_i32(code, p);
                    let new_rip = p + 4;
                    cpu.rip = if cond_holds(cc, cpu.flags) { (new_rip as i64 + i64::from(rel)) as usize } else { new_rip };
                    Ok(StepOutcome::Continue)
                }
                other => fault_here!(other),
            }
        }
        // ---- ALU rm,r  (dest = rm) ----
        0x01 | 0x09 | 0x21 | 0x29 | 0x31 | 0x39 => {
            let digit = alu_digit(op);
            step_alu_reg_form(cpu, code, mem_base, rex, p, digit, true);
            Ok(StepOutcome::Continue)
        }
        // ---- ALU r,rm  (dest = reg) ----
        0x03 | 0x0B | 0x23 | 0x2B | 0x33 | 0x3B => {
            let digit = alu_digit(op);
            step_alu_reg_form(cpu, code, mem_base, rex, p, digit, false);
            Ok(StepOutcome::Continue)
        }
        // ---- 81 /digit: ALU rm, imm32 ----
        0x81 => {
            let m = decode::decode_modrm(code, p, rex, &cpu.regs);
            if m.reg == 2 || m.reg == 3 {
                fault_here!(op); // adc/sbb out of scope
            }
            let imm_pos = p + m.len;
            let imm = decode::read_i32(code, imm_pos);
            let new_rip = imm_pos + 4;
            let eff = eff_of(m.rm, mem_base, new_rip);
            let dest_val = read_eff(cpu, eff, rex.w);
            let src_val = if rex.w { imm as i64 as u64 } else { imm as u32 as u64 };
            let (result, flags) = alu_apply(m.reg, dest_val, src_val, rex.w);
            cpu.flags = flags;
            if m.reg != 7 {
                write_eff(cpu, eff, result, rex.w);
            }
            cpu.rip = new_rip;
            Ok(StepOutcome::Continue)
        }
        // ---- 83 /digit: ALU rm, imm8 (sign-extended) ----
        0x83 => {
            let m = decode::decode_modrm(code, p, rex, &cpu.regs);
            if m.reg == 2 || m.reg == 3 {
                fault_here!(op);
            }
            let imm_pos = p + m.len;
            let imm = decode::read_i8(code, imm_pos);
            let new_rip = imm_pos + 1;
            let eff = eff_of(m.rm, mem_base, new_rip);
            let dest_val = read_eff(cpu, eff, rex.w);
            let src_val = imm as i64 as u64;
            let (result, flags) = alu_apply(m.reg, dest_val, src_val, rex.w);
            cpu.flags = flags;
            if m.reg != 7 {
                write_eff(cpu, eff, result, rex.w);
            }
            cpu.rip = new_rip;
            Ok(StepOutcome::Continue)
        }
        // ---- 85 test rm,r ----
        0x85 => {
            let m = decode::decode_modrm(code, p, rex, &cpu.regs);
            let new_rip = p + m.len;
            let eff = eff_of(m.rm, mem_base, new_rip);
            let a = read_eff(cpu, eff, rex.w);
            let b = cpu.get_width(m.reg, rex.w);
            let (_, flags) = do_logic(a & b, rex.w);
            cpu.flags = flags;
            cpu.rip = new_rip;
            Ok(StepOutcome::Continue)
        }
        // ---- push r64 (50+r) ----
        0x50..=0x57 => {
            let reg = (op - 0x50) | if rex.b { 0x8 } else { 0 };
            let val = cpu.get(reg);
            let new_rsp = cpu.get(RSP).wrapping_sub(8);
            unsafe { mem::write_u64(new_rsp, val) };
            cpu.set64(RSP, new_rsp);
            cpu.rip = p;
            Ok(StepOutcome::Continue)
        }
        // ---- pop r64 (58+r) ----
        0x58..=0x5F => {
            let reg = (op - 0x58) | if rex.b { 0x8 } else { 0 };
            let rsp = cpu.get(RSP);
            let val = unsafe { mem::read_u64(rsp) };
            cpu.set64(reg, val);
            cpu.set64(RSP, rsp.wrapping_add(8));
            cpu.rip = p;
            Ok(StepOutcome::Continue)
        }
        // ---- E8 call rel32 ----
        0xE8 => {
            let rel = decode::read_i32(code, p);
            let new_rip = p + 4;
            let ret_addr = new_rip as u64; // a guest-image-relative offset, see interp.rs header note below
            let new_rsp = cpu.get(RSP).wrapping_sub(8);
            unsafe { mem::write_u64(new_rsp, ret_addr) };
            cpu.set64(RSP, new_rsp);
            cpu.rip = (new_rip as i64 + i64::from(rel)) as usize;
            Ok(StepOutcome::Continue)
        }
        // ---- C3 ret ----
        0xC3 => {
            let rsp = cpu.get(RSP);
            let ret_addr = unsafe { mem::read_u64(rsp) };
            cpu.set64(RSP, rsp.wrapping_add(8));
            if ret_addr > code.len() as u64 {
                return Err(GuestFault::StackFault { reason: "ret popped an address outside the guest image (corrupt/underflowed stack?)" });
            }
            cpu.rip = ret_addr as usize;
            Ok(StepOutcome::Continue)
        }
        // ---- E9 jmp rel32 / EB jmp rel8 ----
        0xE9 => {
            let rel = decode::read_i32(code, p);
            let new_rip = p + 4;
            cpu.rip = (new_rip as i64 + i64::from(rel)) as usize;
            Ok(StepOutcome::Continue)
        }
        0xEB => {
            let rel = decode::read_i8(code, p);
            let new_rip = p + 1;
            cpu.rip = (new_rip as i64 + i64::from(rel)) as usize;
            Ok(StepOutcome::Continue)
        }
        // ---- 70-7F jcc rel8 ----
        0x70..=0x7F => {
            let cc = op & 0x0F;
            let rel = decode::read_i8(code, p);
            let new_rip = p + 1;
            cpu.rip = if cond_holds(cc, cpu.flags) { (new_rip as i64 + i64::from(rel)) as usize } else { new_rip };
            Ok(StepOutcome::Continue)
        }
        // ---- 90 nop ----
        0x90 => {
            cpu.rip = p;
            Ok(StepOutcome::Continue)
        }
        other => fault_here!(other),
    }
}

fn alu_digit(op: u8) -> u8 {
    match op {
        0x01 | 0x03 => 0,
        0x09 | 0x0B => 1,
        0x21 | 0x23 => 4,
        0x29 | 0x2B => 5,
        0x31 | 0x33 => 6,
        0x39 | 0x3B => 7,
        _ => unreachable!("alu_digit only called for the six ALU rm/r and r/rm opcodes"),
    }
}

/// Shared body for both ALU register-form directions (`rm,r` and `r,rm`).
/// `rm_is_dest`: `true` for the `01/09/21/29/31/39` (`rm,r`) family, `false`
/// for `03/0B/23/2B/33/3B` (`r,rm`).
fn step_alu_reg_form(cpu: &mut Cpu, code: &[u8], mem_base: u64, rex: Rex, p: usize, digit: u8, rm_is_dest: bool) {
    let m: Modrm = decode::decode_modrm(code, p, rex, &cpu.regs);
    let new_rip = p + m.len;
    let eff = eff_of(m.rm, mem_base, new_rip);
    let rm_val = read_eff(cpu, eff, rex.w);
    let reg_val = cpu.get_width(m.reg, rex.w);
    let (dest_val, src_val) = if rm_is_dest { (rm_val, reg_val) } else { (reg_val, rm_val) };
    let (result, flags) = alu_apply(digit, dest_val, src_val, rex.w);
    cpu.flags = flags;
    if digit != 7 {
        if rm_is_dest {
            write_eff(cpu, eff, result, rex.w);
        } else {
            cpu.set_width(m.reg, result, rex.w);
        }
    }
    cpu.rip = new_rip;
}

// Note on `call`/`ret`'s return-address value: it is pushed/popped as a
// guest-image-RELATIVE offset (the same thing `cpu.rip` always holds), not a
// host pointer -- unlike every value this interpreter hands to a real Win32
// call (which is always a genuine host pointer, see `mem.rs`'s header), a
// return address is only ever consumed by this interpreter's own `ret`
// handling above, so there is no reason to promote it to a host address and
// every reason not to (bounds-checking a corrupt/underflowed stack against
// `code.len()` above is only possible because it stays a plain offset).
// Ordinary `push`/`pop` of a general register is untouched by this and
// still moves the register's raw 64-bit value verbatim, including real host
// pointers a program may have computed via `lea`.
