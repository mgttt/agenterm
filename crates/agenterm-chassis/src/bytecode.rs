//! Tiny Chassis-L2 AOT ISA: assemble a JSON-friendly IR to deterministic bytecode.
//!
//! Daily path is pack-not-compile. This module only encodes a bounded custom ISA.

use serde::de::{self, Deserializer};
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};

/// Hard cap on IR ops accepted by [`assemble`].
pub const MAX_OPS: usize = 4096;

/// `CallCap` indexes `Program::caps` with a single byte.
pub const MAX_CAPS: usize = 256;

/// Bytecode opcodes. Discriminants are the on-wire `u8` values.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    PushI64 = 0,
    Add = 1,
    Sub = 2,
    Eq = 3,
    Not = 4,
    Jump = 5,
    JumpIfZero = 6,
    CallCap = 7,
    Ret = 8,
    Halt = 9,
}

impl Op {
    /// Decode a raw opcode byte. Unknown values are `None`.
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::PushI64),
            1 => Some(Self::Add),
            2 => Some(Self::Sub),
            3 => Some(Self::Eq),
            4 => Some(Self::Not),
            5 => Some(Self::Jump),
            6 => Some(Self::JumpIfZero),
            7 => Some(Self::CallCap),
            8 => Some(Self::Ret),
            9 => Some(Self::Halt),
            _ => None,
        }
    }

    /// On-wire size of this opcode plus its immediate operands.
    pub const fn encoded_len(self) -> usize {
        match self {
            Self::PushI64 | Self::Jump | Self::JumpIfZero => 1 + 8,
            Self::CallCap => 1 + 1,
            Self::Add | Self::Sub | Self::Eq | Self::Not | Self::Ret | Self::Halt => 1,
        }
    }
}

/// Assembled program: opcode stream plus the capability name table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub code: Vec<u8>,
    pub caps: Vec<String>,
}

/// One decoded instruction. Jump operands are absolute byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    PushI64(i64),
    Add,
    Sub,
    Eq,
    Not,
    Jump(usize),
    JumpIfZero(usize),
    CallCap(u8),
    Ret,
    Halt,
}

/// JSON-friendly IR. Ops serialize as arrays: `["push", 1]`, `["add"]`, `["call", "tabs.list"]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct L2Source {
    pub caps: Vec<String>,
    pub ops: Vec<IrOp>,
}

/// One IR operation. Jump targets are 0-based instruction indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrOp {
    Push(i64),
    Add,
    Sub,
    Eq,
    Not,
    Jump(i64),
    JumpIfZero(i64),
    Call(String),
    Ret,
    Halt,
}

impl L2Source {
    /// Parse the JSON IR used in pack artifacts.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|err| err.to_string())
    }
}

/// Assemble `source` to a deterministic bytecode [`Program`].
///
/// When `allow_list` is `Some`, every name in `source.caps` (and every `call`)
/// must appear in that list.
pub fn assemble(source: &L2Source, allow_list: Option<&[String]>) -> Result<Program, String> {
    if source.ops.len() > MAX_OPS {
        return Err(format!(
            "program has {} ops; max is {MAX_OPS}",
            source.ops.len()
        ));
    }
    if source.caps.len() > MAX_CAPS {
        return Err(format!(
            "program has {} caps; max is {MAX_CAPS}",
            source.caps.len()
        ));
    }

    if let Some(allow) = allow_list {
        for name in &source.caps {
            if !allow.iter().any(|allowed| allowed == name) {
                return Err(format!("unknown cap name `{name}`"));
            }
        }
    }

    let mut cap_index = std::collections::BTreeMap::<&str, u8>::new();
    for (i, name) in source.caps.iter().enumerate() {
        cap_index.insert(name.as_str(), i as u8);
    }

    let mut sizes = Vec::with_capacity(source.ops.len());
    let mut parsed = Vec::with_capacity(source.ops.len());
    for (i, op) in source.ops.iter().enumerate() {
        let (kind, jump_idx, push_val, cap) = match op {
            IrOp::Push(v) => (Op::PushI64, None, Some(*v), None),
            IrOp::Add => (Op::Add, None, None, None),
            IrOp::Sub => (Op::Sub, None, None, None),
            IrOp::Eq => (Op::Eq, None, None, None),
            IrOp::Not => (Op::Not, None, None, None),
            IrOp::Jump(t) => (Op::Jump, Some(*t), None, None),
            IrOp::JumpIfZero(t) => (Op::JumpIfZero, Some(*t), None, None),
            IrOp::Call(name) => {
                if let Some(allow) = allow_list
                    && !allow.iter().any(|allowed| allowed == name)
                {
                    return Err(format!("unknown cap name `{name}`"));
                }
                let idx = cap_index.get(name.as_str()).copied().ok_or_else(|| {
                    format!("call `{name}` is not in the program cap table (op {i})")
                })?;
                (Op::CallCap, None, None, Some(idx))
            }
            IrOp::Ret => (Op::Ret, None, None, None),
            IrOp::Halt => (Op::Halt, None, None, None),
        };
        sizes.push(kind.encoded_len());
        parsed.push((kind, jump_idx, push_val, cap));
    }

    let mut offsets = Vec::with_capacity(parsed.len());
    let mut cursor = 0usize;
    for size in &sizes {
        offsets.push(cursor);
        cursor = cursor.saturating_add(*size);
    }

    let mut code = Vec::with_capacity(cursor);
    for (i, (kind, jump_idx, push_val, cap)) in parsed.into_iter().enumerate() {
        code.push(kind as u8);
        match kind {
            Op::PushI64 => {
                let v =
                    push_val.ok_or_else(|| format!("internal: missing push immediate (op {i})"))?;
                code.extend_from_slice(&v.to_le_bytes());
            }
            Op::Jump | Op::JumpIfZero => {
                let target =
                    jump_idx.ok_or_else(|| format!("internal: missing jump target (op {i})"))?;
                if target < 0 {
                    return Err(format!("jump target {target} is negative (op {i})"));
                }
                let idx = target as usize;
                if idx >= offsets.len() {
                    return Err(format!(
                        "jump target {idx} is out of range (op {i}, {} ops)",
                        offsets.len()
                    ));
                }
                let off = offsets[idx] as i64;
                code.extend_from_slice(&off.to_le_bytes());
            }
            Op::CallCap => {
                let idx = cap.ok_or_else(|| format!("internal: missing cap index (op {i})"))?;
                code.push(idx);
            }
            Op::Add | Op::Sub | Op::Eq | Op::Not | Op::Ret | Op::Halt => {}
        }
    }

    Ok(Program {
        code,
        caps: source.caps.clone(),
    })
}

