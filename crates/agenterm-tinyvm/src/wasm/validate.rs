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

use super::{BlockType, FuncType, GlobalDesc, Op, WasmError};

const I32: u8 = 0x7F;
const I64: u8 = 0x7E;
const F32: u8 = 0x7D;
const F64: u8 = 0x7C;
#[cfg(feature = "simd")]
const V128: u8 = 0x7B;
const FUNCREF: u8 = 0x70;
const EXTERNREF: u8 = 0x6F;
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
    /// Global definitions retain both the value type and mutability proof.
    pub globals: &'a [GlobalDesc],
    /// DataCount section value. Bulk data instructions require it even when
    /// the final data section is available to this non-streaming decoder.
    pub data_count: Option<usize>,
    /// Reference type of every element segment and table in standard index order.
    pub elem_types: &'a [u8],
    pub table_types: &'a [u8],
    pub memory_count: usize,
    /// Function indices forward-declared by element segments and therefore
    /// legal operands of `ref.func`.
    pub declared_refs: &'a [bool],
}

/// A non-owning value-type vector. Control frames point into the already
/// decode-budgeted type section instead of cloning a potentially huge
/// signature once per nested block.
#[derive(Clone, Copy)]
enum Types<'a> {
    Empty,
    One(u8),
    Slice(&'a [u8]),
}

impl Types<'_> {
    fn len(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::One(_) => 1,
            Self::Slice(types) => types.len(),
        }
    }

    fn get(self, index: usize) -> Option<u8> {
        match self {
            Self::Empty => None,
            Self::One(ty) => (index == 0).then_some(ty),
            Self::Slice(types) => types.get(index).copied(),
        }
    }

    fn same(self, other: Self) -> bool {
        self.len() == other.len()
            && (0..self.len()).all(|index| self.get(index) == other.get(index))
    }
}

/// A control frame: a block, loop, `if`, or the function body itself.
struct Ctrl<'a> {
    /// Types a branch to this label carries. Blocks/ifs use results; loops use
    /// parameters because their label is the start of the body.
    label: Types<'a>,
    /// Values available at the beginning of each arm/body.
    params: Types<'a>,
    /// Types the frame leaves behind when it ends normally.
    results: Types<'a>,
    /// Operand-stack height when the frame was entered.
    height: usize,
    /// Set after `br`/`br_table`/`return`/`unreachable`: the rest of the frame
    /// is dead code, validated against a polymorphic stack.
    unreachable: bool,
    /// `if` frames without an `else` use their parameters as the implicit arm.
    if_without_else: bool,
}

struct V<'a> {
    m: &'a ModuleCtx<'a>,
    locals: &'a [u8],
    results: Types<'a>,
    branch_targets: &'a [u32],
    stack: Vec<u8>,
    ctrl: Vec<Ctrl<'a>>,
}

fn type_error() -> WasmError {
    WasmError::Decode("validation: type mismatch")
}

/// The legacy, untyped `select` instruction is restricted to numeric values
/// (plus vectors when that proposal is enabled). Reference values require the
/// explicit result type carried by `select t`; accepting equal reference arms
/// here would admit a module rejected by standard validators.
fn is_untyped_select_type(value_type: u8) -> bool {
    match value_type {
        ANY | I32 | I64 | F32 | F64 => true,
        #[cfg(feature = "simd")]
        V128 => true,
        _ => false,
    }
}

impl<'a> V<'a> {
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

