//! Slot A: a thin interpreter for **WebAssembly 1.0 MVP** (all 172 opcodes).
//!
//! The public face is [`eval`]: standard `.wasm` bytes in, a [`Val`] sequence
//! or a loud [`WasmError`] out. WAT is not an input. Host functions enter
//! through the module import table ([`Module::bind_import_typed_in_place`]),
//! not a general FFI. [`Module::bind_import`] remains the narrower i32
//! compatibility door used by simple embeddings. No JIT/AOT — that is parked
//! slot B.
//!
//! Every fault is loud: a malformed body fails to decode, and a run-time trap
//! (stack underflow, out-of-range local/label/func, unbound import, budget
//! / depth overrun) returns a [`WasmError`] rather than misbehaving silently.

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

mod validate;

/// Maximum simultaneously live guest-defined call activations.
///
/// Activations live in a fallibly grown VM vector rather than on the native
/// stack, so debug and release builds enforce the same deterministic boundary.
/// Exceeding it is a loud `Trap("call depth")`.
pub const WASM_MAX_DEPTH: usize = 512;
/// Maximum aggregate value/control slots held by the active function and all
/// suspended callers in one top-level invocation. This bounds guest call-stack
/// heap independently of linear memory and instruction fuel.
pub const WASM_MAX_ACTIVATION_SLOTS: usize = 1 << 20;
/// Max executed instructions per top-level [`Module::invoke`].
pub const WASM_MAX_STEPS: u64 = 16_000_000;
/// WebAssembly linear-memory page size (64 KiB). Memory starts at one page per
/// invocation and can grow via `memory.grow`.
pub const WASM_PAGE_SIZE: usize = 65_536;
/// Maximum pages `memory.grow` will allocate (the spec's 32-bit maximum).
pub const WASM_MAX_PAGES: usize = 65_536;
/// Default host robustness cap on linear memory (pages), including growth.
/// A declared min above this is `Err` before any multi-gigabyte allocation is
/// touched. Callers may choose a different cap through [`Limits`], still
/// bounded by [`WASM_MAX_PAGES`].
pub const WASM_MAX_ALLOC_PAGES: usize = 256;
/// Cap on the WASM operand stack. Independent of [`WASM_MAX_STEPS`]: a
/// push-only loop traps here instead of growing to hundreds of megabytes.
pub const WASM_STACK_LIMIT: usize = 65_536;
/// Maximum declared locals per function (a decode-time sanity bound).
pub const WASM_MAX_LOCALS: usize = 1 << 20;
/// Maximum allocation-amplifying logical records decoded from one module.
///
/// Raw data/name bytes are already bounded by their containing input. This
/// budget covers types, imports, functions, locals, instructions, branch-table
/// targets and other records whose in-memory representation is much larger
/// than their shortest wire encoding.
pub const WASM_MAX_DECODE_ITEMS: usize = 262_144;

/// A tagged WebAssembly value. The operand stack, locals, arguments, and
/// results are all sequences of these so the four numeric types can coexist.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, PartialEq)]
pub enum Val {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    /// A nullable reference into this module instance's combined function
    /// index space. `None` is the standard null `funcref` value.
    FuncRef(Option<usize>),
}

/// A standard value type accepted by this VM profile.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
    FuncRef,
}

impl ValueType {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x7F => Some(Self::I32),
            0x7E => Some(Self::I64),
            0x7D => Some(Self::F32),
            0x7C => Some(Self::F64),
            0x70 => Some(Self::FuncRef),
            _ => None,
        }
    }
}

/// A decode-time or run-time WebAssembly fault. Messages are `&'static str`
/// so the crate never pulls in the formatting machinery.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WasmError {
    /// The function body could not be decoded.
    Decode(&'static str),
    /// The program trapped at run time.
    Trap(&'static str),
}

struct DecodeBudget {
    remaining: usize,
    /// Resource context for validating decoded standard memory instructions.
    has_memory: bool,
}

impl DecodeBudget {
    fn new() -> Self {
        Self {
            remaining: WASM_MAX_DECODE_ITEMS,
            has_memory: false,
        }
    }

    fn charge(&mut self, count: usize) -> Result<(), WasmError> {
        self.remaining = self
            .remaining
            .checked_sub(count)
            .ok_or(WasmError::Decode("module decode budget"))?;
        Ok(())
    }
}

fn reserve_exact<T>(values: &mut Vec<T>, count: usize) -> Result<(), WasmError> {
    values
        .try_reserve_exact(count)
        .map_err(|_| WasmError::Decode("module allocation"))
}

impl WasmError {
    /// The static message for this fault (no formatting).
    pub fn message(&self) -> &'static str {
        match self {
            Self::Decode(m) | Self::Trap(m) => m,
        }
    }
}

/// Host-owned resource budget for loading and invoking one module.
///
/// Table and initial-memory limits are checked before allocation. The step
/// budget resets for every top-level invocation, and the memory-page limit
/// also caps `memory.grow` for the complete lifetime of an [`Instance`].
#[derive(Clone, Copy)]
pub struct Limits {
    /// Maximum aggregate funcref elements the host will instantiate across all
    /// tables. Compared against the sum of declared minima before allocation.
    pub max_table_elems: usize,
    /// Maximum linear-memory pages the host will allocate. Compared against
    /// the declared memory `min` before an instance is created and enforced
    /// again by every `memory.grow` (one page is 64 KiB).
    pub max_memory_pages: usize,
    /// Maximum instructions executed by one top-level call. Nested calls share
    /// that call's counter; the next top-level call receives a fresh budget.
    pub max_steps: u64,
    /// Maximum simultaneously live guest-defined activations. Guest calls use
    /// VM heap storage rather than the native stack.
    pub max_call_depth: usize,
    /// Maximum aggregate locals, operand values and control frames across the
    /// current activation and every suspended caller.
    pub max_activation_slots: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_table_elems: 65_536,
            max_memory_pages: WASM_MAX_ALLOC_PAGES,
            max_steps: WASM_MAX_STEPS,
            max_call_depth: WASM_MAX_DEPTH,
            max_activation_slots: WASM_MAX_ACTIVATION_SLOTS,
        }
    }
}

/// Slot A face: load a standard WebAssembly 1.0 module and evaluate it.
///
/// Runs the start function if present, then the first declared function
/// export (or the first defined function if there is no export). Host
/// imports stay unbound and trap if called; bind them via
/// [`Module::from_bytes`] + [`Module::bind_import`] + [`Module::eval`].
/// Uses [`Limits::default`] as the host budget.
pub fn eval(bytes: &[u8]) -> Result<Vec<Val>, WasmError> {
    eval_with(bytes, Limits::default())
}

/// Like [`eval`], but the caller supplies the host budget.
pub fn eval_with(bytes: &[u8], limits: Limits) -> Result<Vec<Val>, WasmError> {
    Module::from_bytes_with(bytes, limits)?.eval(&[])
}

/// A decoded instruction. Branch/call operands keep their WASM indices; block
/// and loop carry the index of their matching `End` so branches resolve in O(1).
///
/// No `Eq` derive: `F32Const` holds an `f32`, which is not `Eq` (float consts
/// are stored raw so decoding a `.wasm` module never needs value comparison).
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, PartialEq)]
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
    /// The validated memarg alignment hint does not affect scalar execution.
    I32Load {
        offset: u32,
    },
    /// `i32.store` — pop value then address; write 4 little-endian bytes at
    /// `addr + offset`. Alignment hint decoded and ignored.
    I32Store {
        offset: u32,
    },
    /// Narrow loads (sign/zero extended to i32) and stores (low bytes).
    I32Load8S {
        offset: u32,
    },
    I32Load8U {
        offset: u32,
    },
    I32Load16S {
        offset: u32,
    },
    I32Load16U {
        offset: u32,
    },
    I32Store8 {
        offset: u32,
    },
    I32Store16 {
        offset: u32,
    },
    /// `memory.size` — push the current size in pages.
    MemorySize,
    /// `memory.grow` — pop delta pages, grow, push old size (or -1 on failure).
    MemoryGrow,
    /// Bulk-memory `memory.copy` (0xfc 10). Overlap has memmove semantics.
    MemoryCopy,
    /// Bulk-memory `memory.fill` (0xfc 11). The low byte of the value is used.
    MemoryFill,
    /// Bulk-memory `memory.init`: copy from a module data segment.
    MemoryInit {
        data_index: u32,
    },
    /// Bulk-memory `data.drop`: make one instance's segment empty.
    DataDrop {
        data_index: u32,
    },
    /// Bulk-memory `table.init`: copy a passive funcref element segment.
    TableInit {
        elem_index: u32,
        table_index: u32,
    },
    /// Bulk-memory `elem.drop`: make one instance's segment empty.
    ElemDrop {
        elem_index: u32,
    },
    /// Bulk-memory `table.copy`, including copies between distinct tables.
    TableCopy {
        destination_table: u32,
        source_table: u32,
    },
    /// Reference-types table operations with their standard table immediate.
    TableGet(u32),
    TableSet(u32),
    TableGrow(u32),
    TableSize(u32),
    TableFill(u32),
    /// `i64.load`/`i64.store` — 8 little-endian bytes at `addr + offset`.
    I64Load {
        offset: u32,
    },
    I64Store {
        offset: u32,
    },
    /// Narrow i64 loads (sign/zero extended to i64) and stores (low bytes).
    I64Load8S {
        offset: u32,
    },
    I64Load8U {
        offset: u32,
    },
    I64Load16S {
        offset: u32,
    },
    I64Load16U {
        offset: u32,
    },
    I64Load32S {
        offset: u32,
    },
    I64Load32U {
        offset: u32,
    },
    I64Store8 {
        offset: u32,
    },
    I64Store16 {
        offset: u32,
    },
    I64Store32 {
        offset: u32,
    },
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
    F64Load {
        offset: u32,
    },
    F64Store {
        offset: u32,
    },
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
    F32Load {
        offset: u32,
    },
    F32Store {
        offset: u32,
    },
    /// `global.get` / `global.set` — read/write a module global by index.
    GlobalGet(u32),
    GlobalSet(u32),
    // --- numeric conversion family (0xA7..=0xBF) ---
    I32WrapI64,
    I64ExtendI32S,
    I64ExtendI32U,
    I32TruncF32S,
    I32TruncF32U,
    I32TruncF64S,
    I32TruncF64U,
    I64TruncF32S,
    I64TruncF32U,
    I64TruncF64S,
    I64TruncF64U,
    I32TruncSatF32S,
    I32TruncSatF32U,
    I32TruncSatF64S,
    I32TruncSatF64U,
    I64TruncSatF32S,
    I64TruncSatF32U,
    I64TruncSatF64S,
    I64TruncSatF64U,
    F32ConvertI32S,
    F32ConvertI32U,
    F32ConvertI64S,
    F32ConvertI64U,
    F32DemoteF64,
    F64ConvertI32S,
    F64ConvertI32U,
    F64ConvertI64S,
    F64ConvertI64U,
    F64PromoteF32,
    I32ReinterpretF32,
    I64ReinterpretF64,
    F32ReinterpretI32,
    F64ReinterpretI64,
    I32Extend8S,
    I32Extend16S,
    I64Extend8S,
    I64Extend16S,
    I64Extend32S,
    LocalGet(u32),
    LocalSet(u32),
    Call(u32),
    /// Tail-call variants replace the current activation instead of consuming
    /// another native-stack frame.
    ReturnCall(u32),
    ReturnCallIndirect {
        type_index: u32,
        table_index: u32,
    },
    /// `ty` is an inline empty/single-result type or a function-type index;
    /// `end` indexes its `End`.
    Block {
        ty: BlockType,
        end: usize,
    },
    /// A loop branch carries the block type's parameters back to its start.
    Loop {
        ty: BlockType,
        end: usize,
    },
    Br(u32),
    BrIf(u32),
    /// `call_indirect` — pop an element index, look up the funcref in the
    /// immediate-selected table, type-check it, and call it.
    CallIndirect {
        type_index: u32,
        table_index: u32,
    },
    /// `br_table` — pop an index; branch to `targets[index]` or `default`.
    BrTable {
        target_start: u32,
        target_len: u32,
        default: u32,
    },
    /// `if` — pop a condition; run the then-body when nonzero, else the
    /// else-body. `else_pc` indexes the [`Op::Else`] (if any); `end` indexes the
    /// matching `End`.
    If {
        ty: BlockType,
        else_pc: Option<usize>,
        end: usize,
    },
    /// `else` — end of the then-body; `end` indexes the `if`'s matching `End`.
    Else {
        end: usize,
    },
    /// `unreachable` — always traps.
    Unreachable,
    /// `nop` — does nothing.
    Nop,
    /// `drop` — discard the top of stack.
    Drop,
    /// `select` — pop c, b, a; push `a` if `c != 0` else `b`.
    Select,
    /// Typed `select`: reference types require the explicit value type.
    TypedSelect(u8),
    RefNull,
    RefIsNull,
    RefFunc(u32),
    /// `local.tee` — like `local.set` but leaves the value on the stack.
    LocalTee(u32),
    Return,
    End,
}

/// Standard structured-control block type. A non-negative s33 value indexes a
/// function type and therefore supplies both parameter and result vectors.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockType {
    Empty,
    Value(u8),
    TypeIndex(u32),
}

fn leb_u32(bytes: &[u8], mut i: usize) -> Result<(u32, usize), WasmError> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = *bytes
            .get(i)
            .ok_or(WasmError::Decode("truncated unsigned LEB128"))?;
        i += 1;
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(WasmError::Decode("unsigned LEB128 too long"));
        }
    }
    u32::try_from(result)
        .map(|v| (v, i))
        .map_err(|_| WasmError::Decode("unsigned LEB128 exceeds u32"))
}

fn leb_s32(bytes: &[u8], mut i: usize) -> Result<(i32, usize), WasmError> {
    let mut result: i64 = 0;
    let mut shift = 0;
    loop {
        let byte = *bytes
            .get(i)
            .ok_or(WasmError::Decode("truncated signed LEB128"))?;
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
            return Err(WasmError::Decode("signed LEB128 too long"));
        }
    }
    i32::try_from(result)
        .map(|v| (v, i))
        .map_err(|_| WasmError::Decode("signed LEB128 exceeds i32"))
}

fn leb_s33(bytes: &[u8], mut i: usize) -> Result<(i64, usize), WasmError> {
    let mut result = 0i64;
    for byte_index in 0..5 {
        let byte = *bytes
            .get(i)
            .ok_or(WasmError::Decode("truncated block type"))?;
        i += 1;
        let shift = byte_index * 7;
        result |= i64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            if byte_index == 4 {
                let unused = byte & 0x60;
                let negative = byte & 0x10 != 0;
                if (!negative && unused != 0) || (negative && unused != 0x60) {
                    return Err(WasmError::Decode("block type s33 overflow"));
                }
                if negative {
                    result |= !((1i64 << 33) - 1);
                }
            } else if byte & 0x40 != 0 {
                result |= !((1i64 << (shift + 7)) - 1);
            }
            return Ok((result, i));
        }
    }
    Err(WasmError::Decode("block type s33 too long"))
}

/// Decode the standard s33 block type. Inline numeric result types are the
/// small negative encodings; every non-negative value is a function type
/// index. Type index 64 consequently uses `c0 00`, not the reserved inline
/// empty byte `40`.
fn block_type(bytes: &[u8], i: usize) -> Result<(BlockType, usize), WasmError> {
    let (encoded, next) = leb_s33(bytes, i)?;
    match encoded {
        -64 => Ok((BlockType::Empty, next)),
        -1 => Ok((BlockType::Value(0x7F), next)),
        -2 => Ok((BlockType::Value(0x7E), next)),
        -3 => Ok((BlockType::Value(0x7D), next)),
        -4 => Ok((BlockType::Value(0x7C), next)),
        0..=4_294_967_295 => Ok((BlockType::TypeIndex(encoded as u32), next)),
        _other => Err(WasmError::Decode("unsupported block type")),
    }
}

struct DecodedCode {
    ops: Vec<Op>,
    branch_targets: Vec<u32>,
}

fn decode(body: &[u8], budget: &mut DecodeBudget) -> Result<DecodedCode, WasmError> {
    let mut ops: Vec<Op> = Vec::new();
    let mut branch_targets = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        budget.charge(1)?;
        let opcode = body[i];
        i += 1;
        if !budget.has_memory && opcode.wrapping_sub(0x28) <= 0x18 {
            return Err(WasmError::Decode(
                "validation: memory instruction requires memory",
            ));
        }
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
                let (offset, ni) = memarg(body, i, 2)?;
                i = ni;
                ops.push(Op::I32Load { offset });
            }
            0x36 => {
                let (offset, ni) = memarg(body, i, 2)?;
                i = ni;
                ops.push(Op::I32Store { offset });
            }
            0x2C => {
                let (offset, ni) = memarg(body, i, 0)?;
                i = ni;
                ops.push(Op::I32Load8S { offset });
            }
            0x2D => {
                let (offset, ni) = memarg(body, i, 0)?;
                i = ni;
                ops.push(Op::I32Load8U { offset });
            }
            0x2E => {
                let (offset, ni) = memarg(body, i, 1)?;
                i = ni;
                ops.push(Op::I32Load16S { offset });
            }
            0x2F => {
                let (offset, ni) = memarg(body, i, 1)?;
                i = ni;
                ops.push(Op::I32Load16U { offset });
            }
            0x3A => {
                let (offset, ni) = memarg(body, i, 0)?;
                i = ni;
                ops.push(Op::I32Store8 { offset });
            }
            0x3B => {
                let (offset, ni) = memarg(body, i, 1)?;
                i = ni;
                ops.push(Op::I32Store16 { offset });
            }
            0x3F => {
                // memory.size — a single reserved memory-index byte (must be 0)
                let b = *body
                    .get(i)
                    .ok_or(WasmError::Decode("truncated memory.size"))?;
                if b != 0x00 {
                    return Err(WasmError::Decode("memory.size reserved byte must be 0"));
                }
                i += 1;
                ops.push(Op::MemorySize);
            }
            0x40 => {
                let b = *body
                    .get(i)
                    .ok_or(WasmError::Decode("truncated memory.grow"))?;
                if b != 0x00 {
                    return Err(WasmError::Decode("memory.grow reserved byte must be 0"));
                }
                i += 1;
                ops.push(Op::MemoryGrow);
            }
            0xFC => {
                let (subopcode, ni) = leb_u32(body, i)?;
                i = ni;
                if !budget.has_memory && matches!(subopcode, 8 | 10 | 11) {
                    return Err(WasmError::Decode(
                        "validation: memory instruction requires memory",
                    ));
                }
                match subopcode {
                    0 => ops.push(Op::I32TruncSatF32S),
                    1 => ops.push(Op::I32TruncSatF32U),
                    2 => ops.push(Op::I32TruncSatF64S),
                    3 => ops.push(Op::I32TruncSatF64U),
                    4 => ops.push(Op::I64TruncSatF32S),
                    5 => ops.push(Op::I64TruncSatF32U),
                    6 => ops.push(Op::I64TruncSatF64S),
                    7 => ops.push(Op::I64TruncSatF64U),
                    8 => {
                        let (data_index, n1) = leb_u32(body, i)?;
                        let (memory, n2) = leb_u32(body, n1)?;
                        i = n2;
                        if memory != 0 {
                            return Err(WasmError::Decode("memory.init memory index must be 0"));
                        }
                        ops.push(Op::MemoryInit { data_index });
                    }
                    9 => {
                        let (data_index, ni) = leb_u32(body, i)?;
                        i = ni;
                        ops.push(Op::DataDrop { data_index });
                    }
                    10 => {
                        let (destination_memory, n1) = leb_u32(body, i)?;
                        let (source_memory, n2) = leb_u32(body, n1)?;
                        i = n2;
                        if destination_memory != 0 || source_memory != 0 {
                            return Err(WasmError::Decode("memory.copy memory indices must be 0"));
                        }
                        ops.push(Op::MemoryCopy);
                    }
                    11 => {
                        let (memory, ni) = leb_u32(body, i)?;
                        i = ni;
                        if memory != 0 {
                            return Err(WasmError::Decode("memory.fill memory index must be 0"));
                        }
                        ops.push(Op::MemoryFill);
                    }
                    12 => {
                        let (elem_index, n1) = leb_u32(body, i)?;
                        let (table, n2) = leb_u32(body, n1)?;
                        i = n2;
                        ops.push(Op::TableInit {
                            elem_index,
                            table_index: table,
                        });
                    }
                    13 => {
                        let (elem_index, ni) = leb_u32(body, i)?;
                        i = ni;
                        ops.push(Op::ElemDrop { elem_index });
                    }
                    14 => {
                        let (destination_table, n1) = leb_u32(body, i)?;
                        let (source_table, n2) = leb_u32(body, n1)?;
                        i = n2;
                        ops.push(Op::TableCopy {
                            destination_table,
                            source_table,
                        });
                    }
                    15 => {
                        let (table, ni) = leb_u32(body, i)?;
                        i = ni;
                        ops.push(Op::TableGrow(table));
                    }
                    16 => {
                        let (table, ni) = leb_u32(body, i)?;
                        i = ni;
                        ops.push(Op::TableSize(table));
                    }
                    17 => {
                        let (table, ni) = leb_u32(body, i)?;
                        i = ni;
                        ops.push(Op::TableFill(table));
                    }
                    _ => return Err(WasmError::Decode("unsupported 0xfc opcode")),
                }
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
            0x11 => {
                let (type_index, n1) = leb_u32(body, i)?;
                let (table_index, n2) = leb_u32(body, n1)?;
                i = n2;
                ops.push(Op::CallIndirect {
                    type_index,
                    table_index,
                });
            }
            0x12 => {
                let (function, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::ReturnCall(function));
            }
            0x13 => {
                let (type_index, n1) = leb_u32(body, i)?;
                let (table_index, n2) = leb_u32(body, n1)?;
                i = n2;
                ops.push(Op::ReturnCallIndirect {
                    type_index,
                    table_index,
                });
            }
            0x02 => {
                let (ty, ni) = block_type(body, i)?;
                i = ni;
                open.push(ops.len());
                ops.push(Op::Block { ty, end: 0 });
            }
            0x03 => {
                let (ty, ni) = block_type(body, i)?;
                i = ni;
                open.push(ops.len());
                ops.push(Op::Loop { ty, end: 0 });
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
                let count = count as usize;
                budget.charge(count)?;
                let target_start = branch_targets.len();
                reserve_exact(&mut branch_targets, count)?;
                for _ in 0..count {
                    let (t, ni) = leb_u32(body, cur)?;
                    cur = ni;
                    branch_targets.push(t);
                }
                let (default, ni) = leb_u32(body, cur)?;
                i = ni;
                ops.push(Op::BrTable {
                    target_start: target_start as u32,
                    target_len: count as u32,
                    default,
                });
            }
            0x04 => {
                let (ty, ni) = block_type(body, i)?;
                i = ni;
                open.push(ops.len());
                ops.push(Op::If {
                    ty,
                    else_pc: None,
                    end: 0,
                });
            }
            0x05 => {
                let else_idx = ops.len();
                let open_idx = *open
                    .last()
                    .ok_or(WasmError::Decode("else without matching if"))?;
                match &mut ops[open_idx] {
                    Op::If { else_pc, .. } => {
                        if else_pc.replace(else_idx).is_some() {
                            return Err(WasmError::Decode("duplicate else in if"));
                        }
                    }
                    _ => return Err(WasmError::Decode("else not inside an if")),
                }
                ops.push(Op::Else { end: 0 });
            }
            0x00 => ops.push(Op::Unreachable),
            0x01 => ops.push(Op::Nop),
            0x1A => ops.push(Op::Drop),
            0x1B => ops.push(Op::Select),
            0x1C => {
                let (count, ni) = leb_u32(body, i)?;
                if count != 1 {
                    return Err(WasmError::Decode("typed select requires one value type"));
                }
                let ty = *body
                    .get(ni)
                    .ok_or(WasmError::Decode("truncated typed select"))?;
                if !matches!(ty, 0x70 | 0x7C..=0x7F) {
                    return Err(WasmError::Decode("unsupported typed select value type"));
                }
                i = ni + 1;
                ops.push(Op::TypedSelect(ty));
            }
            0x22 => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::LocalTee(x));
            }
            0x0F => ops.push(Op::Return),
            0x25 => {
                let (table, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::TableGet(table));
            }
            0x26 => {
                let (table, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::TableSet(table));
            }
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
                } else if i != body.len() {
                    return Err(WasmError::Decode("instructions follow function end"));
                }
            }
            0x29 => {
                let (offset, ni) = memarg(body, i, 3)?;
                i = ni;
                ops.push(Op::I64Load { offset });
            }
            0x37 => {
                let (offset, ni) = memarg(body, i, 3)?;
                i = ni;
                ops.push(Op::I64Store { offset });
            }
            0x30 => {
                let (offset, ni) = memarg(body, i, 0)?;
                i = ni;
                ops.push(Op::I64Load8S { offset });
            }
            0x31 => {
                let (offset, ni) = memarg(body, i, 0)?;
                i = ni;
                ops.push(Op::I64Load8U { offset });
            }
            0x32 => {
                let (offset, ni) = memarg(body, i, 1)?;
                i = ni;
                ops.push(Op::I64Load16S { offset });
            }
            0x33 => {
                let (offset, ni) = memarg(body, i, 1)?;
                i = ni;
                ops.push(Op::I64Load16U { offset });
            }
            0x34 => {
                let (offset, ni) = memarg(body, i, 2)?;
                i = ni;
                ops.push(Op::I64Load32S { offset });
            }
            0x35 => {
                let (offset, ni) = memarg(body, i, 2)?;
                i = ni;
                ops.push(Op::I64Load32U { offset });
            }
            0x3C => {
                let (offset, ni) = memarg(body, i, 0)?;
                i = ni;
                ops.push(Op::I64Store8 { offset });
            }
            0x3D => {
                let (offset, ni) = memarg(body, i, 1)?;
                i = ni;
                ops.push(Op::I64Store16 { offset });
            }
            0x3E => {
                let (offset, ni) = memarg(body, i, 2)?;
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
                let bytes = le8(body
                    .get(i..i + 8)
                    .ok_or(WasmError::Decode("truncated f64.const immediate"))?);
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
                let (offset, ni) = memarg(body, i, 3)?;
                i = ni;
                ops.push(Op::F64Load { offset });
            }
            0x39 => {
                let (offset, ni) = memarg(body, i, 3)?;
                i = ni;
                ops.push(Op::F64Store { offset });
            }
            0x43 => {
                let end = i
                    .checked_add(4)
                    .filter(|&e| e <= body.len())
                    .ok_or(WasmError::Decode("truncated f32.const literal"))?;
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
                let (offset, ni) = memarg(body, i, 2)?;
                i = ni;
                ops.push(Op::F32Load { offset });
            }
            0x38 => {
                let (offset, ni) = memarg(body, i, 2)?;
                i = ni;
                ops.push(Op::F32Store { offset });
            }
            0x23 => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::GlobalGet(x));
            }
            0x24 => {
                let (x, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::GlobalSet(x));
            }
            0xA7 => ops.push(Op::I32WrapI64),
            0xA8 => ops.push(Op::I32TruncF32S),
            0xA9 => ops.push(Op::I32TruncF32U),
            0xAA => ops.push(Op::I32TruncF64S),
            0xAB => ops.push(Op::I32TruncF64U),
            0xAC => ops.push(Op::I64ExtendI32S),
            0xAD => ops.push(Op::I64ExtendI32U),
            0xAE => ops.push(Op::I64TruncF32S),
            0xAF => ops.push(Op::I64TruncF32U),
            0xB0 => ops.push(Op::I64TruncF64S),
            0xB1 => ops.push(Op::I64TruncF64U),
            0xB2 => ops.push(Op::F32ConvertI32S),
            0xB3 => ops.push(Op::F32ConvertI32U),
            0xB4 => ops.push(Op::F32ConvertI64S),
            0xB5 => ops.push(Op::F32ConvertI64U),
            0xB6 => ops.push(Op::F32DemoteF64),
            0xB7 => ops.push(Op::F64ConvertI32S),
            0xB8 => ops.push(Op::F64ConvertI32U),
            0xB9 => ops.push(Op::F64ConvertI64S),
            0xBA => ops.push(Op::F64ConvertI64U),
            0xBB => ops.push(Op::F64PromoteF32),
            0xBC => ops.push(Op::I32ReinterpretF32),
            0xBD => ops.push(Op::I64ReinterpretF64),
            0xBE => ops.push(Op::F32ReinterpretI32),
            0xBF => ops.push(Op::F64ReinterpretI64),
            0xC0 => ops.push(Op::I32Extend8S),
            0xC1 => ops.push(Op::I32Extend16S),
            0xC2 => ops.push(Op::I64Extend8S),
            0xC3 => ops.push(Op::I64Extend16S),
            0xC4 => ops.push(Op::I64Extend32S),
            0xD0 => {
                let reftype = *body
                    .get(i)
                    .ok_or(WasmError::Decode("truncated ref.null type"))?;
                i += 1;
                if reftype != 0x70 {
                    return Err(WasmError::Decode("only funcref ref.null is supported"));
                }
                ops.push(Op::RefNull);
            }
            0xD1 => ops.push(Op::RefIsNull),
            0xD2 => {
                let (function, ni) = leb_u32(body, i)?;
                i = ni;
                ops.push(Op::RefFunc(function));
            }
            _other => {
                return Err(WasmError::Decode("unsupported opcode 0x"));
            }
        }
    }
    if !open.is_empty() {
        return Err(WasmError::Decode("unterminated block or loop"));
    }
    Ok(DecodedCode {
        ops,
        branch_targets,
    })
}

