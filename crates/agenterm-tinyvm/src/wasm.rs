//! A minimal interpreter for **WebAssembly 1.0 function-body bytecode**.
//!
//! This runs real `.wasm` opcode bytes — not a re-invented instruction set —
//! for a first subset large enough to execute one real function:
//!
//! - `i32.const` (0x41), `i32.add` (0x6A)
//! - `local.get` (0x20), `local.set` (0x21)
//! - `call` (0x10)
//! - `block` (0x02), `loop` (0x03), `br` (0x0C), `br_if` (0x0D),
//!   `return` (0x0F), `end` (0x0B)
//!
//! It does **not** implement the whole spec: no linear memory, tables, globals,
//! or whole-module (import/export/section) parsing — those come later. A caller
//! registers function bodies (with their param/local/result counts) into a
//! [`Module`] and invokes them. Block types are limited to empty (`0x40`) or a
//! single value type. No JIT/AOT — pure interpretation.
//!
//! Every fault is loud: a malformed body fails to decode, and a run-time trap
//! (stack underflow, out-of-range local/label/func, budget/depth overrun)
//! returns a [`WasmError`] rather than misbehaving silently.

use std::fmt;

/// Max call recursion depth (via `call`).
pub const WASM_MAX_DEPTH: usize = 1_024;
/// Max executed instructions per top-level [`Module::invoke`].
pub const WASM_MAX_STEPS: u64 = 16_000_000;
/// WebAssembly linear-memory page size (64 KiB). Memory starts at one page per
/// invocation and can grow via `memory.grow`.
pub const WASM_PAGE_SIZE: usize = 65_536;
/// Maximum pages `memory.grow` will allocate (the spec's 32-bit maximum).
pub const WASM_MAX_PAGES: usize = 65_536;

/// A tagged WebAssembly value. The operand stack, locals, arguments, and
/// results are all sequences of these so the four numeric types can coexist.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Val {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

/// A decode-time or run-time WebAssembly fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WasmError {
    /// The function body could not be decoded.
    Decode(String),
    /// The program trapped at run time.
    Trap(String),
}

impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(m) => write!(f, "wasm decode error: {m}"),
            Self::Trap(m) => write!(f, "wasm trap: {m}"),
        }
    }
}

impl std::error::Error for WasmError {}

/// A decoded instruction. Branch/call operands keep their WASM indices; block
/// and loop carry the index of their matching `End` so branches resolve in O(1).
///
/// No `Eq` derive: `F32Const` holds an `f32`, which is not `Eq` (float consts
/// are stored raw so decoding a `.wasm` module never needs value comparison).
#[derive(Debug, Clone, PartialEq)]
enum Op {
    I32Const(i32),
    I32Add,
    I32Sub,
    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,
    /// `i32.load` — pop address, push 4 little-endian bytes at `addr + offset`.
    /// The memarg alignment hint is decoded and ignored (a valid MVP choice).
    I32Load { offset: u32 },
    /// `i32.store` — pop value then address; write 4 little-endian bytes at
    /// `addr + offset`. Alignment hint decoded and ignored.
    I32Store { offset: u32 },
    /// Narrow loads (sign/zero extended to i32) and stores (low bytes).
    I32Load8S { offset: u32 },
    I32Load8U { offset: u32 },
    I32Load16S { offset: u32 },
    I32Load16U { offset: u32 },
    I32Store8 { offset: u32 },
    I32Store16 { offset: u32 },
    /// `memory.size` — push the current size in pages.
    MemorySize,
    /// `memory.grow` — pop delta pages, grow, push old size (or -1 on failure).
    MemoryGrow,
    /// `i64.load`/`i64.store` — 8 little-endian bytes at `addr + offset`.
    I64Load { offset: u32 },
    I64Store { offset: u32 },
    /// Narrow i64 loads (sign/zero extended to i64) and stores (low bytes).
    I64Load8S { offset: u32 },
    I64Load8U { offset: u32 },
    I64Load16S { offset: u32 },
    I64Load16U { offset: u32 },
    I64Load32S { offset: u32 },
    I64Load32U { offset: u32 },
    I64Store8 { offset: u32 },
    I64Store16 { offset: u32 },
    I64Store32 { offset: u32 },
    // --- i64 integer family ---
    I64Const(i64),
    I64Eqz,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,
    I64Clz,
    I64Ctz,
    I64Popcnt,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Rotl,
    I64Rotr,
    // --- f64 family (const stored as raw bits) ---
    F64Const(u64),
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,
    F64Abs,
    F64Neg,
    F64Ceil,
    F64Floor,
    F64Trunc,
    F64Nearest,
    F64Sqrt,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Min,
    F64Max,
    F64Copysign,
    F64Load { offset: u32 },
    F64Store { offset: u32 },
    // --- f32 family ---
    F32Const(f32),
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,
    F32Abs,
    F32Neg,
    F32Ceil,
    F32Floor,
    F32Trunc,
    F32Nearest,
    F32Sqrt,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Min,
    F32Max,
    F32Copysign,
    F32Load { offset: u32 },
    F32Store { offset: u32 },
    LocalGet(u32),
    LocalSet(u32),
    Call(u32),
    /// `arity` is the block's result count (0 or 1); `end` indexes its `End`.
    Block {
        arity: u32,
        end: usize,
    },
    /// `arity` is the loop's result count; the back-edge arity is always 0
    /// (MVP loops take no inputs). `end` indexes its `End`.
    Loop {
        arity: u32,
        end: usize,
    },
    Br(u32),
    BrIf(u32),
    /// `br_table` — pop an index; branch to `targets[index]` or `default`.
    BrTable { targets: Vec<u32>, default: u32 },
    /// `if` — pop a condition; run the then-body when nonzero, else the
    /// else-body. `else_pc` indexes the [`Op::Else`] (if any); `end` indexes the
    /// matching `End`.
    If {
        arity: u32,
        else_pc: Option<usize>,
        end: usize,
    },
    /// `else` — end of the then-body; `end` indexes the `if`'s matching `End`.
    Else { end: usize },
    /// `unreachable` — always traps.
    Unreachable,
    /// `nop` — does nothing.
    Nop,
    /// `drop` — discard the top of stack.
    Drop,
    /// `select` — pop c, b, a; push `a` if `c != 0` else `b`.
    Select,
    /// `local.tee` — like `local.set` but leaves the value on the stack.
    LocalTee(u32),
    Return,
    End,
}

fn leb_u32(bytes: &[u8], mut i: usize) -> Result<(u32, usize), WasmError> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = *bytes
            .get(i)
            .ok_or_else(|| WasmError::Decode("truncated unsigned LEB128".into()))?;
        i += 1;
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(WasmError::Decode("unsigned LEB128 too long".into()));
        }
    }
    u32::try_from(result)
        .map(|v| (v, i))
        .map_err(|_| WasmError::Decode("unsigned LEB128 exceeds u32".into()))
}

fn leb_s32(bytes: &[u8], mut i: usize) -> Result<(i32, usize), WasmError> {
    let mut result: i64 = 0;
    let mut shift = 0;
    loop {
        let byte = *bytes
            .get(i)
            .ok_or_else(|| WasmError::Decode("truncated signed LEB128".into()))?;
        i += 1;
        result |= i64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && (byte & 0x40) != 0 {
                result |= -(1i64 << shift);
            }
            break;
        }
        if shift >= 35 {
            return Err(WasmError::Decode("signed LEB128 too long".into()));
        }
    }
    i32::try_from(result)
        .map(|v| (v, i))
        .map_err(|_| WasmError::Decode("signed LEB128 exceeds i32".into()))
}

/// Decode a block type byte: empty (`0x40`) -> arity 0; a single value type
/// (i32/i64/f32/f64) -> arity 1. Anything else is unsupported in this cut.
fn block_arity(bytes: &[u8], i: usize) -> Result<(u32, usize), WasmError> {
    let byte = *bytes
        .get(i)
        .ok_or_else(|| WasmError::Decode("truncated block type".into()))?;
    match byte {
        0x40 => Ok((0, i + 1)),
        0x7C..=0x7F => Ok((1, i + 1)),
        other => Err(WasmError::Decode(format!(
            "unsupported block type 0x{other:02x} (this cut allows empty or one value type)"
        ))),
    }
}

