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
    LocalGet(u32),
    LocalSet(u32),
    Call(u32),
    /// `arity` is the block's result count (0 or 1); `end` indexes its `End`.
    Block { arity: u32, end: usize },
    /// `arity` is the loop's result count; the back-edge arity is always 0
    /// (MVP loops take no inputs). `end` indexes its `End`.
    Loop { arity: u32, end: usize },
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

    /// Invoke function `idx` with `args`, returning its result values.
    pub fn invoke(&self, idx: usize, args: &[i32]) -> Result<Vec<i32>, WasmError> {
        let mut steps: u64 = 0;
        self.invoke_inner(idx, args, 0, &mut steps)
    }

    fn invoke_inner(
        &self,
        idx: usize,
        args: &[i32],
        depth: usize,
        steps: &mut u64,
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
                    let res = self.invoke_inner(f as usize, &cargs, depth + 1, steps)?;
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
        // 0x6B is i32.sub, deliberately outside this subset.
        assert!(matches!(
            m.add_function(0, 0, 1, &[0x6B, 0x0B]),
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
