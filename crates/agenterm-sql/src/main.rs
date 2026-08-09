//! `agenterm-sql` binary — thin shell over [`agenterm_sql::cli::run`].
//! CLI logic lives in `crates/agenterm-sql/src/cli.rs` (SUB-M1, see
//! `plan/design-script-engine-subcommands.md` §2) so the root `agenterm`
//! binary can reach the same entry point as a subcommand later (SUB-M3).

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    ExitCode::from(agenterm_sql::cli::run(&arguments))
}