    /// Types carried by a branch to label depth `l`.
    fn label_types(&self, l: u32) -> Result<Types<'a>, WasmError> {
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
            .map(|global| global.value_type)
            .ok_or(WasmError::Decode("validation: global index out of range"))
    }

    fn mutable_global_type(&self, i: u32) -> Result<u8, WasmError> {
        self.m
            .globals
            .get(i as usize)
            .filter(|global| global.mutable)
            .map(|global| global.value_type)
            .ok_or(WasmError::Decode("global.set"))
    }

    fn func_type_index(&self, f: u32) -> Result<usize, WasmError> {
        self.m
            .func_sigs
            .get(f as usize)
            .copied()
            .ok_or(WasmError::Decode("validation: function index out of range"))
    }

    /// Apply one declared function type without cloning its vectors. Looking up
    /// one copied byte at a time keeps the immutable module borrow disjoint
    /// from mutations of the abstract operand stack.
    fn apply_type_index(&mut self, type_index: usize) -> Result<(), WasmError> {
        let (param_len, result_len) = self
            .m
            .types
            .get(type_index)
            .map(|ft| (ft.params.len(), ft.results.len()))
            .ok_or(WasmError::Decode("validation: type index out of range"))?;
        for index in (0..param_len).rev() {
            let want = self.m.types[type_index].params[index];
            self.pop_expect(want)?;
        }
        for index in 0..result_len {
            let result = self.m.types[type_index].results[index];
            self.push(result);
        }
        Ok(())
    }

    /// Validate a tail call against the current function's complete result
    /// vector, consume its arguments, then make the continuation polymorphic.
    fn apply_tail_type_index(&mut self, type_index: usize) -> Result<(), WasmError> {
        let function_type = self
            .m
            .types
            .get(type_index)
            .ok_or(WasmError::Decode("validation: type index out of range"))?;
        if !Types::Slice(&function_type.results).same(self.results) {
            return Err(WasmError::Decode(
                "validation: tail call result type mismatch",
            ));
        }
        for index in (0..function_type.params.len()).rev() {
            self.pop_expect(self.m.types[type_index].params[index])?;
        }
        self.mark_unreachable();
        Ok(())
    }

    fn pop_types(&mut self, types: Types<'a>) -> Result<(), WasmError> {
        match types {
            Types::Empty => {}
            Types::One(want) => self.pop_expect(want)?,
            Types::Slice(types) => {
                for want in types.iter().rev() {
                    self.pop_expect(*want)?;
                }
            }
        }
        Ok(())
    }

    fn push_types(&mut self, types: Types<'a>) {
        match types {
            Types::Empty => {}
            Types::One(ty) => self.push(ty),
            Types::Slice(types) => {
                for ty in types {
                    self.push(*ty);
                }
            }
        }
    }

    fn block_signature(&self, ty: BlockType) -> Result<(Types<'a>, Types<'a>), WasmError> {
        match ty {
            BlockType::Empty => Ok((Types::Empty, Types::Empty)),
            BlockType::Value(result) => Ok((Types::Empty, Types::One(result))),
            BlockType::TypeIndex(index) => {
                let ft = self.m.types.get(index as usize).ok_or(WasmError::Decode(
                    "validation: block type index out of range",
                ))?;
                Ok((Types::Slice(&ft.params), Types::Slice(&ft.results)))
            }
        }
    }

    fn enter(
        &mut self,
        params: Types<'a>,
        results: Types<'a>,
        label: Types<'a>,
        if_without_else: bool,
    ) -> Result<(), WasmError> {
        self.pop_types(params)?;
        let height = self.stack.len();
        self.push_types(params);
        self.ctrl.push(Ctrl {
            label,
            params,
            results,
            height,
            unreachable: false,
            if_without_else,
        });
        Ok(())
    }

    /// Close the innermost frame: its result must be on the stack, and nothing
    /// else may be left above the height it was entered at.
    fn leave(&mut self) -> Result<Types<'a>, WasmError> {
        let frame = self
            .ctrl
            .last()
            .ok_or(WasmError::Decode("validation: end without a block"))?;
        let (params, results, height, if_without_else) = (
            frame.params,
            frame.results,
            frame.height,
            frame.if_without_else,
        );
        if if_without_else && !params.same(results) {
            return Err(WasmError::Decode(
                "validation: if without else has incompatible parameters/results",
            ));
        }
        self.pop_types(results)?;
        if self.stack.len() != height {
            return Err(WasmError::Decode(
                "validation: block leaves the operand stack unbalanced",
            ));
        }
        self.ctrl.pop();
        Ok(results)
    }
}