/// Take the first 4 bytes of a slice known to hold at least 4. Written as
/// plain indexing rather than `try_into().expect(..)`: the latter needs
/// `Debug`, which links `core::fmt` into the otherwise fmt-free static core.
fn le4(s: &[u8]) -> [u8; 4] {
    [s[0], s[1], s[2], s[3]]
}

/// The 8-byte counterpart of [`le4`].
fn le8(s: &[u8]) -> [u8; 8] {
    [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]
}

/// Read `n` value-type bytes, bounds-checked, returning them and the next
/// offset. `call_indirect` compares these, so the bytes are kept, not skipped.
fn read_valtypes(
    p: &[u8],
    i: usize,
    n: u32,
    budget: &mut DecodeBudget,
) -> Result<(Vec<u8>, usize), WasmError> {
    let n = n as usize;
    budget.charge(n)?;
    let end = i
        .checked_add(n)
        .filter(|&e| e <= p.len())
        .ok_or(WasmError::Decode("value-type list runs past section"))?;
    let mut values = Vec::new();
    reserve_exact(&mut values, n)?;
    if p[i..end]
        .iter()
        .any(|value| !matches!(value, 0x70 | 0x7C..=0x7F))
    {
        return Err(WasmError::Decode("unsupported value type"));
    }
    values.extend_from_slice(&p[i..end]);
    Ok((values, end))
}

/// Read a UTF-8 name (LEB length + bytes), returning it and the next offset.
fn read_name(p: &[u8], i: usize) -> Result<(String, usize), WasmError> {
    let (len, ni) = leb_u32(p, i)?;
    let end = ni
        .checked_add(len as usize)
        .filter(|&e| e <= p.len())
        .ok_or(WasmError::Decode("name runs past section"))?;
    let decoded = core::str::from_utf8(&p[ni..end])
        .map_err(|_| WasmError::Decode("name is not valid UTF-8"))?;
    let mut s = String::new();
    s.try_reserve_exact(decoded.len())
        .map_err(|_| WasmError::Decode("module allocation"))?;
    s.push_str(decoded);
    Ok((s, end))
}

/// Parse every internally defined funcref table and its limits.
fn parse_table_section(
    p: &[u8],
    budget: &mut DecodeBudget,
) -> Result<Vec<(usize, Option<usize>)>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let count = count as usize;
    budget.charge(count)?;
    let mut tables = Vec::new();
    reserve_exact(&mut tables, count)?;
    for _ in 0..count {
        let reftype = *p.get(i).ok_or(WasmError::Decode("truncated table type"))?;
        if reftype != 0x70 {
            return Err(WasmError::Decode("unsupported reftype 0x"));
        }
        i += 1;
        let flag = *p
            .get(i)
            .ok_or(WasmError::Decode("truncated table limits"))?;
        i += 1;
        let (min, ni) = leb_u32(p, i)?;
        i = ni;
        let max = match flag {
            0x00 => None,
            0x01 => {
                let (max, ni) = leb_u32(p, i)?;
                i = ni;
                Some(max as usize)
            }
            _other => {
                return Err(WasmError::Decode("unsupported table limits flag 0x"));
            }
        };
        let min = min as usize;
        if max.is_some_and(|maximum| maximum < min) {
            return Err(WasmError::Decode("table limits out of range"));
        }
        tables.push((min, max));
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing table section bytes"));
    }
    Ok(tables)
}

/// Parse the element section into `(offset, func_indices)` per active segment.
/// Parse the memory section (id 5) into `(min_pages, max_pages)`. WASM 1.0
/// allows at most one memory per module.
fn parse_memory_section(p: &[u8]) -> Result<(usize, Option<usize>), WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    if count != 1 {
        return Err(WasmError::Decode("unsupported memory count"));
    }
    let flag = *p
        .get(i)
        .ok_or(WasmError::Decode("truncated memory limits"))?;
    i += 1;
    let (min, ni) = leb_u32(p, i)?;
    i = ni;
    let max = match flag {
        0x00 => None,
        0x01 => {
            let (m, ni) = leb_u32(p, i)?;
            i = ni;
            Some(m as usize)
        }
        _other => return Err(WasmError::Decode("unsupported memory limits flag 0x")),
    };
    let min = min as usize;
    if min > WASM_MAX_PAGES || max.is_some_and(|m| m > WASM_MAX_PAGES || m < min) {
        return Err(WasmError::Decode("memory limits out of range"));
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing memory section bytes"));
    }
    Ok((min, max))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DataMode {
    Active(usize),
    Passive,
}

struct DataSegment {
    mode: DataMode,
    bytes: Vec<u8>,
}

/// Parse active and passive data segments from section id 11.
fn parse_data_section(p: &[u8], budget: &mut DecodeBudget) -> Result<Vec<DataSegment>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        let (flag, ni) = leb_u32(p, i)?;
        i = ni;
        let mode = match flag {
            0 => {
                let (offset_val, ni) = parse_const_expr(p, i)?;
                i = ni;
                match offset_val {
                    Val::I32(v) => DataMode::Active(v as u32 as usize),
                    _other => return Err(WasmError::Decode("data offset must be i32, got")),
                }
            }
            1 => DataMode::Passive,
            2 => {
                let (memory, ni) = leb_u32(p, i)?;
                i = ni;
                if memory != 0 {
                    return Err(WasmError::Decode("data segment memory index must be 0"));
                }
                let (offset_val, ni) = parse_const_expr(p, i)?;
                i = ni;
                match offset_val {
                    Val::I32(v) => DataMode::Active(v as u32 as usize),
                    _other => return Err(WasmError::Decode("data offset must be i32, got")),
                }
            }
            _ => return Err(WasmError::Decode("unsupported data segment flag")),
        };
        let (len, ni) = leb_u32(p, i)?;
        i = ni;
        let end = i
            .checked_add(len as usize)
            .filter(|&e| e <= p.len())
            .ok_or(WasmError::Decode("data segment runs past section"))?;
        let mut bytes = Vec::new();
        reserve_exact(&mut bytes, len as usize)?;
        bytes.extend_from_slice(&p[i..end]);
        out.push(DataSegment { mode, bytes });
        i = end;
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing data section bytes"));
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElemMode {
    Active { table_index: usize, offset: usize },
    Passive,
    Declarative,
}

struct ElemSegment {
    mode: ElemMode,
    refs: Vec<Option<usize>>,
}

fn parse_elem_section(p: &[u8], budget: &mut DecodeBudget) -> Result<Vec<ElemSegment>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        let (flag, ni) = leb_u32(p, i)?;
        i = ni;
        let mode = match flag {
            0 => {
                let (offset_val, ni) = parse_const_expr(p, i)?;
                i = ni;
                match offset_val {
                    Val::I32(v) => ElemMode::Active {
                        table_index: 0,
                        offset: v as u32 as usize,
                    },
                    _other => return Err(WasmError::Decode("elem offset must be i32, got")),
                }
            }
            1 => {
                let kind = *p.get(i).ok_or(WasmError::Decode("truncated elem kind"))?;
                i += 1;
                if kind != 0 {
                    return Err(WasmError::Decode("element kind must be funcref"));
                }
                ElemMode::Passive
            }
            2 => {
                let (table, ni) = leb_u32(p, i)?;
                i = ni;
                let (offset_val, ni) = parse_const_expr(p, i)?;
                i = ni;
                let mode = match offset_val {
                    Val::I32(v) => ElemMode::Active {
                        table_index: table as usize,
                        offset: v as u32 as usize,
                    },
                    _other => return Err(WasmError::Decode("elem offset must be i32, got")),
                };
                let kind = *p.get(i).ok_or(WasmError::Decode("truncated elem kind"))?;
                i += 1;
                if kind != 0 {
                    return Err(WasmError::Decode("element kind must be funcref"));
                }
                mode
            }
            3 => {
                let kind = *p.get(i).ok_or(WasmError::Decode("truncated elem kind"))?;
                i += 1;
                if kind != 0 {
                    return Err(WasmError::Decode("element kind must be funcref"));
                }
                ElemMode::Declarative
            }
            4 => {
                let (offset_val, ni) = parse_const_expr(p, i)?;
                i = ni;
                match offset_val {
                    Val::I32(v) => ElemMode::Active {
                        table_index: 0,
                        offset: v as u32 as usize,
                    },
                    _other => return Err(WasmError::Decode("elem offset must be i32, got")),
                }
            }
            5 => {
                let reftype = *p
                    .get(i)
                    .ok_or(WasmError::Decode("truncated elem reftype"))?;
                i += 1;
                if reftype != 0x70 {
                    return Err(WasmError::Decode(
                        "only funcref elem segments are supported",
                    ));
                }
                ElemMode::Passive
            }
            6 => {
                let (table, ni) = leb_u32(p, i)?;
                i = ni;
                let (offset_val, ni) = parse_const_expr(p, i)?;
                i = ni;
                let mode = match offset_val {
                    Val::I32(v) => ElemMode::Active {
                        table_index: table as usize,
                        offset: v as u32 as usize,
                    },
                    _other => return Err(WasmError::Decode("elem offset must be i32, got")),
                };
                let reftype = *p
                    .get(i)
                    .ok_or(WasmError::Decode("truncated elem reftype"))?;
                i += 1;
                if reftype != 0x70 {
                    return Err(WasmError::Decode(
                        "only funcref elem segments are supported",
                    ));
                }
                mode
            }
            7 => {
                let reftype = *p
                    .get(i)
                    .ok_or(WasmError::Decode("truncated elem reftype"))?;
                i += 1;
                if reftype != 0x70 {
                    return Err(WasmError::Decode(
                        "only funcref elem segments are supported",
                    ));
                }
                ElemMode::Declarative
            }
            _ => return Err(WasmError::Decode("unsupported element segment flag")),
        };
        let (n_refs, ni) = leb_u32(p, i)?;
        i = ni;
        let n_refs = n_refs as usize;
        budget.charge(n_refs)?;
        let mut refs = Vec::new();
        reserve_exact(&mut refs, n_refs)?;
        for _ in 0..n_refs {
            if flag < 4 {
                let (function, next) = leb_u32(p, i)?;
                i = next;
                refs.push(Some(function as usize));
            } else {
                let (value, next) = parse_const_expr(p, i)?;
                i = next;
                match value {
                    Val::FuncRef(function) => refs.push(function),
                    _other => {
                        return Err(WasmError::Decode(
                            "funcref element expression has non-reference value",
                        ));
                    }
                }
            }
        }
        out.push(ElemSegment { mode, refs });
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing element section bytes"));
    }
    Ok(out)
}

/// Evaluate a constant init expression (`<t.const imm> end`) to a value.
fn parse_const_expr(p: &[u8], i: usize) -> Result<(Val, usize), WasmError> {
    let op = *p.get(i).ok_or(WasmError::Decode("truncated const expr"))?;
    let (val, mut j) = match op {
        0x41 => {
            let (v, ni) = leb_s32(p, i + 1)?;
            (Val::I32(v), ni)
        }
        0x42 => {
            let (v, ni) = leb_s64(p, i + 1)?;
            (Val::I64(v), ni)
        }
        0x43 => {
            let end = (i + 1)
                .checked_add(4)
                .filter(|&e| e <= p.len())
                .ok_or(WasmError::Decode("truncated f32 const expr"))?;
            let b = le4(&p[i + 1..end]);
            (Val::F32(f32::from_le_bytes(b)), end)
        }
        0x44 => {
            let end = (i + 1)
                .checked_add(8)
                .filter(|&e| e <= p.len())
                .ok_or(WasmError::Decode("truncated f64 const expr"))?;
            let b = le8(&p[i + 1..end]);
            (Val::F64(f64::from_le_bytes(b)), end)
        }
        0xD0 => {
            let reftype = *p
                .get(i + 1)
                .ok_or(WasmError::Decode("truncated ref.null const expr"))?;
            if reftype != 0x70 {
                return Err(WasmError::Decode("only funcref ref.null is supported"));
            }
            (Val::FuncRef(None), i + 2)
        }
        0xD2 => {
            let (function, ni) = leb_u32(p, i + 1)?;
            (Val::FuncRef(Some(function as usize)), ni)
        }
        _other => {
            return Err(WasmError::Decode("unsupported const-expr opcode 0x"));
        }
    };
    // Expect a closing `end` (0x0B).
    match p.get(j) {
        Some(0x0B) => {
            j += 1;
            Ok((val, j))
        }
        _ => Err(WasmError::Decode("const expr missing end")),
    }
}

/// Parse the global section into `(init_value, mutable)` per global.
fn parse_global_section(
    p: &[u8],
    budget: &mut DecodeBudget,
) -> Result<Vec<(Val, bool)>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        // globaltype = valtype byte + mutability byte
        let valtype = *p.get(i).ok_or(WasmError::Decode("truncated global type"))?;
        if !matches!(valtype, 0x70 | 0x7C..=0x7F) {
            return Err(WasmError::Decode("unsupported global value type"));
        }
        let mutability = *p
            .get(i + 1)
            .ok_or(WasmError::Decode("truncated global mutability"))?;
        let mutable = match mutability {
            0 => false,
            1 => true,
            _ => return Err(WasmError::Decode("invalid global mutability")),
        };
        i += 2;
        let (val, ni) = parse_const_expr(p, i)?;
        i = ni;
        if valtype_of(&val) != valtype {
            return Err(WasmError::Decode("global initializer type mismatch"));
        }
        out.push((val, mutable));
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing global section bytes"));
    }
    Ok(out)
}

struct ExportEntry {
    name: String,
    kind: u8,
    index: usize,
}

/// Parse every standard export kind. The embedding currently exposes function
/// lookup only, but validation must still own table/memory/global exports.
fn parse_export_section(
    p: &[u8],
    budget: &mut DecodeBudget,
) -> Result<Vec<ExportEntry>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        let (name, ni) = read_name(p, i)?;
        let kind = *p.get(ni).ok_or(WasmError::Decode("truncated export"))?;
        let (index, nj) = leb_u32(p, ni + 1)?;
        i = nj;
        if !matches!(kind, 0x00..=0x03) {
            return Err(WasmError::Decode("unsupported export kind"));
        }
        out.push(ExportEntry {
            name,
            kind,
            index: index as usize,
        });
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing export section bytes"));
    }
    Ok(out)
}

/// A function import: `(module, field)` name and its type arity.
///
/// This is the host door. Bind an i32-profile callback with
/// [`Module::bind_import`] or a standard typed callback with
/// [`Module::bind_import_typed_in_place`]; an unbound import traps if the guest
/// calls it.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, PartialEq, Eq)]
pub struct ImportDesc {
    pub module: String,
    pub field: String,
    pub n_params: usize,
    pub n_results: usize,
    /// Whether every parameter and result uses the game-safe i32 host ABI.
    pub i32_only: bool,
}

/// Parse the import section, returning each imported **function** in order.
/// Only function imports are supported here.
fn parse_import_section(
    p: &[u8],
    types: &[FuncType],
    budget: &mut DecodeBudget,
) -> Result<Vec<(ImportDesc, usize)>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        let (module, ni) = read_name(p, i)?;
        let (field, ni) = read_name(p, ni)?;
        i = ni;
        let kind = *p.get(i).ok_or(WasmError::Decode("truncated import"))?;
        i += 1;
        match kind {
            0x00 => {
                let (tidx, ni) = leb_u32(p, i)?;
                i = ni;
                let t = types.get(tidx as usize).ok_or(WasmError::Decode(
                    "imported function references missing type",
                ))?;
                out.push((
                    ImportDesc {
                        module,
                        field,
                        n_params: t.params.len(),
                        n_results: t.results.len(),
                        i32_only: t.params.iter().chain(&t.results).all(|&ty| ty == 0x7F),
                    },
                    tidx as usize,
                ));
            }
            _other => {
                return Err(WasmError::Decode("unsupported import kind 0x"));
            }
        }
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing import section bytes"));
    }
    Ok(out)
}

/// Parse the type section into `(n_params, n_results)` per function type.
fn parse_type_section(p: &[u8], budget: &mut DecodeBudget) -> Result<Vec<FuncType>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        let form = *p.get(i).ok_or(WasmError::Decode("truncated func type"))?;
        i += 1;
        if form != 0x60 {
            return Err(WasmError::Decode("unsupported type form 0x"));
        }
        let (n_params, ni) = leb_u32(p, i)?;
        let (params, ni) = read_valtypes(p, ni, n_params, budget)?;
        let (n_results, ni) = leb_u32(p, ni)?;
        let (results, ni) = read_valtypes(p, ni, n_results, budget)?;
        i = ni;
        out.push(FuncType { params, results });
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing type section bytes"));
    }
    Ok(out)
}

/// Parse the function section into a type index per function.
fn parse_func_section(p: &[u8], budget: &mut DecodeBudget) -> Result<Vec<usize>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        let (tidx, ni) = leb_u32(p, i)?;
        i = ni;
        out.push(tidx as usize);
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing function section bytes"));
    }
    Ok(out)
}

