//! agenterm-sql CLI logic — extracted from `main.rs` (SUB-M1, see
//! `plan/archive/design-script-engine-subcommands.md` §2/§5) so the root `agenterm`
//! binary can later call [`run`] directly as a subcommand entry point
//! without spawning a separate process (SUB-M3, not this pass). CLI verb
//! shape aligned with `agenterm-rh`/`agenterm-lua`/`agenterm-qjs`
//! (`crates/agenterm-{rh,lua,qjs}/src/{main,cli}.rs`) where the verb is
//! real, and reserved-but-honest where it isn't:
//!
//! - `check`, `check-many`, `corpus-scan`, `version`, `--help` — **real**,
//!   same as the other three engines.
//! - `eval`, `run` — **real as of M1** (`plan/design-sql-execution-target.md`
//!   §5's acceptance criterion). `run` is currently identical to `eval` in
//!   every observable way (see [`run_eval_or_run`]'s doc for why) — kept as
//!   a separate verb for CLI-shape parity with rh/lua/qjs, not because sql
//!   does anything different with it yet.
//! - `pack`, `qualify`, `task` — **still honest not-implemented stubs**
//!   (M1's scope is `eval`/`run` only; design doc §5's "不做" list). Each
//!   exits `2` (this crate's usage/configuration-error convention, matching
//!   `agenterm-qjs`'s own exit-code table) and prints a message naming the
//!   actual reason, not a generic "unknown command". Reserving the verb
//!   table now means a wrapper script that already calls
//!   `agenterm-sql pack ...` today gets a stable, greppable error instead
//!   of "unknown command `pack`" — and gets upgraded to real behavior later
//!   without a CLI shape change.
//!
//! ## Exit-code convention (2026-08, aligned with qjs — see
//! `crates/agenterm-qjs/src/cli.rs`'s matching doc, and
//! `tests/script_cli_verb_parity.rs`'s module doc for the full
//! cross-engine divergence table this pair now narrows)
//!
//! `0` success; `1` a **script-level** failure — the root cause is the
//! script content itself (a `check` syntax error, or now an `eval`/`run`
//! execution-time error), i.e. [`SqlError::Parse`]/[`SqlError::Check`]; `2`
//! a **usage/configuration-level** failure — bad argv, an unknown verb, a
//! missing/unreadable required flag or manifest, a reserved-but-not-
//! implemented verb, i.e. [`SqlError::Usage`] (or the literal `2` the
//! reserved-verb stubs return directly, see `not_implemented_stub` below —
//! they never construct an error at all). [`run`] (this module's top-level
//! dispatcher, not the CLI `run` verb — same name, different thing, see its
//! own doc) reads the returned `SqlError`'s variant to pick between `1`/`2`
//! — see `error.rs`'s doc for the full rationale. Previously `main()` folded
//! every `Err(_)` into a blanket exit `2` regardless of variant, so
//! `check <broken file>` (a `SqlError::Parse`, i.e. script-level) exited
//! `2`, not `1` — the exact cross-engine divergence
//! `tests/script_cli_verb_parity.rs` pinned and this pass fixes.
//!
//! See `lib.rs`'s module doc for the crate-wide "what's real vs. placeholder"
//! table; `plan/design-sql-execution-target.md` is the SSOT for the M1
//! `eval`/`run` execution semantics this module now wires up.
//!
//! Argv parsing goes through `agenterm_script_common::cli`'s slice-based
//! helpers (re-exported from `agenterm_sql`'s lib — see that re-export's
//! doc in `lib.rs`).

use std::{fs, path::PathBuf};

use crate::{SQL_VERSION, SqlError, check, execute_entry, positional, run_check_many};

/// Run the `agenterm-sql` CLI over `args` (argv **excluding** argv\[0\],
/// matching the former `main()`'s `env::args().skip(1)`) and return the
/// process exit code. Mirrors `main()`'s former `SqlError`-variant-to-exit-
/// code mapping (`Parse`/`Check` -> 1, `Usage` -> 2) plus its
/// `eprintln!("{error}")` on failure.
pub fn run(args: &[String]) -> u8 {
    match dispatch(args) {
        Ok(code) => code,
        Err(error) => {
            // Script-level (`Parse`/`Check`) -> 1, usage/configuration-level
            // (`Usage`) -> 2 — see this file's module doc and `error.rs`'s
            // doc for the classification rationale.
            let exit_code = match &error {
                SqlError::Parse(_) | SqlError::Check(_) => 1,
                SqlError::Usage(_) => 2,
            };
            eprintln!("{error}");
            exit_code
        }
    }
}