/// Validate one function body. `locals` is params-then-declared-locals, and
/// `results` is the complete function result vector.
pub(super) fn validate_body(
    m: &ModuleCtx<'_>,
    locals: &[u8],
    results: &[u8],
    code: &[Op],
    branch_targets: &[u32],
) -> Result<(), WasmError> {
    let mut v = V {
        m,
        locals,
        results: Types::Slice(results),
        branch_targets,
        stack: Vec::new(),
        ctrl: Vec::new(),
    };
    // The function body is itself a block: `br 0` targets it, and its `end`
    // has to leave exactly the declared result behind.
    v.enter(
        Types::Empty,
        Types::Slice(results),
        Types::Slice(results),
        false,
    )?;

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
    let memory_count = v.m.memory_count;
    let require_memory = |index: u32| {
        if index as usize >= memory_count {
            Err(WasmError::Decode("validation: memory index"))
        } else {
            Ok(())
        }
    };
    match op {
        // --- constants ---
        I32Const(_) => v.push(I32),
        I64Const(_) => v.push(I64),
        F32Const(_) => v.push(F32),
        F64Const(_) => v.push(F64),

        // --- fixed-width SIMD audio kernel ---
        #[cfg(feature = "simd")]
        V128Load(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(I32)?;
            v.push(V128);
        }
        #[cfg(feature = "simd")]
        V128Const(_) => v.push(V128),
        #[cfg(feature = "simd")]
        V128Store(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(V128)?;
            v.pop_expect(I32)?;
        }
        #[cfg(feature = "simd")]
        I16x8AddSatS | I16x8SubSatS => {
            v.pop_expect(V128)?;
            v.pop_expect(V128)?;
            v.push(V128);
        }

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
        I32TruncF32S | I32TruncF32U | I32TruncSatF32S | I32TruncSatF32U | I32ReinterpretF32 => {
            v.pop_expect(F32)?;
            v.push(I32);
        }
        I32TruncF64S | I32TruncF64U | I32TruncSatF64S | I32TruncSatF64U => {
            v.pop_expect(F64)?;
            v.push(I32);
        }
        I64TruncF32S | I64TruncF32U | I64TruncSatF32S | I64TruncSatF32U => {
            v.pop_expect(F32)?;
            v.push(I64);
        }
        I64TruncF64S | I64TruncF64U | I64TruncSatF64S | I64TruncSatF64U | I64ReinterpretF64 => {
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
        I32Extend8S | I32Extend16S => {
            v.pop_expect(I32)?;
            v.push(I32);
        }
        I64Extend8S | I64Extend16S | I64Extend32S => {
            v.pop_expect(I64)?;
            v.push(I64);
        }

        // --- memory ---
        I32Load(arg) | I32Load8S(arg) | I32Load8U(arg) | I32Load16S(arg) | I32Load16U(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(I32)?;
            v.push(I32);
        }
        I64Load(arg) | I64Load8S(arg) | I64Load8U(arg) | I64Load16S(arg) | I64Load16U(arg)
        | I64Load32S(arg) | I64Load32U(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(I32)?;
            v.push(I64);
        }
        F32Load(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(I32)?;
            v.push(F32);
        }
        F64Load(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(I32)?;
            v.push(F64);
        }
        I32Store(arg) | I32Store8(arg) | I32Store16(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        I64Store(arg) | I64Store8(arg) | I64Store16(arg) | I64Store32(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(I64)?;
            v.pop_expect(I32)?;
        }
        F32Store(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(F32)?;
            v.pop_expect(I32)?;
        }
        F64Store(arg) => {
            require_memory(arg.memory)?;
            v.pop_expect(F64)?;
            v.pop_expect(I32)?;
        }
        MemorySize(memory) => {
            require_memory(*memory)?;
            v.push(I32);
        }
        MemoryGrow(memory) => {
            require_memory(*memory)?;
            v.pop_expect(I32)?;
            v.push(I32);
        }
        MemoryCopy {
            destination_memory,
            source_memory,
        } => {
            require_memory(*destination_memory)?;
            require_memory(*source_memory)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        MemoryFill(memory) => {
            require_memory(*memory)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        MemoryInit {
            data_index,
            memory_index,
        } => {
            require_memory(*memory_index)?;
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
        TableInit {
            elem_index,
            table_index,
        } => {
            let table_type =
                v.m.table_types
                    .get(*table_index as usize)
                    .ok_or(WasmError::Decode("validation: table.init table index"))?;
            let elem_type =
                v.m.elem_types
                    .get(*elem_index as usize)
                    .ok_or(WasmError::Decode(
                        "validation: table.init element segment index",
                    ))?;
            if table_type != elem_type {
                return Err(type_error());
            }
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
            v.pop_expect(I32)?;
        }
        ElemDrop { elem_index } => {
            if *elem_index as usize >= v.m.elem_types.len() {
                return Err(WasmError::Decode("validation: elem.drop segment index"));
            }
        }
        TableCopy {
            destination_table,
            source_table,
        } => {
            let destination_type =
                v.m.table_types
                    .get(*destination_table as usize)
                    .ok_or(WasmError::Decode("validation: table.copy table index"))?;
            let source_type =
                v.m.table_types
                    .get(*source_table as usize)
                    .ok_or(WasmError::Decode("validation: table.copy table index"))?;
            if destination_type != source_type {
                return Err(type_error());
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
            let t = v.mutable_global_type(*i)?;
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
            let selected_type = if a == ANY { b } else { a };
            if !is_untyped_select_type(selected_type) {
                return Err(type_error());
            }
            v.push(selected_type);
        }
        TypedSelect(ty) => {
            v.pop_expect(I32)?;
            v.pop_expect(*ty)?;
            v.pop_expect(*ty)?;
            v.push(*ty);
        }
        Nop => {}

        // --- funcref / table ---
        RefNull(reference_type) => v.push(*reference_type),
        RefIsNull => {
            let reference_type = v.pop()?;
            if !matches!(reference_type, FUNCREF | EXTERNREF | ANY) {
                return Err(type_error());
            }
            v.push(I32);
        }
        RefFunc(function) => {
            v.func_type_index(*function)?;
            if !v
                .m
                .declared_refs
                .get(*function as usize)
                .copied()
                .unwrap_or(false)
            {
                return Err(WasmError::Decode("validation: undeclared ref.func"));
            }
            v.push(FUNCREF);
        }
        TableGet(table_index) => {
            let element_type =
                *v.m.table_types
                    .get(*table_index as usize)
                    .ok_or(WasmError::Decode("validation: table.get table index"))?;
            v.pop_expect(I32)?;
            v.push(element_type);
        }
        TableSet(table_index) => {
            let element_type =
                *v.m.table_types
                    .get(*table_index as usize)
                    .ok_or(WasmError::Decode("validation: table.set table index"))?;
            v.pop_expect(element_type)?;
            v.pop_expect(I32)?;
        }
        TableGrow(table_index) => {
            let element_type =
                *v.m.table_types
                    .get(*table_index as usize)
                    .ok_or(WasmError::Decode("validation: table.grow table index"))?;
            v.pop_expect(I32)?;
            v.pop_expect(element_type)?;
            v.push(I32);
        }
        TableSize(table_index) => {
            if *table_index as usize >= v.m.table_types.len() {
                return Err(WasmError::Decode("validation: table.size table index"));
            }
            v.push(I32);
        }
        TableFill(table_index) => {
            let element_type =
                *v.m.table_types
                    .get(*table_index as usize)
                    .ok_or(WasmError::Decode("validation: table.fill table index"))?;
            v.pop_expect(I32)?;
            v.pop_expect(element_type)?;
            v.pop_expect(I32)?;
        }

        // --- calls ---
        Call(f) => {
            let type_index = v.func_type_index(*f)?;
            v.apply_type_index(type_index)?;
        }
        ReturnCall(function) => {
            let type_index = v.func_type_index(*function)?;
            v.apply_tail_type_index(type_index)?;
        }
        CallIndirect {
            type_index,
            table_index,
        } => {
            if v.m.table_types.get(*table_index as usize) != Some(&FUNCREF) {
                return Err(WasmError::Decode("validation: call_indirect table index"));
            }
            let type_index = *type_index as usize;
            v.pop_expect(I32)?; // the table index
            v.apply_type_index(type_index)?;
        }
        ReturnCallIndirect {
            type_index,
            table_index,
        } => {
            if v.m.table_types.get(*table_index as usize) != Some(&FUNCREF) {
                return Err(WasmError::Decode(
                    "validation: return_call_indirect table index",
                ));
            }
            let type_index = *type_index as usize;
            v.pop_expect(I32)?;
            v.apply_tail_type_index(type_index)?;
        }

        // --- structured control ---
        Block { ty, .. } => {
            let (params, results) = v.block_signature(*ty)?;
            v.enter(params, results, results, false)?;
        }
        // A loop's label is its start, so a branch to it carries parameters.
        Loop { ty, .. } => {
            let (params, results) = v.block_signature(*ty)?;
            v.enter(params, results, params, false)?;
        }
        If { ty, else_pc, .. } => {
            v.pop_expect(I32)?;
            let (params, results) = v.block_signature(*ty)?;
            v.enter(params, results, results, else_pc.is_none())?;
        }
        Else { .. } => {
            // Close the then-arm, then re-open the frame for the else-arm.
            let frame = v
                .ctrl
                .last()
                .ok_or(WasmError::Decode("validation: else without if"))?;
            let (params, results, height) = (frame.params, frame.results, frame.height);
            v.pop_types(results)?;
            if v.stack.len() != height {
                return Err(WasmError::Decode(
                    "validation: if arm leaves the operand stack unbalanced",
                ));
            }
            v.push_types(params);
            if let Some(frame) = v.ctrl.last_mut() {
                frame.unreachable = false;
            }
        }
        End => {
            let results = v.leave()?;
            v.push_types(results);
        }

        // --- branches ---
        Br(l) => {
            let types = v.label_types(*l)?;
            v.pop_types(types)?;
            v.mark_unreachable();
        }
        BrIf(l) => {
            let types = v.label_types(*l)?;
            v.pop_expect(I32)?;
            v.pop_types(types)?;
            v.push_types(types);
        }
        BrTable {
            target_start,
            target_len,
            default,
        } => {
            let target_start = *target_start as usize;
            // Both the offsets and this private arena are emitted by the same
            // decoder; validation never accepts caller-constructed `Op`s.
            let targets = &v.branch_targets[target_start..target_start + *target_len as usize];
            let want = v.label_types(*default)?;
            for t in targets {
                if !v.label_types(*t)?.same(want) {
                    return Err(WasmError::Decode(
                        "validation: br_table targets disagree on type",
                    ));
                }
            }
            v.pop_expect(I32)?;
            v.pop_types(want)?;
            v.mark_unreachable();
        }
        Return => {
            let results = v.results;
            v.pop_types(results)?;
            v.mark_unreachable();
        }
        Unreachable => v.mark_unreachable(),
    }
    Ok(())
}
