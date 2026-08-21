//! Load-time validation: a module is proved before it becomes a [`Module`].
//!
//! `from_bytes` decodes, then runs this pass, and only then hands out a
//! `Module`. A module that fails here never becomes something you can invoke,
//! so an invalid program is a `Decode` error at load time rather than a `Trap`
//! discovered halfway through execution. That is the load gate; the execution
//! trap conditions (division by zero, out-of-bounds access, unreachable) stay
//! where they belong, at run time.
//!
//! This is the standard WASM validation algorithm over the already-decoded
//! instruction list: an abstract operand stack of value types, a control stack
//! of blocks, and the polymorphic-stack rule for code after `br`/`return`/
//! `unreachable`.

use alloc::vec::Vec;

use super::{FuncType, Op, WasmError};

const I32: u8 = 0x7F;
const I64: u8 = 0x7E;
const F32: u8 = 0x7D;
const F64: u8 = 0x7C;
/// Empty block type: the block leaves nothing behind.
const VOID: u8 = 0x40;
/// The bottom type produced by an unreachable stack: matches anything.
const ANY: u8 = 0x00;

/// What a function body needs to know about the module around it.
pub(super) struct ModuleCtx<'a> {
    /// Declared function types.
    pub types: &'a [FuncType],
    /// Type index per function index (imported functions first).
    pub func_sigs: &'a [usize],
    /// Value type per global.
    pub globals: &'a [u8],
    /// DataCount section value. Bulk data instructions require it even when
    /// the final data section is available to this non-streaming decoder.
    pub data_count: Option<usize>,
    /// Number of element segments and whether table index zero exists.
    pub elem_count: usize,
    pub has_table: bool,
}

/// A control frame: a block, loop, `if`, or the function body itself.
struct Ctrl {
    /// Type a branch to this label carries (`VOID` for none, and always
    /// `VOID` for a loop, whose label is its start).
    label: u8,
    /// Type the frame leaves behind when it ends normally.
    result: u8,
    /// Operand-stack height when the frame was entered.
    height: usize,
    /// Set after `br`/`br_table`/`return`/`unreachable`: the rest of the frame
    /// is dead code, validated against a polymorphic stack.
    unreachable: bool,
    /// `if` frames without an `else` may not produce a result.
    if_without_else: bool,
}

struct V<'a> {
    m: &'a ModuleCtx<'a>,
    locals: &'a [u8],
    result: u8,
    stack: Vec<u8>,
    ctrl: Vec<Ctrl>,
}

fn type_error() -> WasmError {
    WasmError::Decode("validation: type mismatch")
}

