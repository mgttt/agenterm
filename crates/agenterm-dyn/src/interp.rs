//! Portable soft-executor of the dyn `Op` stream (the no-JIT / iOS floor).
//!
//! The JIT backend ([`crate::Engine`], unix-only) lowers an `Op` stream to host
//! bytes and **jumps into** them. This backend keeps a soft program counter and
//! **reads** the very same `Op` stream instead. Same program, two execution
//! modes — that parity is the point. Because it never maps executable memory,
//! it has **no `mmap`, no W^X, no `unsafe`** and runs anywhere Rust runs,
//! including platforms that forbid runtime-generated native code (iOS, Wasm).
//!
//! Identity is preserved: the `Op` stream is still the program; iOS merely
//! forbids *jumping into* the lowered bytes, so here we interpret the ops. It
//! matches the JIT backend's time-axis behaviour — names may be used before
//! defined, and names persist across appends — and it keeps dyn's loud-failure
//! discipline: unbounded recursion or an unresolved call fails with an error
//! rather than looping or misbehaving silently.

use std::collections::{HashMap, HashSet};

use crate::encoder::Op;
use crate::error::DynError;

/// Maximum interpreter steps for one [`Interp::enter_i64`] call.
pub const MAX_STEPS: u64 = 16_000_000;
/// Maximum call-stack depth for one [`Interp::enter_i64`] call.
pub const MAX_CALL_DEPTH: usize = 4_096;

/// One decoded instruction in the flat interpreted stream.
#[derive(Debug, Clone)]
enum Inst {
    /// Set the `i64` accumulator (the eventual return value).
    MovRaxImm(i64),
    /// Call a named block, resolved by name at run time.
    Call(String),
    /// Return from the current block.
    Ret,
}

/// A growable, portably-interpreted image of an `Op` stream.
///
/// Mirrors the surface of the JIT [`crate::Engine`]: `assemble` appends, names
/// persist across appends, and `enter_i64` runs a named block — but execution
/// is pure interpretation.
#[derive(Debug, Default)]
pub struct Interp {
    code: Vec<Inst>,
    names: HashMap<String, usize>,
}

impl Interp {
    /// A fresh, empty interpreter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of defined names.
    pub fn name_count(&self) -> usize {
        self.names.len()
    }

    /// Whether `name` is defined.
    pub fn is_defined(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    /// Number of `Call` sites whose target name is not yet defined (open
    /// forward references), matching the JIT engine's pending semantics.
    pub fn pending_count(&self) -> usize {
        self.code
            .iter()
            .filter(|i| matches!(i, Inst::Call(n) if !self.names.contains_key(n)))
            .count()
    }

    /// Append `ops` to the interpreted image. Labels define names at the next
    /// instruction position and persist across appends; a duplicate or
    /// already-defined label is rejected loudly before any mutation.
    pub fn assemble(&mut self, ops: &[Op]) -> Result<(), DynError> {
        // Validate all new labels first so a rejected batch mutates nothing.
        let mut batch: HashSet<&str> = HashSet::new();
        for op in ops {
            if let Op::Label(name) = op
                && (self.names.contains_key(name) || !batch.insert(name.as_str()))
            {
                return Err(DynError::Interp(format!("label `{name}` already defined")));
            }
        }
        for op in ops {
            match op {
                Op::Label(name) => {
                    self.names.insert(name.clone(), self.code.len());
                }
                Op::MovRaxImm(v) => self.code.push(Inst::MovRaxImm(*v)),
                Op::Call(name) => self.code.push(Inst::Call(name.clone())),
                Op::Ret => self.code.push(Inst::Ret),
            }
        }
        Ok(())
    }

    /// Interpret the block at `name` as `fn() -> i64` and return its result.
    ///
    /// Safe: no native code is generated or entered. Fails loudly on an
    /// unresolved call, a step-budget overrun (runaway recursion/loop), or a
    /// call-depth overrun, rather than hanging or corrupting state.
    pub fn enter_i64(&self, name: &str) -> Result<i64, DynError> {
        let start = *self
            .names
            .get(name)
            .ok_or_else(|| DynError::Interp(format!("unknown entry name `{name}`")))?;

        let mut rax: i64 = 0;
        let mut pc = start;
        let mut stack: Vec<usize> = Vec::new();
        let mut steps: u64 = 0;

        loop {
            steps += 1;
            if steps > MAX_STEPS {
                return Err(DynError::Interp(format!(
                    "step budget {MAX_STEPS} exceeded"
                )));
            }
            let inst = self.code.get(pc).ok_or_else(|| {
                DynError::Interp(format!("program counter {pc} ran off the code stream"))
            })?;
            match inst {
                Inst::MovRaxImm(v) => {
                    rax = *v;
                    pc += 1;
                }
                Inst::Call(target_name) => {
                    let target = *self.names.get(target_name).ok_or_else(|| {
                        DynError::Interp(format!("call to unresolved name `{target_name}`"))
                    })?;
                    if stack.len() >= MAX_CALL_DEPTH {
                        return Err(DynError::Interp(format!(
                            "call depth {MAX_CALL_DEPTH} exceeded"
                        )));
                    }
                    stack.push(pc + 1);
                    pc = target;
                }
                Inst::Ret => match stack.pop() {
                    Some(ret) => pc = ret,
                    None => return Ok(rax),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_1_forward_reference_then_supply() {
        let mut it = Interp::new();
        it.assemble(&[Op::Label("main".into()), Op::Call("answer".into()), Op::Ret])
            .unwrap();
        assert_eq!(it.pending_count(), 1);
        it.assemble(&[Op::Label("answer".into()), Op::MovRaxImm(1234), Op::Ret])
            .unwrap();
        assert_eq!(it.pending_count(), 0);
        assert_eq!(it.enter_i64("main").unwrap(), 1234);
    }

    #[test]
    fn scene_2_names_across_appends() {
        let mut it = Interp::new();
        it.assemble(&[Op::Label("helper".into()), Op::MovRaxImm(7), Op::Ret])
            .unwrap();
        it.assemble(&[Op::Label("caller".into()), Op::Call("helper".into()), Op::Ret])
            .unwrap();
        assert_eq!(it.enter_i64("caller").unwrap(), 7);
    }

    #[test]
    fn entering_with_open_forward_reference_fails_loudly() {
        let mut it = Interp::new();
        it.assemble(&[Op::Label("m".into()), Op::Call("missing".into()), Op::Ret])
            .unwrap();
        assert!(it.enter_i64("m").is_err());
    }

    #[test]
    fn runaway_recursion_hits_a_bound() {
        let mut it = Interp::new();
        // loop: call loop  — unbounded; must fail (depth or step budget), not hang.
        it.assemble(&[Op::Label("loop".into()), Op::Call("loop".into()), Op::Ret])
            .unwrap();
        assert!(it.enter_i64("loop").is_err());
    }

    #[test]
    fn duplicate_label_rejected() {
        let mut it = Interp::new();
        it.assemble(&[Op::Label("x".into()), Op::Ret]).unwrap();
        assert!(it.assemble(&[Op::Label("x".into()), Op::Ret]).is_err());
    }
}