fn decode(body: &[u8]) -> Result<Vec<Op>, WasmError> {
    let mut ops: Vec<Op> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let opcode = body[i];
        i += 1;
        match opcode {
            0x41 => {
                let (v, ni) = leb_s32(body, i)?;
                i = ni;
                ops.push(Op::I32Const(v));
            }
            0x67 => ops.push(Op::I32Clz),
            0x68 => ops.push(Op::I32Ctz),
            0x69 => ops.push(Op::I32Popcnt),
            0x6A => ops.push(Op::I32Add),
            0x6B => ops.push(Op::I32Sub),
            0x6C => ops.push(Op::I32Mul),
            0x6D => ops.push(Op::I32DivS),
            0x6E => ops.push(Op::I32DivU),
            0x6F => ops.push(Op::I32RemS),
            0x70 => ops.push(Op::I32RemU),
            0x71 => ops.push(Op::I32And),
            0x72 => ops.push(Op::I32Or),
            0x73 => ops.push(Op::I32Xor),
            0x74 => ops.push(Op::I32Shl),
            0x75 => ops.push(Op::I32ShrS),
            0x76 => ops.push(Op::I32ShrU),
            0x77 => ops.push(Op::I32Rotl),
            0x78 => ops.push(Op::I32Rotr),
            0x45 => ops.push(Op::I32Eqz),
            0x46 => ops.push(Op::I32Eq),
            0x47 => ops.push(Op::I32Ne),
            0x48 => ops.push(Op::I32LtS),
            0x49 => ops.push(Op::I32LtU),
            0x4A => ops.push(Op::I32GtS),
            0x4B => ops.push(Op::I32GtU),
            0x4C => ops.push(Op::I32LeS),
            0x4D => ops.push(Op::I32LeU),
            0x4E => ops.push(Op::I32GeS),
            0x4F => ops.push(Op::I32GeU),
            0x28 => {
                // memarg = align (LEB u32, ignored) then offset (LEB u32)
                let (_align, n1) = leb_u32(body, i)?;
                let (offset, n2) = leb_u32(body, n1)?;
                i = n2;
                ops.push(Op::I32Load { offset });
            }
            0x36 => {
                let (_align, n1) = leb_u32(body, i)?;
                let (offset, n2) = leb_u32(body, n1)?;
                i = n2;
                ops.push(Op::I32Store { offset });
            }
            0x2C => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I32Load8S { offset });
            }
            0x2D => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I32Load8U { offset });
            }
            0x2E => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I32Load16S { offset });
            }
            0x2F => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I32Load16U { offset });
            }
            0x3A => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I32Store8 { offset });
            }
            0x3B => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I32Store16 { offset });
            }
            0x3F => {
                // memory.size — a single reserved memory-index byte (must be 0)
                let b = *body
                    .get(i)
                    .ok_or_else(|| WasmError::Decode("truncated memory.size".into()))?;
                if b != 0x00 {
                    return Err(WasmError::Decode("memory.size reserved byte must be 0".into()));
                }
                i += 1;
                ops.push(Op::MemorySize);
            }
            0x40 => {
                let b = *body
                    .get(i)
                    .ok_or_else(|| WasmError::Decode("truncated memory.grow".into()))?;
                if b != 0x00 {
                    return Err(WasmError::Decode("memory.grow reserved byte must be 0".into()));
                }
                i += 1;
                ops.push(Op::MemoryGrow);
            }
            0x20 => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::LocalGet(x));
            }
            0x21 => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::LocalSet(x));
            }
            0x10 => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::Call(x));
            }
            0x02 => {
                let (arity, ni) = block_arity(body, i)?;
                i = ni;
                open.push(ops.len());
                ops.push(Op::Block { arity, end: 0 });
            }
            0x03 => {
                let (arity, ni) = block_arity(body, i)?;
                i = ni;
                open.push(ops.len());
                ops.push(Op::Loop { arity, end: 0 });
            }
            0x0C => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::Br(x));
            }
            0x0D => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::BrIf(x));
            }
            0x0E => {
                let (count, n1) = leb_u32(body, i)?;
                let mut cur = n1;
                let mut targets = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let (t, ni) = leb_u32(body, cur)?;
                    cur = ni;
                    targets.push(t);
                }
                let (default, ni) = leb_u32(body, cur)?;
                i = ni;
                ops.push(Op::BrTable { targets, default });
            }
            0x04 => {
                let (arity, ni) = block_arity(body, i)?;
                i = ni;
                open.push(ops.len());
                ops.push(Op::If {
                    arity,
                    else_pc: None,
                    end: 0,
                });
            }
            0x05 => {
                let else_idx = ops.len();
                let open_idx = *open
                    .last()
                    .ok_or_else(|| WasmError::Decode("else without matching if".into()))?;
                match &mut ops[open_idx] {
                    Op::If { else_pc, .. } => *else_pc = Some(else_idx),
                    _ => return Err(WasmError::Decode("else not inside an if".into())),
                }
                ops.push(Op::Else { end: 0 });
            }
            0x00 => ops.push(Op::Unreachable),
            0x01 => ops.push(Op::Nop),
            0x1A => ops.push(Op::Drop),
            0x1B => ops.push(Op::Select),
            0x22 => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::LocalTee(x));
            }
            0x0F => ops.push(Op::Return),
            0x0B => {
                let end_idx = ops.len();
                ops.push(Op::End);
                if let Some(open_idx) = open.pop() {
                    let else_pc = match &mut ops[open_idx] {
                        Op::Block { end, .. } | Op::Loop { end, .. } => {
                            *end = end_idx;
                            None
                        }
                        Op::If { end, else_pc, .. } => {
                            *end = end_idx;
                            *else_pc
                        }
                        _ => unreachable!("open index always points at a block, loop, or if"),
                    };
                    if let Some(e) = else_pc
                        && let Op::Else { end } = &mut ops[e]
                    {
                        *end = end_idx;
                    }
                }
            }
            0x29 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Load { offset });
            }
            0x37 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Store { offset });
            }
            0x30 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Load8S { offset });
            }
            0x31 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Load8U { offset });
            }
            0x32 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Load16S { offset });
            }
            0x33 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Load16U { offset });
            }
            0x34 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Load32S { offset });
            }
            0x35 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Load32U { offset });
            }
            0x3C => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Store8 { offset });
            }
            0x3D => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Store16 { offset });
            }
            0x3E => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::I64Store32 { offset });
            }
            0x42 => {
                let (v, ni) = leb_s64(body, i)?;
                i = ni;
                ops.push(Op::I64Const(v));
            }
            0x50 => ops.push(Op::I64Eqz),
            0x51 => ops.push(Op::I64Eq),
            0x52 => ops.push(Op::I64Ne),
            0x53 => ops.push(Op::I64LtS),
            0x54 => ops.push(Op::I64LtU),
            0x55 => ops.push(Op::I64GtS),
            0x56 => ops.push(Op::I64GtU),
            0x57 => ops.push(Op::I64LeS),
            0x58 => ops.push(Op::I64LeU),
            0x59 => ops.push(Op::I64GeS),
            0x5A => ops.push(Op::I64GeU),
            0x79 => ops.push(Op::I64Clz),
            0x7A => ops.push(Op::I64Ctz),
            0x7B => ops.push(Op::I64Popcnt),
            0x7C => ops.push(Op::I64Add),
            0x7D => ops.push(Op::I64Sub),
            0x7E => ops.push(Op::I64Mul),
            0x7F => ops.push(Op::I64DivS),
            0x80 => ops.push(Op::I64DivU),
            0x81 => ops.push(Op::I64RemS),
            0x82 => ops.push(Op::I64RemU),
            0x83 => ops.push(Op::I64And),
            0x84 => ops.push(Op::I64Or),
            0x85 => ops.push(Op::I64Xor),
            0x86 => ops.push(Op::I64Shl),
            0x87 => ops.push(Op::I64ShrS),
            0x88 => ops.push(Op::I64ShrU),
            0x89 => ops.push(Op::I64Rotl),
            0x8A => ops.push(Op::I64Rotr),
            0x44 => {
                let bytes: [u8; 8] = body
                    .get(i..i + 8)
                    .ok_or_else(|| WasmError::Decode("truncated f64.const immediate".into()))?
                    .try_into()
                    .expect("slice of length 8 converts to [u8; 8]");
                i += 8;
                ops.push(Op::F64Const(u64::from_le_bytes(bytes)));
            }
            0x61 => ops.push(Op::F64Eq),
            0x62 => ops.push(Op::F64Ne),
            0x63 => ops.push(Op::F64Lt),
            0x64 => ops.push(Op::F64Gt),
            0x65 => ops.push(Op::F64Le),
            0x66 => ops.push(Op::F64Ge),
            0x99 => ops.push(Op::F64Abs),
            0x9A => ops.push(Op::F64Neg),
            0x9B => ops.push(Op::F64Ceil),
            0x9C => ops.push(Op::F64Floor),
            0x9D => ops.push(Op::F64Trunc),
            0x9E => ops.push(Op::F64Nearest),
            0x9F => ops.push(Op::F64Sqrt),
            0xA0 => ops.push(Op::F64Add),
            0xA1 => ops.push(Op::F64Sub),
            0xA2 => ops.push(Op::F64Mul),
            0xA3 => ops.push(Op::F64Div),
            0xA4 => ops.push(Op::F64Min),
            0xA5 => ops.push(Op::F64Max),
            0xA6 => ops.push(Op::F64Copysign),
            0x2B => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::F64Load { offset });
            }
            0x39 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::F64Store { offset });
            }
            0x43 => {
                let end = i
                    .checked_add(4)
                    .filter(|&e| e <= body.len())
                    .ok_or_else(|| WasmError::Decode("truncated f32.const literal".into()))?;
                let v = f32::from_le_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
                i = end;
                ops.push(Op::F32Const(v));
            }
            0x5B => ops.push(Op::F32Eq),
            0x5C => ops.push(Op::F32Ne),
            0x5D => ops.push(Op::F32Lt),
            0x5E => ops.push(Op::F32Gt),
            0x5F => ops.push(Op::F32Le),
            0x60 => ops.push(Op::F32Ge),
            0x8B => ops.push(Op::F32Abs),
            0x8C => ops.push(Op::F32Neg),
            0x8D => ops.push(Op::F32Ceil),
            0x8E => ops.push(Op::F32Floor),
            0x8F => ops.push(Op::F32Trunc),
            0x90 => ops.push(Op::F32Nearest),
            0x91 => ops.push(Op::F32Sqrt),
            0x92 => ops.push(Op::F32Add),
            0x93 => ops.push(Op::F32Sub),
            0x94 => ops.push(Op::F32Mul),
            0x95 => ops.push(Op::F32Div),
            0x96 => ops.push(Op::F32Min),
            0x97 => ops.push(Op::F32Max),
            0x98 => ops.push(Op::F32Copysign),
            0x2A => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::F32Load { offset });
            }
            0x38 => {
                let (offset, ni) = memarg(body, i)?;
                i = ni;
                ops.push(Op::F32Store { offset });
            }
            other => {
                return Err(WasmError::Decode(format!(
                    "unsupported opcode 0x{other:02x} in this subset"
                )));
            }
        }
    }
    if !open.is_empty() {
        return Err(WasmError::Decode("unterminated block or loop".into()));
    }
    Ok(ops)
}

/// Skip `n` value-type bytes, bounds-checked.
fn skip_valtypes(p: &[u8], i: usize, n: u32) -> Result<usize, WasmError> {
    let end = i
        .checked_add(n as usize)
        .filter(|&e| e <= p.len())
        .ok_or_else(|| WasmError::Decode("value-type list runs past section".into()))?;
    Ok(end)
}

/// Skip a name (LEB length + that many bytes).
fn skip_name(p: &[u8], i: usize) -> Result<usize, WasmError> {
    let (len, ni) = leb_u32(p, i)?;
    ni.checked_add(len as usize)
        .filter(|&e| e <= p.len())
        .ok_or_else(|| WasmError::Decode("name runs past section".into()))
}

/// Parse the import section, returning `(n_params, n_results)` for each
/// imported **function** in order. Only function imports are supported here.
fn parse_import_section(
    p: &[u8],
    types: &[(usize, usize)],
) -> Result<Vec<(usize, usize)>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    for _ in 0..count {
        i = skip_name(p, i)?; // module name
        i = skip_name(p, i)?; // field name
        let kind = *p
            .get(i)
            .ok_or_else(|| WasmError::Decode("truncated import".into()))?;
        i += 1;
        match kind {
            0x00 => {
                let (tidx, ni) = leb_u32(p, i)?;
                i = ni;
                let t = types.get(tidx as usize).ok_or_else(|| {
                    WasmError::Decode("imported function references missing type".into())
                })?;
                out.push(*t);
            }
            other => {
                return Err(WasmError::Decode(format!(
                    "unsupported import kind 0x{other:02x} (only functions)"
                )));
            }
        }
    }
    Ok(out)
}

/// Parse the type section into `(n_params, n_results)` per function type.
fn parse_type_section(p: &[u8]) -> Result<Vec<(usize, usize)>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let form = *p
            .get(i)
            .ok_or_else(|| WasmError::Decode("truncated func type".into()))?;
        i += 1;
        if form != 0x60 {
            return Err(WasmError::Decode(format!(
                "unsupported type form 0x{form:02x} (only func 0x60)"
            )));
        }
        let (n_params, ni) = leb_u32(p, i)?;
        i = skip_valtypes(p, ni, n_params)?;
        let (n_results, ni) = leb_u32(p, i)?;
        i = skip_valtypes(p, ni, n_results)?;
        out.push((n_params as usize, n_results as usize));
    }
    Ok(out)
}

/// Parse the function section into a type index per function.
fn parse_func_section(p: &[u8]) -> Result<Vec<usize>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let (tidx, ni) = leb_u32(p, i)?;
        i = ni;
        out.push(tidx as usize);
    }
    Ok(out)
}

/// Parse the code section into `(n_locals, expr_bytes)` per function.
fn parse_code_section(p: &[u8]) -> Result<Vec<(usize, Vec<u8>)>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let (body_size, ni) = leb_u32(p, i)?;
        let bstart = ni;
        let bend = bstart
            .checked_add(body_size as usize)
            .filter(|&e| e <= p.len())
            .ok_or_else(|| WasmError::Decode("code entry runs past section".into()))?;
        let body = &p[bstart..bend];

        // locals: vec of (count, valtype)
        let (n_decls, mut j) = leb_u32(body, 0)?;
        let mut n_locals: usize = 0;
        for _ in 0..n_decls {
            let (n, nj) = leb_u32(body, j)?;
            j = nj;
            // one value-type byte follows
            body.get(j)
                .ok_or_else(|| WasmError::Decode("truncated local declaration".into()))?;
            j += 1;
            n_locals = n_locals
                .checked_add(n as usize)
                .ok_or_else(|| WasmError::Decode("local count overflow".into()))?;
        }
        out.push((n_locals, body[j..].to_vec()));
        i = bend;
    }
    Ok(out)
}

/// A registered function: its param/local/result counts and decoded body.
#[derive(Debug, Clone)]
struct Func {
    n_params: usize,
    n_locals: usize,
    arity: usize,
    code: Vec<Op>,
}

/// A structured-control frame on the control stack.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// Operand-stack height when the construct was entered.
    base: usize,
    /// Values preserved when branching to this label.
    branch_arity: usize,
    /// Program counter to resume at when branching to this label.
    cont: usize,
    /// Loops stay on the stack when branched to (back-edge); blocks are exited.
    is_loop: bool,
}

/// A registered host (native) function callable from WASM via an import index.
struct HostFunc {
    n_params: usize,
    n_results: usize,
    #[allow(clippy::type_complexity)]
    f: Box<dyn Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError>>,
}

/// A collection of function bodies that can call one another (and registered
/// host functions) by index.
///
/// The function index space places **host/imported functions first** (indices
/// `0..hosts.len()`), then WASM-defined functions. So when there are no host
/// functions, a defined function's index equals its position — matching the
/// pre-host behaviour.
#[derive(Default)]
pub struct Module {
    hosts: Vec<HostFunc>,
    funcs: Vec<Func>,
}

impl Module {
    /// An empty module.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a function from its WASM body bytes, returning its index.
    ///
    /// `n_params` values are taken from the operand stack on `call` and become
    /// locals `0..n_params`; `n_locals` further zero-initialised locals follow.
    /// `result_arity` is how many values the function leaves on return.
    pub fn add_function(
        &mut self,
        n_params: usize,
        n_locals: usize,
        result_arity: usize,
        body: &[u8],
    ) -> Result<usize, WasmError> {
        let code = decode(body)?;
        let combined = self.hosts.len() + self.funcs.len();
        self.funcs.push(Func {
            n_params,
            n_locals,
            arity: result_arity,
            code,
        });
        Ok(combined)
    }

