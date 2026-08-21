//! PR-A4 extractability gate: run a `.rh` file with **nothing but this crate**.
//!
//! This is the proof that `crates/agenterm-rh` is extractable. `cargo tree -p
//! agenterm-rh` omitting the `agenterm` package has been true all along and
//! proves nothing on its own — the real question is whether a `.rh` file can
//! *execute* without the root package's host. So this example actually runs
//! one, with `print` and `std::fs` going through `StdHost`.
//!
//! Usage:
//! ```text
//! cargo run -p agenterm-rh --example standalone_eval -- <file.rh> [ARGS...]
//! cargo run -p agenterm-rh --example standalone_eval -- --selftest
//! cargo run -p agenterm-rh --example standalone_eval -- --selftest --sandboxed
//! ```
//!
//! `--sandboxed` swaps `StdHost` for a host that implements nothing. It exists
//! so the gate is falsifiable: if the program still succeeded without
//! `StdHost`, the example would not be proving anything.

use std::path::PathBuf;
use std::process::ExitCode;

use agenterm_rh::{Engine, Error, NullHost, StdHost, Value, exit_from_int};

/// A Language-1 program that exercises **both** host capabilities the gate
/// cares about: `print` and the `std::fs` round trip. Deliberately inline so
/// the self-test needs no fixture file and no `tempfile` dependency.
const SELFTEST: &str = r#"
fn entry() {
    let dir = rh::runtime::temp_dir();
    let path = dir + "/rh-standalone-selftest.txt";

    std::fs::write(path, "written-by-rh");
    if !std::fs::exists(path) {
        return 2;
    }

    let text = std::fs::read_to_string(path);
    if text != "written-by-rh" {
        return 3;
    }

    let meta = std::fs::metadata(path);
    print("standalone_eval: wrote " + meta.len + " bytes to " + path);

    std::fs::remove_file(path);
    if std::fs::exists(path) {
        return 4;
    }

    print("standalone_eval: ok");
    0
}
"#;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let sandboxed = argv.iter().any(|arg| arg == "--sandboxed");
    let positional: Vec<String> = argv
        .into_iter()
        .filter(|arg| arg != "--sandboxed")
        .collect();

    let Some((first, rest)) = positional.split_first() else {
        eprintln!(
            "usage: standalone_eval <file.rh> [ARGS...]\n       standalone_eval --selftest [--sandboxed]"
        );
        return ExitCode::from(2);
    };

    // Script arguments are everything after the script path, exactly as the
    // product CLI defines `args` (design "argv to script").
    let script_args: Vec<String> = rest.to_vec();

    let result = if first == "--selftest" {
        run(SELFTEST, script_args, sandboxed)
    } else {
        let path = PathBuf::from(first);
        match std::fs::read_to_string(&path) {
            Ok(source) => run(&source, script_args, sandboxed),
            Err(error) => {
                eprintln!("standalone_eval: {}: {error}", path.display());
                return ExitCode::from(2);
            }
        }
    };

    match result {
        Ok(value) => {
            // The script's own value decides the exit code, via the same
            // mapping the product CLI freezes (D21).
            match value {
                Value::Int(code) => ExitCode::from(exit_from_int(code)),
                Value::Unit => ExitCode::SUCCESS,
                other => {
                    println!("standalone_eval: => {other:?}");
                    ExitCode::SUCCESS
                }
            }
        }
        Err(error) => {
            // One structured line on stderr, as the Observability table says.
            eprintln!("{error}");
            match error {
                Error::Parse(_) | Error::Subset { .. } => ExitCode::from(2),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

fn run(source: &str, args: Vec<String>, sandboxed: bool) -> Result<Value, Error> {
    if sandboxed {
        // No `StdHost`: `print` and `std::fs::*` must fail closed. This is the
        // control arm of the gate.
        return Engine::new_with_host(NullHost).eval(source);
    }
    Engine::new_with_host(StdHost::new().with_args(args)).eval(source)
}
