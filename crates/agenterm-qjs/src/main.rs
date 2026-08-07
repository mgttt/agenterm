//! agenterm-qjs-dev — throwaway dev CLI for QJS-M0 smoke testing.
//!
//! Not the real `agenterm-qjs` CLI contract (that's QJS-M1, mirroring
//! `agenterm-rh`'s check/eval/pack/check-many/task verbs once this crate is
//! wired into the root workspace). For now: `agenterm-qjs-dev eval '1+2'`.

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("eval"), Some(source)) => match agenterm_qjs::eval_to_string(&source) {
            Ok(result) => {
                println!("{result}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("eval error: {err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: agenterm-qjs-dev eval <source>");
            ExitCode::FAILURE
        }
    }
}