    /// Register a native host function and return its function index.
    ///
    /// WASM code reaches it via `call <index>`. The closure receives the popped
    /// i32 arguments and a mutable view of the invocation's linear memory (so it
    /// can read what WASM stored or write results back), and returns the result
    /// values. **Register host functions before defined functions** so indices
    /// line up with a module's import-first ordering.
    pub fn add_host_function<F>(&mut self, n_params: usize, n_results: usize, f: F) -> usize
    where
        F: Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError> + 'static,
    {
        let idx = self.hosts.len();
        self.hosts.push(HostFunc {
            n_params,
            n_results,
            f: Box::new(f),
        });
        idx
    }

    /// Load a standard `.wasm` **module** (magic `\0asm` + version 1) with a
    /// minimal set of sections: type (1), function (3), and code (10). Other
    /// sections (custom, import, export, memory, …) are skipped. Suitable for a
    /// `wat2wasm` product of a single function with no imports.
    ///
    /// Function bodies are decoded with the same instruction subset as
    /// [`Module::add_function`]; result/param counts come from the type section
    /// and local counts from each code entry.
    pub fn from_bytes(wasm: &[u8]) -> Result<Module, WasmError> {
        if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
            return Err(WasmError::Decode("not a wasm module (bad magic)".into()));
        }
        if wasm[4..8] != [0x01, 0x00, 0x00, 0x00] {
            return Err(WasmError::Decode(
                "unsupported wasm version (expected 1)".into(),
            ));
        }

        let mut types: Vec<(usize, usize)> = Vec::new();
        let mut imports: Vec<(usize, usize)> = Vec::new();
        let mut func_types: Vec<usize> = Vec::new();
        let mut codes: Vec<(usize, Vec<u8>)> = Vec::new();

        let mut i = 8;
        while i < wasm.len() {
            let id = wasm[i];
            i += 1;
            let (size, ni) = leb_u32(wasm, i)?;
            i = ni;
            let start = i;
            let end = start
                .checked_add(size as usize)
                .filter(|&e| e <= wasm.len())
                .ok_or_else(|| WasmError::Decode("section runs past end of module".into()))?;
            let payload = &wasm[start..end];
            match id {
                1 => types = parse_type_section(payload)?,
                2 => imports = parse_import_section(payload, &types)?,
                3 => func_types = parse_func_section(payload)?,
                10 => codes = parse_code_section(payload)?,
                _ => {} // custom / export / memory / … : skipped
            }
            i = end;
        }

        if func_types.len() != codes.len() {
            return Err(WasmError::Decode(format!(
                "function count {} does not match code count {}",
                func_types.len(),
                codes.len()
            )));
        }