/// One code-section entry: the zero values of its declared locals, and the
/// instruction bytes of its body.
type CodeEntry = (Vec<Val>, Vec<u8>);

/// A native host implementation behind an import.
type HostImpl = dyn Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError>;
/// Typed compatibility host implementation for standard numeric/reference
/// function imports of arbitrary arity.
type TypedHostImpl = dyn Fn(&[Val], usize, &mut [u8]) -> Result<Vec<Val>, WasmError>;
/// Allocation-free bounded host implementation used by product embeddings.
/// The result slice has the function's exact declared result arity.
#[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
type BoundedHostImpl = dyn Fn(&[i32], &mut [i32], &mut [u8]) -> Result<(), WasmError>;
#[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
type TypedBoundedHostImpl = dyn Fn(&[Val], &mut [Val], &mut [u8]) -> Result<(), WasmError>;
const MAX_BOUNDED_HOST_ARITY: usize = 16;

enum HostBinding {
    TypedReturning(Rc<TypedHostImpl>),
    #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
    Bounded(Rc<BoundedHostImpl>),
    #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
    TypedBounded(Rc<TypedBoundedHostImpl>),
}

/// The value type byte of a runtime value.
fn valtype_of(v: &Val) -> u8 {
    match v {
        Val::I32(_) => 0x7F,
        Val::I64(_) => 0x7E,
        Val::F32(_) => 0x7D,
        Val::F64(_) => 0x7C,
        Val::FuncRef(_) => 0x70,
    }
}

fn values_have_types(values: &[Val], types: &[u8]) -> bool {
    if values.len() != types.len() {
        return false;
    }
    for (value, &value_type) in values.iter().zip(types) {
        if valtype_of(value) != value_type {
            return false;
        }
    }
    true
}

fn host_results_are_valid(values: &[Val], types: &[u8], function_count: usize) -> bool {
    values_have_types(values, types) && host_refs_are_valid(values, function_count)
}

fn host_refs_are_valid(values: &[Val], function_count: usize) -> bool {
    for value in values {
        if let Val::FuncRef(Some(index)) = value
            && *index >= function_count
        {
            return false;
        }
    }
    true
}

fn adapt_i32_host(callback: Rc<HostImpl>) -> Rc<TypedHostImpl> {
    Rc::new(move |args, n_results, memory| {
        let mut i32_args = Vec::new();
        i32_args
            .try_reserve_exact(args.len())
            .map_err(|_| WasmError::Trap("host arguments"))?;
        for value in args {
            if let Val::I32(number) = value {
                i32_args.push(*number);
            } else {
                return Err(WasmError::Trap("host function"));
            }
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(n_results)
            .map_err(|_| WasmError::Trap("host results"))?;
        let results = callback(&i32_args, memory)?;
        if results.len() != n_results {
            return Err(WasmError::Trap("host function"));
        }
        values.extend(results.into_iter().map(Val::I32));
        Ok(values)
    })
}

/// The zero value of a value type byte (spec 4.5.3: locals start at zero of
/// their declared type).
fn zero_of_valtype(vt: u8) -> Result<Val, WasmError> {
    match vt {
        0x7F => Ok(Val::I32(0)),
        0x7E => Ok(Val::I64(0)),
        0x7D => Ok(Val::F32(0.0)),
        0x7C => Ok(Val::F64(0.0)),
        0x70 => Ok(Val::FuncRef(None)),
        _other => Err(WasmError::Decode("unsupported local value type 0x")),
    }
}

/// Parse the code section into `(local_zero_values, expr_bytes)` per function.
fn parse_code_section(p: &[u8], budget: &mut DecodeBudget) -> Result<Vec<CodeEntry>, WasmError> {
    let (count, mut i) = leb_u32(p, 0)?;
    let mut out = Vec::new();
    let count = count as usize;
    budget.charge(count)?;
    for _ in 0..count {
        let (body_size, ni) = leb_u32(p, i)?;
        let bstart = ni;
        let bend = bstart
            .checked_add(body_size as usize)
            .filter(|&e| e <= p.len())
            .ok_or(WasmError::Decode("code entry runs past section"))?;
        let body = &p[bstart..bend];

        // locals: vec of (count, valtype)
        let (n_decls, mut j) = leb_u32(body, 0)?;
        let mut locals: Vec<Val> = Vec::new();
        for _ in 0..n_decls {
            let (n, nj) = leb_u32(body, j)?;
            j = nj;
            // one value-type byte follows
            let vt = *body
                .get(j)
                .ok_or(WasmError::Decode("truncated local declaration"))?;
            j += 1;
            let zero = zero_of_valtype(vt)?;
            budget.charge(n as usize)?;
            let total = locals
                .len()
                .checked_add(n as usize)
                .filter(|&t| t <= WASM_MAX_LOCALS)
                .ok_or(WasmError::Decode("local count overflow"))?;
            reserve_exact(&mut locals, n as usize)?;
            locals.resize(total, zero);
        }
        let mut expression = Vec::new();
        reserve_exact(&mut expression, body.len() - j)?;
        expression.extend_from_slice(&body[j..]);
        out.push((locals, expression));
        i = bend;
    }
    if i != p.len() {
        return Err(WasmError::Decode("trailing code section bytes"));
    }
    Ok(out)
}

/// A declared function type: the value-type bytes of its parameters and
/// results. `call_indirect` requires an exact match (spec 4.4.8), so the value
/// types are kept, not just their counts.
#[derive(Clone, PartialEq, Eq)]
struct FuncType {
    params: Vec<u8>,
    results: Vec<u8>,
}

/// A registered function: its param count, declared locals (as their typed
/// zero values), result arity, and decoded body.
#[derive(Clone)]
struct Func {
    n_params: usize,
    /// One entry per declared local, holding the zero value of its type.
    locals: Vec<Val>,
    arity: usize,
    code: Vec<Op>,
    /// Guest-sized `br_table` target lists live outside [`Op`] so decoded
    /// instructions stay cheap to copy during execution.
    branch_targets: Vec<u32>,
    /// Index into [`Module::types`] for functions that came from a module's
    /// type section; `None` for functions registered through
    /// [`Module::add_function`], which carry counts only.
    sig: Option<usize>,
}

/// A structured-control frame on the control stack.
#[derive(Clone, Copy)]
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
    /// Index into [`Module::types`] when the host function stands in for an
    /// imported function declared in the module's type section.
    sig: Option<usize>,
    binding: HostBinding,
}

/// A collection of function bodies that can call one another (and registered
/// host functions) by index.
///
/// The function index space places **host/imported functions first** (indices
/// `0..hosts.len()`), then WASM-defined functions. So when there are no host
/// functions, a defined function's index equals its position — matching the
/// pre-host behaviour.
/// A module global: its initial value and whether `global.set` may write it.
#[derive(Clone, Copy)]
struct GlobalDesc {
    init: Val,
    mutable: bool,
}

/// One internally defined funcref table template and its declared maximum.
struct TableDesc {
    elements: Vec<Option<usize>>,
    max: Option<usize>,
}

type TableState = (Vec<Vec<Option<usize>>>, Vec<bool>);

#[derive(Default)]
pub struct Module {
    hosts: Vec<HostFunc>,
    funcs: Vec<Func>,
    /// Exported function names -> combined function index (from a module's
    /// export section, or registered via [`Module::export`]).
    exports: BTreeMap<String, usize>,
    /// Function exports in declaration order (first one is [`Module::eval`]'s
    /// preferred entry).
    export_list: Vec<(String, usize)>,
    /// Imported functions in index order (the host door).
    import_descs: Vec<ImportDesc>,
    /// Global initializers and mutability. A working copy is created for each
    /// fresh convenience call or persistent [`Instance`].
    globals: Vec<GlobalDesc>,
    /// Optional start function (run by [`Module::run_start`]).
    start: Option<usize>,
    /// Funcref table templates. Live instances own independent element arrays;
    /// declared maxima remain immutable module metadata.
    tables: Vec<TableDesc>,
    /// Standard active/passive/declarative funcref element segments.
    elems: Vec<ElemSegment>,
    /// The module's function types as `(n_params, n_results)`, so
    /// `call_indirect` can type-check the callee against a declared type.
    types: Vec<FuncType>,
    /// Initial memory pages. Parsed standard modules without a memory section
    /// store `Some(0)` and reject memory instructions at validation; `None` is
    /// retained only for programmatic [`Module::new`] compatibility and gives
    /// that builder one implicit page. `None` max means "up to
    /// [`WASM_MAX_PAGES`]".
    mem_min_pages: Option<usize>,
    mem_max_pages: Option<usize>,
    /// Standard active/passive data segments. Their bytes belong to the module;
    /// dropped/live state belongs to each instance.
    data: Vec<DataSegment>,
    /// Host budget retained for instantiation and every top-level call.
    limits: Limits,
}

/// One live WebAssembly instance.
///
/// Unlike the convenience methods on [`Module`], an `Instance` retains linear
/// memory and mutable globals across exported calls. Its start function runs
/// exactly once during [`Module::instantiate`].
pub struct Instance {
    module: Module,
    memory: Vec<u8>,
    globals: Vec<Val>,
    data_live: Vec<bool>,
    tables: Vec<Vec<Option<usize>>>,
    elem_live: Vec<bool>,
    last_steps: u64,
    last_peak_call_depth: usize,
    last_peak_activation_slots: usize,
}

/// Mutable store owned by one evaluation or persistent instance. Keeping the
/// store explicit prevents module definitions and sibling instances from being
/// mutated by segment/table instructions.
struct BulkState<'a> {
    data_live: &'a mut [bool],
    tables: &'a mut [Vec<Option<usize>>],
    elem_live: &'a mut [bool],
}

struct WasmCall<'a> {
    index: usize,
    args: &'a [Val],
}

#[derive(Default)]
struct CallResourceStats {
    peak_call_depth: usize,
    peak_activation_slots: usize,
}

struct CallContext<'a> {
    base_depth: usize,
    stats: &'a mut CallResourceStats,
}

struct ActivationResources<'a> {
    available_slots: usize,
    suspended_slots: usize,
    call_depth: usize,
    stats: &'a mut CallResourceStats,
}

impl CallResourceStats {
    fn observe(&mut self, depth: usize, slots: usize) {
        self.peak_call_depth = self.peak_call_depth.max(depth);
        self.peak_activation_slots = self.peak_activation_slots.max(slots);
    }
}

enum DefinedOutcome {
    Values(CallValues),
    Call {
        index: usize,
        args: Vec<Val>,
        caller: DefinedActivation,
    },
    TailCall {
        index: usize,
        args: Vec<Val>,
    },
}

enum CallValues {
    Owned(Vec<Val>),
    #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
    BoundedI32 {
        values: [i32; MAX_BOUNDED_HOST_ARITY],
        len: usize,
    },
    #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
    BoundedTyped {
        values: [Val; MAX_BOUNDED_HOST_ARITY],
        len: usize,
    },
}

impl CallValues {
    fn len(&self) -> usize {
        match self {
            Self::Owned(values) => values.len(),
            #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
            Self::BoundedI32 { len, .. } | Self::BoundedTyped { len, .. } => *len,
        }
    }

    fn append_to(self, destination: &mut Vec<Val>) {
        match self {
            Self::Owned(values) => destination.extend(values),
            #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
            Self::BoundedI32 { values, len } => {
                destination.extend(values[..len].iter().copied().map(Val::I32));
            }
            #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
            Self::BoundedTyped { values, len } => {
                destination.extend_from_slice(&values[..len]);
            }
        }
    }

    fn into_vec(self) -> Result<Vec<Val>, WasmError> {
        match self {
            Self::Owned(values) => Ok(values),
            #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
            Self::BoundedI32 { values, len } => {
                let mut owned = Vec::new();
                owned
                    .try_reserve_exact(len)
                    .map_err(|_| WasmError::Trap("host results"))?;
                owned.extend(values[..len].iter().copied().map(Val::I32));
                Ok(owned)
            }
            #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
            Self::BoundedTyped { values, len } => {
                let mut owned = Vec::new();
                owned
                    .try_reserve_exact(len)
                    .map_err(|_| WasmError::Trap("host results"))?;
                owned.extend_from_slice(&values[..len]);
                Ok(owned)
            }
        }
    }
}

/// One guest-defined function activation. Guest calls are represented by a
/// bounded vector of these values; they never recurse through the Rust/native
/// call stack.
struct DefinedActivation {
    def_idx: usize,
    locals: Vec<Val>,
    stack: Vec<Val>,
    control: Vec<Frame>,
    pc: usize,
}

impl DefinedActivation {
    fn live_slots(&self) -> Result<usize, WasmError> {
        self.locals
            .len()
            .checked_add(self.stack.len())
            .and_then(|slots| slots.checked_add(self.control.len()))
            .ok_or(WasmError::Trap("call stack"))
    }
}

impl Module {
    /// An empty module.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty programmatic module under an explicit host budget.
    pub fn new_with_limits(limits: Limits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
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
        let mut budget = DecodeBudget::new();
        budget.has_memory = true;
        let DecodedCode {
            ops: code,
            branch_targets,
        } = decode(body, &mut budget)?;
        let combined = self.hosts.len() + self.funcs.len();
        self.funcs.push(Func {
            n_params,
            locals: vec![Val::I32(0); n_locals],
            arity: result_arity,
            code,
            branch_targets,
            sig: None,
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
        let callback: Rc<HostImpl> = Rc::new(f);
        self.hosts.push(HostFunc {
            n_params,
            n_results,
            sig: None,
            binding: HostBinding::TypedReturning(adapt_i32_host(callback)),
        });
        idx
    }

    /// Load a standard `.wasm` **module** (magic `\0asm` + version 1).
    ///
    /// Sections read: type (1), import (2, function imports), function (3),
    /// table (4), memory (5), global (6), export (7), start (8), element (9),
    /// code (10), and data (11). Custom sections are skipped.
    ///
    /// A parsed module without a memory section has no linear memory and any
    /// memory instruction fails load-time validation. A module that declares
    /// one gets exactly its declared minimum, with data segments applied at
    /// each invocation and `memory.grow` bounded by its declared maximum.
    ///
    /// Function bodies are decoded with the same instruction subset as
    /// [`Module::add_function`]; result/param counts come from the type section
    /// and locals (with their declared value types) from each code entry.
    pub fn from_bytes(wasm: &[u8]) -> Result<Module, WasmError> {
        Self::from_bytes_with(wasm, Limits::default())
    }

    /// Load a module under an explicit host budget. Table `min` is checked
    /// against [`Limits::max_table_elems`] before any table allocation.
    pub fn from_bytes_with(wasm: &[u8], limits: Limits) -> Result<Module, WasmError> {
        if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
            return Err(WasmError::Decode("not a wasm module (bad magic)"));
        }
        if wasm[4..8] != [0x01, 0x00, 0x00, 0x00] {
            return Err(WasmError::Decode("unsupported wasm version (expected 1)"));
        }

        let mut types: Vec<FuncType> = Vec::new();
        let mut imports: Vec<(ImportDesc, usize)> = Vec::new();
        let mut func_types: Vec<usize> = Vec::new();
        let mut codes: Vec<CodeEntry> = Vec::new();
        let mut exports: Vec<ExportEntry> = Vec::new();
        let mut globals: Vec<(Val, bool)> = Vec::new();
        let mut start_fn: Option<usize> = None;
        let mut table_limits: Vec<(usize, Option<usize>)> = Vec::new();
        let mut elems: Vec<ElemSegment> = Vec::new();
        let mut mem_limits: Option<(usize, Option<usize>)> = None;
        let mut data: Vec<DataSegment> = Vec::new();
        let mut budget = DecodeBudget::new();
        let mut last_standard_section_rank = 0u8;
        let mut data_count: Option<usize> = None;

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
                .ok_or(WasmError::Decode("section runs past end of module"))?;
            let payload = &wasm[start..end];
            if id != 0 {
                // DataCount (id 12) is ordered between element (9) and code
                // (10), so numeric section ids are not an ordering relation.
                let rank = match id {
                    1..=9 => id,
                    12 => 10,
                    10 => 11,
                    11 => 12,
                    _ => return Err(WasmError::Decode("unsupported section id")),
                };
                if rank <= last_standard_section_rank {
                    return Err(WasmError::Decode("duplicate or out-of-order section"));
                }
                last_standard_section_rank = rank;
            }
            match id {
                0 => {}
                1 => types = parse_type_section(payload, &mut budget)?,
                2 => imports = parse_import_section(payload, &types, &mut budget)?,
                3 => func_types = parse_func_section(payload, &mut budget)?,
                4 => table_limits = parse_table_section(payload, &mut budget)?,
                5 => mem_limits = Some(parse_memory_section(payload)?),
                6 => globals = parse_global_section(payload, &mut budget)?,
                7 => exports = parse_export_section(payload, &mut budget)?,
                8 => {
                    let (function, consumed) = leb_u32(payload, 0)?;
                    if consumed != payload.len() {
                        return Err(WasmError::Decode("trailing start section bytes"));
                    }
                    start_fn = Some(function as usize);
                }
                9 => elems = parse_elem_section(payload, &mut budget)?,
                12 => {
                    let (count, consumed) = leb_u32(payload, 0)?;
                    if consumed != payload.len() {
                        return Err(WasmError::Decode("trailing data count section bytes"));
                    }
                    data_count = Some(count as usize);
                }
                10 => codes = parse_code_section(payload, &mut budget)?,
                11 => data = parse_data_section(payload, &mut budget)?,
                _ => unreachable!("standard section id checked above"),
            }
            i = end;
        }

        if func_types.len() != codes.len() {
            return Err(WasmError::Decode("function count"));
        }
        if data_count.is_some_and(|count| count != data.len()) {
            return Err(WasmError::Decode("data count does not match data section"));
        }
        let table_count = table_limits.len();

        let mut module = Module::new_with_limits(limits);
        // Imported functions occupy the low indices; without a bound host
        // implementation they trap loudly if actually called.
        for (desc, tidx) in imports {
            let callback: Rc<TypedHostImpl> = Rc::new(|_args, _n_results, _memory| {
                Err(WasmError::Trap("call to unbound imported function"))
            });
            module.hosts.push(HostFunc {
                n_params: desc.n_params,
                n_results: desc.n_results,
                sig: Some(tidx),
                binding: HostBinding::TypedReturning(callback),
            });
            module.import_descs.push(desc);
        }
        budget.has_memory = mem_limits.is_some();
        for (tidx, (locals, expr)) in func_types.into_iter().zip(codes) {
            let ft = types.get(tidx).ok_or(WasmError::Decode("function"))?;
            let (n_params, n_results) = (ft.params.len(), ft.results.len());
            let DecodedCode {
                ops: code,
                branch_targets,
            } = decode(&expr, &mut budget)?;
            module.funcs.push(Func {
                n_params,
                locals,
                arity: n_results,
                code,
                branch_targets,
                sig: Some(tidx),
            });
        }
        for (init, mutable) in globals {
            module.globals.push(GlobalDesc { init, mutable });
        }
        // Record declared function types before validating indices/signatures
        // used by the export and start sections.
        module.types = types;
        let function_count = module.hosts.len() + module.funcs.len();
        let memory_count = usize::from(mem_limits.is_some());
        let mut export_names = BTreeMap::new();
        for export in exports {
            if export_names.insert(export.name.clone(), ()).is_some() {
                return Err(WasmError::Decode("duplicate export name"));
            }
            let bound = match export.kind {
                0x00 => function_count,
                0x01 => table_count,
                0x02 => memory_count,
                0x03 => module.globals.len(),
                _ => unreachable!("export kind checked while parsing"),
            };
            if export.index >= bound {
                return Err(WasmError::Decode("export index out of bounds"));
            }
            if export.kind == 0x00 {
                module.exports.insert(export.name.clone(), export.index);
                module.export_list.push((export.name, export.index));
            }
        }
        if let Some(start) = start_fn {
            let signature = if start < module.hosts.len() {
                module.hosts[start].sig
            } else {
                module
                    .funcs
                    .get(start - module.hosts.len())
                    .and_then(|function| function.sig)
            }
            .and_then(|index| module.types.get(index))
            .ok_or(WasmError::Decode("start function index out of bounds"))?;
            if !signature.params.is_empty() || !signature.results.is_empty() {
                return Err(WasmError::Decode("start function must have type [] -> []"));
            }
        }
        module.start = start_fn;
        // Memory limits and data segments (both were previously skipped, which
        // left every module on a hardcoded single zeroed page).
        let (mem_min, mem_max) = mem_limits.unwrap_or((0, None));
        module.mem_min_pages = Some(mem_min);
        module.mem_max_pages = mem_max;
        if module.initial_mem_pages() > limits.max_memory_pages.min(WASM_MAX_PAGES) {
            return Err(WasmError::Trap("memory size"));
        }
        let mem_bytes = module.initial_mem_pages() * WASM_PAGE_SIZE;
        for segment in &data {
            if let DataMode::Active(offset) = segment.mode {
                offset
                    .checked_add(segment.bytes.len())
                    .filter(|&e| memory_count != 0 && e <= mem_bytes)
                    .ok_or(WasmError::Decode("data segment runs past memory bounds"))?;
            }
        }
        module.data = data;
        let mut declared_refs = Vec::new();
        declared_refs
            .try_reserve(function_count)
            .map_err(|_| WasmError::Decode("module allocation"))?;
        declared_refs.resize(function_count, false);
        for segment in &elems {
            for &function in segment.refs.iter().flatten() {
                let declared = declared_refs
                    .get_mut(function)
                    .ok_or(WasmError::Decode("element function index out of bounds"))?;
                *declared = true;
            }
        }
        for global in &module.globals {
            if let Val::FuncRef(Some(function)) = global.init
                && !declared_refs.get(function).copied().unwrap_or(false)
            {
                return Err(WasmError::Decode(
                    "global initializer has undeclared ref.func",
                ));
            }
        }
        // --- load gate: prove every body before this Module is handed out ---
        // A module that fails validation is a Decode error here; it never
        // becomes something the caller can invoke, and no invalid program is
        // left for an execution-time Trap to catch.
        {
            let mut func_sigs: Vec<usize> = Vec::new();
            for h in &module.hosts {
                func_sigs.push(h.sig.unwrap_or(0));
            }
            for f in &module.funcs {
                func_sigs.push(f.sig.unwrap_or(0));
            }
            let mut global_types = Vec::new();
            global_types.extend(module.globals.iter().map(|g| valtype_of(&g.init)));
            let ctx = validate::ModuleCtx {
                types: &module.types,
                func_sigs: &func_sigs,
                globals: &global_types,
                data_count,
                elem_count: elems.len(),
                table_count,
                declared_refs: &declared_refs,
            };
            for f in &module.funcs {
                let ft = f
                    .sig
                    .and_then(|s| module.types.get(s))
                    .ok_or(WasmError::Decode("function has no declared type"))?;
                let mut locals = Vec::new();
                locals.extend_from_slice(&ft.params);
                locals.extend(f.locals.iter().map(valtype_of));
                validate::validate_body(&ctx, &locals, &ft.results, &f.code, &f.branch_targets)?;
            }
        }

        // Allocate every table, then apply active element segments. The host
        // budget caps their aggregate live element count, not each table in
        // isolation, so many small guest tables cannot amplify allocation.
        let total_table_size = table_limits.iter().try_fold(0usize, |total, (min, _)| {
            total.checked_add(*min).ok_or(WasmError::Trap("table size"))
        })?;
        if total_table_size > limits.max_table_elems {
            return Err(WasmError::Trap("table size"));
        }
        module
            .tables
            .try_reserve_exact(table_limits.len())
            .map_err(|_| WasmError::Trap("table size"))?;
        for (table_size, table_max) in table_limits {
            let mut elements = Vec::new();
            elements
                .try_reserve_exact(table_size)
                .map_err(|_| WasmError::Trap("table size"))?;
            elements.resize(table_size, None);
            module.tables.push(TableDesc {
                elements,
                max: table_max,
            });
        }
        let function_count = module.hosts.len() + module.funcs.len();
        for segment in &elems {
            if segment
                .refs
                .iter()
                .flatten()
                .any(|&index| index >= function_count)
            {
                return Err(WasmError::Decode("element function index out of bounds"));
            }
            if let ElemMode::Active {
                table_index,
                offset,
            } = segment.mode
            {
                let table = module
                    .tables
                    .get_mut(table_index)
                    .ok_or(WasmError::Decode("active element segment table index"))?;
                let end = offset
                    .checked_add(segment.refs.len())
                    .filter(|&end| end <= table.elements.len())
                    .ok_or(WasmError::Decode("elem segment runs past table bounds"))?;
                for (slot, &function) in table.elements[offset..end]
                    .iter_mut()
                    .zip(segment.refs.iter())
                {
                    *slot = function;
                }
            }
        }
        module.elems = elems;
        Ok(module)
    }

