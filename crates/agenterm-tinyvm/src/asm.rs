//! One-line-per-instruction textual assembly for [`crate::Instr`].
//!
//! This is deliberately *not* a language: each non-blank line is a single
//! mnemonic — the same name as the [`crate::Instr`] variant — optionally
//! followed by one operand. Blank lines and `;`/`#` comments are ignored.
//!
//! ```text
//! push 40
//! push 2
//! add        ; stack top is now 42
//! ```
//!
//! No new opcodes are introduced: mnemonics map one-to-one onto existing
//! instructions. A malformed line fails loudly with its line number.

use crate::Instr;
use alloc::vec::Vec;

/// A textual-assembly parse fault, with the 1-based source line.
#[cfg_attr(test, derive(Debug))]
#[derive(Clone, PartialEq, Eq)]
pub struct AsmError {
    /// 1-based line number of the offending line.
    pub line: usize,
    /// What went wrong (a static message; the crate carries no formatting).
    pub msg: &'static str,
}

/// Assemble one-instruction-per-line `src` into a program.
pub fn assemble(src: &str) -> Result<Vec<Instr>, AsmError> {
    let mut program = Vec::new();
    for (idx, raw) in src.lines().enumerate() {
        let line = idx + 1;
        // Strip a trailing `;` or `#` comment, then surrounding whitespace.
        let code = raw.split([';', '#']).next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        let mut toks = code.split_whitespace();
        let mnem = toks.next().expect("non-empty line has a first token");
        let operand = toks.next();
        if toks.next().is_some() {
            return Err(AsmError {
                line,
                msg: "unexpected extra token after mnemonic",
            });
        }
        program.push(parse_one(mnem, operand, line)?);
    }
    Ok(program)
}

fn parse_one(mnem: &str, operand: Option<&str>, line: usize) -> Result<Instr, AsmError> {
    let instr = match mnem {
        "push" => Instr::Push(int_operand(mnem, operand, line)?),
        "load" => Instr::Load(addr_operand(mnem, operand, line)?),
        "store" => Instr::Store(addr_operand(mnem, operand, line)?),
        "jmp" => Instr::Jmp(addr_operand(mnem, operand, line)?),
        "jz" => Instr::Jz(addr_operand(mnem, operand, line)?),
        "call" => Instr::Call(addr_operand(mnem, operand, line)?),
        "pop" => nullary(Instr::Pop, mnem, operand, line)?,
        "dup" => nullary(Instr::Dup, mnem, operand, line)?,
        "swap" => nullary(Instr::Swap, mnem, operand, line)?,
        "add" => nullary(Instr::Add, mnem, operand, line)?,
        "sub" => nullary(Instr::Sub, mnem, operand, line)?,
        "mul" => nullary(Instr::Mul, mnem, operand, line)?,
        "div" => nullary(Instr::Div, mnem, operand, line)?,
        "mod" => nullary(Instr::Mod, mnem, operand, line)?,
        "eq" => nullary(Instr::Eq, mnem, operand, line)?,
        "lt" => nullary(Instr::Lt, mnem, operand, line)?,
        "gt" => nullary(Instr::Gt, mnem, operand, line)?,
        "not" => nullary(Instr::Not, mnem, operand, line)?,
        "ret" => nullary(Instr::Ret, mnem, operand, line)?,
        "halt" => nullary(Instr::Halt, mnem, operand, line)?,
        _other => {
            return Err(AsmError {
                line,
                msg: "unknown mnemonic",
            });
        }
    };
    Ok(instr)
}

fn nullary(
    instr: Instr,
    _mnem: &str,
    operand: Option<&str>,
    line: usize,
) -> Result<Instr, AsmError> {
    match operand {
        None => Ok(instr),
        Some(_tok) => Err(AsmError {
            line,
            msg: "instruction takes no operand",
        }),
    }
}

fn int_operand(_mnem: &str, operand: Option<&str>, line: usize) -> Result<i64, AsmError> {
    let tok = operand.ok_or(AsmError {
        line,
        msg: "instruction requires an integer operand",
    })?;
    tok.parse::<i64>().map_err(|_| AsmError {
        line,
        msg: "operand is not a valid i64",
    })
}

fn addr_operand(_mnem: &str, operand: Option<&str>, line: usize) -> Result<usize, AsmError> {
    let tok = operand.ok_or(AsmError {
        line,
        msg: "instruction requires a non-negative operand",
    })?;
    tok.parse::<usize>().map_err(|_| AsmError {
        line,
        msg: "operand is not a valid address/index",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_simple_program() {
        let prog = assemble("push 40\npush 2\nadd").unwrap();
        assert_eq!(prog, vec![Instr::Push(40), Instr::Push(2), Instr::Add]);
    }

    #[test]
    fn ignores_blanks_and_comments() {
        let prog = assemble("  ; a comment\n\npush 1   # trailing\nhalt\n").unwrap();
        assert_eq!(prog, vec![Instr::Push(1), Instr::Halt]);
    }

    #[test]
    fn address_and_negative_operands() {
        let prog = assemble("push -5\njz 3\nload 2").unwrap();
        assert_eq!(prog, vec![Instr::Push(-5), Instr::Jz(3), Instr::Load(2)]);
    }

    #[test]
    fn unknown_mnemonic_is_loud() {
        let e = assemble("frobnicate 1").unwrap_err();
        assert_eq!(e.line, 1);
    }

    #[test]
    fn missing_operand_is_loud() {
        assert!(assemble("push").is_err());
        assert!(assemble("jz").is_err());
    }

    #[test]
    fn extra_operand_is_loud() {
        assert!(assemble("add 1").is_err());
        assert!(assemble("push 1 2").is_err());
    }
}
