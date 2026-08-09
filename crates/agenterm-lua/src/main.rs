//! `agenterm-lua` binary — thin shell over [`agenterm_lua::cli::run`].
//! CLI logic lives in `crates/agenterm-lua/src/cli.rs` (SUB-M1, see
//! `plan/design-script-engine-subcommands.md` §2) so the root `agenterm`
//! binary can reach the same entry point as a subcommand later (SUB-M3).

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(agenterm_lua::cli::run(&args) as i32);
}