    /// Register a module global with an initial value and mutability, returning
    /// its index. A working copy is created for each fresh convenience call or
    /// persistent [`Instance`].
    pub fn add_global(&mut self, init: Val, mutable: bool) -> usize {
        let idx = self.globals.len();
        self.globals.push(GlobalDesc { init, mutable });
        idx
    }

    /// Allocate a funcref table of `size` uninitialised slots (for tests /
    /// programmatic module construction). This compatibility helper replaces
    /// table zero; [`Module::add_funcref_table`] appends additional tables.
    pub fn add_table(&mut self, size: usize) {
        let table = TableDesc {
            elements: vec![None; size],
            max: None,
        };
        if self.tables.is_empty() {
            self.tables.push(table);
        } else {
            self.tables[0] = table;
        }
    }

    /// Safely append an internally defined funcref table and return its index.
    pub fn add_funcref_table(
        &mut self,
        size: usize,
        max: Option<usize>,
    ) -> Result<usize, WasmError> {
        if max.is_some_and(|maximum| maximum < size) {
            return Err(WasmError::Decode("table limits out of range"));
        }
        let current = self.tables.iter().try_fold(0usize, |total, table| {
            total
                .checked_add(table.elements.len())
                .ok_or(WasmError::Trap("table size"))
        })?;
        if current
            .checked_add(size)
            .filter(|&total| total <= self.limits.max_table_elems)
            .is_none()
        {
            return Err(WasmError::Trap("table size"));
        }
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(size)
            .map_err(|_| WasmError::Trap("table size"))?;
        elements.resize(size, None);
        self.tables
            .try_reserve(1)
            .map_err(|_| WasmError::Trap("table size"))?;
        let index = self.tables.len();
        self.tables.push(TableDesc { elements, max });
        Ok(index)
    }

    /// Set table `slot` to point at combined function index `func_index`.
    pub fn set_table_entry(&mut self, slot: usize, func_index: usize) {
        if let Some(cell) = self
            .tables
            .get_mut(0)
            .and_then(|table| table.elements.get_mut(slot))
        {
            *cell = Some(func_index);
        }
    }

    /// Register a function type `(n_params, n_results)` for `call_indirect`
    /// type-checking, returning its type index.
    pub fn add_type(&mut self, n_params: usize, n_results: usize) -> usize {
        let idx = self.types.len();
        self.types.push(FuncType {
            params: vec![0x7F; n_params],
            results: vec![0x7F; n_results],
        });
        idx
    }

    /// Set the module's start function (run by [`Module::run_start`]).
    pub fn set_start(&mut self, func_index: usize) {
        self.start = Some(func_index);
    }

    /// The start function's index, if the module has one.
    pub fn start_index(&self) -> Option<usize> {
        self.start
    }

    /// Run the start function (no args, no results), if present. This is how a
    /// module instantiates: the start function runs once against a fresh
    /// memory/globals instance.
    pub fn run_start(&self) -> Result<(), WasmError> {
        if let Some(idx) = self.start {
            self.invoke_val(idx, &[])?;
        }
        Ok(())
    }

    /// Consume this decoded module and create one persistent instance.
    ///
    /// Active data segments and initial globals are applied once, then the
    /// module start function runs once. Later calls through the returned
    /// [`Instance`] preserve its memory and mutable globals.
    pub fn instantiate(self) -> Result<Instance, WasmError> {
        Instance::new(self)
    }

    /// Record `name` as an exported function index (for [`Module::invoke_by_name`]).
    pub fn export(&mut self, name: &str, func_index: usize) {
        self.exports.insert(name.to_owned(), func_index);
        self.export_list.push((name.to_owned(), func_index));
    }

    /// Function imports in module-index order. Bind each with [`Module::bind_import`].
    pub fn imports(&self) -> &[ImportDesc] {
        &self.import_descs
    }

    /// Exact standard type of one imported function parameter.
    pub fn import_parameter_type(
        &self,
        import_position: usize,
        parameter_position: usize,
    ) -> Option<ValueType> {
        let signature = self.hosts.get(import_position)?.sig?;
        let value = *self.types.get(signature)?.params.get(parameter_position)?;
        ValueType::from_byte(value)
    }

    /// Exact standard type of one imported function result.
    pub fn import_result_type(
        &self,
        import_position: usize,
        result_position: usize,
    ) -> Option<ValueType> {
        let signature = self.hosts.get(import_position)?.sig?;
        let value = *self.types.get(signature)?.results.get(result_position)?;
        ValueType::from_byte(value)
    }

    /// Bind a host callback to imported `module.field`. Replaces the unbound
    /// stub installed by [`Module::from_bytes`].
    pub fn bind_import<F>(&mut self, module: &str, field: &str, f: F) -> Result<(), WasmError>
    where
        F: Fn(&[i32], &mut [u8]) -> Result<Vec<i32>, WasmError> + 'static,
    {
        let mut found = false;
        for desc in &self.import_descs {
            if desc.module == module && desc.field == field {
                found = true;
                if !desc.i32_only {
                    return Err(WasmError::Trap("host ABI is i32-only; import declares"));
                }
            }
        }
        if !found {
            return Err(WasmError::Trap("no imported function named"));
        }
        // A module may import the same (module, field) pair more than once;
        // every matching slot binds to the same implementation, so a bind that
        // reports success never leaves a sibling slot silently unbound.
        let shared = adapt_i32_host(Rc::new(f));
        let mut bound = 0usize;
        for (pos, desc) in self.import_descs.iter().enumerate() {
            if desc.module == module && desc.field == field {
                self.hosts[pos].binding = HostBinding::TypedReturning(shared.clone());
                bound += 1;
            }
        }
        debug_assert!(bound != 0);
        Ok(())
    }

    /// Bind a standard typed host callback to every imported `module.field`.
    ///
    /// Unlike the legacy i32 convenience door, this form preserves i32, i64,
    /// f32, f64 and funcref values. The runtime verifies exact parameter and
    /// result types around the callback. The callback-returned vector is an
    /// explicit arbitrary-arity compatibility path; latency-sensitive hosts
    /// should prefer [`Module::bind_import_typed_in_place`].
    pub fn bind_import_typed<F>(&mut self, module: &str, field: &str, f: F) -> Result<(), WasmError>
    where
        F: Fn(&[Val], &mut [u8]) -> Result<Vec<Val>, WasmError> + 'static,
    {
        let shared: Rc<TypedHostImpl> = Rc::new(move |args, _n_results, memory| f(args, memory));
        let mut bound = 0usize;
        for (position, desc) in self.import_descs.iter().enumerate() {
            if desc.module == module && desc.field == field {
                self.hosts[position].binding = HostBinding::TypedReturning(shared.clone());
                bound += 1;
            }
        }
        if bound == 0 {
            return Err(WasmError::Trap("no imported function named"));
        }
        Ok(())
    }

    /// Bind a typed host callback using exact borrowed parameter/result
    /// slices backed by the VM's fixed 16-value staging storage.
    ///
    /// The complete result destination is available before app code runs and
    /// its types are checked afterwards. Imports above the bounded arity remain
    /// valid standard Wasm and can use [`Module::bind_import_typed`] instead.
    #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
    pub fn bind_import_typed_in_place<F>(
        &mut self,
        module: &str,
        field: &str,
        f: F,
    ) -> Result<(), WasmError>
    where
        F: Fn(&[Val], &mut [Val], &mut [u8]) -> Result<(), WasmError> + 'static,
    {
        let mut found = false;
        for (position, desc) in self.import_descs.iter().enumerate() {
            if desc.module == module && desc.field == field {
                found = true;
                let host = &self.hosts[position];
                if host.n_params > MAX_BOUNDED_HOST_ARITY || host.n_results > MAX_BOUNDED_HOST_ARITY
                {
                    return Err(WasmError::Trap("bounded host arity"));
                }
            }
        }
        if !found {
            return Err(WasmError::Trap("no imported function named"));
        }
        let shared: Rc<TypedBoundedHostImpl> = Rc::new(f);
        for (position, desc) in self.import_descs.iter().enumerate() {
            if desc.module == module && desc.field == field {
                self.hosts[position].binding = HostBinding::TypedBounded(shared.clone());
            }
        }
        Ok(())
    }

    /// Bind one exact import slot to a bounded in-place host implementation.
    ///
    /// Game embeddings validate unique imports before calling this internal
    /// door. Parameters use a fixed stack buffer and `results` is the exact
    /// declared output slice, so the callback cannot force an infallible
    /// per-dispatch `Vec` allocation.
    #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
    pub(crate) fn bind_import_at_bounded<F>(
        &mut self,
        position: usize,
        f: F,
    ) -> Result<(), WasmError>
    where
        F: Fn(&[i32], &mut [i32], &mut [u8]) -> Result<(), WasmError> + 'static,
    {
        let host = self
            .hosts
            .get_mut(position)
            .ok_or(WasmError::Trap("host import position"))?;
        if host.n_params > MAX_BOUNDED_HOST_ARITY || host.n_results > MAX_BOUNDED_HOST_ARITY {
            return Err(WasmError::Trap("bounded host arity"));
        }
        host.binding = HostBinding::Bounded(Rc::new(f));
        Ok(())
    }

    /// The isolated static-core feature measures the generic interpreter, not
    /// the product embedding. Keep the same source API for compilation while
    /// adapting bounded callbacks through the already fallible legacy form.
    #[cfg(all(feature = "staticcore", not(feature = "std")))]
    pub(crate) fn bind_import_at_bounded<F>(
        &mut self,
        position: usize,
        f: F,
    ) -> Result<(), WasmError>
    where
        F: Fn(&[i32], &mut [i32], &mut [u8]) -> Result<(), WasmError> + 'static,
    {
        let host = self
            .hosts
            .get_mut(position)
            .ok_or(WasmError::Trap("host import position"))?;
        if host.n_params > MAX_BOUNDED_HOST_ARITY || host.n_results > MAX_BOUNDED_HOST_ARITY {
            return Err(WasmError::Trap("bounded host arity"));
        }
        let n_results = host.n_results;
        let callback: Rc<HostImpl> = Rc::new(move |args, memory| {
            let mut results = Vec::new();
            results
                .try_reserve_exact(n_results)
                .map_err(|_| WasmError::Trap("host results"))?;
            results.resize(n_results, 0);
            f(args, &mut results, memory)?;
            Ok(results)
        });
        host.binding = HostBinding::TypedReturning(adapt_i32_host(callback));
        Ok(())
    }

    /// Run start (if any), then the first declared function export, else the
    /// first defined function. This is the crate's one-face evaluator once a
    /// module is loaded.
    pub fn eval(&self, args: &[Val]) -> Result<Vec<Val>, WasmError> {
        // One instance: the start function and the entry point share the same
        // linear memory and globals, so start-time host writes are visible.
        let mut steps: u64 = 0;
        let mut mem = self.new_memory()?;
        let mut globals = self.new_globals()?;
        let mut data_live = self.new_data_state()?;
        let (mut tables, mut elem_live) = self.new_table_state()?;
        let mut bulk = BulkState {
            data_live: &mut data_live,
            tables: &mut tables,
            elem_live: &mut elem_live,
        };
        let mut resources = CallResourceStats::default();
        if let Some(start) = self.start {
            let mut call_context = CallContext {
                base_depth: 0,
                stats: &mut resources,
            };
            self.call_any(
                WasmCall {
                    index: start,
                    args: &[],
                },
                &mut steps,
                &mut mem,
                &mut globals,
                &mut bulk,
                &mut call_context,
            )?;
        }
        let entry = match self.export_list.first() {
            Some((_, idx)) => *idx,
            // No export: a start-only module is already done; otherwise the
            // first defined function is the entry.
            None if self.start.is_some() => return Ok(Vec::new()),
            None if !self.funcs.is_empty() => self.hosts.len(),
            None => return Ok(Vec::new()),
        };
        let mut call_context = CallContext {
            base_depth: 0,
            stats: &mut resources,
        };
        self.call_any(
            WasmCall { index: entry, args },
            &mut steps,
            &mut mem,
            &mut globals,
            &mut bulk,
            &mut call_context,
        )
    }

    /// Resolve an exported function name to its index, if present.
    pub fn export_index(&self, name: &str) -> Option<usize> {
        self.exports.get(name).copied()
    }

    /// Return `(parameter_count, result_count)` when an exported function's
    /// complete signature uses the portable i32 host ABI.
    pub fn export_i32_arity(&self, name: &str) -> Option<(usize, usize)> {
        let idx = self.export_index(name)?;
        let (params, results, signature) = if idx < self.hosts.len() {
            let host = &self.hosts[idx];
            (host.n_params, host.n_results, host.sig)
        } else {
            let function = self.funcs.get(idx.checked_sub(self.hosts.len())?)?;
            (function.n_params, function.arity, function.sig)
        };
        if let Some(function_type) = signature.and_then(|index| self.types.get(index))
            && (function_type.params.iter().any(|&ty| ty != 0x7f)
                || function_type.results.iter().any(|&ty| ty != 0x7f))
        {
            return None;
        }
        Some((params, results))
    }

    /// Invoke an exported function by name with typed [`Val`] arguments.
    pub fn invoke_by_name(&self, name: &str, args: &[Val]) -> Result<Vec<Val>, WasmError> {
        let idx = self
            .exports
            .get(name)
            .copied()
            .ok_or(WasmError::Trap("no exported function named `"))?;
        self.invoke_val(idx, args)
    }

    /// Invoke function `idx` with `args`, returning its result values.
    ///
    /// Fresh zero-initialised linear memory is allocated for the call and
    /// shared across every nested `call`; it is discarded when the top-level
    /// invocation returns. Use [`Module::instantiate`] to retain state.
    pub fn invoke(&self, idx: usize, args: &[i32]) -> Result<Vec<i32>, WasmError> {
        let vals = i32_args_to_vals(args)?;
        let results = self.invoke_val(idx, &vals)?;
        vals_to_i32(results)
    }

    /// Invoke function `idx` with typed [`Val`] arguments, returning typed
    /// results. This is the full entry point; [`Module::invoke`] is the i32
    /// convenience wrapper over it.
    pub fn invoke_val(&self, idx: usize, args: &[Val]) -> Result<Vec<Val>, WasmError> {
        let mut steps: u64 = 0;
        let mut mem = self.new_memory()?;
        let mut globals = self.new_globals()?;
        let mut data_live = self.new_data_state()?;
        let (mut tables, mut elem_live) = self.new_table_state()?;
        let mut bulk = BulkState {
            data_live: &mut data_live,
            tables: &mut tables,
            elem_live: &mut elem_live,
        };
        let mut resources = CallResourceStats::default();
        let mut call_context = CallContext {
            base_depth: 0,
            stats: &mut resources,
        };
        self.call_any(
            WasmCall { index: idx, args },
            &mut steps,
            &mut mem,
            &mut globals,
            &mut bulk,
            &mut call_context,
        )
    }

    /// Pages the module's linear memory starts at. Parsed modules record their
    /// exact standard minimum (including zero for no memory); the programmatic
    /// compatibility builder retains one implicit page.
    fn initial_mem_pages(&self) -> usize {
        self.mem_min_pages.unwrap_or(1)
    }

    /// A fresh zeroed linear memory with the module's active data segments
    /// applied (spec 4.5.4 instantiation).
    ///
    /// Oversized `min` or an allocator refusal is `Err`, never a process abort.
    fn new_memory(&self) -> Result<Vec<u8>, WasmError> {
        let pages = self.initial_mem_pages();
        if pages > self.limits.max_memory_pages.min(WASM_MAX_PAGES) {
            return Err(WasmError::Trap("memory size"));
        }
        let nbytes = pages
            .checked_mul(WASM_PAGE_SIZE)
            .ok_or(WasmError::Trap("memory size"))?;
        let mut mem = Vec::new();
        mem.try_reserve(nbytes)
            .map_err(|_| WasmError::Trap("memory size"))?;
        mem.resize(nbytes, 0);
        for segment in &self.data {
            if let DataMode::Active(offset) = segment.mode {
                let end = offset + segment.bytes.len();
                if end <= mem.len() {
                    mem[offset..end].copy_from_slice(&segment.bytes);
                }
            }
        }
        Ok(mem)
    }

    fn new_globals(&self) -> Result<Vec<Val>, WasmError> {
        let mut globals = Vec::new();
        globals
            .try_reserve_exact(self.globals.len())
            .map_err(|_| WasmError::Trap("global state"))?;
        globals.extend(self.globals.iter().map(|global| global.init));
        Ok(globals)
    }

    /// Passive data starts live. Active data is implicitly dropped after its
    /// instantiation-time initialization, as required by bulk memory.
    fn new_data_state(&self) -> Result<Vec<bool>, WasmError> {
        let mut state = Vec::new();
        state
            .try_reserve(self.data.len())
            .map_err(|_| WasmError::Trap("data segment state"))?;
        state.extend(
            self.data
                .iter()
                .map(|segment| segment.mode == DataMode::Passive),
        );
        Ok(state)
    }

    /// Create one instance's table and passive-element liveness without any
    /// infallible guest-sized allocation.
    fn new_table_state(&self) -> Result<TableState, WasmError> {
        let mut tables = Vec::new();
        tables
            .try_reserve_exact(self.tables.len())
            .map_err(|_| WasmError::Trap("table size"))?;
        for template in &self.tables {
            let mut table = Vec::new();
            table
                .try_reserve_exact(template.elements.len())
                .map_err(|_| WasmError::Trap("table size"))?;
            table.extend_from_slice(&template.elements);
            tables.push(table);
        }

        let mut elem_live = Vec::new();
        elem_live
            .try_reserve(self.elems.len())
            .map_err(|_| WasmError::Trap("element segment state"))?;
        elem_live.extend(
            self.elems
                .iter()
                .map(|segment| segment.mode == ElemMode::Passive),
        );
        Ok((tables, elem_live))
    }

    /// Number of parameters a function index (host or defined) expects.
    fn param_count(&self, idx: usize) -> Result<usize, WasmError> {
        if idx < self.hosts.len() {
            Ok(self.hosts[idx].n_params)
        } else {
            self.funcs
                .get(idx - self.hosts.len())
                .map(|f| f.n_params)
                .ok_or(WasmError::Trap("call to unknown function"))
        }
    }

    fn indirect_target(
        &self,
        table_index: u32,
        type_index: u32,
        element_index: usize,
        bulk: &BulkState<'_>,
    ) -> Result<usize, WasmError> {
        let table = bulk
            .tables
            .get(table_index as usize)
            .ok_or(WasmError::Trap("call_indirect: table immediate"))?;
        let combined = table
            .get(element_index)
            .copied()
            .flatten()
            .ok_or(WasmError::Trap(
                "call_indirect: uninitialised table element",
            ))?;
        let expected = self
            .types
            .get(type_index as usize)
            .ok_or(WasmError::Trap("call_indirect: bad type index"))?;
        let (actual_params, actual_results, actual_sig) = if combined < self.hosts.len() {
            let host = &self.hosts[combined];
            (host.n_params, host.n_results, host.sig)
        } else {
            let function = self
                .funcs
                .get(combined - self.hosts.len())
                .ok_or(WasmError::Trap("call_indirect: bad table entry"))?;
            (function.n_params, function.arity, function.sig)
        };
        let matches = match actual_sig.and_then(|signature| self.types.get(signature)) {
            Some(actual) => actual == expected,
            None => {
                expected.params.len() == actual_params && expected.results.len() == actual_results
            }
        };
        if !matches {
            return Err(WasmError::Trap("call_indirect: signature mismatch"));
        }
        Ok(combined)
    }

    fn block_counts(&self, ty: BlockType) -> Result<(usize, usize), WasmError> {
        match ty {
            BlockType::Empty => Ok((0, 0)),
            BlockType::Value(_) => Ok((0, 1)),
            BlockType::TypeIndex(index) => self
                .types
                .get(index as usize)
                .map(|ft| (ft.params.len(), ft.results.len()))
                .ok_or(WasmError::Trap("block type index")),
        }
    }

    fn new_defined_activation(
        &self,
        def_idx: usize,
        args: Vec<Val>,
        available_slots: usize,
    ) -> Result<DefinedActivation, WasmError> {
        let func = self
            .funcs
            .get(def_idx)
            .ok_or(WasmError::Trap("call to unknown function"))?;
        if args.len() != func.n_params {
            return Err(WasmError::Trap("function"));
        }
        let local_count = args
            .len()
            .checked_add(func.locals.len())
            .ok_or(WasmError::Trap("function locals"))?;
        if local_count
            .checked_add(1)
            .filter(|&slots| slots <= available_slots)
            .is_none()
        {
            return Err(WasmError::Trap("call stack"));
        }
        let mut locals = Vec::new();
        locals
            .try_reserve_exact(local_count)
            .map_err(|_| WasmError::Trap("function locals"))?;
        locals.extend(args);
        // Declared locals default to the zero value of their declared type.
        locals.extend_from_slice(&func.locals);

        let mut control = Vec::new();
        control
            .try_reserve_exact(1)
            .map_err(|_| WasmError::Trap("control stack"))?;
        control.push(Frame {
            base: 0,
            branch_arity: func.arity,
            cont: func.code.len(),
            is_loop: false,
        });
        Ok(DefinedActivation {
            def_idx,
            locals,
            stack: Vec::new(),
            control,
            pc: 0,
        })
    }

