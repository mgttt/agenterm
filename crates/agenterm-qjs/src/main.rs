//! `agenterm-qjs` binary — thin shell over [`agenterm_qjs::cli::run`].
//! CLI logic lives in `crates/agenterm-qjs/src/cli.rs` (SUB-M1, see
//! `plan/design-script-engine-subcommands.md` §2) so the root `agenterm`
//! binary can reach the same entry point as a subcommand later (SUB-M3).

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    ExitCode::from(agenterm_qjs::cli::run(&arguments))
}