        let mut module = Module::new();
        // Imported functions occupy the low indices; without a bound host
        // implementation they trap loudly if actually called.
        for (np, nr) in &imports {
            module.add_host_function(*np, *nr, |_args, _mem| {
                Err(WasmError::Trap("call to unbound imported function".into()))
            });
        }
        for (fi, &tidx) in func_types.iter().enumerate() {
            let (n_params, n_results) = *types
                .get(tidx)
                .ok_or_else(|| WasmError::Decode(format!("function {fi} references missing type")))?;
            let (n_locals, expr) = &codes[fi];
            let code = decode(expr)?;
            module.funcs.push(Func {
                n_params,
                n_locals: *n_locals,
                arity: n_results,
                code,
            });
        }
        Ok(module)
    }

    /// Invoke function `idx` with `args`, returning its result values.
    ///
    /// A fresh single-page (64 KiB) zero-initialised linear memory is allocated
    /// for the call and shared across every nested `call`; it is discarded when
    /// the top-level invocation returns.
    pub fn invoke(&self, idx: usize, args: &[i32]) -> Result<Vec<i32>, WasmError> {
        let vals: Vec<Val> = args.iter().map(|&a| Val::I32(a)).collect();
        let results = self.invoke_val(idx, &vals)?;
        results
            .into_iter()
            .map(|v| match v {
                Val::I32(n) => Ok(n),
                other => Err(WasmError::Trap(format!(
                    "invoke: expected i32 result, got {other:?}"
                ))),
            })
            .collect()
    }

    /// Invoke function `idx` with typed [`Val`] arguments, returning typed
    /// results. This is the full entry point; [`Module::invoke`] is the i32
    /// convenience wrapper over it.
    pub fn invoke_val(&self, idx: usize, args: &[Val]) -> Result<Vec<Val>, WasmError> {
        let mut steps: u64 = 0;
        let mut mem = vec![0u8; WASM_PAGE_SIZE];
        self.call_any(idx, args, 0, &mut steps, &mut mem)
    }

    /// Number of parameters a function index (host or defined) expects.
    fn param_count(&self, idx: usize) -> Result<usize, WasmError> {
        if idx < self.hosts.len() {
            Ok(self.hosts[idx].n_params)
        } else {
            self.funcs
                .get(idx - self.hosts.len())
                .map(|f| f.n_params)
                .ok_or_else(|| WasmError::Trap(format!("call to unknown function {idx}")))
        }
    }

    /// Dispatch a call by combined index to a host function or a defined one.
    fn call_any(
        &self,
        idx: usize,
        args: &[Val],
        depth: usize,
        steps: &mut u64,
        mem: &mut Vec<u8>,
    ) -> Result<Vec<Val>, WasmError> {
        if idx < self.hosts.len() {
            let host = &self.hosts[idx];
            if args.len() != host.n_params {
                return Err(WasmError::Trap(format!(
                    "host function {idx} expects {} args, got {}",
                    host.n_params,
                    args.len()
                )));
            }
            // Host functions use an i32 ABI: every arg/result must be i32.
            let i32_args: Vec<i32> = args
                .iter()
                .map(|v| match v {
                    Val::I32(n) => Ok(*n),
                    other => Err(WasmError::Trap(format!(
                        "host function {idx} takes i32 args, got {other:?}"
                    ))),
                })
                .collect::<Result<_, _>>()?;
            let res = (host.f)(&i32_args, mem)?;
            if res.len() != host.n_results {
                return Err(WasmError::Trap(format!(
                    "host function {idx} returned {} results, expected {}",
                    res.len(),
                    host.n_results
                )));
            }
            return Ok(res.into_iter().map(Val::I32).collect());
        }
        self.run_defined(idx - self.hosts.len(), args, depth, steps, mem)
    }

    fn run_defined(
        &self,
        def_idx: usize,
        args: &[Val],
        depth: usize,
        steps: &mut u64,
        mem: &mut Vec<u8>,
    ) -> Result<Vec<Val>, WasmError> {
        if depth > WASM_MAX_DEPTH {
            return Err(WasmError::Trap(format!(
                "call depth {WASM_MAX_DEPTH} exceeded"
            )));
        }
        let func = self
            .funcs
            .get(def_idx)
            .ok_or_else(|| WasmError::Trap(format!("call to unknown function {def_idx}")))?;
        if args.len() != func.n_params {
            return Err(WasmError::Trap(format!(
                "function {def_idx} expects {} args, got {}",
                func.n_params,
                args.len()
            )));
        }

        let mut locals: Vec<Val> = args.to_vec();
        // Declared locals default to i32 zero (local value types are not tracked
        // in this cut; pass non-i32 locals as parameters via invoke_val).
        locals.resize(func.n_params + func.n_locals, Val::I32(0));
        let mut stack: Vec<Val> = Vec::new();
        let mut control: Vec<Frame> = vec![Frame {
            base: 0,
            branch_arity: func.arity,
            cont: func.code.len(),
            is_loop: false,
        }];
        let mut pc = 0usize;

        loop {
            *steps += 1;
            if *steps > WASM_MAX_STEPS {
                return Err(WasmError::Trap(format!(
                    "step budget {WASM_MAX_STEPS} exceeded"
                )));
            }
            if pc >= func.code.len() {
                return take_results(&mut stack, func.arity);
            }
            let op = func.code[pc].clone();
            pc += 1;
            match op {
                Op::I32Const(v) => stack.push(Val::I32(v)),
                Op::I32Add => {
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(Val::I32(a.wrapping_add(b)));
                }
                Op::I32Sub => {
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(Val::I32(a.wrapping_sub(b)));
                }
                Op::I32Eqz => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::I32(i32::from(a == 0)));
                }
                Op::I32Eq => bin_i32(&mut stack, |a, b| i32::from(a == b))?,
                Op::I32Ne => bin_i32(&mut stack, |a, b| i32::from(a != b))?,
                Op::I32LtS => bin_i32(&mut stack, |a, b| i32::from(a < b))?,
                Op::I32LtU => bin_i32(&mut stack, |a, b| i32::from((a as u32) < (b as u32)))?,
                Op::I32GtS => bin_i32(&mut stack, |a, b| i32::from(a > b))?,
                Op::I32GtU => bin_i32(&mut stack, |a, b| i32::from((a as u32) > (b as u32)))?,
                Op::I32LeS => bin_i32(&mut stack, |a, b| i32::from(a <= b))?,
                Op::I32LeU => bin_i32(&mut stack, |a, b| i32::from((a as u32) <= (b as u32)))?,
                Op::I32GeS => bin_i32(&mut stack, |a, b| i32::from(a >= b))?,
                Op::I32GeU => bin_i32(&mut stack, |a, b| i32::from((a as u32) >= (b as u32)))?,
                Op::I32Clz => un_i32(&mut stack, |a| (a as u32).leading_zeros() as i32)?,
                Op::I32Ctz => un_i32(&mut stack, |a| (a as u32).trailing_zeros() as i32)?,
                Op::I32Popcnt => un_i32(&mut stack, |a| (a as u32).count_ones() as i32)?,
                Op::I32Mul => bin_i32(&mut stack, |a, b| a.wrapping_mul(b))?,
                Op::I32DivS => bin_i32_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i32.div_s by zero".into()))
                    } else {
                        a.checked_div(b)
                            .ok_or_else(|| WasmError::Trap("i32.div_s overflow".into()))
                    }
                })?,
                Op::I32DivU => bin_i32_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i32.div_u by zero".into()))
                    } else {
                        Ok(((a as u32) / (b as u32)) as i32)
                    }
                })?,
                Op::I32RemS => bin_i32_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i32.rem_s by zero".into()))
                    } else {
                        Ok(a.wrapping_rem(b))
                    }
                })?,
                Op::I32RemU => bin_i32_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i32.rem_u by zero".into()))
                    } else {
                        Ok(((a as u32) % (b as u32)) as i32)
                    }
                })?,
                Op::I32And => bin_i32(&mut stack, |a, b| a & b)?,
                Op::I32Or => bin_i32(&mut stack, |a, b| a | b)?,
                Op::I32Xor => bin_i32(&mut stack, |a, b| a ^ b)?,
                Op::I32Shl => bin_i32(&mut stack, |a, b| ((a as u32) << ((b as u32) & 31)) as i32)?,
                Op::I32ShrS => bin_i32(&mut stack, |a, b| a >> ((b as u32) & 31))?,
                Op::I32ShrU => bin_i32(&mut stack, |a, b| ((a as u32) >> ((b as u32) & 31)) as i32)?,
                Op::I32Rotl => {
                    bin_i32(&mut stack, |a, b| (a as u32).rotate_left((b as u32) & 31) as i32)?
                }
                Op::I32Rotr => {
                    bin_i32(&mut stack, |a, b| (a as u32).rotate_right((b as u32) & 31) as i32)?
                }
                Op::I32Load { offset } => {
                    let addr = pop(&mut stack)?;
                    stack.push(Val::I32(mem_read_i32(mem, addr, offset)?));
                }
                Op::I32Store { offset } => {
                    let value = pop(&mut stack)?;
                    let addr = pop(&mut stack)?;
                    mem_write_i32(mem, addr, offset, value)?;
                }
                Op::I32Load8S { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 1)?;
                    stack.push(Val::I32(mem[ea] as i8 as i32));
                }
                Op::I32Load8U { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 1)?;
                    stack.push(Val::I32(mem[ea] as i32));
                }
                Op::I32Load16S { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 2)?;
                    stack.push(Val::I32(i16::from_le_bytes([mem[ea], mem[ea + 1]]) as i32));
                }
                Op::I32Load16U { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 2)?;
                    stack.push(Val::I32(u16::from_le_bytes([mem[ea], mem[ea + 1]]) as i32));
                }
                Op::I32Store8 { offset } => {
                    let value = pop(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 1)?;
                    mem[ea] = value as u8;
                }
                Op::I32Store16 { offset } => {
                    let value = pop(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 2)?;
                    mem[ea..ea + 2].copy_from_slice(&(value as u16).to_le_bytes());
                }
                Op::F32Const(v) => stack.push(Val::F32(v)),
                Op::F32Eq => bin_f32_cmp(&mut stack, |a, b| a == b)?,
                Op::F32Ne => bin_f32_cmp(&mut stack, |a, b| a != b)?,
                Op::F32Lt => bin_f32_cmp(&mut stack, |a, b| a < b)?,
                Op::F32Gt => bin_f32_cmp(&mut stack, |a, b| a > b)?,
                Op::F32Le => bin_f32_cmp(&mut stack, |a, b| a <= b)?,
                Op::F32Ge => bin_f32_cmp(&mut stack, |a, b| a >= b)?,
                Op::F32Abs => un_f32(&mut stack, f32::abs)?,
                Op::F32Neg => un_f32(&mut stack, |a| -a)?,
                Op::F32Ceil => un_f32(&mut stack, f32::ceil)?,
                Op::F32Floor => un_f32(&mut stack, f32::floor)?,
                Op::F32Trunc => un_f32(&mut stack, f32::trunc)?,
                Op::F32Nearest => un_f32(&mut stack, f32::round_ties_even)?,
                Op::F32Sqrt => un_f32(&mut stack, f32::sqrt)?,
                Op::F32Add => bin_f32(&mut stack, |a, b| a + b)?,
                Op::F32Sub => bin_f32(&mut stack, |a, b| a - b)?,
                Op::F32Mul => bin_f32(&mut stack, |a, b| a * b)?,
                Op::F32Div => bin_f32(&mut stack, |a, b| a / b)?,
                Op::F32Min => bin_f32(&mut stack, wasm_min_f32)?,
                Op::F32Max => bin_f32(&mut stack, wasm_max_f32)?,
                Op::F32Copysign => bin_f32(&mut stack, f32::copysign)?,
                Op::F32Load { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    let bytes: [u8; 4] = mem[ea..ea + 4].try_into().expect("4 bytes");
                    stack.push(Val::F32(f32::from_le_bytes(bytes)));
                }
                Op::F32Store { offset } => {
                    let value = pop_f32(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    mem[ea..ea + 4].copy_from_slice(&value.to_le_bytes());
                }
                Op::F64Const(bits) => stack.push(Val::F64(f64::from_bits(bits))),
                Op::F64Eq => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::I32((a == b) as i32));
                }
                Op::F64Ne => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::I32((a != b) as i32));
                }
                Op::F64Lt => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::I32((a < b) as i32));
                }
                Op::F64Gt => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::I32((a > b) as i32));
                }
                Op::F64Le => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::I32((a <= b) as i32));
                }
                Op::F64Ge => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::I32((a >= b) as i32));
                }
                Op::F64Abs => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a.abs()));
                }
                Op::F64Neg => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(-a));
                }
                Op::F64Ceil => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a.ceil()));
                }
                Op::F64Floor => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a.floor()));
                }
                Op::F64Trunc => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a.trunc()));
                }
                Op::F64Nearest => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a.round_ties_even()));
                }
                Op::F64Sqrt => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a.sqrt()));
                }
                Op::F64Add => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a + b));
                }
                Op::F64Sub => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a - b));
                }
                Op::F64Mul => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a * b));
                }
                Op::F64Div => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a / b));
                }
                Op::F64Min => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(wasm_min_f64(a, b)));
                }
                Op::F64Max => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(wasm_max_f64(a, b)));
                }
                Op::F64Copysign => {
                    let b = pop_f64(&mut stack)?;
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(a.copysign(b)));
                }
                Op::F64Load { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 8)?;
                    let bytes: [u8; 8] = mem[ea..ea + 8].try_into().expect("8 bytes");
                    stack.push(Val::F64(f64::from_le_bytes(bytes)));
                }
                Op::F64Store { offset } => {
                    let value = pop_f64(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 8)?;
                    mem[ea..ea + 8].copy_from_slice(&value.to_le_bytes());
                }
                Op::MemorySize => stack.push(Val::I32((mem.len() / WASM_PAGE_SIZE) as i32)),
                Op::MemoryGrow => {
                    let delta = pop(&mut stack)? as u32 as usize;
                    let old_pages = mem.len() / WASM_PAGE_SIZE;
                    let new_pages = old_pages.saturating_add(delta);
                    if new_pages > WASM_MAX_PAGES {
                        stack.push(Val::I32(-1));
                    } else {
                        mem.resize(new_pages * WASM_PAGE_SIZE, 0);
                        stack.push(Val::I32(old_pages as i32));
                    }
                }
                Op::I64Load { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 8)?;
                    let bytes: [u8; 8] = mem[ea..ea + 8].try_into().expect("8 bytes");
                    stack.push(Val::I64(i64::from_le_bytes(bytes)));
                }
                Op::I64Store { offset } => {
                    let value = pop_i64(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 8)?;
                    mem[ea..ea + 8].copy_from_slice(&value.to_le_bytes());
                }
                Op::I64Load8S { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 1)?;
                    stack.push(Val::I64(mem[ea] as i8 as i64));
                }
                Op::I64Load8U { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 1)?;
                    stack.push(Val::I64(mem[ea] as u64 as i64));
                }
                Op::I64Load16S { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 2)?;
                    stack.push(Val::I64(i16::from_le_bytes([mem[ea], mem[ea + 1]]) as i64));
                }
                Op::I64Load16U { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 2)?;
                    stack.push(Val::I64(u16::from_le_bytes([mem[ea], mem[ea + 1]]) as u64 as i64));
                }
                Op::I64Load32S { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    let bytes: [u8; 4] = mem[ea..ea + 4].try_into().expect("4 bytes");
                    stack.push(Val::I64(i32::from_le_bytes(bytes) as i64));
                }
                Op::I64Load32U { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    let bytes: [u8; 4] = mem[ea..ea + 4].try_into().expect("4 bytes");
                    stack.push(Val::I64(u32::from_le_bytes(bytes) as u64 as i64));
                }
                Op::I64Store8 { offset } => {
                    let value = pop_i64(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 1)?;
                    mem[ea] = value as u8;
                }
                Op::I64Store16 { offset } => {
                    let value = pop_i64(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 2)?;
                    mem[ea..ea + 2].copy_from_slice(&(value as u16).to_le_bytes());
                }
                Op::I64Store32 { offset } => {
                    let value = pop_i64(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    mem[ea..ea + 4].copy_from_slice(&(value as u32).to_le_bytes());
                }
                Op::I64Const(v) => stack.push(Val::I64(v)),
                Op::I64Eqz => {
                    let a = pop_i64(&mut stack)?;
                    stack.push(Val::I32(i32::from(a == 0)));
                }
                Op::I64Eq => cmp_i64(&mut stack, |a, b| a == b)?,
                Op::I64Ne => cmp_i64(&mut stack, |a, b| a != b)?,
                Op::I64LtS => cmp_i64(&mut stack, |a, b| a < b)?,
                Op::I64LtU => cmp_i64(&mut stack, |a, b| (a as u64) < (b as u64))?,
                Op::I64GtS => cmp_i64(&mut stack, |a, b| a > b)?,
                Op::I64GtU => cmp_i64(&mut stack, |a, b| (a as u64) > (b as u64))?,
                Op::I64LeS => cmp_i64(&mut stack, |a, b| a <= b)?,
                Op::I64LeU => cmp_i64(&mut stack, |a, b| (a as u64) <= (b as u64))?,
                Op::I64GeS => cmp_i64(&mut stack, |a, b| a >= b)?,
                Op::I64GeU => cmp_i64(&mut stack, |a, b| (a as u64) >= (b as u64))?,
                Op::I64Clz => un_i64(&mut stack, |a| (a as u64).leading_zeros() as i64)?,
                Op::I64Ctz => un_i64(&mut stack, |a| (a as u64).trailing_zeros() as i64)?,
                Op::I64Popcnt => un_i64(&mut stack, |a| (a as u64).count_ones() as i64)?,
                Op::I64Add => bin_i64(&mut stack, |a, b| a.wrapping_add(b))?,
                Op::I64Sub => bin_i64(&mut stack, |a, b| a.wrapping_sub(b))?,
                Op::I64Mul => bin_i64(&mut stack, |a, b| a.wrapping_mul(b))?,
                Op::I64DivS => bin_i64_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i64.div_s by zero".into()))
                    } else {
                        a.checked_div(b)
                            .ok_or_else(|| WasmError::Trap("i64.div_s overflow".into()))
                    }
                })?,
                Op::I64DivU => bin_i64_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i64.div_u by zero".into()))
                    } else {
                        Ok(((a as u64) / (b as u64)) as i64)
                    }
                })?,
                Op::I64RemS => bin_i64_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i64.rem_s by zero".into()))
                    } else {
                        Ok(a.wrapping_rem(b))
                    }
                })?,
                Op::I64RemU => bin_i64_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i64.rem_u by zero".into()))
                    } else {
                        Ok(((a as u64) % (b as u64)) as i64)
                    }
                })?,
                Op::I64And => bin_i64(&mut stack, |a, b| a & b)?,
                Op::I64Or => bin_i64(&mut stack, |a, b| a | b)?,
                Op::I64Xor => bin_i64(&mut stack, |a, b| a ^ b)?,
                Op::I64Shl => bin_i64(&mut stack, |a, b| ((a as u64) << ((b as u64) & 63)) as i64)?,
                Op::I64ShrS => bin_i64(&mut stack, |a, b| a >> ((b as u64) & 63))?,
                Op::I64ShrU => bin_i64(&mut stack, |a, b| ((a as u64) >> ((b as u64) & 63)) as i64)?,
                Op::I64Rotl => {
                    bin_i64(&mut stack, |a, b| {
                        (a as u64).rotate_left((b as u64 & 63) as u32) as i64
                    })?
                }
                Op::I64Rotr => {
                    bin_i64(&mut stack, |a, b| {
                        (a as u64).rotate_right((b as u64 & 63) as u32) as i64
                    })?
                }
                Op::LocalGet(l) => {
                    let v = *locals
                        .get(l as usize)
                        .ok_or_else(|| WasmError::Trap(format!("local.get {l} out of range")))?;
                    stack.push(v);
                }
                Op::LocalSet(l) => {
                    let v = pop_val(&mut stack)?;
                    let cell = locals
                        .get_mut(l as usize)
                        .ok_or_else(|| WasmError::Trap(format!("local.set {l} out of range")))?;
                    *cell = v;
                }
                Op::Call(f) => {
                    let combined = f as usize;
                    let n = self.param_count(combined)?;
                    if stack.len() < n {
                        return Err(WasmError::Trap(format!(
                            "call {f}: stack has {} values, needs {n}",
                            stack.len()
                        )));
                    }
                    let cargs = stack.split_off(stack.len() - n);
                    let res = self.call_any(combined, &cargs, depth + 1, steps, mem)?;
                    stack.extend(res);
                }
                Op::Block { arity, end } => control.push(Frame {
                    base: stack.len(),
                    branch_arity: arity as usize,
                    cont: end + 1,
                    is_loop: false,
                }),
                Op::Loop { .. } => control.push(Frame {
                    base: stack.len(),
                    branch_arity: 0, // MVP loop back-edge takes no operands
                    cont: pc,        // branch re-enters at the loop body start
                    is_loop: true,
                }),
                Op::Br(l) => {
                    pc = do_branch(&mut stack, &mut control, l)?;
                    if control.is_empty() {
                        return take_results(&mut stack, func.arity);
                    }
                }
                Op::BrIf(l) => {
                    let cond = pop(&mut stack)?;
                    if cond != 0 {
                        pc = do_branch(&mut stack, &mut control, l)?;
                        if control.is_empty() {
                            return take_results(&mut stack, func.arity);
                        }
                    }
                }
                Op::BrTable { targets, default } => {
                    let idx = pop(&mut stack)? as u32 as usize;
                    let label = targets.get(idx).copied().unwrap_or(default);
                    pc = do_branch(&mut stack, &mut control, label)?;
                    if control.is_empty() {
                        return take_results(&mut stack, func.arity);
                    }
                }
                Op::If {
                    arity,
                    else_pc,
                    end,
                } => {
                    let cond = pop(&mut stack)?;
                    control.push(Frame {
                        base: stack.len(),
                        branch_arity: arity as usize,
                        cont: end + 1,
                        is_loop: false,
                    });
                    if cond == 0 {
                        // Skip the then-body: jump to the else-body if present,
                        // otherwise to the `End` (which pops this frame).
                        pc = match else_pc {
                            Some(e) => e + 1,
                            None => end,
                        };
                    }
                }
                Op::Else { end } => {
                    // Reached only by falling through the then-body; skip the
                    // else-body by jumping to the matching `End`.
                    pc = end;
                }
                Op::Unreachable => {
                    return Err(WasmError::Trap("unreachable executed".into()));
                }
                Op::Nop => {}
                Op::Drop => {
                    pop_val(&mut stack)?;
                }
                Op::Select => {
                    let c = pop(&mut stack)?;
                    let b = pop_val(&mut stack)?;
                    let a = pop_val(&mut stack)?;
                    stack.push(if c != 0 { a } else { b });
                }
                Op::LocalTee(l) => {
                    let v = *stack
                        .last()
                        .ok_or_else(|| WasmError::Trap("local.tee on empty stack".into()))?;
                    let cell = locals
                        .get_mut(l as usize)
                        .ok_or_else(|| WasmError::Trap(format!("local.tee {l} out of range")))?;
                    *cell = v;
                }
                Op::Return => return take_results(&mut stack, func.arity),
                Op::End => {
                    control.pop();
                    if control.is_empty() {
                        return take_results(&mut stack, func.arity);
                    }
                }
            }
        }
    }
}

