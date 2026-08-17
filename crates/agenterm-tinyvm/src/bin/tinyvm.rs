//! Minimal `tinyvm` command: `tinyvm eval [asm...]`.
//!
//! Assembles one-instruction-per-line text and runs it on a fresh [`Vm`],
//! printing the resulting stack top. Assembly comes from the arguments after
//! `eval` (joined with newlines) or, if none are given, from stdin.
//!
//! This is intentionally not a REPL framework — the persistent-image REPL is
//! the library's `Vm::eval` loop; this binary is a thin one-shot front door.

use std::io::Read;
use std::process::ExitCode;

use agenterm_tinyvm::Vm;

const MEM_CELLS: usize = 4_096;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("eval") => {
            let rest: Vec<String> = args.collect();
            let src = if rest.is_empty() {
                let mut buf = String::new();
                if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                    eprintln!("tinyvm: reading stdin: {e}");
                    return ExitCode::FAILURE;
                }
                buf
            } else {
                rest.join("\n")
            };
            run_eval(&src)
        }
        Some(other) => {
            eprintln!("tinyvm: unknown command `{other}` (try: tinyvm eval)");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("usage: tinyvm eval [asm...]   (asm from args or stdin; prints stack top)");
            ExitCode::FAILURE
        }
    }
}

fn run_eval(src: &str) -> ExitCode {
    let mut vm = Vm::new(MEM_CELLS);
    match vm.eval(src) {
        Ok(Some(top)) => {
            println!("{top}");
            ExitCode::SUCCESS
        }
        Ok(None) => {
            println!("(empty)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("tinyvm: {}", e.message());
            ExitCode::FAILURE
        }
    }
}