    fn call_host(
        &self,
        index: usize,
        args: &[Val],
        mem: &mut [u8],
        force_owned: bool,
    ) -> Result<CallValues, WasmError> {
        #[cfg(all(feature = "staticcore", not(feature = "std")))]
        let _ = force_owned;
        let host = self
            .hosts
            .get(index)
            .ok_or(WasmError::Trap("host function"))?;
        if args.len() != host.n_params {
            return Err(WasmError::Trap("host function"));
        }
        let signature = host.sig.and_then(|index| self.types.get(index));
        if let Some(function_type) = signature
            && !values_have_types(args, &function_type.params)
        {
            return Err(WasmError::Trap("host argument type"));
        }
        match &host.binding {
            HostBinding::TypedReturning(callback) => {
                let results = callback(args, host.n_results, mem)?;
                let function_count = self.hosts.len() + self.funcs.len();
                let valid = if let Some(function_type) = signature {
                    host_results_are_valid(&results, &function_type.results, function_count)
                } else {
                    results.len() == host.n_results && host_refs_are_valid(&results, function_count)
                };
                if !valid {
                    return Err(WasmError::Trap("host result type"));
                }
                Ok(CallValues::Owned(results))
            }
            #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
            HostBinding::Bounded(callback) => {
                if signature.is_some_and(|function_type| {
                    function_type.params.iter().any(|&ty| ty != 0x7F)
                        || function_type.results.iter().any(|&ty| ty != 0x7F)
                }) {
                    return Err(WasmError::Trap("host ABI is i32-only; import declares"));
                }
                let owned = if force_owned {
                    let mut values = Vec::new();
                    values
                        .try_reserve_exact(host.n_results)
                        .map_err(|_| WasmError::Trap("host results"))?;
                    Some(values)
                } else {
                    None
                };
                let mut i32_args = [0; MAX_BOUNDED_HOST_ARITY];
                for (slot, value) in i32_args.iter_mut().zip(args) {
                    *slot = match value {
                        Val::I32(number) => *number,
                        _other => return Err(WasmError::Trap("host function")),
                    };
                }
                let mut i32_results = [0; MAX_BOUNDED_HOST_ARITY];
                callback(
                    &i32_args[..host.n_params],
                    &mut i32_results[..host.n_results],
                    mem,
                )?;
                if let Some(mut values) = owned {
                    values.extend(i32_results[..host.n_results].iter().copied().map(Val::I32));
                    Ok(CallValues::Owned(values))
                } else {
                    Ok(CallValues::BoundedI32 {
                        values: i32_results,
                        len: host.n_results,
                    })
                }
            }
            #[cfg(not(all(feature = "staticcore", not(feature = "std"))))]
            HostBinding::TypedBounded(callback) => {
                let function_type = signature.ok_or(WasmError::Trap("host function type"))?;
                let owned = if force_owned {
                    let mut values = Vec::new();
                    values
                        .try_reserve_exact(host.n_results)
                        .map_err(|_| WasmError::Trap("host results"))?;
                    Some(values)
                } else {
                    None
                };
                let mut values = [Val::I32(0); MAX_BOUNDED_HOST_ARITY];
                for (slot, &value_type) in values.iter_mut().zip(&function_type.results) {
                    *slot = zero_of_valtype(value_type)?;
                }
                callback(args, &mut values[..host.n_results], mem)?;
                if !host_results_are_valid(
                    &values[..host.n_results],
                    &function_type.results,
                    self.hosts.len() + self.funcs.len(),
                ) {
                    return Err(WasmError::Trap("host result type"));
                }
                if let Some(mut owned) = owned {
                    owned.extend_from_slice(&values[..host.n_results]);
                    Ok(CallValues::Owned(owned))
                } else {
                    Ok(CallValues::BoundedTyped {
                        values,
                        len: host.n_results,
                    })
                }
            }
        }
    }

    /// Dispatch a call by combined index to a host function or a defined one.
    fn call_any(
        &self,
        call: WasmCall<'_>,
        steps: &mut u64,
        mem: &mut Vec<u8>,
        globals: &mut [Val],
        bulk: &mut BulkState<'_>,
        context: &mut CallContext<'_>,
    ) -> Result<Vec<Val>, WasmError> {
        let mut index = call.index;
        let mut args = Vec::new();
        args.try_reserve_exact(call.args.len())
            .map_err(|_| WasmError::Trap("call arguments"))?;
        args.extend_from_slice(call.args);
        let mut activation: Option<DefinedActivation> = None;
        let mut callers: Vec<DefinedActivation> = Vec::new();
        let mut suspended_slots = 0usize;
        loop {
            let available_slots = self
                .limits
                .max_activation_slots
                .checked_sub(suspended_slots)
                .ok_or(WasmError::Trap("call stack"))?;
            let outcome = if let Some(current) = activation.take() {
                let current_depth = context
                    .base_depth
                    .checked_add(callers.len())
                    .and_then(|value| value.checked_add(1))
                    .ok_or(WasmError::Trap("call depth"))?;
                let activation_resources = ActivationResources {
                    available_slots,
                    suspended_slots,
                    call_depth: current_depth,
                    stats: context.stats,
                };
                self.run_defined(current, steps, mem, globals, bulk, activation_resources)?
            } else if index < self.hosts.len() {
                let force_owned = callers.is_empty();
                if let Some(caller) = callers.last_mut() {
                    let result_count = self.hosts[index].n_results;
                    if suspended_slots
                        .checked_add(result_count)
                        .filter(|&slots| slots <= self.limits.max_activation_slots)
                        .is_none()
                        || caller
                            .stack
                            .len()
                            .checked_add(result_count)
                            .filter(|&len| len <= WASM_STACK_LIMIT)
                            .is_none()
                    {
                        return Err(WasmError::Trap("call stack"));
                    }
                    // Reserve the suspended destination before the host can
                    // mutate memory or embedding state.
                    caller
                        .stack
                        .try_reserve(result_count)
                        .map_err(|_| WasmError::Trap("call stack"))?;
                }
                DefinedOutcome::Values(self.call_host(index, &args, mem, force_owned)?)
            } else {
                let current_depth = context
                    .base_depth
                    .checked_add(callers.len())
                    .and_then(|value| value.checked_add(1))
                    .ok_or(WasmError::Trap("call depth"))?;
                if current_depth > self.limits.max_call_depth {
                    return Err(WasmError::Trap("call depth"));
                }
                let current = self.new_defined_activation(
                    index - self.hosts.len(),
                    core::mem::take(&mut args),
                    available_slots,
                )?;
                let activation_resources = ActivationResources {
                    available_slots,
                    suspended_slots,
                    call_depth: current_depth,
                    stats: context.stats,
                };
                self.run_defined(current, steps, mem, globals, bulk, activation_resources)?
            };
            match outcome {
                DefinedOutcome::Values(values) => {
                    if let Some(mut caller) = callers.pop() {
                        let caller_slots = caller.live_slots()?;
                        suspended_slots = suspended_slots
                            .checked_sub(caller_slots)
                            .ok_or(WasmError::Trap("call stack"))?;
                        let resumed_slots = caller_slots
                            .checked_add(values.len())
                            .ok_or(WasmError::Trap("call stack"))?;
                        let available_slots = self
                            .limits
                            .max_activation_slots
                            .checked_sub(suspended_slots)
                            .ok_or(WasmError::Trap("call stack"))?;
                        if resumed_slots > available_slots {
                            return Err(WasmError::Trap("call stack"));
                        }
                        caller
                            .stack
                            .try_reserve(values.len())
                            .map_err(|_| WasmError::Trap("call stack"))?;
                        values.append_to(&mut caller.stack);
                        activation = Some(caller);
                    } else {
                        return values.into_vec();
                    }
                }
                DefinedOutcome::Call {
                    index: next,
                    args: next_args,
                    caller,
                } => {
                    if next >= self.hosts.len() {
                        let next_depth = context
                            .base_depth
                            .checked_add(callers.len())
                            .and_then(|value| value.checked_add(2))
                            .ok_or(WasmError::Trap("call depth"))?;
                        if next_depth > self.limits.max_call_depth {
                            return Err(WasmError::Trap("call depth"));
                        }
                    }
                    callers
                        .try_reserve(1)
                        .map_err(|_| WasmError::Trap("call stack"))?;
                    let caller_slots = caller.live_slots()?;
                    suspended_slots = suspended_slots
                        .checked_add(caller_slots)
                        .filter(|&slots| slots <= self.limits.max_activation_slots)
                        .ok_or(WasmError::Trap("call stack"))?;
                    callers.push(caller);
                    index = next;
                    args = next_args;
                }
                DefinedOutcome::TailCall {
                    index: next,
                    args: next_args,
                } => {
                    index = next;
                    args = next_args;
                }
            }
        }
    }