/// Decode a memarg (align LEB, ignored; then offset LEB) and return the offset.
fn memarg(body: &[u8], i: usize) -> Result<(u32, usize), WasmError> {
    let (_align, n1) = leb_u32(body, i)?;
    let (offset, n2) = leb_u32(body, n1)?;
    Ok((offset, n2))
}

/// Effective address `addr as u32 + offset`, bounds-checked for a `width`-byte
/// access. Alignment is not checked (MVP). Out of range is a loud trap.
fn mem_ea(mem_len: usize, addr: i32, offset: u32, width: usize) -> Result<usize, WasmError> {
    let ea = (addr as u32 as usize)
        .checked_add(offset as usize)
        .ok_or_else(|| WasmError::Trap("memory address overflow".into()))?;
    let end = ea
        .checked_add(width)
        .ok_or_else(|| WasmError::Trap("memory access overflow".into()))?;
    if end > mem_len {
        return Err(WasmError::Trap(format!(
            "memory access [{ea}, {end}) out of bounds for {mem_len}-byte memory"
        )));
    }
    Ok(ea)
}

fn mem_read_i32(mem: &[u8], addr: i32, offset: u32) -> Result<i32, WasmError> {
    let ea = mem_ea(mem.len(), addr, offset, 4)?;
    let bytes: [u8; 4] = mem[ea..ea + 4]
        .try_into()
        .expect("range is exactly 4 bytes");
    Ok(i32::from_le_bytes(bytes))
}

fn mem_write_i32(mem: &mut [u8], addr: i32, offset: u32, value: i32) -> Result<(), WasmError> {
    let ea = mem_ea(mem.len(), addr, offset, 4)?;
    mem[ea..ea + 4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Pop `b` then `a` and push `f(a, b)` — the shape of every binary f32 op.
fn bin_f32(stack: &mut Vec<Val>, f: impl FnOnce(f32, f32) -> f32) -> Result<(), WasmError> {
    let b = pop_f32(stack)?;
    let a = pop_f32(stack)?;
    stack.push(Val::F32(f(a, b)));
    Ok(())
}

/// Pop `b` then `a`, push `1`/`0` — the shape of every f32 comparison.
fn bin_f32_cmp(stack: &mut Vec<Val>, f: impl FnOnce(f32, f32) -> bool) -> Result<(), WasmError> {
    let b = pop_f32(stack)?;
    let a = pop_f32(stack)?;
    stack.push(Val::I32(i32::from(f(a, b))));
    Ok(())
}

/// Pop `a` and push `f(a)` — the shape of every unary f32 op.
fn un_f32(stack: &mut Vec<Val>, f: impl FnOnce(f32) -> f32) -> Result<(), WasmError> {
    let a = pop_f32(stack)?;
    stack.push(Val::F32(f(a)));
    Ok(())
}

/// WASM `f32.min`: NaN propagates; `min(-0.0, +0.0)` is `-0.0`.
fn wasm_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// WASM `f32.max`: NaN propagates; `max(-0.0, +0.0)` is `+0.0`.
fn wasm_max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// WASM `f64.min`: either NaN yields NaN; `min(-0.0, +0.0)` is `-0.0`.
fn wasm_min_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// WASM `f64.max`: either NaN yields NaN; `max(-0.0, +0.0)` is `+0.0`.
fn wasm_max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// Decode a signed LEB128 up to 64 bits (like [`leb_s32`] but wider).
fn leb_s64(bytes: &[u8], mut i: usize) -> Result<(i64, usize), WasmError> {
    let mut result: i64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *bytes
            .get(i)
            .ok_or_else(|| WasmError::Decode("truncated signed LEB128".into()))?;
        i += 1;
        result |= i64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && (byte & 0x40) != 0 {
                result |= (-1i64) << shift;
            }
            break;
        }
        if shift >= 70 {
            return Err(WasmError::Decode("signed LEB128 too long".into()));
        }
    }
    Ok((result, i))
}

/// Pop `b` then `a` and push `f(a, b)` — the shape of every binary i64 op.
fn bin_i64(stack: &mut Vec<Val>, f: impl FnOnce(i64, i64) -> i64) -> Result<(), WasmError> {
    let b = pop_i64(stack)?;
    let a = pop_i64(stack)?;
    stack.push(Val::I64(f(a, b)));
    Ok(())
}

/// Like [`bin_i64`] but the operation may trap (e.g. divide by zero).
fn bin_i64_try(
    stack: &mut Vec<Val>,
    f: impl FnOnce(i64, i64) -> Result<i64, WasmError>,
) -> Result<(), WasmError> {
    let b = pop_i64(stack)?;
    let a = pop_i64(stack)?;
    let r = f(a, b)?;
    stack.push(Val::I64(r));
    Ok(())
}

/// Pop `b` then `a` and push an i32 boolean — the shape of every i64 comparison.
fn cmp_i64(stack: &mut Vec<Val>, f: impl FnOnce(i64, i64) -> bool) -> Result<(), WasmError> {
    let b = pop_i64(stack)?;
    let a = pop_i64(stack)?;
    stack.push(Val::I32(i32::from(f(a, b))));
    Ok(())
}

/// Pop `a` and push `f(a)` — the shape of every unary i64 op.
fn un_i64(stack: &mut Vec<Val>, f: impl FnOnce(i64) -> i64) -> Result<(), WasmError> {
    let a = pop_i64(stack)?;
    stack.push(Val::I64(f(a)));
    Ok(())
}

/// Pop `b` then `a` and push `f(a, b)` — the shape of every binary i32 op.
fn bin_i32(stack: &mut Vec<Val>, f: impl FnOnce(i32, i32) -> i32) -> Result<(), WasmError> {
    let b = pop(stack)?;
    let a = pop(stack)?;
    stack.push(Val::I32(f(a, b)));
    Ok(())
}

/// Like [`bin_i32`] but the operation may trap (e.g. divide by zero).
fn bin_i32_try(
    stack: &mut Vec<Val>,
    f: impl FnOnce(i32, i32) -> Result<i32, WasmError>,
) -> Result<(), WasmError> {
    let b = pop(stack)?;
    let a = pop(stack)?;
    let r = f(a, b)?;
    stack.push(Val::I32(r));
    Ok(())
}

/// Pop `a` and push `f(a)` — the shape of every unary i32 op.
fn un_i32(stack: &mut Vec<Val>, f: impl FnOnce(i32) -> i32) -> Result<(), WasmError> {
    let a = pop(stack)?;
    stack.push(Val::I32(f(a)));
    Ok(())
}

/// Pop any value.
fn pop_val(stack: &mut Vec<Val>) -> Result<Val, WasmError> {
    stack
        .pop()
        .ok_or_else(|| WasmError::Trap("operand stack underflow".into()))
}

/// Pop a value expected to be an `i32`.
fn pop(stack: &mut Vec<Val>) -> Result<i32, WasmError> {
    match pop_val(stack)? {
        Val::I32(v) => Ok(v),
        other => Err(WasmError::Trap(format!("expected i32 on stack, got {other:?}"))),
    }
}

/// Pop a value expected to be an `i64`.
#[allow(dead_code)]
fn pop_i64(stack: &mut Vec<Val>) -> Result<i64, WasmError> {
    match pop_val(stack)? {
        Val::I64(v) => Ok(v),
        other => Err(WasmError::Trap(format!("expected i64 on stack, got {other:?}"))),
    }
}

/// Pop a value expected to be an `f32`.
#[allow(dead_code)]
fn pop_f32(stack: &mut Vec<Val>) -> Result<f32, WasmError> {
    match pop_val(stack)? {
        Val::F32(v) => Ok(v),
        other => Err(WasmError::Trap(format!("expected f32 on stack, got {other:?}"))),
    }
}

/// Pop a value expected to be an `f64`.
#[allow(dead_code)]
fn pop_f64(stack: &mut Vec<Val>) -> Result<f64, WasmError> {
    match pop_val(stack)? {
        Val::F64(v) => Ok(v),
        other => Err(WasmError::Trap(format!("expected f64 on stack, got {other:?}"))),
    }
}

fn take_results(stack: &mut Vec<Val>, arity: usize) -> Result<Vec<Val>, WasmError> {
    if stack.len() < arity {
        return Err(WasmError::Trap(format!(
            "result arity {arity} exceeds stack height {}",
            stack.len()
        )));
    }
    Ok(stack.split_off(stack.len() - arity))
}