impl V<'_> {
    fn frame_height(&self) -> usize {
        self.ctrl.last().map_or(0, |c| c.height)
    }

    fn unreachable(&self) -> bool {
        self.ctrl.last().is_some_and(|c| c.unreachable)
    }

    fn push(&mut self, t: u8) {
        if t != VOID {
            self.stack.push(t);
        }
    }

    /// Pop one value. Inside dead code an exhausted frame yields [`ANY`].
    fn pop(&mut self) -> Result<u8, WasmError> {
        if self.stack.len() <= self.frame_height() {
            if self.unreachable() {
                return Ok(ANY);
            }
            return Err(WasmError::Decode("validation: operand stack underflow"));
        }
        self.stack
            .pop()
            .ok_or(WasmError::Decode("validation: operand stack underflow"))
    }

    fn pop_expect(&mut self, want: u8) -> Result<(), WasmError> {
        if want == VOID {
            return Ok(());
        }
        let got = self.pop()?;
        if got == ANY || got == want {
            Ok(())
        } else {
            Err(type_error())
        }
    }

    /// After a branch or a trap the rest of the frame is dead: drop whatever it
    /// left on the stack and validate the remainder polymorphically.
    fn mark_unreachable(&mut self) {
        let height = self.frame_height();
        self.stack.truncate(height);
        if let Some(frame) = self.ctrl.last_mut() {
            frame.unreachable = true;
        }
    }

    /// The type carried by a branch to label depth `l`.
    fn label_type(&self, l: u32) -> Result<u8, WasmError> {
        let idx = self
            .ctrl
            .len()
            .checked_sub(1 + l as usize)
            .ok_or(WasmError::Decode("validation: branch label out of range"))?;
        Ok(self.ctrl[idx].label)
    }

    fn local_type(&self, i: u32) -> Result<u8, WasmError> {
        self.locals
            .get(i as usize)
            .copied()
            .ok_or(WasmError::Decode("validation: local index out of range"))
    }

    fn global_type(&self, i: u32) -> Result<u8, WasmError> {
        self.m
            .globals
            .get(i as usize)
            .copied()
            .ok_or(WasmError::Decode("validation: global index out of range"))
    }

    fn func_type(&self, f: u32) -> Result<&FuncType, WasmError> {
        let tidx = self
            .m
            .func_sigs
            .get(f as usize)
            .copied()
            .ok_or(WasmError::Decode("validation: function index out of range"))?;
        self.m
            .types
            .get(tidx)
            .ok_or(WasmError::Decode("validation: type index out of range"))
    }

    /// Pop a call's parameters (deepest first on the stack) and push results.
    fn apply_type(&mut self, ft_params: &[u8], ft_results: &[u8]) -> Result<(), WasmError> {
        for want in ft_params.iter().rev() {
            self.pop_expect(*want)?;
        }
        for r in ft_results {
            self.push(*r);
        }
        Ok(())
    }

    fn enter(&mut self, label: u8, result: u8, if_without_else: bool) {
        self.ctrl.push(Ctrl {
            label,
            result,
            height: self.stack.len(),
            unreachable: false,
            if_without_else,
        });
    }

    /// Close the innermost frame: its result must be on the stack, and nothing
    /// else may be left above the height it was entered at.
    fn leave(&mut self) -> Result<u8, WasmError> {
        let frame = self
            .ctrl
            .last()
            .ok_or(WasmError::Decode("validation: end without a block"))?;
        let (result, height, if_without_else) = (frame.result, frame.height, frame.if_without_else);
        if if_without_else && result != VOID {
            return Err(WasmError::Decode(
                "validation: if without else cannot leave a result",
            ));
        }
        self.pop_expect(result)?;
        if self.stack.len() != height {
            return Err(WasmError::Decode(
                "validation: block leaves the operand stack unbalanced",
            ));
        }
        self.ctrl.pop();
        Ok(result)
    }
}

/// Validate one function body. `locals` is params-then-declared-locals, and
/// `result` is the function's result type (`VOID` when it returns nothing).
pub(super) fn validate_body(
    m: &ModuleCtx<'_>,
    locals: &[u8],
    results: &[u8],
    code: &[Op],
) -> Result<(), WasmError> {
    let result = results.first().copied().unwrap_or(VOID);
    let mut v = V {
        m,
        locals,
        result,
        stack: Vec::new(),
        ctrl: Vec::new(),
    };
    // The function body is itself a block: `br 0` targets it, and its `end`
    // has to leave exactly the declared result behind.
    v.enter(result, result, false);

    for op in code {
        step(&mut v, op)?;
    }

    if !v.ctrl.is_empty() {
        return Err(WasmError::Decode("validation: function body has no end"));
    }
    Ok(())
}

