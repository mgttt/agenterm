//! IR definition — a naive register-machine bytecode (Q3 §2). DATA, not the lowerer.
//! Shared verbatim by `lower.rs` (runtime consumer) and `tools/ir_gen.rs` (author-time
//! producer). Contains only constants: opcode bytes, the vreg count, and env-table
//! slot indices. No logic lives here, so it is NOT part of the measured lowerer (X).
//!
//! Encoding: each instruction = 1 opcode byte + fixed operands.
//!   rd/rs/rb : u8 vreg id (0..=12)      off : i32 (little-endian)
//!   imm64    : i64 (8 bytes LE)          imm8 : u8       L : u8 label id
//!   CALL rd, idx, argc, a0..a(argc-1)   (rd = 0xFF means "discard result")
//!
//! 13 vregs. vr0..vr3 -> callee-saved x86 regs (survive CALL); vr4..vr12 -> caller-saved.
//! r15 is reserved by the lowerer for the env-table pointer (see slots below).

#![allow(dead_code)]

pub const OP_IMM: u8 = 0x01; // rd, imm64
pub const OP_MOV: u8 = 0x02; // rd, rs
pub const OP_ADD: u8 = 0x03; // rd += rs
pub const OP_SUB: u8 = 0x04; // rd -= rs
pub const OP_MUL: u8 = 0x05; // rd *= rs (imul)
pub const OP_AND: u8 = 0x06; // rd &= rs
pub const OP_OR: u8 = 0x07; // rd |= rs
pub const OP_XOR: u8 = 0x08; // rd ^= rs
pub const OP_SHL: u8 = 0x09; // rd <<= imm8
pub const OP_SHR: u8 = 0x0A; // rd >>= imm8 (logical)
pub const OP_LD8: u8 = 0x0B; // rd = zext byte [rb + off]
pub const OP_LD64: u8 = 0x0C; // rd = qword [rb + off]
pub const OP_ST8: u8 = 0x0D; // byte [rb + off] = rs (low 8)
pub const OP_ST64: u8 = 0x0E; // qword [rb + off] = rs
pub const OP_JMP: u8 = 0x0F; // L
pub const OP_JB: u8 = 0x10; // ra, rb, L   (unsigned <)
pub const OP_JAE: u8 = 0x11; // ra, rb, L   (unsigned >=)
pub const OP_JE: u8 = 0x12; // ra, rb, L   (==)
pub const OP_JNE: u8 = 0x13; // ra, rb, L   (!=)
pub const OP_LABEL: u8 = 0x14; // L
pub const OP_CALL: u8 = 0x15; // rd, idx, argc, a0..
pub const OP_HALT: u8 = 0x16; // ud2 (safety terminator; payloads normally exit via CALL)
pub const OP_LDE: u8 = 0x17; // rd, slot -> rd = qword env[slot]  (data slots ENV_K/PATH/HEX)

pub const RD_DISCARD: u8 = 0xFF;

// env-table slot indices (each slot is one machine word).
pub const ENV_K: u8 = 0; //   k pointer (data)          -> passed to adapters
pub const ENV_MEM_ALLOC: u8 = 1; // fn(size)->ptr        (kernel primitive ①)
pub const ENV_READ_FILE: u8 = 2; // fn(k,path,buf,cap)->n
pub const ENV_WRITE: u8 = 3; //   fn(k,buf,len)
pub const ENV_SPAWN: u8 = 4; //   fn(k)->i32
pub const ENV_EXIT: u8 = 5; //    fn(code)->!           (kernel primitive)
pub const ENV_PATH: u8 = 6; //    "input.txt\0" pointer (data)
pub const ENV_HEX: u8 = 7; //     "0123456789abcdef" pointer (data)
pub const ENV_LEN: usize = 8;