/// Branch to label depth `l`: preserve the label's `branch_arity` top values,
/// unwind the operand stack to the label's base, unwind the control stack, and
/// return the resume program counter. For a loop the label stays (back-edge);
/// for a block it is exited.
fn do_branch(stack: &mut Vec<Val>, control: &mut Vec<Frame>, l: u32) -> Result<usize, WasmError> {
    let l = l as usize;
    let idx = control
        .len()
        .checked_sub(1 + l)
        .ok_or_else(|| WasmError::Trap(format!("branch label {l} out of range")))?;
    let frame = control[idx];
    if stack.len() < frame.base + frame.branch_arity {
        return Err(WasmError::Trap("branch operand stack underflow".into()));
    }
    let keep = stack.split_off(stack.len() - frame.branch_arity);
    stack.truncate(frame.base);
    stack.extend(keep);
    let new_len = if frame.is_loop { idx + 1 } else { idx };
    control.truncate(new_len);
    Ok(frame.cont)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Acceptance 1: a real function body — (i32.const 40)(i32.const 2) i32.add.
    #[test]
    fn acceptance_1_const_add_returns_42() {
        // 41 28  i32.const 40 | 41 02  i32.const 2 | 6A i32.add | 0B end
        let body = [0x41, 0x28, 0x41, 0x02, 0x6A, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![42]);
    }

    // Acceptance 2: local + loop + br_if — sum n down to 0 (1..=n).
    #[test]
    fn acceptance_2_loop_sums_one_through_n() {
        // local0 = n (param), local1 = acc (declared local, starts 0)
        // loop:
        //   local.get1 local.get0 i32.add local.set1     ; acc += n
        //   local.get0 i32.const -1 i32.add local.set0    ; n  -= 1
        //   local.get0 br_if 0                            ; if n != 0, loop
        // end
        // local.get1                                       ; result = acc
        let body = [
            0x03, 0x40, // loop (empty)
            0x20, 0x01, // local.get 1
            0x20, 0x00, // local.get 0
            0x6A, // i32.add
            0x21, 0x01, // local.set 1
            0x20, 0x00, // local.get 0
            0x41, 0x7F, // i32.const -1
            0x6A, // i32.add
            0x21, 0x00, // local.set 0
            0x20, 0x00, // local.get 0
            0x0D, 0x00, // br_if 0
            0x0B, // end (loop)
            0x20, 0x01, // local.get 1
            0x0B, // end (func)
        ];
        // A do-while loop (decrement then test), valid for n >= 1.
        let mut m = Module::new();
        let f = m.add_function(1, 1, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[5]).unwrap(), vec![15]); // 5+4+3+2+1
        assert_eq!(m.invoke(f, &[10]).unwrap(), vec![55]); // 1..=10
        assert_eq!(m.invoke(f, &[1]).unwrap(), vec![1]);
    }

    // Acceptance (tinyvm.4): a proper while loop (test at the top) that is
    // correct for n == 0 as well as n > 0, using i32.eqz + br_if to exit and
    // i32.sub to decrement.
    #[test]
    fn while_sum_is_correct_including_zero() {
        // local0 = n, local1 = acc
        // block
        //   loop
        //     local.get0 i32.eqz br_if 1        ; if n==0, exit block
        //     local.get1 local.get0 i32.add local.set1   ; acc += n
        //     local.get0 i32.const 1 i32.sub local.set0  ; n -= 1
        //     br 0                              ; continue loop
        //   end
        // end
        // local.get1                            ; result acc
        let body = [
            0x02, 0x40, // block
            0x03, 0x40, // loop
            0x20, 0x00, // local.get 0
            0x45, // i32.eqz
            0x0D, 0x01, // br_if 1  (exit block when n==0)
            0x20, 0x01, // local.get 1
            0x20, 0x00, // local.get 0
            0x6A, // i32.add
            0x21, 0x01, // local.set 1
            0x20, 0x00, // local.get 0
            0x41, 0x01, // i32.const 1
            0x6B, // i32.sub
            0x21, 0x00, // local.set 0
            0x0C, 0x00, // br 0  (loop)
            0x0B, // end (loop)
            0x0B, // end (block)
            0x20, 0x01, // local.get 1
            0x0B, // end (func)
        ];
        let mut m = Module::new();
        let f = m.add_function(1, 1, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[0]).unwrap(), vec![0]);
        assert_eq!(m.invoke(f, &[5]).unwrap(), vec![15]);
        assert_eq!(m.invoke(f, &[10]).unwrap(), vec![55]);
    }

    #[test]
    fn i32_sub_and_eqz_basics() {
        let mut m = Module::new();
        // i32.const 10 ; i32.const 3 ; i32.sub -> 7
        let sub = m
            .add_function(0, 0, 1, &[0x41, 0x0A, 0x41, 0x03, 0x6B, 0x0B])
            .unwrap();
        assert_eq!(m.invoke(sub, &[]).unwrap(), vec![7]);
        // i32.const 0 ; i32.eqz -> 1 ; and i32.const 9 ; i32.eqz -> 0
        let eqz0 = m.add_function(0, 0, 1, &[0x41, 0x00, 0x45, 0x0B]).unwrap();
        let eqz9 = m.add_function(0, 0, 1, &[0x41, 0x09, 0x45, 0x0B]).unwrap();
        assert_eq!(m.invoke(eqz0, &[]).unwrap(), vec![1]);
        assert_eq!(m.invoke(eqz9, &[]).unwrap(), vec![0]);
    }

    // Acceptance (tinyvm.5): linear memory store/load round-trips.
    #[test]
    fn memory_store_then_load_returns_42() {
        // i32.const 0 ; i32.const 42 ; i32.store 0 0 ; i32.const 0 ; i32.load 0 0 ; end
        let body = [
            0x41, 0x00, // i32.const 0   (addr)
            0x41, 0x2A, // i32.const 42  (value)
            0x36, 0x00, 0x00, // i32.store align=0 offset=0
            0x41, 0x00, // i32.const 0   (addr)
            0x28, 0x00, 0x00, // i32.load align=0 offset=0
            0x0B, // end
        ];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![42]);
    }

    // Acceptance (tinyvm.5): a second address is independent; the first survives.
    #[test]
    fn memory_two_addresses_are_independent() {
        // store 42@0 ; store 99@4 ; load@0 ; load@4 ; (result arity 2 -> [42, 99])
        // Note: i32.const 99 is LEB128 `E3 00` (99 >= 64, so a single 0x63 byte
        // would sign-extend to -29).
        let body = [
            0x41, 0x00, 0x41, 0x2A, 0x36, 0x00, 0x00, // mem[0] = 42
            0x41, 0x04, 0x41, 0xE3, 0x00, 0x36, 0x00, 0x00, // mem[4] = 99
            0x41, 0x00, 0x28, 0x00, 0x00, // load mem[0]
            0x41, 0x04, 0x28, 0x00, 0x00, // load mem[4]
            0x0B, // end
        ];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 2, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![42, 99]);
    }

    #[test]
    fn out_of_bounds_load_traps() {
        // i32.const 65534 ; i32.load 0 0  -> reads [65534, 65538) > 65536
        let body = [0x41, 0xFE, 0xFF, 0x03, 0x28, 0x00, 0x00, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert!(matches!(m.invoke(f, &[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn out_of_bounds_store_traps() {
        // i32.const 65534 ; i32.const 1 ; i32.store 0 0  -> writes past the page
        let body = [
            0x41, 0xFE, 0xFF, 0x03, // i32.const 65534
            0x41, 0x01, // i32.const 1
            0x36, 0x00, 0x00, // i32.store
            0x0B,
        ];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 0, &body).unwrap();
        assert!(matches!(m.invoke(f, &[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn store_uses_offset_immediate() {
        // store value 7 at addr 0 with memarg offset 4 -> mem[4]; load addr 4 -> 7
        let body = [
            0x41, 0x00, 0x41, 0x07, 0x36, 0x00, 0x04, // i32.store offset=4 -> mem[4]=7
            0x41, 0x04, 0x28, 0x00, 0x00, // load mem[4]
            0x0B,
        ];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![7]);
    }

    /// Encode a small integer in [-64, 63] as a single signed-LEB byte.
    fn leb1(v: i32) -> u8 {
        (v as u8) & 0x7f
    }

    /// Run `i32.const a ; i32.const b ; <op> ; end` and return the result.
    fn binop(op: u8, a: i32, b: i32) -> Result<i32, WasmError> {
        let body = [0x41, leb1(a), 0x41, leb1(b), op, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body)?;
        Ok(m.invoke(f, &[])?[0])
    }

    /// Run `i32.const a ; <op> ; end` and return the result.
    fn unop(op: u8, a: i32) -> i32 {
        let body = [0x41, leb1(a), op, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        m.invoke(f, &[]).unwrap()[0]
    }

    // Gate (tinyvm.12): the tagged Val stack; invoke_val carries typed args/results.
    #[test]
    fn invoke_val_roundtrips_typed_i32() {
        let mut m = Module::new();
        // (param i32)(result i32): local.get 0 ; i32.const 1 ; i32.add
        let f = m
            .add_function(1, 0, 1, &[0x20, 0x00, 0x41, 0x01, 0x6A, 0x0B])
            .unwrap();
        assert_eq!(m.invoke_val(f, &[Val::I32(41)]).unwrap(), vec![Val::I32(42)]);
        // The i32 convenience wrapper still works.
        assert_eq!(m.invoke(f, &[41]).unwrap(), vec![42]);
    }

    // Family 1: i32 comparisons — signed vs unsigned must differ, true/false both.
    #[test]
    fn i32_comparisons() {
        assert_eq!(binop(0x46, 5, 5).unwrap(), 1); // eq true
        assert_eq!(binop(0x46, 5, 6).unwrap(), 0); // eq false
        assert_eq!(binop(0x47, 5, 6).unwrap(), 1); // ne true
        assert_eq!(binop(0x47, 5, 5).unwrap(), 0); // ne false
        assert_eq!(binop(0x48, -1, 1).unwrap(), 1); // lt_s(-1,1) true
        assert_eq!(binop(0x49, -1, 1).unwrap(), 0); // lt_u(0xffffffff,1) false
        assert_eq!(binop(0x4A, 1, -1).unwrap(), 1); // gt_s(1,-1) true
        assert_eq!(binop(0x4B, 1, -1).unwrap(), 0); // gt_u(1,huge) false
        assert_eq!(binop(0x4C, 5, 5).unwrap(), 1); // le_s
        assert_eq!(binop(0x4D, -1, -1).unwrap(), 1); // le_u equal
        assert_eq!(binop(0x4E, 5, 5).unwrap(), 1); // ge_s
        assert_eq!(binop(0x4F, -1, 1).unwrap(), 1); // ge_u(huge,1) true
    }

    // Family 2: i32 arithmetic and bitwise.
    #[test]
    fn i32_arithmetic_and_bitwise() {
        assert_eq!(binop(0x6C, 6, 7).unwrap(), 42); // mul
        assert_eq!(binop(0x6D, 20, 4).unwrap(), 5); // div_s
        assert_eq!(binop(0x6D, -20, 4).unwrap(), -5); // div_s neg
        assert_eq!(binop(0x6E, 20, 4).unwrap(), 5); // div_u
        assert_eq!(binop(0x6F, 20, 6).unwrap(), 2); // rem_s
        assert_eq!(binop(0x70, 20, 6).unwrap(), 2); // rem_u
        assert_eq!(binop(0x71, 6, 3).unwrap(), 2); // and
        assert_eq!(binop(0x72, 4, 1).unwrap(), 5); // or
        assert_eq!(binop(0x73, 6, 3).unwrap(), 5); // xor
        assert_eq!(binop(0x74, 1, 4).unwrap(), 16); // shl
        assert_eq!(binop(0x75, -8, 1).unwrap(), -4); // shr_s
        assert_eq!(binop(0x76, -8, 1).unwrap(), 0x7FFF_FFFC); // shr_u
        assert_eq!(binop(0x77, 1, 4).unwrap(), 16); // rotl
        assert_eq!(binop(0x78, 16, 4).unwrap(), 1); // rotr
        assert_eq!(unop(0x67, 1), 31); // clz
        assert_eq!(unop(0x68, 8), 3); // ctz
        assert_eq!(unop(0x69, 7), 3); // popcnt
    }

    #[test]
    fn i32_div_rem_by_zero_traps() {
        assert!(matches!(binop(0x6D, 5, 0), Err(WasmError::Trap(_)))); // div_s
        assert!(matches!(binop(0x6E, 5, 0), Err(WasmError::Trap(_)))); // div_u
        assert!(matches!(binop(0x6F, 5, 0), Err(WasmError::Trap(_)))); // rem_s
        assert!(matches!(binop(0x70, 5, 0), Err(WasmError::Trap(_)))); // rem_u
    }

    #[test]
    fn i32_div_s_min_over_neg_one_traps() {
        // i32.const i32::MIN (LEB 80 80 80 80 78) ; i32.const -1 ; i32.div_s
        let body = [
            0x41, 0x80, 0x80, 0x80, 0x80, 0x78, // i32.const -2147483648
            0x41, 0x7F, // i32.const -1
            0x6D, // i32.div_s
            0x0B,
        ];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert!(matches!(m.invoke(f, &[]), Err(WasmError::Trap(_))));
    }

    // Family 3: control flow.
    #[test]
    fn if_else_selects_branch() {
        // (param i32)(result i32): local.get0 ; if (result i32) 10 else 20 end
        let body = [
            0x20, 0x00, // local.get 0
            0x04, 0x7F, // if (result i32)
            0x41, 0x0A, // i32.const 10
            0x05, // else
            0x41, 0x14, // i32.const 20
            0x0B, // end (if)
            0x0B, // end (func)
        ];
        let mut m = Module::new();
        let f = m.add_function(1, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[1]).unwrap(), vec![10]); // true -> then
        assert_eq!(m.invoke(f, &[0]).unwrap(), vec![20]); // false -> else
    }

    #[test]
    fn if_without_else_runs_or_skips() {
        // (param i32)(result i32): base 7; if (no result) then set local via... keep simple:
        // i32.const 7 ; local.get0 ; if  (empty)  i32.const 0 drop  end ; end -> always 7
        // Simpler: prove skip doesn't corrupt: push 7, if(empty){ nop }, return 7.
        let body = [
            0x41, 0x07, // i32.const 7
            0x20, 0x00, // local.get 0 (condition)
            0x04, 0x40, // if (empty)
            0x01, // nop
            0x0B, // end if
            0x0B, // end func
        ];
        let mut m = Module::new();
        let f = m.add_function(1, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[1]).unwrap(), vec![7]); // cond true, runs nop
        assert_eq!(m.invoke(f, &[0]).unwrap(), vec![7]); // cond false, skips
    }

    #[test]
    fn drop_discards_top() {
        // i32.const 5 ; i32.const 9 ; drop -> 5
        let body = [0x41, 0x05, 0x41, 0x09, 0x1A, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![5]);
    }

    #[test]
    fn select_picks_by_condition() {
        // a=11, b=22 ; select on c
        let make = |c: u8| [0x41, 0x0B, 0x41, 0x16, 0x41, c, 0x1B, 0x0B];
        let mut m = Module::new();
        let t = m.add_function(0, 0, 1, &make(0x01)).unwrap();
        let e = m.add_function(0, 0, 1, &make(0x00)).unwrap();
        assert_eq!(m.invoke(t, &[]).unwrap(), vec![11]); // c!=0 -> a
        assert_eq!(m.invoke(e, &[]).unwrap(), vec![22]); // c==0 -> b
    }

    #[test]
    fn local_tee_writes_and_keeps() {
        // i32.const 7 ; local.tee 0 ; drop ; local.get 0 -> 7 (proves the write)
        let body = [0x41, 0x07, 0x22, 0x00, 0x1A, 0x20, 0x00, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 1, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![7]);
    }

    #[test]
    fn unreachable_traps() {
        let body = [0x00, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 0, &body).unwrap();
        assert!(matches!(m.invoke(f, &[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn br_table_switches() {
        // switch(local0) { 0 -> 10, 1 -> 20, default -> 30 } via three nested blocks
        let body = [
            0x02, 0x40, // block (label 2 / default)
            0x02, 0x40, // block (label 1)
            0x02, 0x40, // block (label 0)
            0x20, 0x00, // local.get 0
            0x0E, 0x02, 0x00, 0x01, 0x02, // br_table [0,1] default 2
            0x0B, // end block0
            0x41, 0x0A, 0x0F, // i32.const 10 ; return
            0x0B, // end block1
            0x41, 0x14, 0x0F, // i32.const 20 ; return
            0x0B, // end block2
            0x41, 0x1E, 0x0F, // i32.const 30 ; return
            0x0B, // end func
        ];
        let mut m = Module::new();
        let f = m.add_function(1, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[0]).unwrap(), vec![10]);
        assert_eq!(m.invoke(f, &[1]).unwrap(), vec![20]);
        assert_eq!(m.invoke(f, &[7]).unwrap(), vec![30]);
    }

    // Family 4: narrow memory access + memory.size/grow.
    fn run_body(n_locals: usize, arity: usize, body: &[u8]) -> Result<Vec<i32>, WasmError> {
        let mut m = Module::new();
        let f = m.add_function(0, n_locals, arity, body)?;
        m.invoke(f, &[])
    }

    #[test]
    fn narrow_byte_store_load() {
        // store8 171 @0 ; load8_u -> 171 ; load8_s -> -85
        let u = [
            0x41, 0x00, 0x41, 0xAB, 0x01, 0x3A, 0x00, 0x00, 0x41, 0x00, 0x2D, 0x00, 0x00, 0x0B,
        ];
        let s = [
            0x41, 0x00, 0x41, 0xAB, 0x01, 0x3A, 0x00, 0x00, 0x41, 0x00, 0x2C, 0x00, 0x00, 0x0B,
        ];
        assert_eq!(run_body(0, 1, &u).unwrap(), vec![171]);
        assert_eq!(run_body(0, 1, &s).unwrap(), vec![-85]);
    }

    #[test]
    fn narrow_halfword_store_load() {
        // store16 4660 @0 ; load16_u -> 4660
        let u = [
            0x41, 0x00, 0x41, 0xB4, 0x24, 0x3B, 0x00, 0x00, 0x41, 0x00, 0x2F, 0x00, 0x00, 0x0B,
        ];
        assert_eq!(run_body(0, 1, &u).unwrap(), vec![4660]);
        // store16 -1 ; load16_s -> -1 ; load16_u -> 65535
        let s = [
            0x41, 0x00, 0x41, 0x7F, 0x3B, 0x00, 0x00, 0x41, 0x00, 0x2E, 0x00, 0x00, 0x0B,
        ];
        let uu = [
            0x41, 0x00, 0x41, 0x7F, 0x3B, 0x00, 0x00, 0x41, 0x00, 0x2F, 0x00, 0x00, 0x0B,
        ];
        assert_eq!(run_body(0, 1, &s).unwrap(), vec![-1]);
        assert_eq!(run_body(0, 1, &uu).unwrap(), vec![65535]);
    }

    #[test]
    fn memory_size_and_grow() {
        // fresh memory is one page
        assert_eq!(run_body(0, 1, &[0x3F, 0x00, 0x0B]).unwrap(), vec![1]);
        // grow by 2 returns the old page count (1)
        assert_eq!(
            run_body(0, 1, &[0x41, 0x02, 0x40, 0x00, 0x0B]).unwrap(),
            vec![1]
        );
        // grow by 1, drop the old count, then memory.size -> 2
        assert_eq!(
            run_body(0, 1, &[0x41, 0x01, 0x40, 0x00, 0x1A, 0x3F, 0x00, 0x0B]).unwrap(),
            vec![2]
        );
    }

    #[test]
    fn narrow_load_out_of_bounds_traps() {
        // i32.const 65536 ; i32.load8_u -> [65536, 65537) > one page
        let body = [0x41, 0x80, 0x80, 0x04, 0x2D, 0x00, 0x00, 0x0B];
        assert!(matches!(run_body(0, 1, &body), Err(WasmError::Trap(_))));
    }

    // Family 5: host imports.
    #[test]
    fn wasm_calls_host_and_gets_return_value() {
        let mut m = Module::new();
        // host 0: increment its argument
        let _h = m.add_host_function(1, 1, |args, _mem| Ok(vec![args[0] + 1]));
        // defined func (index 1): local.get 0 ; call 0 ; end
        let f = m
            .add_function(1, 0, 1, &[0x20, 0x00, 0x10, 0x00, 0x0B])
            .unwrap();
        assert_eq!(f, 1); // host is 0, defined func is 1
        assert_eq!(m.invoke(f, &[41]).unwrap(), vec![42]);
    }

    #[test]
    fn host_reads_linear_memory_written_by_wasm() {
        let mut m = Module::new();
        // host 0: read i32 at mem[0] and return it
        let _h = m.add_host_function(0, 1, |_args, mem| {
            Ok(vec![i32::from_le_bytes([mem[0], mem[1], mem[2], mem[3]])])
        });
        // defined func: i32.const 0 ; i32.const 42 ; i32.store ; call 0 ; end
        let f = m
            .add_function(
                0,
                0,
                1,
                &[
                    0x41, 0x00, 0x41, 0x2A, 0x36, 0x00, 0x00, // mem[0] = 42
                    0x10, 0x00, // call host 0 -> reads mem[0]
                    0x0B,
                ],
            )
            .unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![42]);
    }

    #[test]
    fn module_with_import_section_traps_when_unbound() {
        // (module (import "env" "h" (func (result i32))) (func (result i32) call 0))
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            // type: func () -> (i32)
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F,
            // import: "env"."h" func type 0
            0x02, 0x09, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x01, 0x68, 0x00, 0x00,
            // func: type 0 (the defined function, index 1)
            0x03, 0x02, 0x01, 0x00,
            // code: call 0 (the import) ; end
            0x0A, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0B,
        ];
        let m = Module::from_bytes(&wasm).unwrap();
        // defined function is index 1 (import occupies 0)
        assert!(matches!(m.invoke(1, &[]), Err(WasmError::Trap(_))));
    }

    // Acceptance (tinyvm.6): load a standard .wasm module and invoke.
    //
    // Equivalent to:  (module (func (result i32) i32.const 42))
    #[test]
    fn module_from_bytes_returns_42() {
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // \0asm, version 1
            // type section: 1 type, func () -> (i32)
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F,
            // function section: 1 func, type 0
            0x03, 0x02, 0x01, 0x00,
            // code section: 1 body: 0 locals, i32.const 42, end
            0x0A, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2A, 0x0B,
        ];
        let m = Module::from_bytes(&wasm).unwrap();
        assert_eq!(m.invoke(0, &[]).unwrap(), vec![42]);
    }

    // A module that also carries an export section — which must be skipped —
    // and whose function takes a param and adds a local: (func (param i32)
    // (result i32) local.get 0 i32.const 1 i32.add). Section order: type(1),
    // func(3), export(7), code(10).
    #[test]
    fn module_skips_export_section_and_uses_params() {
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            // type: func (i32) -> (i32)
            0x01, 0x06, 0x01, 0x60, 0x01, 0x7F, 0x01, 0x7F,
            // func: type 0
            0x03, 0x02, 0x01, 0x00,
            // export "inc" func 0  -> skipped
            0x07, 0x07, 0x01, 0x03, 0x69, 0x6E, 0x63, 0x00, 0x00,
            // code: 0 locals, local.get 0, i32.const 1, i32.add, end
            0x0A, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x41, 0x01, 0x6A, 0x0B,
        ];
        let m = Module::from_bytes(&wasm).unwrap();
        assert_eq!(m.invoke(0, &[41]).unwrap(), vec![42]);
    }

    #[test]
    fn module_bad_magic_fails_to_decode() {
        assert!(matches!(
            Module::from_bytes(&[0x00, 0x61, 0x73, 0x00, 0x01, 0x00, 0x00, 0x00]),
            Err(WasmError::Decode(_))
        ));
    }

    #[test]
    fn module_bad_version_fails_to_decode() {
        assert!(matches!(
            Module::from_bytes(&[0x00, 0x61, 0x73, 0x6D, 0x02, 0x00, 0x00, 0x00]),
            Err(WasmError::Decode(_))
        ));
    }

    // Family: f32.
    fn f32_run(body: &[u8]) -> Vec<Val> {
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, body).unwrap();
        m.invoke_val(f, &[]).unwrap()
    }
    fn f32_bin(a: f32, b: f32, op: u8) -> Vec<Val> {
        let mut body = vec![0x43];
        body.extend_from_slice(&a.to_le_bytes());
        body.push(0x43);
        body.extend_from_slice(&b.to_le_bytes());
        body.push(op);
        body.push(0x0B);
        f32_run(&body)
    }
    fn f32_un(a: f32, op: u8) -> Vec<Val> {
        let mut body = vec![0x43];
        body.extend_from_slice(&a.to_le_bytes());
        body.push(op);
        body.push(0x0B);
        f32_run(&body)
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn f32_const_add_and_ops() {
        assert_eq!(f32_bin(1.5, 2.5, 0x92), vec![Val::F32(4.0)]); // add
        assert_eq!(f32_bin(4.0, 2.5, 0x93), vec![Val::F32(1.5)]); // sub
        assert_eq!(f32_bin(3.0, 4.0, 0x94), vec![Val::F32(12.0)]); // mul
        assert_eq!(f32_bin(9.0, 4.0, 0x95), vec![Val::F32(2.25)]); // div
        assert_eq!(f32_un(4.0, 0x91), vec![Val::F32(2.0)]); // sqrt
        assert_eq!(f32_un(-3.0, 0x8B), vec![Val::F32(3.0)]); // abs
        assert_eq!(f32_bin(3.0, -1.0, 0x98), vec![Val::F32(-3.0)]); // copysign
    }

    #[test]
    fn f32_comparisons_and_nan() {
        assert_eq!(f32_bin(1.5, 2.5, 0x5D), vec![Val::I32(1)]); // lt true
        assert_eq!(f32_bin(2.5, 1.5, 0x5D), vec![Val::I32(0)]); // lt false
        assert_eq!(f32_bin(2.5, 2.5, 0x5B), vec![Val::I32(1)]); // eq true
        assert_eq!(f32_bin(f32::NAN, 1.0, 0x5B), vec![Val::I32(0)]); // eq false on NaN
        assert_eq!(f32_bin(f32::NAN, 1.0, 0x5C), vec![Val::I32(1)]); // ne true on NaN
    }

    #[test]
    fn f32_min_max_nan_signed_zero() {
        assert!(matches!(f32_bin(f32::NAN, 1.0, 0x96)[0], Val::F32(x) if x.is_nan()));
        assert!(matches!(f32_bin(-0.0, 0.0, 0x96)[0], Val::F32(x) if x == 0.0 && x.is_sign_negative()));
        assert!(matches!(f32_bin(-0.0, 0.0, 0x97)[0], Val::F32(x) if x == 0.0 && x.is_sign_positive()));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn f32_store_load_roundtrip() {
        let mut body = vec![0x41, 0x00, 0x43];
        body.extend_from_slice(&3.25f32.to_le_bytes());
        body.extend_from_slice(&[0x38, 0x00, 0x00, 0x41, 0x00, 0x2A, 0x00, 0x00, 0x0B]);
        assert_eq!(f32_run(&body), vec![Val::F32(3.25)]);
    }

    // Family: f64.
    fn f64_const_bytes(v: f64) -> Vec<u8> {
        let mut b = vec![0x44];
        b.extend_from_slice(&v.to_le_bytes());
        b
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn f64_add_two_constants() {
        let mut body = f64_const_bytes(1.5);
        body.extend_from_slice(&f64_const_bytes(2.5));
        body.push(0xA0); // f64.add
        body.push(0x0B);
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke_val(f, &[]).unwrap(), vec![Val::F64(4.0)]);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn f64_compare_unary_binary_goldens() {
        let run = |body: &[u8]| {
            let mut m = Module::new();
            let f = m.add_function(0, 0, 1, body).unwrap();
            m.invoke_val(f, &[]).unwrap()
        };
        let bin = |a: f64, b: f64, op: u8| {
            let mut body = f64_const_bytes(a);
            body.extend_from_slice(&f64_const_bytes(b));
            body.push(op);
            body.push(0x0B);
            run(&body)
        };
        let un = |a: f64, op: u8| {
            let mut body = f64_const_bytes(a);
            body.push(op);
            body.push(0x0B);
            run(&body)
        };
        assert_eq!(bin(1.5, 2.5, 0x63), vec![Val::I32(1)]); // lt true
        assert_eq!(bin(2.5, 1.5, 0x63), vec![Val::I32(0)]); // lt false
        assert_eq!(un(4.0, 0x9F), vec![Val::F64(2.0)]); // sqrt
        assert_eq!(un(-3.0, 0x99), vec![Val::F64(3.0)]); // abs
        assert_eq!(un(3.0, 0x9A), vec![Val::F64(-3.0)]); // neg
        assert_eq!(bin(1.0, 2.0, 0xA4), vec![Val::F64(1.0)]); // min
        assert_eq!(bin(1.0, 2.0, 0xA5), vec![Val::F64(2.0)]); // max
        assert_eq!(bin(3.0, -1.0, 0xA6), vec![Val::F64(-3.0)]); // copysign
        // NaN + signed zero semantics for min/max
        assert!(matches!(bin(f64::NAN, 1.0, 0xA4)[0], Val::F64(x) if x.is_nan()));
        assert!(matches!(bin(-0.0, 0.0, 0xA4)[0], Val::F64(x) if x == 0.0 && x.is_sign_negative()));
        assert!(matches!(bin(-0.0, 0.0, 0xA5)[0], Val::F64(x) if x == 0.0 && x.is_sign_positive()));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn f64_store_load_roundtrip() {
        let mut body = vec![0x41, 0x00];
        body.extend_from_slice(&f64_const_bytes(3.25));
        body.extend_from_slice(&[0x39, 0x00, 0x00, 0x41, 0x00, 0x2B, 0x00, 0x00, 0x0B]);
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke_val(f, &[]).unwrap(), vec![Val::F64(3.25)]);
    }

    // Family: i64 integer ops.
    fn run_i64(body: &[u8]) -> i64 {
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, body).unwrap();
        match m.invoke_val(f, &[]).unwrap().as_slice() {
            [Val::I64(v)] => *v,
            other => panic!("expected one i64 result, got {other:?}"),
        }
    }
    fn run_i32_res(body: &[u8]) -> i32 {
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, body).unwrap();
        match m.invoke_val(f, &[]).unwrap().as_slice() {
            [Val::I32(v)] => *v,
            other => panic!("expected one i32 result, got {other:?}"),
        }
    }

    #[test]
    fn i64_const_add_returns_43() {
        // i64.const 42 ; i64.const 1 ; i64.add
        assert_eq!(run_i64(&[0x42, 0x2A, 0x42, 0x01, 0x7C, 0x0B]), 43);
    }

    #[test]
    fn i64_compare_and_eqz() {
        assert_eq!(run_i32_res(&[0x42, 0x05, 0x42, 0x05, 0x51, 0x0B]), 1); // eq true
        assert_eq!(run_i32_res(&[0x42, 0x05, 0x42, 0x06, 0x52, 0x0B]), 1); // ne true
        assert_eq!(run_i32_res(&[0x42, 0x7F, 0x42, 0x01, 0x53, 0x0B]), 1); // lt_s(-1,1)
        assert_eq!(run_i32_res(&[0x42, 0x7F, 0x42, 0x01, 0x54, 0x0B]), 0); // lt_u(-1,1)
        assert_eq!(run_i32_res(&[0x42, 0x00, 0x50, 0x0B]), 1); // eqz 0
        assert_eq!(run_i32_res(&[0x42, 0x01, 0x50, 0x0B]), 0); // eqz 1
    }

    #[test]
    fn i64_arith_shift_bitcount_golden() {
        assert_eq!(run_i64(&[0x42, 0x06, 0x42, 0x07, 0x7E, 0x0B]), 42); // mul
        assert_eq!(run_i64(&[0x42, 0x79, 0x42, 0x02, 0x7F, 0x0B]), -3); // div_s(-7,2)
        assert_eq!(run_i64(&[0x42, 0x06, 0x42, 0x03, 0x85, 0x0B]), 5); // xor
        assert_eq!(run_i64(&[0x42, 0x01, 0x42, 0x04, 0x86, 0x0B]), 16); // shl
        assert_eq!(run_i64(&[0x42, 0x78, 0x42, 0x01, 0x87, 0x0B]), -4); // shr_s
        assert_eq!(run_i64(&[0x42, 0x7F, 0x42, 0x3F, 0x88, 0x0B]), 1); // shr_u(-1,63)
        assert_eq!(run_i64(&[0x42, 0x01, 0x42, 0x01, 0x8A, 0x0B]), i64::MIN); // rotr(1,1)
        assert_eq!(run_i64(&[0x42, 0x01, 0x79, 0x0B]), 63); // clz
        assert_eq!(run_i64(&[0x42, 0x08, 0x7A, 0x0B]), 3); // ctz
        assert_eq!(run_i64(&[0x42, 0x7F, 0x7B, 0x0B]), 64); // popcnt(-1)
    }

    #[test]
    fn i64_div_rem_by_zero_and_overflow_trap() {
        let mut m = Module::new();
        for op in [0x7F, 0x80, 0x81, 0x82] {
            let f = m
                .add_function(0, 0, 1, &[0x42, 0x01, 0x42, 0x00, op, 0x0B])
                .unwrap();
            assert!(matches!(m.invoke_val(f, &[]), Err(WasmError::Trap(_))));
        }
        // i64::MIN / -1 overflow: const LEB = nine 0x80 then 0x7F.
        let min = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7F];
        let mut body = vec![0x42];
        body.extend_from_slice(&min);
        body.extend_from_slice(&[0x42, 0x7F, 0x7F, 0x0B]); // i64.const -1 ; div_s ; end
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert!(matches!(m.invoke_val(f, &[]), Err(WasmError::Trap(_))));
    }

    // Family: i64 linear memory (values supplied as params via invoke_val).
    #[test]
    fn i64_store_then_load_returns_42() {
        let body = [
            0x41, 0x00, 0x20, 0x00, 0x37, 0x00, 0x00, 0x41, 0x00, 0x29, 0x00, 0x00, 0x0B,
        ];
        let mut m = Module::new();
        let f = m.add_function(1, 0, 1, &body).unwrap();
        assert_eq!(m.invoke_val(f, &[Val::I64(42)]).unwrap(), vec![Val::I64(42)]);
    }

    #[test]
    fn i64_narrow_store_load_sign_vs_zero() {
        // store8 then load8_u / load8_s of -1 -> 255 / -1
        let b8 = [
            0x41, 0x00, 0x20, 0x00, 0x3C, 0x00, 0x00, 0x41, 0x00, 0x31, 0x00, 0x00, 0x41, 0x00,
            0x30, 0x00, 0x00, 0x0B,
        ];
        // store32 then load32_u / load32_s of -1 -> 4294967295 / -1
        let b32 = [
            0x41, 0x00, 0x20, 0x00, 0x3E, 0x00, 0x00, 0x41, 0x00, 0x35, 0x00, 0x00, 0x41, 0x00,
            0x34, 0x00, 0x00, 0x0B,
        ];
        let mut m = Module::new();
        let f8 = m.add_function(1, 0, 2, &b8).unwrap();
        let f32_ = m.add_function(1, 0, 2, &b32).unwrap();
        assert_eq!(
            m.invoke_val(f8, &[Val::I64(-1)]).unwrap(),
            vec![Val::I64(255), Val::I64(-1)]
        );
        assert_eq!(
            m.invoke_val(f32_, &[Val::I64(-1)]).unwrap(),
            vec![Val::I64(4294967295), Val::I64(-1)]
        );
    }

    #[test]
    fn i64_memory_out_of_bounds_traps() {
        let load = [0x41, 0xFA, 0xFF, 0x03, 0x29, 0x00, 0x00, 0x0B];
        let store = [0x41, 0xFA, 0xFF, 0x03, 0x20, 0x00, 0x37, 0x00, 0x00, 0x0B];
        let mut m = Module::new();
        let fl = m.add_function(0, 0, 1, &load).unwrap();
        let fs = m.add_function(1, 0, 0, &store).unwrap();
        assert!(matches!(m.invoke_val(fl, &[]), Err(WasmError::Trap(_))));
        assert!(matches!(
            m.invoke_val(fs, &[Val::I64(1)]),
            Err(WasmError::Trap(_))
        ));
    }

    #[test]
    fn call_invokes_another_function() {
        let mut m = Module::new();
        // func0: call 1 ; end
        let f0 = m.add_function(0, 0, 1, &[0x10, 0x01, 0x0B]).unwrap();
        // func1: i32.const 40 ; i32.const 2 ; i32.add ; end
        let _f1 = m
            .add_function(0, 0, 1, &[0x41, 0x28, 0x41, 0x02, 0x6A, 0x0B])
            .unwrap();
        assert_eq!(m.invoke(f0, &[]).unwrap(), vec![42]);
    }

    #[test]
    fn br_exits_a_block() {
        // block  i32.const 7  br 0  i32.const 99(unreached)  end  -> leaves 7
        // block arity 1 so the 7 is the block/function result.
        let body = [
            0x02, 0x7F, // block (result i32)
            0x41, 0x07, // i32.const 7
            0x0C, 0x00, // br 0  (exit block, keep the 7)
            0x41, 0x63, // i32.const 99 (unreachable)
            0x0B, // end block
            0x0B, // end func
        ];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![7]);
    }

    #[test]
    fn return_ends_function_early() {
        // i32.const 5  return  i32.const 9(unreached)  end
        let body = [0x41, 0x05, 0x0F, 0x41, 0x09, 0x0B];
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![5]);
    }

    #[test]
    fn stack_underflow_traps() {
        // i32.add with nothing on the stack.
        let mut m = Module::new();
        let f = m.add_function(0, 0, 0, &[0x6A, 0x0B]).unwrap();
        assert!(matches!(m.invoke(f, &[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn out_of_range_local_traps() {
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &[0x20, 0x05, 0x0B]).unwrap();
        assert!(matches!(m.invoke(f, &[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn unsupported_opcode_fails_to_decode() {
        let mut m = Module::new();
        // 0xC0 is i32.extend8_s (sign-extension proposal), outside this cut.
        assert!(matches!(
            m.add_function(0, 0, 1, &[0xC0, 0x0B]),
            Err(WasmError::Decode(_))
        ));
    }

    #[test]
    fn unterminated_block_fails_to_decode() {
        let mut m = Module::new();
        assert!(matches!(
            m.add_function(0, 0, 0, &[0x02, 0x40]),
            Err(WasmError::Decode(_))
        ));
    }

    #[test]
    fn leb128_signed_negative_roundtrips() {
        // i32.const -1 (0x7F) then end; result arity 1.
        let mut m = Module::new();
        let f = m.add_function(0, 0, 1, &[0x41, 0x7F, 0x0B]).unwrap();
        assert_eq!(m.invoke(f, &[]).unwrap(), vec![-1]);
    }
}