    fn run_defined(
        &self,
        activation: DefinedActivation,
        steps: &mut u64,
        mem: &mut Vec<u8>,
        globals: &mut [Val],
        bulk: &mut BulkState<'_>,
        resources: ActivationResources<'_>,
    ) -> Result<DefinedOutcome, WasmError> {
        let DefinedActivation {
            def_idx,
            mut locals,
            mut stack,
            mut control,
            mut pc,
        } = activation;
        let func = self
            .funcs
            .get(def_idx)
            .ok_or(WasmError::Trap("call to unknown function"))?;

        loop {
            let live_slots = locals
                .len()
                .checked_add(stack.len())
                .and_then(|slots| slots.checked_add(control.len()))
                .ok_or(WasmError::Trap("call stack"))?;
            *steps += 1;
            if *steps > self.limits.max_steps {
                return Err(WasmError::Trap("step budget"));
            }
            if stack.len() > WASM_STACK_LIMIT {
                return Err(WasmError::Trap("operand stack"));
            }
            if live_slots > resources.available_slots {
                return Err(WasmError::Trap("call stack"));
            }
            resources.stats.observe(
                resources.call_depth,
                resources
                    .suspended_slots
                    .checked_add(live_slots)
                    .ok_or(WasmError::Trap("call stack"))?,
            );
            if pc >= func.code.len() {
                return finish_defined(&mut stack, func.arity);
            }
            let op = func.code[pc];
            pc += 1;
            match op {
                Op::I32Const(v) => {
                    push_operand(
                        &mut stack,
                        Val::I32(v),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
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
                        Err(WasmError::Trap("i32.div_s by zero"))
                    } else {
                        a.checked_div(b)
                            .ok_or(WasmError::Trap("i32.div_s overflow"))
                    }
                })?,
                Op::I32DivU => bin_i32_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i32.div_u by zero"))
                    } else {
                        Ok(((a as u32) / (b as u32)) as i32)
                    }
                })?,
                Op::I32RemS => bin_i32_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i32.rem_s by zero"))
                    } else {
                        Ok(a.wrapping_rem(b))
                    }
                })?,
                Op::I32RemU => bin_i32_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i32.rem_u by zero"))
                    } else {
                        Ok(((a as u32) % (b as u32)) as i32)
                    }
                })?,
                Op::I32And => bin_i32(&mut stack, |a, b| a & b)?,
                Op::I32Or => bin_i32(&mut stack, |a, b| a | b)?,
                Op::I32Xor => bin_i32(&mut stack, |a, b| a ^ b)?,
                Op::I32Shl => bin_i32(&mut stack, |a, b| ((a as u32) << ((b as u32) & 31)) as i32)?,
                Op::I32ShrS => bin_i32(&mut stack, |a, b| a >> ((b as u32) & 31))?,
                Op::I32ShrU => {
                    bin_i32(&mut stack, |a, b| ((a as u32) >> ((b as u32) & 31)) as i32)?
                }
                Op::I32Rotl => bin_i32(&mut stack, |a, b| {
                    (a as u32).rotate_left((b as u32) & 31) as i32
                })?,
                Op::I32Rotr => bin_i32(&mut stack, |a, b| {
                    (a as u32).rotate_right((b as u32) & 31) as i32
                })?,
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
                Op::GlobalGet(g) => {
                    let v = *globals
                        .get(g as usize)
                        .ok_or(WasmError::Trap("global.get"))?;
                    push_operand(&mut stack, v, live_slots, resources.available_slots)?;
                }
                Op::GlobalSet(g) => {
                    let gi = g as usize;
                    if self.globals.get(gi).is_some_and(|d| !d.mutable) {
                        return Err(WasmError::Trap("global.set"));
                    }
                    let v = pop_val(&mut stack)?;
                    let cell = globals.get_mut(gi).ok_or(WasmError::Trap("global.set"))?;
                    *cell = v;
                }
                Op::I32WrapI64 => {
                    let a = pop_i64(&mut stack)?;
                    stack.push(Val::I32(a as i32));
                }
                Op::I64ExtendI32S => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::I64(a as i64));
                }
                Op::I64ExtendI32U => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::I64((a as u32) as i64));
                }
                Op::I32TruncF32S => {
                    let x = pop_f32(&mut stack)?;
                    stack.push(Val::I32(trunc_f32_to_i32_s(x)?));
                }
                Op::I32TruncF32U => {
                    let x = pop_f32(&mut stack)?;
                    stack.push(Val::I32(trunc_f32_to_i32_u(x)?));
                }
                Op::I32TruncF64S => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I32(trunc_f64_to_i32_s(x)?));
                }
                Op::I32TruncF64U => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I32(trunc_f64_to_i32_u(x)?));
                }
                Op::I64TruncF32S => {
                    let x = pop_f32(&mut stack)?;
                    stack.push(Val::I64(trunc_f32_to_i64_s(x)?));
                }
                Op::I64TruncF32U => {
                    let x = pop_f32(&mut stack)?;
                    stack.push(Val::I64(trunc_f32_to_i64_u(x)?));
                }
                Op::I64TruncF64S => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I64(trunc_f64_to_i64_s(x)?));
                }
                Op::I64TruncF64U => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I64(trunc_f64_to_i64_u(x)?));
                }
                Op::I32TruncSatF32S => {
                    let x = f64::from(pop_f32(&mut stack)?);
                    stack.push(Val::I32(sat_f64_to_i32_s(x)));
                }
                Op::I32TruncSatF32U => {
                    let x = f64::from(pop_f32(&mut stack)?);
                    stack.push(Val::I32(sat_f64_to_i32_u(x)));
                }
                Op::I32TruncSatF64S => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I32(sat_f64_to_i32_s(x)));
                }
                Op::I32TruncSatF64U => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I32(sat_f64_to_i32_u(x)));
                }
                Op::I64TruncSatF32S => {
                    let x = f64::from(pop_f32(&mut stack)?);
                    stack.push(Val::I64(sat_f64_to_i64_s(x)));
                }
                Op::I64TruncSatF32U => {
                    let x = f64::from(pop_f32(&mut stack)?);
                    stack.push(Val::I64(sat_f64_to_i64_u(x)));
                }
                Op::I64TruncSatF64S => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I64(sat_f64_to_i64_s(x)));
                }
                Op::I64TruncSatF64U => {
                    let x = pop_f64(&mut stack)?;
                    stack.push(Val::I64(sat_f64_to_i64_u(x)));
                }
                Op::F32ConvertI32S => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::F32(a as f32));
                }
                Op::F32ConvertI32U => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::F32((a as u32) as f32));
                }
                Op::F32ConvertI64S => {
                    let a = pop_i64(&mut stack)?;
                    stack.push(Val::F32(a as f32));
                }
                Op::F32ConvertI64U => {
                    let a = pop_i64(&mut stack)?;
                    stack.push(Val::F32((a as u64) as f32));
                }
                Op::F32DemoteF64 => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F32(a as f32));
                }
                Op::F64ConvertI32S => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::F64(a as f64));
                }
                Op::F64ConvertI32U => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::F64((a as u32) as f64));
                }
                Op::F64ConvertI64S => {
                    let a = pop_i64(&mut stack)?;
                    stack.push(Val::F64(a as f64));
                }
                Op::F64ConvertI64U => {
                    let a = pop_i64(&mut stack)?;
                    stack.push(Val::F64((a as u64) as f64));
                }
                Op::F64PromoteF32 => {
                    let a = pop_f32(&mut stack)?;
                    stack.push(Val::F64(a as f64));
                }
                Op::I32ReinterpretF32 => {
                    let f = pop_f32(&mut stack)?;
                    stack.push(Val::I32(f.to_bits() as i32));
                }
                Op::I64ReinterpretF64 => {
                    let f = pop_f64(&mut stack)?;
                    stack.push(Val::I64(f.to_bits() as i64));
                }
                Op::F32ReinterpretI32 => {
                    let a = pop(&mut stack)?;
                    stack.push(Val::F32(f32::from_bits(a as u32)));
                }
                Op::F64ReinterpretI64 => {
                    let a = pop_i64(&mut stack)?;
                    stack.push(Val::F64(f64::from_bits(a as u64)));
                }
                Op::I32Extend8S => {
                    let value = pop(&mut stack)?;
                    stack.push(Val::I32(value as i8 as i32));
                }
                Op::I32Extend16S => {
                    let value = pop(&mut stack)?;
                    stack.push(Val::I32(value as i16 as i32));
                }
                Op::I64Extend8S => {
                    let value = pop_i64(&mut stack)?;
                    stack.push(Val::I64(value as i8 as i64));
                }
                Op::I64Extend16S => {
                    let value = pop_i64(&mut stack)?;
                    stack.push(Val::I64(value as i16 as i64));
                }
                Op::I64Extend32S => {
                    let value = pop_i64(&mut stack)?;
                    stack.push(Val::I64(value as i32 as i64));
                }
                Op::F32Const(v) => {
                    push_operand(
                        &mut stack,
                        Val::F32(v),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
                Op::F32Eq => bin_f32_cmp(&mut stack, |a, b| a == b)?,
                Op::F32Ne => bin_f32_cmp(&mut stack, |a, b| a != b)?,
                Op::F32Lt => bin_f32_cmp(&mut stack, |a, b| a < b)?,
                Op::F32Gt => bin_f32_cmp(&mut stack, |a, b| a > b)?,
                Op::F32Le => bin_f32_cmp(&mut stack, |a, b| a <= b)?,
                Op::F32Ge => bin_f32_cmp(&mut stack, |a, b| a >= b)?,
                Op::F32Abs => un_f32(&mut stack, libm::fabsf)?,
                Op::F32Neg => un_f32(&mut stack, |a| -a)?,
                Op::F32Ceil => un_f32(&mut stack, libm::ceilf)?,
                Op::F32Floor => un_f32(&mut stack, libm::floorf)?,
                Op::F32Trunc => un_f32(&mut stack, libm::truncf)?,
                Op::F32Nearest => un_f32(&mut stack, libm::rintf)?,
                Op::F32Sqrt => un_f32(&mut stack, libm::sqrtf)?,
                Op::F32Add => bin_f32(&mut stack, |a, b| a + b)?,
                Op::F32Sub => bin_f32(&mut stack, |a, b| a - b)?,
                Op::F32Mul => bin_f32(&mut stack, |a, b| a * b)?,
                Op::F32Div => bin_f32(&mut stack, |a, b| a / b)?,
                Op::F32Min => bin_f32(&mut stack, wasm_min_f32)?,
                Op::F32Max => bin_f32(&mut stack, wasm_max_f32)?,
                Op::F32Copysign => bin_f32(&mut stack, libm::copysignf)?,
                Op::F32Load { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    let bytes = le4(&mem[ea..ea + 4]);
                    stack.push(Val::F32(f32::from_le_bytes(bytes)));
                }
                Op::F32Store { offset } => {
                    let value = pop_f32(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    mem[ea..ea + 4].copy_from_slice(&value.to_le_bytes());
                }
                Op::F64Const(bits) => {
                    push_operand(
                        &mut stack,
                        Val::F64(f64::from_bits(bits)),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
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
                    stack.push(Val::F64(libm::fabs(a)));
                }
                Op::F64Neg => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(-a));
                }
                Op::F64Ceil => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(libm::ceil(a)));
                }
                Op::F64Floor => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(libm::floor(a)));
                }
                Op::F64Trunc => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(libm::trunc(a)));
                }
                Op::F64Nearest => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(libm::rint(a)));
                }
                Op::F64Sqrt => {
                    let a = pop_f64(&mut stack)?;
                    stack.push(Val::F64(libm::sqrt(a)));
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
                    stack.push(Val::F64(libm::copysign(a, b)));
                }
                Op::F64Load { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 8)?;
                    let bytes = le8(&mem[ea..ea + 8]);
                    stack.push(Val::F64(f64::from_le_bytes(bytes)));
                }
                Op::F64Store { offset } => {
                    let value = pop_f64(&mut stack)?;
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 8)?;
                    mem[ea..ea + 8].copy_from_slice(&value.to_le_bytes());
                }
                Op::MemorySize => {
                    push_operand(
                        &mut stack,
                        Val::I32((mem.len() / WASM_PAGE_SIZE) as i32),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
                Op::MemoryGrow => {
                    let delta = pop(&mut stack)? as u32 as usize;
                    let old_pages = mem.len() / WASM_PAGE_SIZE;
                    let new_pages = old_pages.checked_add(delta);
                    // The declared maximum is the module's own ceiling; the
                    // spec-wide cap applies when it declares none. Growth that
                    // the allocator refuses reports -1 rather than aborting.
                    let cap = self
                        .mem_max_pages
                        .unwrap_or(WASM_MAX_PAGES)
                        .min(self.limits.max_memory_pages)
                        .min(WASM_MAX_PAGES);
                    let growth = new_pages.filter(|&pages| pages <= cap).and_then(|pages| {
                        pages
                            .checked_sub(old_pages)
                            .and_then(|delta_pages| delta_pages.checked_mul(WASM_PAGE_SIZE))
                            .map(|extra| (pages, extra))
                    });
                    if let Some((new_pages, extra)) = growth
                        && mem.try_reserve(extra).is_ok()
                    {
                        mem.resize(new_pages * WASM_PAGE_SIZE, 0);
                        stack.push(Val::I32(old_pages as i32));
                    } else {
                        stack.push(Val::I32(-1));
                    }
                }
                Op::MemoryInit { data_index } => {
                    let len = pop(&mut stack)? as u32 as usize;
                    let source = pop(&mut stack)? as u32 as usize;
                    let destination = pop(&mut stack)? as u32 as usize;
                    let index = data_index as usize;
                    let segment = self
                        .data
                        .get(index)
                        .ok_or(WasmError::Trap("memory.init data segment index"))?;
                    let live = *bulk
                        .data_live
                        .get(index)
                        .ok_or(WasmError::Trap("memory.init data segment state"))?;
                    let bytes = if live { segment.bytes.as_slice() } else { &[] };
                    let source_range = bulk_memory_range(bytes.len(), source, len)?;
                    let destination_range = bulk_memory_range(mem.len(), destination, len)?;
                    charge_bulk_steps(steps, len, self.limits.max_steps)?;
                    mem[destination_range].copy_from_slice(&bytes[source_range]);
                }
                Op::DataDrop { data_index } => {
                    let live = bulk
                        .data_live
                        .get_mut(data_index as usize)
                        .ok_or(WasmError::Trap("data.drop segment index"))?;
                    *live = false;
                }
                Op::TableInit {
                    elem_index,
                    table_index,
                } => {
                    let len = pop(&mut stack)? as u32 as usize;
                    let source = pop(&mut stack)? as u32 as usize;
                    let destination = pop(&mut stack)? as u32 as usize;
                    let index = elem_index as usize;
                    let segment = self
                        .elems
                        .get(index)
                        .ok_or(WasmError::Trap("table.init element segment index"))?;
                    let live = *bulk
                        .elem_live
                        .get(index)
                        .ok_or(WasmError::Trap("table.init element segment state"))?;
                    let refs = if live { segment.refs.as_slice() } else { &[] };
                    let source_range = bulk_memory_range(refs.len(), source, len)?;
                    let table = bulk
                        .tables
                        .get_mut(table_index as usize)
                        .ok_or(WasmError::Trap("table.init table index"))?;
                    let destination_range = bulk_memory_range(table.len(), destination, len)?;
                    charge_bulk_elements(steps, len, self.limits.max_steps)?;
                    for (slot, &function) in table[destination_range]
                        .iter_mut()
                        .zip(refs[source_range].iter())
                    {
                        *slot = function;
                    }
                }
                Op::ElemDrop { elem_index } => {
                    let live = bulk
                        .elem_live
                        .get_mut(elem_index as usize)
                        .ok_or(WasmError::Trap("elem.drop segment index"))?;
                    *live = false;
                }
                Op::TableCopy {
                    destination_table,
                    source_table,
                } => {
                    let len = pop(&mut stack)? as u32 as usize;
                    let source = pop(&mut stack)? as u32 as usize;
                    let destination = pop(&mut stack)? as u32 as usize;
                    let source_index = source_table as usize;
                    let destination_index = destination_table as usize;
                    let source_len = bulk
                        .tables
                        .get(source_index)
                        .ok_or(WasmError::Trap("table.copy source table index"))?
                        .len();
                    let destination_len = bulk
                        .tables
                        .get(destination_index)
                        .ok_or(WasmError::Trap("table.copy destination table index"))?
                        .len();
                    let source_range = bulk_memory_range(source_len, source, len)?;
                    bulk_memory_range(destination_len, destination, len)?;
                    charge_bulk_elements(steps, len, self.limits.max_steps)?;
                    if source_index == destination_index {
                        bulk.tables[source_index].copy_within(source_range, destination);
                    } else {
                        for relative in 0..len {
                            let function = bulk.tables[source_index][source + relative];
                            bulk.tables[destination_index][destination + relative] = function;
                        }
                    }
                }
                Op::TableGet(table_index) => {
                    let index = pop(&mut stack)? as u32 as usize;
                    let function = *bulk
                        .tables
                        .get(table_index as usize)
                        .ok_or(WasmError::Trap("table.get table index"))?
                        .get(index)
                        .ok_or(WasmError::Trap("table.get out of bounds"))?;
                    stack.push(Val::FuncRef(function));
                }
                Op::TableSet(table_index) => {
                    let function = pop_funcref(&mut stack)?;
                    let index = pop(&mut stack)? as u32 as usize;
                    let slot = bulk
                        .tables
                        .get_mut(table_index as usize)
                        .ok_or(WasmError::Trap("table.set table index"))?
                        .get_mut(index)
                        .ok_or(WasmError::Trap("table.set out of bounds"))?;
                    *slot = function;
                }
                Op::TableGrow(table_index) => {
                    let delta = pop(&mut stack)? as u32 as usize;
                    let function = pop_funcref(&mut stack)?;
                    let table_index = table_index as usize;
                    let old_size = bulk
                        .tables
                        .get(table_index)
                        .ok_or(WasmError::Trap("table.grow table index"))?
                        .len();
                    let cap = self
                        .tables
                        .get(table_index)
                        .ok_or(WasmError::Trap("table.grow table metadata"))?
                        .max
                        .unwrap_or(u32::MAX as usize)
                        .min(self.limits.max_table_elems);
                    let total_size = bulk.tables.iter().try_fold(0usize, |total, table| {
                        total
                            .checked_add(table.len())
                            .ok_or(WasmError::Trap("table size"))
                    })?;
                    let new_size = old_size.checked_add(delta).filter(|&size| size <= cap);
                    let aggregate_size = new_size.and_then(|size| {
                        total_size
                            .checked_sub(old_size)
                            .and_then(|other| other.checked_add(size))
                    });
                    let new_size = new_size
                        .zip(aggregate_size)
                        .filter(|(_, total)| *total <= self.limits.max_table_elems)
                        .map(|(size, _)| size);
                    if let Some(new_size) = new_size {
                        charge_bulk_elements(steps, delta, self.limits.max_steps)?;
                        let table = &mut bulk.tables[table_index];
                        if table.try_reserve(delta).is_ok() {
                            table.resize(new_size, function);
                            stack.push(Val::I32(old_size as i32));
                        } else {
                            stack.push(Val::I32(-1));
                        }
                    } else {
                        stack.push(Val::I32(-1));
                    }
                }
                Op::TableSize(table_index) => {
                    let table = bulk
                        .tables
                        .get(table_index as usize)
                        .ok_or(WasmError::Trap("table.size table index"))?;
                    push_operand(
                        &mut stack,
                        Val::I32(table.len() as i32),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
                Op::TableFill(table_index) => {
                    let len = pop(&mut stack)? as u32 as usize;
                    let function = pop_funcref(&mut stack)?;
                    let destination = pop(&mut stack)? as u32 as usize;
                    let table = bulk
                        .tables
                        .get_mut(table_index as usize)
                        .ok_or(WasmError::Trap("table.fill table index"))?;
                    let range = bulk_memory_range(table.len(), destination, len)?;
                    charge_bulk_elements(steps, len, self.limits.max_steps)?;
                    table[range].fill(function);
                }
                Op::MemoryCopy => {
                    let len = pop(&mut stack)? as u32 as usize;
                    let source = pop(&mut stack)? as u32 as usize;
                    let destination = pop(&mut stack)? as u32 as usize;
                    let source_range = bulk_memory_range(mem.len(), source, len)?;
                    bulk_memory_range(mem.len(), destination, len)?;
                    charge_bulk_steps(steps, len, self.limits.max_steps)?;
                    mem.copy_within(source_range, destination);
                }
                Op::MemoryFill => {
                    let len = pop(&mut stack)? as u32 as usize;
                    let value = pop(&mut stack)? as u8;
                    let destination = pop(&mut stack)? as u32 as usize;
                    let range = bulk_memory_range(mem.len(), destination, len)?;
                    charge_bulk_steps(steps, len, self.limits.max_steps)?;
                    mem[range].fill(value);
                }
                Op::I64Load { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 8)?;
                    let bytes = le8(&mem[ea..ea + 8]);
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
                    stack.push(Val::I64(
                        u16::from_le_bytes([mem[ea], mem[ea + 1]]) as u64 as i64
                    ));
                }
                Op::I64Load32S { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    let bytes = le4(&mem[ea..ea + 4]);
                    stack.push(Val::I64(i32::from_le_bytes(bytes) as i64));
                }
                Op::I64Load32U { offset } => {
                    let ea = mem_ea(mem.len(), pop(&mut stack)?, offset, 4)?;
                    let bytes = le4(&mem[ea..ea + 4]);
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
                Op::I64Const(v) => {
                    push_operand(
                        &mut stack,
                        Val::I64(v),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
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
                        Err(WasmError::Trap("i64.div_s by zero"))
                    } else {
                        a.checked_div(b)
                            .ok_or(WasmError::Trap("i64.div_s overflow"))
                    }
                })?,
                Op::I64DivU => bin_i64_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i64.div_u by zero"))
                    } else {
                        Ok(((a as u64) / (b as u64)) as i64)
                    }
                })?,
                Op::I64RemS => bin_i64_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i64.rem_s by zero"))
                    } else {
                        Ok(a.wrapping_rem(b))
                    }
                })?,
                Op::I64RemU => bin_i64_try(&mut stack, |a, b| {
                    if b == 0 {
                        Err(WasmError::Trap("i64.rem_u by zero"))
                    } else {
                        Ok(((a as u64) % (b as u64)) as i64)
                    }
                })?,
                Op::I64And => bin_i64(&mut stack, |a, b| a & b)?,
                Op::I64Or => bin_i64(&mut stack, |a, b| a | b)?,
                Op::I64Xor => bin_i64(&mut stack, |a, b| a ^ b)?,
                Op::I64Shl => bin_i64(&mut stack, |a, b| ((a as u64) << ((b as u64) & 63)) as i64)?,
                Op::I64ShrS => bin_i64(&mut stack, |a, b| a >> ((b as u64) & 63))?,
                Op::I64ShrU => {
                    bin_i64(&mut stack, |a, b| ((a as u64) >> ((b as u64) & 63)) as i64)?
                }
                Op::I64Rotl => bin_i64(&mut stack, |a, b| {
                    (a as u64).rotate_left((b as u64 & 63) as u32) as i64
                })?,
                Op::I64Rotr => bin_i64(&mut stack, |a, b| {
                    (a as u64).rotate_right((b as u64 & 63) as u32) as i64
                })?,
                Op::LocalGet(l) => {
                    let v = *locals.get(l as usize).ok_or(WasmError::Trap("local.get"))?;
                    push_operand(&mut stack, v, live_slots, resources.available_slots)?;
                }
                Op::LocalSet(l) => {
                    let v = pop_val(&mut stack)?;
                    let cell = locals
                        .get_mut(l as usize)
                        .ok_or(WasmError::Trap("local.set"))?;
                    *cell = v;
                }
                Op::Call(f) => {
                    let combined = f as usize;
                    let n = self.param_count(combined)?;
                    if stack.len() < n {
                        return Err(WasmError::Trap("call"));
                    }
                    let args = take_values(&mut stack, n, "call arguments")?;
                    return Ok(DefinedOutcome::Call {
                        index: combined,
                        args,
                        caller: DefinedActivation {
                            def_idx,
                            locals,
                            stack,
                            control,
                            pc,
                        },
                    });
                }
                Op::CallIndirect {
                    type_index,
                    table_index,
                } => {
                    let ti = pop(&mut stack)? as u32 as usize;
                    let combined = self.indirect_target(table_index, type_index, ti, bulk)?;
                    let n = self.param_count(combined)?;
                    if stack.len() < n {
                        return Err(WasmError::Trap("call_indirect: stack has"));
                    }
                    let args = take_values(&mut stack, n, "call arguments")?;
                    return Ok(DefinedOutcome::Call {
                        index: combined,
                        args,
                        caller: DefinedActivation {
                            def_idx,
                            locals,
                            stack,
                            control,
                            pc,
                        },
                    });
                }
                Op::ReturnCall(function) => {
                    let combined = function as usize;
                    let n = self.param_count(combined)?;
                    if stack.len() < n {
                        return Err(WasmError::Trap("return_call"));
                    }
                    let args = take_values(&mut stack, n, "call arguments")?;
                    return Ok(DefinedOutcome::TailCall {
                        index: combined,
                        args,
                    });
                }
                Op::ReturnCallIndirect {
                    type_index,
                    table_index,
                } => {
                    let element = pop(&mut stack)? as u32 as usize;
                    let combined = self.indirect_target(table_index, type_index, element, bulk)?;
                    let n = self.param_count(combined)?;
                    if stack.len() < n {
                        return Err(WasmError::Trap("return_call_indirect"));
                    }
                    let args = take_values(&mut stack, n, "call arguments")?;
                    return Ok(DefinedOutcome::TailCall {
                        index: combined,
                        args,
                    });
                }
                Op::Block { ty, end } => {
                    reserve_control_growth(
                        &mut control,
                        live_slots,
                        resources.available_slots,
                        true,
                    )?;
                    let (params, results) = self.block_counts(ty)?;
                    let base = stack
                        .len()
                        .checked_sub(params)
                        .ok_or(WasmError::Trap("block parameter stack underflow"))?;
                    control.push(Frame {
                        base,
                        branch_arity: results,
                        cont: end + 1,
                        is_loop: false,
                    });
                }
                Op::Loop { ty, .. } => {
                    reserve_control_growth(
                        &mut control,
                        live_slots,
                        resources.available_slots,
                        true,
                    )?;
                    let (params, _results) = self.block_counts(ty)?;
                    let base = stack
                        .len()
                        .checked_sub(params)
                        .ok_or(WasmError::Trap("loop parameter stack underflow"))?;
                    control.push(Frame {
                        base,
                        branch_arity: params,
                        cont: pc, // branch re-enters at the loop body start
                        is_loop: true,
                    });
                }
                Op::Br(l) => {
                    pc = do_branch(&mut stack, &mut control, l)?;
                    if control.is_empty() {
                        return finish_defined(&mut stack, func.arity);
                    }
                }
                Op::BrIf(l) => {
                    let cond = pop(&mut stack)?;
                    if cond != 0 {
                        pc = do_branch(&mut stack, &mut control, l)?;
                        if control.is_empty() {
                            return finish_defined(&mut stack, func.arity);
                        }
                    }
                }
                Op::BrTable {
                    target_start,
                    target_len,
                    default,
                } => {
                    let target_start = target_start as usize;
                    // Decoder-created offsets index the same private arena;
                    // no public API can manufacture an inconsistent pair.
                    let targets =
                        &func.branch_targets[target_start..target_start + target_len as usize];
                    let idx = pop(&mut stack)? as u32 as usize;
                    let label = targets.get(idx).copied().unwrap_or(default);
                    pc = do_branch(&mut stack, &mut control, label)?;
                    if control.is_empty() {
                        return finish_defined(&mut stack, func.arity);
                    }
                }
                Op::If { ty, else_pc, end } => {
                    reserve_control_growth(
                        &mut control,
                        live_slots,
                        resources.available_slots,
                        false,
                    )?;
                    let cond = pop(&mut stack)?;
                    let (params, results) = self.block_counts(ty)?;
                    let base = stack
                        .len()
                        .checked_sub(params)
                        .ok_or(WasmError::Trap("if parameter stack underflow"))?;
                    control.push(Frame {
                        base,
                        branch_arity: results,
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
                    return Err(WasmError::Trap("unreachable executed"));
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
                Op::TypedSelect(_) => {
                    let c = pop(&mut stack)?;
                    let b = pop_val(&mut stack)?;
                    let a = pop_val(&mut stack)?;
                    stack.push(if c != 0 { a } else { b });
                }
                Op::RefNull => {
                    push_operand(
                        &mut stack,
                        Val::FuncRef(None),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
                Op::RefIsNull => {
                    let reference = pop_funcref(&mut stack)?;
                    stack.push(Val::I32(i32::from(reference.is_none())));
                }
                Op::RefFunc(function) => {
                    let function = function as usize;
                    if function >= self.hosts.len() + self.funcs.len() {
                        return Err(WasmError::Trap("ref.func function index"));
                    }
                    push_operand(
                        &mut stack,
                        Val::FuncRef(Some(function)),
                        live_slots,
                        resources.available_slots,
                    )?;
                }
                Op::LocalTee(l) => {
                    let v = *stack
                        .last()
                        .ok_or(WasmError::Trap("local.tee on empty stack"))?;
                    let cell = locals
                        .get_mut(l as usize)
                        .ok_or(WasmError::Trap("local.tee"))?;
                    *cell = v;
                }
                Op::Return => return finish_defined(&mut stack, func.arity),
                Op::End => {
                    control.pop();
                    if control.is_empty() {
                        return finish_defined(&mut stack, func.arity);
                    }
                }
            }
        }
    }
}

impl Instance {
    fn new(module: Module) -> Result<Self, WasmError> {
        let memory = module.new_memory()?;
        let globals = module.new_globals()?;
        let data_live = module.new_data_state()?;
        let (tables, elem_live) = module.new_table_state()?;
        let mut instance = Self {
            module,
            memory,
            globals,
            data_live,
            tables,
            elem_live,
            last_steps: 0,
            last_peak_call_depth: 0,
            last_peak_activation_slots: 0,
        };
        if let Some(start) = instance.module.start {
            let mut steps = 0;
            let mut bulk = BulkState {
                data_live: &mut instance.data_live,
                tables: &mut instance.tables,
                elem_live: &mut instance.elem_live,
            };
            let mut resources = CallResourceStats::default();
            let mut call_context = CallContext {
                base_depth: 0,
                stats: &mut resources,
            };
            let result = instance.module.call_any(
                WasmCall {
                    index: start,
                    args: &[],
                },
                &mut steps,
                &mut instance.memory,
                &mut instance.globals,
                &mut bulk,
                &mut call_context,
            );
            instance.last_steps = steps;
            instance.last_peak_call_depth = resources.peak_call_depth;
            instance.last_peak_activation_slots = resources.peak_activation_slots;
            result?;
        }
        Ok(instance)
    }

    /// Resolve and invoke an exported function while retaining instance state.
    pub fn invoke_by_name(&mut self, name: &str, args: &[Val]) -> Result<Vec<Val>, WasmError> {
        let idx = self
            .module
            .exports
            .get(name)
            .copied()
            .ok_or(WasmError::Trap("no exported function named `"))?;
        self.invoke_val(idx, args)
    }

    /// Invoke a function through the i32 convenience ABI while retaining
    /// instance state.
    pub fn invoke(&mut self, idx: usize, args: &[i32]) -> Result<Vec<i32>, WasmError> {
        let vals = i32_args_to_vals(args)?;
        vals_to_i32(self.invoke_val(idx, &vals)?)
    }

    /// Invoke a function with typed values. The instruction counter starts at
    /// zero for this top-level call; memory and globals remain live.
    pub fn invoke_val(&mut self, idx: usize, args: &[Val]) -> Result<Vec<Val>, WasmError> {
        let mut steps = 0;
        let mut bulk = BulkState {
            data_live: &mut self.data_live,
            tables: &mut self.tables,
            elem_live: &mut self.elem_live,
        };
        let mut resources = CallResourceStats::default();
        let mut call_context = CallContext {
            base_depth: 0,
            stats: &mut resources,
        };
        let result = self.module.call_any(
            WasmCall { index: idx, args },
            &mut steps,
            &mut self.memory,
            &mut self.globals,
            &mut bulk,
            &mut call_context,
        );
        self.last_steps = steps;
        self.last_peak_call_depth = resources.peak_call_depth;
        self.last_peak_activation_slots = resources.peak_activation_slots;
        result
    }

    /// Instructions consumed by the last completed top-level invocation,
    /// including a call that returned a guest trap.
    pub fn last_steps(&self) -> u64 {
        self.last_steps
    }

    /// Peak number of simultaneously live guest-defined activations during
    /// the last top-level invocation, including an invocation that trapped.
    pub fn last_peak_call_depth(&self) -> usize {
        self.last_peak_call_depth
    }

    /// Peak aggregate locals, operand values and control frames across the
    /// active function and all suspended callers in the last invocation.
    pub fn last_peak_activation_slots(&self) -> usize {
        self.last_peak_activation_slots
    }

    /// Current live linear-memory size in WebAssembly 64 KiB pages.
    pub fn memory_pages(&self) -> usize {
        self.memory.len() / WASM_PAGE_SIZE
    }

    /// Aggregate live funcref elements across all tables. For the original
    /// single-table profile this is exactly the table-zero length.
    pub fn table_elements(&self) -> usize {
        self.tables.iter().map(Vec::len).sum()
    }

    /// Number of internally defined funcref tables in this live instance.
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// Current length of a selected funcref table.
    pub fn table_elements_at(&self, table_index: usize) -> Option<usize> {
        self.tables.get(table_index).map(Vec::len)
    }

    /// Read-only access to the live linear memory, for bounded native host I/O.
    pub fn memory(&self) -> &[u8] {
        &self.memory
    }

    /// Mutable access to the live linear memory, for writing bounded input or
    /// state payloads before an exported call.
    pub fn memory_mut(&mut self) -> &mut [u8] {
        &mut self.memory
    }
}

/// Decode a standard memarg and validate its alignment exponent against the
/// instruction's natural alignment before returning the offset.
fn memarg(body: &[u8], i: usize, natural_align: u32) -> Result<(u32, usize), WasmError> {
    let (align, n1) = leb_u32(body, i)?;
    if align > natural_align {
        return Err(WasmError::Decode(
            "memory alignment exceeds natural alignment",
        ));
    }
    let (offset, n2) = leb_u32(body, n1)?;
    Ok((offset, n2))
}

/// Effective address `addr as u32 + offset`, bounds-checked for a `width`-byte
/// access. Alignment is not checked (MVP). Out of range is a loud trap.
fn mem_ea(mem_len: usize, addr: i32, offset: u32, width: usize) -> Result<usize, WasmError> {
    let ea = (addr as u32 as usize)
        .checked_add(offset as usize)
        .ok_or(WasmError::Trap("memory address overflow"))?;
    let end = ea
        .checked_add(width)
        .ok_or(WasmError::Trap("memory access overflow"))?;
    if end > mem_len {
        return Err(WasmError::Trap("memory access ["));
    }
    Ok(ea)
}

/// Bounds-check one bulk-memory range before any byte is changed. In
/// particular, a zero-length operation at exactly `mem_len` is valid.
fn bulk_memory_range(
    mem_len: usize,
    start: usize,
    len: usize,
) -> Result<core::ops::Range<usize>, WasmError> {
    let end = start
        .checked_add(len)
        .ok_or(WasmError::Trap("bulk memory access overflow"))?;
    if end > mem_len {
        return Err(WasmError::Trap("bulk memory access out of bounds"));
    }
    Ok(start..end)
}

/// Bulk operations do work proportional to their byte length. Charge one
/// deterministic fuel unit per 16 bytes in addition to the instruction's
/// ordinary unit, and always charge before mutation so a fuel trap is atomic.
fn charge_bulk_steps(steps: &mut u64, len: usize, max_steps: u64) -> Result<(), WasmError> {
    let units = u64::try_from(len.div_ceil(16)).unwrap_or(u64::MAX);
    *steps = steps.saturating_add(units);
    if *steps > max_steps {
        return Err(WasmError::Trap("step budget"));
    }
    Ok(())
}

/// Table elements are pointer-sized work rather than bytes; charge one fuel
/// unit per copied element before modifying the live table.
fn charge_bulk_elements(steps: &mut u64, len: usize, max_steps: u64) -> Result<(), WasmError> {
    let units = u64::try_from(len).unwrap_or(u64::MAX);
    *steps = steps.saturating_add(units);
    if *steps > max_steps {
        return Err(WasmError::Trap("step budget"));
    }
    Ok(())
}

fn mem_read_i32(mem: &[u8], addr: i32, offset: u32) -> Result<i32, WasmError> {
    let ea = mem_ea(mem.len(), addr, offset, 4)?;
    Ok(i32::from_le_bytes(le4(&mem[ea..ea + 4])))
}

fn mem_write_i32(mem: &mut [u8], addr: i32, offset: u32, value: i32) -> Result<(), WasmError> {
    let ea = mem_ea(mem.len(), addr, offset, 4)?;
    mem[ea..ea + 4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Trap for a float that cannot be truncated into the target integer type
/// (NaN, infinity, or out of range). WASM boundaries (±2^31, 2^32, ±2^63, 2^64)
/// are exact powers of two, so the valid range is the half-open `[lo, hi)`.
fn trunc_trap(op: &'static str) -> WasmError {
    WasmError::Trap(op)
}

fn trunc_f32_to_i32_s(x: f32) -> Result<i32, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i32.trunc_f32_s"));
    }
    let t = libm::truncf(x);
    if !(-2147483648.0f32..2147483648.0f32).contains(&t) {
        return Err(trunc_trap("i32.trunc_f32_s"));
    }
    Ok(t as i32)
}

fn trunc_f32_to_i32_u(x: f32) -> Result<i32, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i32.trunc_f32_u"));
    }
    let t = libm::truncf(x);
    if !(0.0f32..4294967296.0f32).contains(&t) {
        return Err(trunc_trap("i32.trunc_f32_u"));
    }
    Ok((t as u32) as i32)
}

