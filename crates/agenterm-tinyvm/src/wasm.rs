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
/// WebAssembly linear-memory page size (64 KiB). This cut allocates exactly one
/// page per invocation; growth and a module memory section come later.
pub const WASM_PAGE_SIZE: usize = 65_536;

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
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    I32Const(i32),
    I32Add,
    I32Sub,
    I32Eqz,
    /// `i32.load` — pop address, push 4 little-endian bytes at `addr + offset`.
    /// The memarg alignment hint is decoded and ignored (a valid MVP choice).
    I32Load { offset: u32 },
    /// `i32.store` — pop value then address; write 4 little-endian bytes at
    /// `addr + offset`. Alignment hint decoded and ignored.
    I32Store { offset: u32 },
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
            0x6A => ops.push(Op::I32Add),
            0x6B => ops.push(Op::I32Sub),
            0x45 => ops.push(Op::I32Eqz),
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
            0x0F => ops.push(Op::Return),
            0x0B => {
                let end_idx = ops.len();
                ops.push(Op::End);
                if let Some(open_idx) = open.pop() {
                    match &mut ops[open_idx] {
                        Op::Block { end, .. } | Op::Loop { end, .. } => *end = end_idx,
                        _ => unreachable!("open index always points at a block or loop"),
                    }
                }
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

/// A collection of function bodies that can call one another by index.
#[derive(Debug, Clone, Default)]
pub struct Module {
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
        let idx = self.funcs.len();
        self.funcs.push(Func {
            n_params,
            n_locals,
            arity: result_arity,
            code,
        });
        Ok(idx)
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
                3 => func_types = parse_func_section(payload)?,
                10 => codes = parse_code_section(payload)?,
                _ => {} // custom / import / export / memory / … : skipped
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
        let mut steps: u64 = 0;
        let mut mem = vec![0u8; WASM_PAGE_SIZE];
        self.invoke_inner(idx, args, 0, &mut steps, &mut mem)
    }

    fn invoke_inner(
        &self,
        idx: usize,
        args: &[i32],
        depth: usize,
        steps: &mut u64,
        mem: &mut [u8],
    ) -> Result<Vec<i32>, WasmError> {
        if depth > WASM_MAX_DEPTH {
            return Err(WasmError::Trap(format!(
                "call depth {WASM_MAX_DEPTH} exceeded"
            )));
        }
        let func = self
            .funcs
            .get(idx)
            .ok_or_else(|| WasmError::Trap(format!("call to unknown function {idx}")))?;
        if args.len() != func.n_params {
            return Err(WasmError::Trap(format!(
                "function {idx} expects {} args, got {}",
                func.n_params,
                args.len()
            )));
        }

        let mut locals = args.to_vec();
        locals.resize(func.n_params + func.n_locals, 0);
        let mut stack: Vec<i32> = Vec::new();
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
                Op::I32Const(v) => stack.push(v),
                Op::I32Add => {
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(a.wrapping_add(b));
                }
                Op::I32Sub => {
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(a.wrapping_sub(b));
                }
                Op::I32Eqz => {
                    let a = pop(&mut stack)?;
                    stack.push(i32::from(a == 0));
                }
                Op::I32Load { offset } => {
                    let addr = pop(&mut stack)?;
                    stack.push(mem_read_i32(mem, addr, offset)?);
                }
                Op::I32Store { offset } => {
                    let value = pop(&mut stack)?;
                    let addr = pop(&mut stack)?;
                    mem_write_i32(mem, addr, offset, value)?;
                }
                Op::LocalGet(l) => {
                    let v = *locals
                        .get(l as usize)
                        .ok_or_else(|| WasmError::Trap(format!("local.get {l} out of range")))?;
                    stack.push(v);
                }
                Op::LocalSet(l) => {
                    let v = pop(&mut stack)?;
                    let cell = locals
                        .get_mut(l as usize)
                        .ok_or_else(|| WasmError::Trap(format!("local.set {l} out of range")))?;
                    *cell = v;
                }
                Op::Call(f) => {
                    let callee = self
                        .funcs
                        .get(f as usize)
                        .ok_or_else(|| WasmError::Trap(format!("call to unknown function {f}")))?;
                    let n = callee.n_params;
                    if stack.len() < n {
                        return Err(WasmError::Trap(format!(
                            "call {f}: stack has {} values, needs {n}",
                            stack.len()
                        )));
                    }
                    let cargs = stack.split_off(stack.len() - n);
                    let res = self.invoke_inner(f as usize, &cargs, depth + 1, steps, mem)?;
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

/// Effective address `addr as u32 + offset`, bounds-checked for a 4-byte i32
/// access. Alignment is not checked (MVP). Out of range is a loud trap.
fn effective_range(mem_len: usize, addr: i32, offset: u32) -> Result<usize, WasmError> {
    let ea = (addr as u32 as usize)
        .checked_add(offset as usize)
        .ok_or_else(|| WasmError::Trap("i32 memory address overflow".into()))?;
    let end = ea
        .checked_add(4)
        .ok_or_else(|| WasmError::Trap("i32 memory access overflow".into()))?;
    if end > mem_len {
        return Err(WasmError::Trap(format!(
            "i32 memory access [{ea}, {end}) out of bounds for {mem_len}-byte memory"
        )));
    }
    Ok(ea)
}

fn mem_read_i32(mem: &[u8], addr: i32, offset: u32) -> Result<i32, WasmError> {
    let ea = effective_range(mem.len(), addr, offset)?;
    let bytes: [u8; 4] = mem[ea..ea + 4]
        .try_into()
        .expect("range is exactly 4 bytes");
    Ok(i32::from_le_bytes(bytes))
}

fn mem_write_i32(mem: &mut [u8], addr: i32, offset: u32, value: i32) -> Result<(), WasmError> {
    let ea = effective_range(mem.len(), addr, offset)?;
    mem[ea..ea + 4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn pop(stack: &mut Vec<i32>) -> Result<i32, WasmError> {
    stack
        .pop()
        .ok_or_else(|| WasmError::Trap("operand stack underflow".into()))
}

fn take_results(stack: &mut Vec<i32>, arity: usize) -> Result<Vec<i32>, WasmError> {
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
fn do_branch(stack: &mut Vec<i32>, control: &mut Vec<Frame>, l: u32) -> Result<usize, WasmError> {
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
        // 0x6C is i32.mul, deliberately outside this subset.
        assert!(matches!(
            m.add_function(0, 0, 1, &[0x6C, 0x0B]),
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
