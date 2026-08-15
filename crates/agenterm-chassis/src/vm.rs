//! Bounded interpreter for Chassis-L2 AOT bytecode.
//!
//! No JIT, no libtcc, no rustc, no unsafe. Host side-effects go through [`CapHost`].

use super::bytecode::{Inst, Program, decode};

/// Maximum operand-stack slots.
pub const MAX_STACK: usize = 256;

/// Typical step budget for a packed L2 program.
pub const DEFAULT_MAX_STEPS: u32 = 1_000_000;

/// Host that implements L2 capability names.
pub trait CapHost {
    fn call(&mut self, cap: &str) -> Result<i64, String>;
}

/// Run `program` until Halt/Ret, a host error, or the step budget is exhausted.
///
/// On Halt/Ret, returns the top of stack, or `0` if the stack is empty.
pub fn run(program: &Program, host: &mut dyn CapHost, max_steps: u32) -> Result<i64, String> {
    let insts = decode(&program.code)?;
    for inst in &insts {
        if let Inst::CallCap(idx) = inst
            && usize::from(*idx) >= program.caps.len()
        {
            return Err(format!("CallCap index {idx} is out of range"));
        }
    }

    let mut stack: Vec<i64> = Vec::new();
    let mut pc = 0usize;
    let mut steps = 0u32;

    loop {
        if steps >= max_steps {
            return Err(format!("step budget exceeded ({max_steps})"));
        }
        steps = steps.saturating_add(1);

        let inst = insts
            .get(pc)
            .ok_or_else(|| format!("program counter {pc} is past the last instruction"))?;

        match inst {
            Inst::PushI64(v) => {
                push(&mut stack, *v)?;
                pc += 1;
            }
            Inst::Add => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, a.wrapping_add(b))?;
                pc += 1;
            }
            Inst::Sub => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, a.wrapping_sub(b))?;
                pc += 1;
            }
            Inst::Eq => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, i64::from(a == b))?;
                pc += 1;
            }
            Inst::Not => {
                let a = pop(&mut stack)?;
                push(&mut stack, i64::from(a == 0))?;
                pc += 1;
            }
            Inst::Jump(target) => {
                pc = *target;
            }
            Inst::JumpIfZero(target) => {
                let a = pop(&mut stack)?;
                if a == 0 {
                    pc = *target;
                } else {
                    pc += 1;
                }
            }
            Inst::CallCap(idx) => {
                let name = program
                    .caps
                    .get(usize::from(*idx))
                    .ok_or_else(|| format!("CallCap index {idx} is out of range"))?;
                let value = host.call(name)?;
                push(&mut stack, value)?;
                pc += 1;
            }
            Inst::Ret | Inst::Halt => {
                return Ok(stack.last().copied().unwrap_or(0));
            }
        }
    }
}

fn push(stack: &mut Vec<i64>, value: i64) -> Result<(), String> {
    if stack.len() >= MAX_STACK {
        return Err(format!("stack overflow ({MAX_STACK} slots)"));
    }
    stack.push(value);
    Ok(())
}

fn pop(stack: &mut Vec<i64>) -> Result<i64, String> {
    stack.pop().ok_or_else(|| "stack underflow".to_string())
}
