//! Thin shell for the `agenterm-rh` binary.
//!
//! All CLI logic (worker modes, incremental-rustc wrapper, verb dispatch)
//! moved to `agenterm::script_rh_cli_main::run_main` — see
//! `plan/design-script-engine-subcommands.md` SUB-M2. This file only
//! collects argv (the same way the moved code used to) and forwards it.

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let os_arguments = env::args_os().skip(1).collect::<Vec<_>>();
    agenterm::script_rh_cli_main::run_main(os_arguments)
}