fn step(v: &mut V<'_>, op: &Op) -> Result<(), WasmError> {
    use Op::*;
    match op {
        // --- constants ---
        I32Const(_) => v.push(I32),
        I64Const(_) => v.push(I64),
        F32Const(_) => v.push(F32),
        F64Const(_) => v.push(F64),

        // --- i32 unary / binary / comparison ---
        I32Clz | I32Ctz | I32Popcnt | I32Eqz => {
            v.pop_expect(I32)?;
            v.push(I32);
        }
        I32Add | I32Sub | I32Mul | I32DivS | I32DivU | I32RemS | I32RemU | I32And | I32Or
        | I32Xor | I32Shl | I32ShrS | I32ShrU | I32Rotl | I32Rotr | I32Eq | I32Ne | I32LtS
        | I32LtU | I32GtS | I32GtU | I32LeS | I32LeU | I32GeS | I32GeU => {
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.push(I32);
        }

        // --- i64 ---
        I64Clz | I64Ctz | I64Popcnt => {
            v.pop_expect(I64)?;
            v.push(I64);
        }
        I64Eqz => {
            v.pop_expect(I64)?;
            v.push(I32);
        }
        I64Add | I64Sub | I64Mul | I64DivS | I64DivU | I64RemS | I64RemU | I64And | I64Or
        | I64Xor | I64Shl | I64ShrS | I64ShrU | I64Rotl | I64Rotr => {
            v.pop_expect(I64)?;
            v.pop_expect(I64)?;
            v.push(I64);
        }
        I64Eq | I64Ne | I64LtS | I64LtU | I64GtS | I64GtU | I64LeS | I64LeU | I64GeS | I64GeU => {
            v.pop_expect(I64)?;
            v.pop_expect(I64)?;
            v.push(I32);
        }

        // --- f32 ---
        F32Abs | F32Neg | F32Ceil | F32Floor | F32Trunc | F32Nearest | F32Sqrt => {
            v.pop_expect(F32)?;
            v.push(F32);
        }
        F32Add | F32Sub | F32Mul | F32Div | F32Min | F32Max | F32Copysign => {
            v.pop_expect(F32)?;
            v.pop_expect(F32)?;
            v.push(F32);
        }
        F32Eq | F32Ne | F32Lt | F32Gt | F32Le | F32Ge => {
            v.pop_expect(F32)?;
            v.pop_expect(F32)?;
            v.push(I32);
        }

        // --- f64 ---
        F64Abs | F64Neg | F64Ceil | F64Floor | F64Trunc | F64Nearest | F64Sqrt => {
            v.pop_expect(F64)?;
            v.push(F64);
        }
        F64Add | F64Sub | F64Mul | F64Div | F64Min | F64Max | F64Copysign => {
            v.pop_expect(F64)?;
            v.pop_expect(F64)?;
            v.push(F64);
        }
        F64Eq | F64Ne | F64Lt | F64Gt | F64Le | F64Ge => {
            v.pop_expect(F64)?;
            v.pop_expect(F64)?;
            v.push(I32);
        }

        // --- conversions ---
        I32WrapI64 => {
            v.pop_expect(I64)?;
            v.push(I32);
        }
        I64ExtendI32S | I64ExtendI32U => {
            v.pop_expect(I32)?;
            v.push(I64);
        }
        I32TruncF32S | I32TruncF32U | I32ReinterpretF32 => {
            v.pop_expect(F32)?;
            v.push(I32);
        }
        I32TruncF64S | I32TruncF64U => {
            v.pop_expect(F64)?;
            v.push(I32);
        }
        I64TruncF32S | I64TruncF32U => {
            v.pop_expect(F32)?;
            v.push(I64);
        }
        I64TruncF64S | I64TruncF64U | I64ReinterpretF64 => {
            v.pop_expect(F64)?;
            v.push(I64);
        }
        F32ConvertI32S | F32ConvertI32U | F32ReinterpretI32 => {
            v.pop_expect(I32)?;
            v.push(F32);
        }
        F32ConvertI64S | F32ConvertI64U => {
            v.pop_expect(I64)?;
            v.push(F32);
        }
        F32DemoteF64 => {
            v.pop_expect(F64)?;
            v.push(F32);
        }
        F64ConvertI32S | F64ConvertI32U => {
            v.pop_expect(I32)?;
            v.push(F64);
        }
        F64ConvertI64S | F64ConvertI64U | F64ReinterpretI64 => {
            v.pop_expect(I64)?;
            v.push(F64);
        }
        F64PromoteF32 => {
            v.pop_expect(F32)?;
            v.push(F64);
        }

        // --- memory ---
        I32Load { .. }
        | I32Load8S { .. }
        | I32Load8U { .. }
        | I32Load16S { .. }
        | I32Load16U { .. } => {
            v.pop_expect(I32)?;
            v.push(I32);
        }
        I64Load { .. }
        | I64Load8S { .. }
        | I64Load8U { .. }
        | I64Load16S { .. }
        | I64Load16U { .. }
        | I64Load32S { .. }
        | I64Load32U { .. } => {
            v.pop_expect(I32)?;
            v.push(I64);
        }
        F32Load { .. } => {
            v.pop_expect(I32)?;
            v.push(F32);
        }
        F64Load { .. } => {
            v.pop_expect(I32)?;
            v.push(F64);
        }
        I32Store { .. } | I32Store8 { .. } | I32Store16 { .. } => {
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        I64Store { .. } | I64Store8 { .. } | I64Store16 { .. } | I64Store32 { .. } => {
            v.pop_expect(I64)?;
            v.pop_expect(I32)?;
        }
        F32Store { .. } => {
            v.pop_expect(F32)?;
            v.pop_expect(I32)?;
        }
        F64Store { .. } => {
            v.pop_expect(F64)?;
            v.pop_expect(I32)?;
        }
        MemorySize => v.push(I32),
        MemoryGrow => {
            v.pop_expect(I32)?;
            v.push(I32);
        }
        MemoryCopy | MemoryFill => {
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        MemoryInit { data_index } => {
            let count = v.m.data_count.ok_or(WasmError::Decode(
                "validation: memory.init requires data count",
            ))?;
            if *data_index as usize >= count {
                return Err(WasmError::Decode(
                    "validation: memory.init data segment index",
                ));
            }
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        DataDrop { data_index } => {
            let count = v.m.data_count.ok_or(WasmError::Decode(
                "validation: data.drop requires data count",
            ))?;
            if *data_index as usize >= count {
                return Err(WasmError::Decode("validation: data.drop segment index"));
            }
        }
        TableInit { elem_index } => {
            if !v.m.has_table {
                return Err(WasmError::Decode("validation: table.init requires table"));
            }
            if *elem_index as usize >= v.m.elem_count {
                return Err(WasmError::Decode(
                    "validation: table.init element segment index",
                ));
            }
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        ElemDrop { elem_index } => {
            if *elem_index as usize >= v.m.elem_count {
                return Err(WasmError::Decode("validation: elem.drop segment index"));
            }
        }
        TableCopy => {
            if !v.m.has_table {
                return Err(WasmError::Decode("validation: table.copy requires table"));
            }
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }

        // --- locals and globals ---
        LocalGet(i) => {
            let t = v.local_type(*i)?;
            v.push(t);
        }
        LocalSet(i) => {
            let t = v.local_type(*i)?;
            v.pop_expect(t)?;
        }
        LocalTee(i) => {
            let t = v.local_type(*i)?;
            v.pop_expect(t)?;
            v.push(t);
        }
        GlobalGet(i) => {
            let t = v.global_type(*i)?;
            v.push(t);
        }
        GlobalSet(i) => {
            let t = v.global_type(*i)?;
            v.pop_expect(t)?;
        }

        // --- parametric ---
        Drop => {
            v.pop()?;
        }
        Select => {
            v.pop_expect(I32)?;
            let b = v.pop()?;
            let a = v.pop()?;
            if a != ANY && b != ANY && a != b {
                return Err(type_error());
            }
            v.push(if a == ANY { b } else { a });
        }
        Nop => {}

        // --- calls ---
        Call(f) => {
            let ft = v.func_type(*f)?;
            let (params, results) = (ft.params.clone(), ft.results.clone());
            v.apply_type(&params, &results)?;
        }
        CallIndirect { type_index } => {
            if !v.m.has_table {
                return Err(WasmError::Decode(
                    "validation: call_indirect requires table",
                ));
            }
            let ft =
                v.m.types
                    .get(*type_index as usize)
                    .ok_or(WasmError::Decode("validation: type index out of range"))?;
            let (params, results) = (ft.params.clone(), ft.results.clone());
            v.pop_expect(I32)?; // the table index
            v.apply_type(&params, &results)?;
        }

        // --- structured control ---
        Block { vt, .. } => v.enter(*vt, *vt, false),
        // A loop's label is its start, so a branch to it carries nothing.
        Loop { vt, .. } => v.enter(VOID, *vt, false),
        If { vt, else_pc, .. } => {
            v.pop_expect(I32)?;
            v.enter(*vt, *vt, else_pc.is_none());
        }
        Else { .. } => {
            // Close the then-arm, then re-open the frame for the else-arm.
            let frame = v
                .ctrl
                .last()
                .ok_or(WasmError::Decode("validation: else without if"))?;
            let (result, height) = (frame.result, frame.height);
            v.pop_expect(result)?;
            if v.stack.len() != height {
                return Err(WasmError::Decode(
                    "validation: if arm leaves the operand stack unbalanced",
                ));
            }
            if let Some(frame) = v.ctrl.last_mut() {
                frame.unreachable = false;
            }
        }
        End => {
            let result = v.leave()?;
            v.push(result);
        }

        // --- branches ---
        Br(l) => {
            let t = v.label_type(*l)?;
            v.pop_expect(t)?;
            v.mark_unreachable();
        }
        BrIf(l) => {
            let t = v.label_type(*l)?;
            v.pop_expect(I32)?;
            v.pop_expect(t)?;
            v.push(t);
        }
        BrTable { targets, default } => {
            let want = v.label_type(*default)?;
            for t in targets {
                if v.label_type(*t)? != want {
                    return Err(WasmError::Decode(
                        "validation: br_table targets disagree on type",
                    ));
                }
            }
            v.pop_expect(I32)?;
            v.pop_expect(want)?;
            v.mark_unreachable();
        }
        Return => {
            let result = v.result;
            v.pop_expect(result)?;
            v.mark_unreachable();
        }
        Unreachable => v.mark_unreachable(),
    }
    Ok(())
}