fn dispatch(args: &[String]) -> Result<u8, SqlError> {
    let Some(command) = args.first() else {
        print_usage();
        return Ok(0);
    };
    let rest = &args[1..];

    match command.as_str() {
        "version" | "--version" | "-V" => {
            println!("agenterm-sql {SQL_VERSION}");
        }
        "check" => {
            let path = PathBuf::from(
                positional(rest, 0, "usage: agenterm-sql check <file.sql>")
                    .map_err(SqlError::Usage)?,
            );
            let source = read_source(&path)?;
            let label = path.display().to_string();
            check(&source, &label)?;
            println!("sql check ok: {label}");
        }
        "check-many" => {
            return run_check_many_command(rest);
        }
        "corpus-scan" => {
            return run_corpus_scan_command(rest);
        }
        "eval" | "run" => {
            run_eval_or_run_command(command, rest)?;
        }
        "pack" | "qualify" | "task" => {
            return Ok(not_implemented_stub(command));
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(SqlError::Usage(format!(
                "unknown command `{other}`; try check | check-many | corpus-scan | eval | run | \
                 version (pack | qualify | task are reserved, not implemented — see \
                 `agenterm-sql pack --help`-shaped output by just running them)"
            )));
        }
    }

    Ok(0)
}

/// `eval <file.sql>` / `run <file.sql>` — both real as of M1
/// (`plan/design-sql-execution-target.md` §5's acceptance criterion:
/// `agenterm-sql eval fixtures/ok.sql` on `SELECT 1;` returns
/// `[{"1": 1}]`-shaped output, not a placeholder error). Reads the file,
/// runs it through `agenterm_sql::execute_entry`, prints any `stdout`
/// (always empty in M1 — see `eval.rs`'s doc) followed by the JSON-rendered
/// `value`.
///
/// `run` is not yet distinguished from `eval` the way it is for rh/lua/qjs
/// (which use `run` for a `-- <args>...` trailing-argument convention,
/// `crates/agenterm-qjs/src/cli.rs`'s `run_run_command` being the
/// reference shape): sql's `execute_entry` has no argument-binding concept
/// in M1 (no `__host.arg()`-equivalent — SQL parameter binding, if it ever
/// lands, is a different, not-yet-designed feature), so there is nothing
/// for a trailing `-- <args>...` to plug into yet. Both verbs are kept
/// (rather than only shipping `eval`) for CLI-shape parity with the other
/// three engines — a caller scripting against all four backends
/// uniformly can rely on both verbs existing — and `verb_label` is
/// threaded through only so the printed "ok" line and usage message say
/// the verb the caller actually typed.
fn run_eval_or_run_command(verb_label: &str, rest: &[String]) -> Result<(), SqlError> {
    let path = PathBuf::from(
        positional(
            rest,
            0,
            &format!("usage: agenterm-sql {verb_label} <file.sql>"),
        )
        .map_err(SqlError::Usage)?,
    );
    let source = read_source(&path)?;
    let label = path.display().to_string();

    let outcome = execute_entry(&source, &label, None)?;
    if !outcome.stdout.is_empty() {
        print!("{}", outcome.stdout);
    }
    println!(
        "sql {verb_label} ok: {label} -> {}",
        render_value(outcome.value.as_ref())
    );
    Ok(())
}

/// Renders `value` the same way `agenterm-qjs`'s `render_value` does
/// (`crates/agenterm-qjs/src/cli.rs`): `serde_json::Value`'s own `Display`
/// (compact JSON) for `Some`, the literal string `"undefined"` for `None`
/// — kept as its own function (not shared code) since the two crates don't
/// share a `cli`-shaped dependency, matching the existing
/// duplication-over-coupling choice already made for `read_source`/
/// `print_usage`-shaped helpers across rh/lua/qjs/sql's `cli.rs` files.
fn render_value(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "undefined".to_owned(),
    }
}