/// Decode `code` into instructions. Jump targets must land on instruction starts.
pub fn decode(code: &[u8]) -> Result<Vec<Inst>, String> {
    let mut starts = Vec::new();
    let mut i = 0usize;
    while i < code.len() {
        starts.push(i);
        let op =
            Op::from_u8(code[i]).ok_or_else(|| format!("unknown opcode {} at {i}", code[i]))?;
        let need = op.encoded_len();
        if i + need > code.len() {
            return Err(format!("truncated operand for {op:?} at {i}"));
        }
        i += need;
    }

    let mut insts = Vec::with_capacity(starts.len());
    for &at in &starts {
        let op =
            Op::from_u8(code[at]).ok_or_else(|| format!("unknown opcode {} at {at}", code[at]))?;
        let inst = match op {
            Op::PushI64 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&code[at + 1..at + 9]);
                Inst::PushI64(i64::from_le_bytes(buf))
            }
            Op::Add => Inst::Add,
            Op::Sub => Inst::Sub,
            Op::Eq => Inst::Eq,
            Op::Not => Inst::Not,
            Op::Jump => Inst::Jump(read_jump_target(code, at, &starts)?),
            Op::JumpIfZero => Inst::JumpIfZero(read_jump_target(code, at, &starts)?),
            Op::CallCap => Inst::CallCap(code[at + 1]),
            Op::Ret => Inst::Ret,
            Op::Halt => Inst::Halt,
        };
        insts.push(inst);
    }
    Ok(insts)
}

fn read_jump_target(code: &[u8], at: usize, starts: &[usize]) -> Result<usize, String> {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&code[at + 1..at + 9]);
    let off = i64::from_le_bytes(buf);
    if off < 0 {
        return Err(format!("bad jump: negative offset {off} at {at}"));
    }
    let off = off as usize;
    starts
        .iter()
        .position(|&s| s == off)
        .ok_or_else(|| format!("bad jump: offset {off} is not an instruction boundary (at {at})"))
}

impl Serialize for IrOp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            IrOp::Push(v) => ser_pair(serializer, "push", serde_json::json!(v)),
            IrOp::Add => ser_name(serializer, "add"),
            IrOp::Sub => ser_name(serializer, "sub"),
            IrOp::Eq => ser_name(serializer, "eq"),
            IrOp::Not => ser_name(serializer, "not"),
            IrOp::Jump(t) => ser_pair(serializer, "jump", serde_json::json!(t)),
            IrOp::JumpIfZero(t) => ser_pair(serializer, "jz", serde_json::json!(t)),
            IrOp::Call(name) => ser_pair(serializer, "call", serde_json::json!(name)),
            IrOp::Ret => ser_name(serializer, "ret"),
            IrOp::Halt => ser_name(serializer, "halt"),
        }
    }
}

impl<'de> Deserialize<'de> for IrOp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let cells = Vec::<serde_json::Value>::deserialize(deserializer)?;
        parse_ir_op(&cells).map_err(de::Error::custom)
    }
}

fn ser_name<S: Serializer>(serializer: S, name: &str) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_seq(Some(1))?;
    seq.serialize_element(name)?;
    seq.end()
}

fn ser_pair<S: Serializer>(
    serializer: S,
    name: &str,
    arg: serde_json::Value,
) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_seq(Some(2))?;
    seq.serialize_element(name)?;
    seq.serialize_element(&arg)?;
    seq.end()
}

fn parse_ir_op(cells: &[serde_json::Value]) -> Result<IrOp, String> {
    let name = cells
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| "op must start with a string name".to_string())?;
    match name {
        "push" | "push_i64" | "pushi64" => {
            let v = cells
                .get(1)
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "push requires an i64 operand".to_string())?;
            Ok(IrOp::Push(v))
        }
        "add" => Ok(IrOp::Add),
        "sub" => Ok(IrOp::Sub),
        "eq" => Ok(IrOp::Eq),
        "not" => Ok(IrOp::Not),
        "jump" | "jmp" => {
            let t = cells
                .get(1)
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "jump requires an i64 target".to_string())?;
            Ok(IrOp::Jump(t))
        }
        "jz" | "jump_if_zero" | "jumpifzero" => {
            let t = cells
                .get(1)
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "jump_if_zero requires an i64 target".to_string())?;
            Ok(IrOp::JumpIfZero(t))
        }
        "call" | "call_cap" | "callcap" => {
            let cap = cells
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| "call requires a capability name".to_string())?;
            Ok(IrOp::Call(cap.to_string()))
        }
        "ret" => Ok(IrOp::Ret),
        "halt" => Ok(IrOp::Halt),
        other => Err(format!("unknown IR op `{other}`")),
    }
}
