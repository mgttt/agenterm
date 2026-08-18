//! Minimal x86_64 encoding table for the live-assembly cut (dyn.2).
//!
//! This is **not** a general assembler. It covers exactly the four operations
//! the two acceptance scenes need — define a name, load an immediate return
//! value, call a name (possibly not yet defined), return — and nothing more.
//! Each emitting op has a golden-byte unit test below.
//!
//! References go through names only. A [`Op::Call`] emits `E8` plus a
//! placeholder rel32 and records a [`Fixup`] at the rel32 field; the engine
//! backpatches it once the target address is known (forward reference).

use crate::error::DynError;

/// One operation in an assembly-level sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Define `name` at the current position (a label).
    Label(String),
    /// `mov rax, imm64` — set the `i64` return value.
    MovRaxImm(i64),
    /// `call <name>` — E8 rel32 to `name`; `name` may be defined later.
    Call(String),
    /// `ret`.
    Ret,
}

/// A rel32 site that must be patched to point at `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixup {
    /// Byte offset (within the encoded batch) of the 4-byte rel32 field.
    pub at: usize,
    /// Name the rel32 must reach.
    pub target: String,
}

/// Result of encoding a batch of [`Op`]s.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Encoded {
    /// Emitted host-ISA bytes.
    pub bytes: Vec<u8>,
    /// `(name, offset-within-batch)` for each label defined in this batch.
    pub labels: Vec<(String, usize)>,
    /// Unresolved rel32 sites in this batch.
    pub fixups: Vec<Fixup>,
}

/// Encode `ops` into bytes, labels, and rel32 fixups.
///
/// A duplicate label name within one batch is rejected loudly.
pub fn encode(ops: &[Op]) -> Result<Encoded, DynError> {
    let mut out = Encoded::default();
    for op in ops {
        match op {
            Op::Label(name) => {
                if out.labels.iter().any(|(n, _)| n == name) {
                    return Err(DynError::Exec(format!("duplicate label `{name}` in batch")));
                }
                out.labels.push((name.clone(), out.bytes.len()));
            }
            Op::MovRaxImm(imm) => {
                // 48 B8 <imm64 LE> : movabs rax, imm64
                out.bytes.push(0x48);
                out.bytes.push(0xB8);
                out.bytes.extend_from_slice(&imm.to_le_bytes());
            }
            Op::Call(name) => {
                // E8 <rel32 LE> : call rel32; rel32 is a placeholder patched later.
                out.bytes.push(0xE8);
                let at = out.bytes.len();
                out.bytes.extend_from_slice(&[0, 0, 0, 0]);
                out.fixups.push(Fixup {
                    at,
                    target: name.clone(),
                });
            }
            Op::Ret => out.bytes.push(0xC3),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_mov_rax_imm() {
        let e = encode(&[Op::MovRaxImm(42)]).unwrap();
        assert_eq!(e.bytes, vec![0x48, 0xB8, 42, 0, 0, 0, 0, 0, 0, 0]);
        assert!(e.fixups.is_empty());
        assert!(e.labels.is_empty());
    }

    #[test]
    fn golden_ret() {
        assert_eq!(encode(&[Op::Ret]).unwrap().bytes, vec![0xC3]);
    }

    #[test]
    fn golden_call_emits_placeholder_and_fixup() {
        let e = encode(&[Op::Call("foo".into())]).unwrap();
        assert_eq!(e.bytes, vec![0xE8, 0, 0, 0, 0]);
        assert_eq!(e.fixups, vec![Fixup { at: 1, target: "foo".into() }]);
    }

    #[test]
    fn label_records_offset_and_rejects_duplicates() {
        let e = encode(&[Op::Ret, Op::Label("here".into()), Op::Ret]).unwrap();
        assert_eq!(e.labels, vec![("here".to_string(), 1)]);
        assert!(encode(&[Op::Label("x".into()), Op::Label("x".into())]).is_err());
    }
}