fn trunc_f64_to_i32_s(x: f64) -> Result<i32, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i32.trunc_f64_s"));
    }
    let t = libm::trunc(x);
    if !(-2147483648.0f64..2147483648.0f64).contains(&t) {
        return Err(trunc_trap("i32.trunc_f64_s"));
    }
    Ok(t as i32)
}

fn trunc_f64_to_i32_u(x: f64) -> Result<i32, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i32.trunc_f64_u"));
    }
    let t = libm::trunc(x);
    if !(0.0f64..4294967296.0f64).contains(&t) {
        return Err(trunc_trap("i32.trunc_f64_u"));
    }
    Ok((t as u32) as i32)
}

fn trunc_f32_to_i64_s(x: f32) -> Result<i64, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i64.trunc_f32_s"));
    }
    let t = libm::truncf(x);
    if !(-9223372036854775808.0f32..9223372036854775808.0f32).contains(&t) {
        return Err(trunc_trap("i64.trunc_f32_s"));
    }
    Ok(t as i64)
}

fn trunc_f32_to_i64_u(x: f32) -> Result<i64, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i64.trunc_f32_u"));
    }
    let t = libm::truncf(x);
    if !(0.0f32..18446744073709551616.0f32).contains(&t) {
        return Err(trunc_trap("i64.trunc_f32_u"));
    }
    Ok((t as u64) as i64)
}

fn trunc_f64_to_i64_s(x: f64) -> Result<i64, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i64.trunc_f64_s"));
    }
    let t = libm::trunc(x);
    if !(-9223372036854775808.0f64..9223372036854775808.0f64).contains(&t) {
        return Err(trunc_trap("i64.trunc_f64_s"));
    }
    Ok(t as i64)
}

fn trunc_f64_to_i64_u(x: f64) -> Result<i64, WasmError> {
    if x.is_nan() || x.is_infinite() {
        return Err(trunc_trap("i64.trunc_f64_u"));
    }
    let t = libm::trunc(x);
    if !(0.0f64..18446744073709551616.0f64).contains(&t) {
        return Err(trunc_trap("i64.trunc_f64_u"));
    }
    Ok((t as u64) as i64)
}

fn sat_f64_to_i32_s(x: f64) -> i32 {
    if x.is_nan() {
        0
    } else if x <= -2147483648.0 {
        i32::MIN
    } else if x >= 2147483648.0 {
        i32::MAX
    } else {
        libm::trunc(x) as i32
    }
}

fn sat_f64_to_i32_u(x: f64) -> i32 {
    if x.is_nan() || x <= 0.0 {
        0
    } else if x >= 4294967296.0 {
        u32::MAX as i32
    } else {
        (libm::trunc(x) as u32) as i32
    }
}

fn sat_f64_to_i64_s(x: f64) -> i64 {
    if x.is_nan() {
        0
    } else if x <= -9223372036854775808.0 {
        i64::MIN
    } else if x >= 9223372036854775808.0 {
        i64::MAX
    } else {
        libm::trunc(x) as i64
    }
}

fn sat_f64_to_i64_u(x: f64) -> i64 {
    if x.is_nan() || x <= 0.0 {
        0
    } else if x >= 18446744073709551616.0 {
        u64::MAX as i64
    } else {
        (libm::trunc(x) as u64) as i64
    }
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
            .ok_or(WasmError::Decode("truncated signed LEB128"))?;
        i += 1;
        if shift == 63 && byte != 0 && byte != 0x7f {
            return Err(WasmError::Decode("signed LEB128 too long"));
        }
        let payload = byte & 0x7f;
        result |= i64::from(payload) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && (byte & 0x40) != 0 {
                result |= (-1i64) << shift;
            }
            break;
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
        .ok_or(WasmError::Trap("operand stack underflow"))
}

/// Pop a value expected to be an `i32`.
fn pop(stack: &mut Vec<Val>) -> Result<i32, WasmError> {
    match pop_val(stack)? {
        Val::I32(v) => Ok(v),
        _other => Err(WasmError::Trap("expected i32 on stack, got")),
    }
}

/// Pop a value expected to be an `i64`.
#[allow(dead_code)]
fn pop_i64(stack: &mut Vec<Val>) -> Result<i64, WasmError> {
    match pop_val(stack)? {
        Val::I64(v) => Ok(v),
        _other => Err(WasmError::Trap("expected i64 on stack, got")),
    }
}

/// Pop a value expected to be an `f32`.
#[allow(dead_code)]
fn pop_f32(stack: &mut Vec<Val>) -> Result<f32, WasmError> {
    match pop_val(stack)? {
        Val::F32(v) => Ok(v),
        _other => Err(WasmError::Trap("expected f32 on stack, got")),
    }
}

/// Pop a value expected to be an `f64`.
#[allow(dead_code)]
fn pop_f64(stack: &mut Vec<Val>) -> Result<f64, WasmError> {
    match pop_val(stack)? {
        Val::F64(v) => Ok(v),
        _other => Err(WasmError::Trap("expected f64 on stack, got")),
    }
}

fn pop_funcref(stack: &mut Vec<Val>) -> Result<Option<usize>, WasmError> {
    match pop_val(stack)? {
        Val::FuncRef(function) => Ok(function),
        _other => Err(WasmError::Trap("expected funcref on stack, got")),
    }
}

fn i32_args_to_vals(args: &[i32]) -> Result<Vec<Val>, WasmError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(args.len())
        .map_err(|_| WasmError::Trap("invoke arguments"))?;
    values.extend(args.iter().copied().map(Val::I32));
    Ok(values)
}

fn vals_to_i32(values: Vec<Val>) -> Result<Vec<i32>, WasmError> {
    let mut integers = Vec::new();
    integers
        .try_reserve_exact(values.len())
        .map_err(|_| WasmError::Trap("invoke results"))?;
    for value in values {
        match value {
            Val::I32(number) => integers.push(number),
            _other => return Err(WasmError::Trap("invoke: expected i32 result, got")),
        }
    }
    Ok(integers)
}

/// Preflight the only instructions that grow the operand stack without first
/// popping a value. This makes both the host live-slot ceiling and allocator
/// refusal a typed trap before the instruction mutates guest state.
fn push_operand(
    stack: &mut Vec<Val>,
    value: Val,
    live_slots: usize,
    available_slots: usize,
) -> Result<(), WasmError> {
    if stack.len() >= WASM_STACK_LIMIT {
        return Err(WasmError::Trap("operand stack"));
    }
    // The dispatch loop already proved `live_slots <= available_slots`.
    if live_slots >= available_slots {
        return Err(WasmError::Trap("call stack"));
    }
    stack
        .try_reserve(1)
        .map_err(|_| WasmError::Trap("operand stack"))?;
    stack.push(value);
    Ok(())
}

fn reserve_control_growth(
    control: &mut Vec<Frame>,
    live_slots: usize,
    available_slots: usize,
    adds_live_slot: bool,
) -> Result<(), WasmError> {
    // The dispatch loop already proved the current count fits. Blocks and
    // loops add one live slot; `if` consumes its condition before pushing the
    // control frame and therefore remains count-neutral.
    if adds_live_slot && live_slots >= available_slots {
        return Err(WasmError::Trap("call stack"));
    }
    control
        .try_reserve(1)
        .map_err(|_| WasmError::Trap("control stack"))
}

/// Copy top values into a new call/result vector only after its complete
/// bounded allocation succeeds. The source stack remains unchanged on
/// allocator refusal.
#[inline(never)]
fn take_values(
    stack: &mut Vec<Val>,
    count: usize,
    allocation_error: &'static str,
) -> Result<Vec<Val>, WasmError> {
    let start = stack
        .len()
        .checked_sub(count)
        .ok_or(WasmError::Trap("result arity"))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| WasmError::Trap(allocation_error))?;
    values.extend_from_slice(&stack[start..]);
    stack.truncate(start);
    Ok(values)
}

fn take_results(stack: &mut Vec<Val>, arity: usize) -> Result<Vec<Val>, WasmError> {
    if stack.len() < arity {
        return Err(WasmError::Trap("result arity"));
    }
    take_values(stack, arity, "function results")
}