/// `pack`/`qualify`/`task` all share one honest-stub behavior: print
/// exactly why, point at the design doc, and exit `2`. Kept as one function
/// so the three verbs can never drift into three slightly different
/// messages.
///
/// 2026-08 M1 update: this used to cover `eval`/`run` too, with a message
/// about sql having "no decided execution target" — that's no longer true
/// (M1 decided it: embedded `rusqlite`/SQLite — see `lib.rs`'s module doc),
/// so `eval`/`run` moved to [`run_eval_or_run_command`] and this function's
/// message was rewritten to name the ACTUAL reason `pack`/`qualify`/`task`
/// remain unimplemented (no decided shape for sql, not an undecided
/// execution target).
fn not_implemented_stub(command: &str) -> u8 {
    eprintln!(
        "{command}: not implemented — sql's M1 execution target is decided (embedded \
         rusqlite/SQLite, see `eval`/`run`), but `pack`/`qualify`/`task` have no decided shape \
         for sql yet; see crates/agenterm-sql/src/lib.rs's module doc and \
         plan/design-sql-execution-target.md \u{a7}5 M3. `check`/`check-many`/`corpus-scan`/\
         `eval`/`run` work today."
    );
    2
}

fn run_check_many_command(args: &[String]) -> Result<u8, SqlError> {
    // The whole command body — argv, manifest, rendering, exit code — is
    // the shared qjs/sql implementation; every error path in it is
    // usage-level (see `agenterm_script_common::cli`'s doc), so one
    // `map_err(SqlError::Usage)` reproduces the exact former
    // classification. The manifest reader closure owns sql's `kind` check,
    // same as `read_manifest` (this crate's typed wrapper) does.
    agenterm_script_common::cli::run_check_many_command(
        args,
        |path| {
            agenterm_script_common::check_many::read_manifest(
                path,
                &[crate::check_many::SQL_CHECK_MANIFEST_KIND],
            )
        },
        run_check_many,
    )
    .map_err(SqlError::Usage)
}

/// `corpus-scan [--dir <dir>]` — scan a directory for `.sql` files and check
/// syntax. Command body shared with qjs ("no `--dir`" falls back to CWD; a
/// dangling `--dir` with no value is a hard error).
fn run_corpus_scan_command(args: &[String]) -> Result<u8, SqlError> {
    agenterm_script_common::cli::run_corpus_scan_command(args, crate::scan_directory)
        .map_err(SqlError::Usage)
}

fn read_source(path: &PathBuf) -> Result<String, SqlError> {
    // Usage-level: a file the caller pointed us at that isn't there/isn't
    // readable is a bad argument, not a syntax error in script content —
    // `check` (called with the source this returns) is what produces the
    // script-level `Parse` failures.
    fs::read_to_string(path).map_err(|err| SqlError::Usage(format!("{}: {err}", path.display())))
}

fn print_usage() {
    println!(
        "agenterm-sql {SQL_VERSION} — SQL script engine (benchmark targets: SQL-92, \
         PostgreSQL), capability-aligned with agenterm-rh/agenterm-lua/agenterm-qjs\n\n\
Usage:\n  \
agenterm-sql check <file.sql>\n  \
agenterm-sql check-many --manifest <file.json> [--project-root DIR] [--timeout-ms N] [--json]\n  \
agenterm-sql corpus-scan [--dir <dir>]\n  \
agenterm-sql eval <file.sql>\n  \
agenterm-sql run <file.sql>\n  \
agenterm-sql version\n\n\
Reserved, not yet implemented (exit 2 with an explanation, not \"unknown command\"):\n  \
agenterm-sql pack ...\n  \
agenterm-sql qualify <file.sql>\n  \
agenterm-sql task ...\n\n\
Why: sql's execution target IS decided (embedded rusqlite/SQLite, see\n\
`eval`/`run` above and plan/design-sql-execution-target.md) but\n\
`pack`/`qualify`/`task` don't have a decided shape for sql yet — see\n\
crates/agenterm-sql/src/lib.rs's module doc and\n\
plan/design-sql-execution-target.md \u{a7}5 M3."
    );
}

#[cfg(test)]
mod tests {
    use crate::find_flag_value;

    #[test]
    fn find_flag_value_reads_multiple_distinct_flags_from_one_collected_slice() {
        let collected = vec![
            "--dir".to_owned(),
            "out".to_owned(),
            "--project-root".to_owned(),
            "proj".to_owned(),
        ];
        assert_eq!(find_flag_value(&collected, "--dir"), Some("out"));
        assert_eq!(find_flag_value(&collected, "--project-root"), Some("proj"));
    }
}