fn finish_defined(stack: &mut Vec<Val>, arity: usize) -> Result<DefinedOutcome, WasmError> {
    take_results(stack, arity).map(|values| DefinedOutcome::Values(CallValues::Owned(values)))
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
        .ok_or(WasmError::Trap("branch label"))?;
    let frame = control[idx];
    if stack.len() < frame.base + frame.branch_arity {
        return Err(WasmError::Trap("branch operand stack underflow"));
    }
    let source = stack.len() - frame.branch_arity;
    stack.copy_within(source.., frame.base);
    stack.truncate(frame.base + frame.branch_arity);
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

    #[test]
    fn every_scalar_memarg_rejects_alignment_above_its_natural_width() {
        const MEMORY_OPS: [(u8, u8); 23] = [
            (0x28, 2),
            (0x29, 3),
            (0x2A, 2),
            (0x2B, 3),
            (0x2C, 0),
            (0x2D, 0),
            (0x2E, 1),
            (0x2F, 1),
            (0x30, 0),
            (0x31, 0),
            (0x32, 1),
            (0x33, 1),
            (0x34, 2),
            (0x35, 2),
            (0x36, 2),
            (0x37, 3),
            (0x38, 2),
            (0x39, 3),
            (0x3A, 0),
            (0x3B, 1),
            (0x3C, 0),
            (0x3D, 1),
            (0x3E, 2),
        ];

        for (opcode, natural_align) in MEMORY_OPS {
            let mut valid = Module::new();
            assert!(
                valid
                    .add_function(0, 0, 0, &[opcode, natural_align, 0, 0x0B])
                    .is_ok(),
                "opcode 0x{opcode:02x} must accept its natural alignment"
            );

            let mut invalid = Module::new();
            assert!(matches!(
                invalid.add_function(0, 0, 0, &[opcode, natural_align + 1, 0, 0x0B]),
                Err(WasmError::Decode(
                    "memory alignment exceeds natural alignment"
                ))
            ));
        }
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
        assert_eq!(
            m.invoke_val(f, &[Val::I32(41)]).unwrap(),
            vec![Val::I32(42)]
        );
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

    #[test]
    fn branch_value_preservation_reuses_the_operand_allocation() {
        let mut stack = Vec::with_capacity(8);
        stack.extend([Val::I32(10), Val::I32(99), Val::I32(20), Val::I32(30)]);
        let capacity = stack.capacity();
        let mut control = vec![Frame {
            base: 1,
            branch_arity: 2,
            cont: 42,
            is_loop: false,
        }];

        assert_eq!(do_branch(&mut stack, &mut control, 0), Ok(42));
        assert_eq!(stack, [Val::I32(10), Val::I32(20), Val::I32(30)]);
        assert_eq!(stack.capacity(), capacity);
        assert!(control.is_empty());
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
    fn bulk_memory_copy_is_overlap_safe_and_fill_uses_low_byte() {
        let mut module = Module::new();
        // (dst, src, len) memory.copy
        let copy = module
            .add_function(
                3,
                0,
                0,
                &[
                    0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0A, 0x00, 0x00, 0x0B,
                ],
            )
            .unwrap();
        // (dst, value, len) memory.fill
        let fill = module
            .add_function(
                3,
                0,
                0,
                &[0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0B, 0x00, 0x0B],
            )
            .unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.memory_mut()[0..8].copy_from_slice(b"abcdefgh");

        instance.invoke(copy, &[2, 0, 6]).unwrap();
        assert_eq!(&instance.memory()[0..8], b"ababcdef");
        instance.invoke(fill, &[1, 0x1234, 3]).unwrap();
        assert_eq!(&instance.memory()[0..8], b"a444cdef");
    }

    #[test]
    fn bulk_memory_traps_atomically_on_bounds_or_fuel() {
        let body = [0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0B, 0x00, 0x0B];
        let mut module = Module::new_with_limits(Limits {
            max_steps: 6,
            ..Limits::default()
        });
        let fill = module.add_function(3, 0, 0, &body).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.memory_mut()[0..8].fill(0xA5);

        assert!(matches!(
            instance.invoke(fill, &[0, 7, 64]),
            Err(WasmError::Trap("step budget"))
        ));
        assert_eq!(&instance.memory()[0..8], &[0xA5; 8]);
        assert!(matches!(
            instance.invoke(fill, &[65_530, 7, 16]),
            Err(WasmError::Trap("bulk memory access out of bounds"))
        ));
        assert_eq!(&instance.memory()[0..8], &[0xA5; 8]);
    }

    #[test]
    fn bulk_memory_decoder_rejects_other_memories_and_unknown_subopcodes() {
        let mut module = Module::new();
        assert!(matches!(
            module.add_function(0, 0, 0, &[0xFC, 0x0A, 0x01, 0x00, 0x0B]),
            Err(WasmError::Decode("memory.copy memory indices must be 0"))
        ));
        assert!(matches!(
            module.add_function(0, 0, 0, &[0xFC, 0x12, 0x0B]),
            Err(WasmError::Decode("unsupported 0xfc opcode"))
        ));
    }

    fn passive_data_module(data: &[u8], body: &[u8], include_data_count: bool) -> Vec<u8> {
        fn section(module: &mut Vec<u8>, id: u8, payload: &[u8]) {
            assert!(payload.len() < 128);
            module.extend_from_slice(&[id, payload.len() as u8]);
            module.extend_from_slice(payload);
        }

        assert!(data.len() < 128 && body.len() < 126);
        let mut wasm = alloc::vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        // (func (param i32 i32 i32))
        section(&mut wasm, 1, &[0x01, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x00]);
        section(&mut wasm, 3, &[0x01, 0x00]);
        section(&mut wasm, 5, &[0x01, 0x00, 0x01]);
        if include_data_count {
            section(&mut wasm, 12, &[0x01]);
        }
        let mut code = alloc::vec![0x01, (body.len() + 1) as u8, 0x00];
        code.extend_from_slice(body);
        section(&mut wasm, 10, &code);
        let mut data_section = alloc::vec![0x01, 0x01, data.len() as u8];
        data_section.extend_from_slice(data);
        section(&mut wasm, 11, &data_section);
        wasm
    }

    #[test]
    fn passive_data_init_drop_is_instance_local_and_spec_exact() {
        let body = [
            0x20, 0x00, 0x20, 0x01, 0x20, 0x02, // dst, src, len
            0xFC, 0x08, 0x00, 0x00, // memory.init data 0 memory 0
            0xFC, 0x09, 0x00, // data.drop 0
            0x0B,
        ];
        let wasm = passive_data_module(b"hello", &body, true);
        let mut first = Module::from_bytes(&wasm).unwrap().instantiate().unwrap();
        let mut second = Module::from_bytes(&wasm).unwrap().instantiate().unwrap();

        first.invoke(0, &[10, 1, 3]).unwrap();
        assert_eq!(&first.memory()[10..13], b"ell");
        assert!(matches!(
            first.invoke(0, &[20, 0, 1]),
            Err(WasmError::Trap(_))
        ));
        // A dropped segment is empty: exactly offset=0,length=0 remains valid.
        first.invoke(0, &[65_536, 0, 0]).unwrap();

        // Dropping in one instance must not mutate the module definition or a
        // sibling instance's segment state.
        second.invoke(0, &[4, 0, 5]).unwrap();
        assert_eq!(&second.memory()[4..9], b"hello");
    }

    #[test]
    fn bulk_data_validation_requires_data_count_and_checks_indices() {
        let init_zero = [
            0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x08, 0x00, 0x00, 0x0B,
        ];
        assert!(matches!(
            Module::from_bytes(&passive_data_module(b"x", &init_zero, false)),
            Err(WasmError::Decode(
                "validation: memory.init requires data count"
            ))
        ));

        let bad_index = [
            0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x08, 0x01, 0x00, 0x0B,
        ];
        assert!(matches!(
            Module::from_bytes(&passive_data_module(b"x", &bad_index, true)),
            Err(WasmError::Decode(
                "validation: memory.init data segment index"
            ))
        ));
    }

    #[test]
    fn memory_init_fuel_trap_is_atomic() {
        let body = [
            0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x08, 0x00, 0x00, 0x0B,
        ];
        let wasm = passive_data_module(&[0xA5; 64], &body, true);
        let module = Module::from_bytes_with(
            &wasm,
            Limits {
                max_steps: 6,
                ..Limits::default()
            },
        )
        .unwrap();
        let mut instance = module.instantiate().unwrap();
        assert!(matches!(
            instance.invoke(0, &[0, 0, 64]),
            Err(WasmError::Trap("step budget"))
        ));
        assert_eq!(&instance.memory()[..8], &[0; 8]);
    }

    fn passive_elem_module() -> Vec<u8> {
        fn section(module: &mut Vec<u8>, id: u8, payload: &[u8]) {
            assert!(payload.len() < 128);
            module.extend_from_slice(&[id, payload.len() as u8]);
            module.extend_from_slice(payload);
        }
        fn body(code: &mut Vec<u8>, instructions: &[u8]) {
            assert!(instructions.len() < 126);
            code.push((instructions.len() + 1) as u8);
            code.push(0); // no declared locals
            code.extend_from_slice(instructions);
        }

        let mut wasm = alloc::vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        // type0 (i32,i32,i32)->(), type1 ()->i32, type2 (i32)->i32
        section(
            &mut wasm,
            1,
            &[
                0x03, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x00, 0x60, 0x00, 0x01, 0x7F, 0x60, 0x01, 0x7F,
                0x01, 0x7F,
            ],
        );
        // init/drop, return42, return7, indirect(index), table.copy
        section(&mut wasm, 3, &[0x05, 0x00, 0x01, 0x01, 0x02, 0x00]);
        section(&mut wasm, 4, &[0x01, 0x70, 0x00, 0x04]);
        // Passive legacy funcref segment containing functions 1 and 2.
        section(&mut wasm, 9, &[0x01, 0x01, 0x00, 0x02, 0x01, 0x02]);

        let mut code = alloc::vec![0x05];
        body(
            &mut code,
            &[
                0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0C, 0x00, 0x00, 0xFC, 0x0D, 0x00, 0x0B,
            ],
        );
        body(&mut code, &[0x41, 0x2A, 0x0B]);
        body(&mut code, &[0x41, 0x07, 0x0B]);
        body(&mut code, &[0x20, 0x00, 0x11, 0x01, 0x00, 0x0B]);
        body(
            &mut code,
            &[
                0x20, 0x00, 0x20, 0x01, 0x20, 0x02, 0xFC, 0x0E, 0x00, 0x00, 0x0B,
            ],
        );
        section(&mut wasm, 10, &code);
        wasm
    }

    #[test]
    fn passive_elem_init_drop_copy_is_instance_local_and_overlap_safe() {
        let wasm = passive_elem_module();
        let mut first = Module::from_bytes(&wasm).unwrap().instantiate().unwrap();
        let mut second = Module::from_bytes(&wasm).unwrap().instantiate().unwrap();

        first.invoke(0, &[1, 0, 2]).unwrap();
        assert_eq!(first.invoke(3, &[1]).unwrap(), vec![42]);
        assert_eq!(first.invoke(3, &[2]).unwrap(), vec![7]);
        assert!(matches!(
            first.invoke(0, &[0, 0, 1]),
            Err(WasmError::Trap(_))
        ));
        first.invoke(0, &[4, 0, 0]).unwrap();

        // Overlap-safe table.copy moves [func1,func2] from 1..3 to 0..2.
        first.invoke(4, &[0, 1, 2]).unwrap();
        assert_eq!(first.invoke(3, &[0]).unwrap(), vec![42]);
        assert_eq!(first.invoke(3, &[1]).unwrap(), vec![7]);

        // A sibling instance still owns a live copy of the passive segment.
        second.invoke(0, &[0, 0, 2]).unwrap();
        assert_eq!(second.invoke(3, &[0]).unwrap(), vec![42]);
    }

    #[test]
    fn table_init_fuel_trap_is_atomic() {
        let module = Module::from_bytes_with(
            &passive_elem_module(),
            Limits {
                max_steps: 5,
                ..Limits::default()
            },
        )
        .unwrap();
        let mut instance = module.instantiate().unwrap();
        assert!(matches!(
            instance.invoke(0, &[0, 0, 2]),
            Err(WasmError::Trap("step budget"))
        ));
        assert!(matches!(instance.invoke(3, &[0]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn active_element_segment_requires_a_declared_table() {
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // () -> ()
            0x03, 0x02, 0x01, 0x00, // one function
            0x09, 0x07, 0x01, 0x00, 0x41, 0x00, 0x0B, 0x01, 0x00, // active elem
            0x0A, 0x04, 0x01, 0x02, 0x00, 0x0B, // function body
        ];
        assert!(matches!(
            Module::from_bytes(&wasm),
            Err(WasmError::Decode("active element segment table index"))
        ));
    }

    #[test]
    fn data_count_has_spec_section_order_and_must_match_data_section() {
        let mut wasm = alloc::vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        wasm.extend_from_slice(&[0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F]);
        wasm.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
        wasm.extend_from_slice(&[0x05, 0x03, 0x01, 0x00, 0x01]);
        wasm.extend_from_slice(&[0x0C, 0x01, 0x00]);
        wasm.extend_from_slice(&[0x0A, 0x06, 0x01, 0x04, 0x00, 0x41, 0x00, 0x0B]);
        wasm.extend_from_slice(&[0x0B, 0x01, 0x00]);
        assert_eq!(
            Module::from_bytes(&wasm).unwrap().eval(&[]).unwrap(),
            vec![Val::I32(0)]
        );

        let count_payload = wasm.iter().position(|&byte| byte == 0x0C).unwrap() + 2;
        wasm[count_payload] = 1;
        assert!(matches!(
            Module::from_bytes(&wasm),
            Err(WasmError::Decode("data count does not match data section"))
        ));
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
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F, // import: "env"."h" func type 0
            0x02, 0x09, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x01, 0x68, 0x00, 0x00,
            // func: type 0 (the defined function, index 1)
            0x03, 0x02, 0x01, 0x00, // code: call 0 (the import) ; end
            0x0A, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0B,
        ];
        let m = Module::from_bytes(&wasm).unwrap();
        // defined function is index 1 (import occupies 0)
        assert!(matches!(m.invoke(1, &[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn bounded_host_results_stay_inline_for_suspended_callers() {
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7F, 0x02, 0x09, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x01, 0x68, 0x00, 0x00, 0x03, 0x02,
            0x01, 0x00, 0x0A, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0B,
        ];
        let mut module = Module::from_bytes(&wasm).unwrap();
        module
            .bind_import_at_bounded(0, |args, results, _| {
                assert!(args.is_empty());
                assert_eq!(results.len(), 1);
                results[0] = 42;
                Ok(())
            })
            .unwrap();
        let mut memory = [0; 1];

        assert!(matches!(
            module.call_host(0, &[], &mut memory, false),
            Ok(CallValues::BoundedI32 { len: 1, .. })
        ));
        assert_eq!(
            module
                .call_host(0, &[], &mut memory, true)
                .unwrap()
                .into_vec()
                .unwrap(),
            [Val::I32(42)]
        );
    }

    #[test]
    fn typed_host_results_stay_inline_for_suspended_callers() {
        // (module (import "env" "h" (func (result i64)))
        //         (func (result i64) call 0))
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7E, 0x02, 0x09, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x01, 0x68, 0x00, 0x00, 0x03, 0x02,
            0x01, 0x00, 0x0A, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0B,
        ];
        let mut module = Module::from_bytes(&wasm).unwrap();
        module
            .bind_import_typed_in_place("env", "h", |args, results, _| {
                assert!(args.is_empty());
                assert_eq!(results.len(), 1);
                results[0] = Val::I64(42);
                Ok(())
            })
            .unwrap();
        let mut memory = [0; 1];

        assert!(matches!(
            module.call_host(0, &[], &mut memory, false),
            Ok(CallValues::BoundedTyped { len: 1, .. })
        ));
        assert_eq!(
            module
                .call_host(0, &[], &mut memory, true)
                .unwrap()
                .into_vec()
                .unwrap(),
            [Val::I64(42)]
        );
    }

    // Acceptance (tinyvm.6): load a standard .wasm module and invoke.
    //
    // Equivalent to:  (module (func (result i32) i32.const 42))
    #[test]
    fn module_from_bytes_returns_42() {
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // \0asm, version 1
            // type section: 1 type, func () -> (i32)
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F, // function section: 1 func, type 0
            0x03, 0x02, 0x01, 0x00, // code section: 1 body: 0 locals, i32.const 42, end
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
            0x01, 0x06, 0x01, 0x60, 0x01, 0x7F, 0x01, 0x7F, // func: type 0
            0x03, 0x02, 0x01, 0x00, // export "inc" func 0  -> skipped
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

    // Opcode: call_indirect + table/elem.
    #[test]
    fn call_indirect_dispatches_through_table() {
        let mut m = Module::new();
        let callee = m
            .add_function(1, 0, 1, &[0x20, 0x00, 0x41, 0x01, 0x6A, 0x0B])
            .unwrap();
        let t = m.add_type(1, 1);
        m.add_table(4);
        m.set_table_entry(0, callee);
        let body = [0x20, 0x00, 0x41, 0x00, 0x11, t as u8, 0x00, 0x0B];
        let caller = m.add_function(1, 0, 1, &body).unwrap();
        assert_eq!(
            m.invoke_val(caller, &[Val::I32(41)]).unwrap(),
            vec![Val::I32(42)]
        );
    }

    #[test]
    fn call_indirect_traps_on_bounds_empty_and_mismatch() {
        let build = |ttype: (usize, usize), fill: bool, tab_index: u8| {
            let mut m = Module::new();
            let callee = m
                .add_function(1, 0, 1, &[0x20, 0x00, 0x41, 0x01, 0x6A, 0x0B])
                .unwrap();
            let t = m.add_type(ttype.0, ttype.1);
            m.add_table(4);
            if fill {
                m.set_table_entry(0, callee);
            }
            let body = [0x20, 0x00, 0x41, tab_index, 0x11, t as u8, 0x00, 0x0B];
            let caller = m.add_function(1, 0, 1, &body).unwrap();
            m.invoke_val(caller, &[Val::I32(1)])
        };
        assert!(matches!(build((1, 1), true, 9), Err(WasmError::Trap(_)))); // out of bounds
        assert!(matches!(build((1, 1), false, 0), Err(WasmError::Trap(_)))); // empty slot
        assert!(matches!(build((0, 1), true, 0), Err(WasmError::Trap(_)))); // signature mismatch
    }

    #[test]
    fn from_bytes_table_elem_call_indirect() {
        // (module (type (func (result i32))) (func (result i32) i32.const 7)
        //   (table 1 funcref) (elem (i32.const 0) 0)
        //   (func (result i32) i32.const 0 call_indirect 0))
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F, // type () -> i32
            0x03, 0x03, 0x02, 0x00, 0x00, // funcs: type 0, type 0
            0x04, 0x04, 0x01, 0x70, 0x00, 0x01, // table 1 funcref
            0x09, 0x07, 0x01, 0x00, 0x41, 0x00, 0x0B, 0x01,
            0x00, // elem (i32.const 0) [func 0]
            0x0A, 0x0E, 0x02, // code: 2 bodies
            0x04, 0x00, 0x41, 0x07, 0x0B, // func 0: i32.const 7
            0x07, 0x00, 0x41, 0x00, 0x11, 0x00, 0x00,
            0x0B, // func 1: i32.const 0 ; call_indirect 0 0
        ];
        let m = Module::from_bytes(wasm).unwrap();
        assert_eq!(m.invoke(1, &[]).unwrap(), vec![7]);
    }

    // Section: start function.
    #[test]
    fn run_start_executes_the_start_function() {
        use std::cell::Cell;
        use std::rc::Rc;
        let flag = Rc::new(Cell::new(false));
        let f = flag.clone();
        let mut m = Module::new();
        // host 0 records that it ran
        m.add_host_function(0, 0, move |_, _| {
            f.set(true);
            Ok(vec![])
        });
        // start function calls host 0
        let start = m.add_function(0, 0, 0, &[0x10, 0x00, 0x0B]).unwrap();
        m.set_start(start);
        assert_eq!(m.start_index(), Some(start));
        assert!(!flag.get());
        m.run_start().unwrap();
        assert!(flag.get());
    }

    #[test]
    fn module_start_section_is_parsed() {
        // (module (func) (start 0)) — func 0 is an empty no-result function.
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type () -> ()
            0x03, 0x02, 0x01, 0x00, // func: type 0
            0x08, 0x01, 0x00, // start section: func 0
            0x0A, 0x04, 0x01, 0x02, 0x00, 0x0B, // code: (empty body) end
        ];
        let m = Module::from_bytes(&wasm).unwrap();
        assert_eq!(m.start_index(), Some(0));
        m.run_start().unwrap(); // runs the empty start function without error
    }

    // Gate: module globals + global.get/set.
    #[test]
    fn global_get_set_roundtrip() {
        let mut m = Module::new();
        let g = m.add_global(Val::I32(10), true);
        assert_eq!(g, 0);
        // global.set 0 (const 7) ; global.get 0
        let body = [0x41, 0x07, 0x24, 0x00, 0x23, 0x00, 0x0B];
        let f = m.add_function(0, 0, 1, &body).unwrap();
        assert_eq!(m.invoke_val(f, &[]).unwrap(), vec![Val::I32(7)]);
    }

    #[test]
    fn global_get_initial_value() {
        let mut m = Module::new();
        m.add_global(Val::I64(1234), false);
        let f = m.add_function(0, 0, 1, &[0x23, 0x00, 0x0B]).unwrap();
        assert_eq!(m.invoke_val(f, &[]).unwrap(), vec![Val::I64(1234)]);
    }

    #[test]
    fn global_set_immutable_traps() {
        let mut m = Module::new();
        m.add_global(Val::I32(1), false);
        let f = m
            .add_function(0, 0, 0, &[0x41, 0x02, 0x24, 0x00, 0x0B])
            .unwrap();
        assert!(matches!(m.invoke_val(f, &[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn module_with_global_section() {
        // (module (global i32 (i32.const 99)) (func (result i32) global.get 0))
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F, // type () -> i32
            0x03, 0x02, 0x01, 0x00, // func: type 0
            // global section: 1 global, i32 mutable=0, init i32.const 99, end
            0x06, 0x07, 0x01, 0x7F, 0x00, 0x41, 0xE3, 0x00, 0x0B, 0x0A, 0x06, 0x01, 0x04, 0x00,
            0x23, 0x00, 0x0B, // code: global.get 0
        ];
        let m = Module::from_bytes(&wasm).unwrap();
        assert_eq!(m.invoke(0, &[]).unwrap(), vec![99]);
    }

    // Gate (tinyvm.18): resolve and invoke an exported function by name.
    #[test]
    fn invoke_exported_function_by_name() {
        // (module (func (export "answer") (result i32) i32.const 42))
        let wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F, // type () -> i32
            0x03, 0x02, 0x01, 0x00, // func: type 0
            // export "answer" func 0
            0x07, 0x0A, 0x01, 0x06, 0x61, 0x6E, 0x73, 0x77, 0x65, 0x72, 0x00, 0x00, 0x0A, 0x06,
            0x01, 0x04, 0x00, 0x41, 0x2A, 0x0B, // code: i32.const 42
        ];
        let m = Module::from_bytes(&wasm).unwrap();
        assert_eq!(m.export_index("answer"), Some(0));
        assert_eq!(m.invoke_by_name("answer", &[]).unwrap(), vec![Val::I32(42)]);
        assert!(matches!(
            m.invoke_by_name("missing", &[]),
            Err(WasmError::Trap(_))
        ));
    }

    // Family: numeric conversions. Source value arrives as a typed param, so
    // no float/i64 const opcodes are needed: body = local.get 0 ; <op> ; end.
    fn conv1(op: u8, arg: Val) -> Result<Vec<Val>, WasmError> {
        let mut m = Module::new();
        let f = m.add_function(1, 0, 1, &[0x20, 0x00, op, 0x0B])?;
        m.invoke_val(f, &[arg])
    }

    #[test]
    fn conv_wrap_extend_reinterpret() {
        // i32.wrap_i64: low 32 bits of 0x1_0000_002A = 42
        assert_eq!(
            conv1(0xA7, Val::I64(0x1_0000_002A)).unwrap(),
            vec![Val::I32(42)]
        );
        // i64.extend_i32_s(-1) = -1 ; extend_i32_u(-1) = 0xFFFFFFFF
        assert_eq!(conv1(0xAC, Val::I32(-1)).unwrap(), vec![Val::I64(-1)]);
        assert_eq!(
            conv1(0xAD, Val::I32(-1)).unwrap(),
            vec![Val::I64(4294967295)]
        );
        // i32.reinterpret_f32(1.0) = 0x3F800000 ; and back
        assert_eq!(
            conv1(0xBC, Val::F32(1.0)).unwrap(),
            vec![Val::I32(0x3F80_0000)]
        );
        assert_eq!(
            conv1(0xBE, Val::I32(0x3F80_0000)).unwrap(),
            vec![Val::F32(1.0)]
        );
        assert_eq!(
            conv1(0xBD, Val::F64(1.5)).unwrap(),
            vec![Val::I64(0x3FF8_0000_0000_0000u64 as i64)]
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn conv_int_float_roundtrips() {
        assert_eq!(conv1(0xB2, Val::I32(42)).unwrap(), vec![Val::F32(42.0)]); // f32.convert_i32_s
        assert_eq!(
            conv1(0xB3, Val::I32(-1)).unwrap(),
            vec![Val::F32(4294967295.0)]
        ); // convert_i32_u
        assert_eq!(conv1(0xB9, Val::I64(-5)).unwrap(), vec![Val::F64(-5.0)]); // f64.convert_i64_s
        assert_eq!(conv1(0xBB, Val::F32(1.5)).unwrap(), vec![Val::F64(1.5)]); // promote
        assert_eq!(conv1(0xB6, Val::F64(1.5)).unwrap(), vec![Val::F32(1.5)]); // demote
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn conv_trunc_happy_and_traps() {
        assert_eq!(conv1(0xA8, Val::F32(42.9)).unwrap(), vec![Val::I32(42)]); // happy
        assert!(matches!(
            conv1(0xA8, Val::F32(f32::NAN)),
            Err(WasmError::Trap(_))
        ));
        assert!(matches!(
            conv1(0xA8, Val::F32(3e9)),
            Err(WasmError::Trap(_))
        )); // > i32::MAX
        assert!(matches!(
            conv1(0xA8, Val::F32(f32::INFINITY)),
            Err(WasmError::Trap(_))
        ));
        assert!(matches!(
            conv1(0xA9, Val::F32(-1.5)),
            Err(WasmError::Trap(_))
        )); // u: past -1
        assert_eq!(conv1(0xA9, Val::F32(-0.5)).unwrap(), vec![Val::I32(0)]); // (-1,0] -> 0
        assert!(matches!(
            conv1(0xAF, Val::F32(1.9e19)),
            Err(WasmError::Trap(_))
        )); // i64.trunc_f32_u > 2^64
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
        assert!(
            matches!(f32_bin(-0.0, 0.0, 0x96)[0], Val::F32(x) if x == 0.0 && x.is_sign_negative())
        );
        assert!(
            matches!(f32_bin(-0.0, 0.0, 0x97)[0], Val::F32(x) if x == 0.0 && x.is_sign_positive())
        );
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
        assert_eq!(
            m.invoke_val(f, &[Val::I64(42)]).unwrap(),
            vec![Val::I64(42)]
        );
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
        // 0xD3 is ref.eq, outside the single-table funcref profile.
        assert!(matches!(
            m.add_function(0, 0, 1, &[0xD3, 0x0B]),
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
    // --- sections that used to be skipped, and the gates they carry ---

    /// Build a one-function module: `(memory min [max])`, optional data
    /// segments, and `body` as the exported entry returning i32.
    fn mem_module(min: u32, max: Option<u32>, data: &[(u32, &[u8])], body: &[u8]) -> Vec<u8> {
        let mut m: Vec<u8> = alloc::vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        m.extend_from_slice(&[0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F]);
        m.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
        let mut mem_sec: Vec<u8> = alloc::vec![0x01];
        match max {
            None => {
                mem_sec.push(0x00);
                mem_sec.push(min as u8);
            }
            Some(mx) => {
                mem_sec.push(0x01);
                mem_sec.push(min as u8);
                mem_sec.push(mx as u8);
            }
        }
        m.push(0x05);
        m.push(mem_sec.len() as u8);
        m.extend_from_slice(&mem_sec);
        m.extend_from_slice(&[0x07, 0x08, 0x01, 0x04, b'm', b'a', b'i', b'n', 0x00, 0x00]);
        let mut code: Vec<u8> = alloc::vec![0x01, (body.len() + 1) as u8, 0x00];
        code.extend_from_slice(body);
        m.push(0x0A);
        m.push(code.len() as u8);
        m.extend_from_slice(&code);
        if !data.is_empty() {
            let mut ds: Vec<u8> = alloc::vec![data.len() as u8];
            for (off, bytes) in data {
                ds.extend_from_slice(&[0x00, 0x41, *off as u8, 0x0B, bytes.len() as u8]);
                ds.extend_from_slice(bytes);
            }
            m.push(0x0B);
            m.push(ds.len() as u8);
            m.extend_from_slice(&ds);
        }
        m
    }

    #[test]
    fn declared_locals_default_to_their_own_type() {
        // (func (result i64) (local i64) local.get 0) -> i64 0, not i32 0.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7E, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6D, 0x61, 0x69, 0x6E, 0x00,
            0x00, 0x0A, 0x08, 0x01, 0x06, 0x01, 0x01, 0x7E, 0x20, 0x00, 0x0B,
        ];
        assert_eq!(eval(wasm).unwrap(), alloc::vec![Val::I64(0)]);
    }

    #[test]
    fn data_section_initialises_linear_memory() {
        let body = &[0x41, 0x00, 0x28, 0x00, 0x00, 0x0B];
        let wasm = mem_module(1, None, &[(0, &[0xEF, 0xBE, 0xAD, 0xDE])], body);
        assert_eq!(eval(&wasm).unwrap(), alloc::vec![Val::I32(-559038737i32)]);
    }

    #[test]
    fn memory_size_reports_the_declared_minimum() {
        let wasm = mem_module(3, None, &[], &[0x3F, 0x00, 0x0B]);
        assert_eq!(eval(&wasm).unwrap(), alloc::vec![Val::I32(3)]);
    }

    #[test]
    fn memory_grow_respects_the_declared_maximum() {
        // grow by 5 with max 2 -> -1, and the memory keeps its old size.
        let wasm = mem_module(1, Some(2), &[], &[0x41, 0x05, 0x40, 0x00, 0x0B]);
        assert_eq!(eval(&wasm).unwrap(), alloc::vec![Val::I32(-1)]);
        let wasm = mem_module(
            1,
            Some(2),
            &[],
            &[0x41, 0x05, 0x40, 0x00, 0x1A, 0x3F, 0x00, 0x0B],
        );
        assert_eq!(eval(&wasm).unwrap(), alloc::vec![Val::I32(1)]);
    }

    #[test]
    fn a_data_segment_past_declared_memory_is_rejected() {
        // Same module twice: the second segment overruns the declared page.
        let inbounds: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7F, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x08, 0x01, 0x04,
            0x6D, 0x61, 0x69, 0x6E, 0x00, 0x00, 0x0A, 0x0B, 0x01, 0x09, 0x00, 0x41, 0xFC, 0xFF,
            0x03, 0x28, 0x00, 0x00, 0x0B, 0x0B, 0x0C, 0x01, 0x00, 0x41, 0xFC, 0xFF, 0x03, 0x0B,
            0x04, 0x01, 0x02, 0x03, 0x04,
        ];
        assert!(Module::from_bytes(inbounds).is_ok());
        let overruns: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7F, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x08, 0x01, 0x04,
            0x6D, 0x61, 0x69, 0x6E, 0x00, 0x00, 0x0A, 0x09, 0x01, 0x07, 0x00, 0x41, 0x00, 0x28,
            0x00, 0x00, 0x0B, 0x0B, 0x0C, 0x01, 0x00, 0x41, 0xFE, 0xFF, 0x03, 0x0B, 0x04, 0x01,
            0x02, 0x03, 0x04,
        ];
        assert!(matches!(
            Module::from_bytes(overruns),
            Err(WasmError::Decode(_))
        ));
    }

    #[test]
    fn call_indirect_requires_an_exact_type_match() {
        // table[0] is (i64) -> i32, called through a declared (i32) -> i32:
        // the arities agree, the value types do not, so it must trap.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0F, 0x03, 0x60, 0x00, 0x01,
            0x7F, 0x60, 0x01, 0x7F, 0x01, 0x7F, 0x60, 0x01, 0x7E, 0x01, 0x7F, 0x03, 0x03, 0x02,
            0x00, 0x02, 0x04, 0x04, 0x01, 0x70, 0x00, 0x01, 0x07, 0x08, 0x01, 0x04, 0x6D, 0x61,
            0x69, 0x6E, 0x00, 0x00, 0x09, 0x07, 0x01, 0x00, 0x41, 0x00, 0x0B, 0x01, 0x01, 0x0A,
            0x10, 0x02, 0x09, 0x00, 0x41, 0x01, 0x41, 0x00, 0x11, 0x01, 0x00, 0x0B, 0x04, 0x00,
            0x41, 0x07, 0x0B,
        ];
        assert!(matches!(eval(wasm), Err(WasmError::Trap(_))));
    }

    #[test]
    fn binding_an_import_binds_every_slot_with_that_name() {
        // Two imports of the same (module, field); calling the second works.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0A, 0x02, 0x60, 0x01, 0x7F,
            0x01, 0x7F, 0x60, 0x00, 0x01, 0x7F, 0x02, 0x17, 0x02, 0x04, 0x68, 0x6F, 0x73, 0x74,
            0x03, 0x64, 0x75, 0x70, 0x00, 0x00, 0x04, 0x68, 0x6F, 0x73, 0x74, 0x03, 0x64, 0x75,
            0x70, 0x00, 0x00, 0x03, 0x02, 0x01, 0x01, 0x07, 0x08, 0x01, 0x04, 0x6D, 0x61, 0x69,
            0x6E, 0x00, 0x02, 0x0A, 0x08, 0x01, 0x06, 0x00, 0x41, 0x05, 0x10, 0x01, 0x0B,
        ];
        let mut m = Module::from_bytes(wasm).unwrap();
        assert_eq!(m.imports().len(), 2);
        assert!(matches!(m.eval(&[]), Err(WasmError::Trap(_))));
        m.bind_import("host", "dup", |args, _mem| Ok(alloc::vec![args[0] * 3]))
            .unwrap();
        assert_eq!(m.eval(&[]).unwrap(), alloc::vec![Val::I32(15)]);
    }

    #[test]
    fn legacy_i32_host_binding_rejects_a_typed_import() {
        // The import declares (result i64); the legacy host ABI is i32-only,
        // so binding fails atomically rather than installing a callback that
        // can only trap when called.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x09, 0x02, 0x60, 0x00, 0x01,
            0x7E, 0x60, 0x00, 0x01, 0x7E, 0x02, 0x0C, 0x01, 0x04, 0x68, 0x6F, 0x73, 0x74, 0x03,
            0x67, 0x65, 0x74, 0x00, 0x00, 0x03, 0x02, 0x01, 0x01, 0x07, 0x08, 0x01, 0x04, 0x6D,
            0x61, 0x69, 0x6E, 0x00, 0x01, 0x0A, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0B,
        ];
        let mut m = Module::from_bytes(wasm).unwrap();
        assert!(matches!(
            m.bind_import("host", "get", |_args, _mem| Ok(alloc::vec![7])),
            Err(WasmError::Trap("host ABI is i32-only; import declares"))
        ));
        assert!(matches!(m.eval(&[]), Err(WasmError::Trap(_))));
    }

    #[test]
    fn deep_recursion_traps_instead_of_overflowing_the_native_stack() {
        // f(x) = f(x + 1): unbounded recursion is a loud trap, not a crash.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0A, 0x02, 0x60, 0x00, 0x01,
            0x7F, 0x60, 0x01, 0x7F, 0x01, 0x7F, 0x03, 0x03, 0x02, 0x00, 0x01, 0x07, 0x08, 0x01,
            0x04, 0x6D, 0x61, 0x69, 0x6E, 0x00, 0x00, 0x0A, 0x12, 0x02, 0x06, 0x00, 0x41, 0x01,
            0x10, 0x01, 0x0B, 0x09, 0x00, 0x20, 0x00, 0x41, 0x01, 0x6A, 0x10, 0x01, 0x0B,
        ];
        assert!(matches!(eval(wasm), Err(WasmError::Trap("call depth"))));
    }
    // --- the load gate ---

    #[test]
    fn an_invalid_body_is_rejected_at_load_not_at_run() {
        // (func (result i32) i32.add) — i32.add on an empty stack.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7F, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, b'm', b'a', b'i', b'n', 0x00,
            0x00, 0x0A, 0x05, 0x01, 0x03, 0x00, 0x6A, 0x0B,
        ];
        // No Module comes out, so nothing reaches the interpreter.
        assert!(matches!(
            Module::from_bytes(wasm),
            Err(WasmError::Decode(_))
        ));
        assert!(matches!(eval(wasm), Err(WasmError::Decode(_))));
    }

    #[test]
    fn an_out_of_range_index_is_a_load_error() {
        // (func (result i32) local.get 99) with no locals.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7F, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, b'm', b'a', b'i', b'n', 0x00,
            0x00, 0x0A, 0x06, 0x01, 0x04, 0x00, 0x20, 0x63, 0x0B,
        ];
        assert!(matches!(
            Module::from_bytes(wasm),
            Err(WasmError::Decode(_))
        ));
    }

    #[test]
    fn validation_keeps_run_time_traps_at_run_time() {
        // A valid module that divides by zero: this one must load fine and
        // trap while running — the gate must not swallow execution semantics.
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
            0x7F, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, b'm', b'a', b'i', b'n', 0x00,
            0x00, 0x0A, 0x09, 0x01, 0x07, 0x00, 0x41, 0x01, 0x41, 0x00, 0x6D, 0x0B,
        ];
        assert!(Module::from_bytes(wasm).is_ok());
        assert!(matches!(eval(wasm), Err(WasmError::Trap(_))));
    }
}
