//! agenterm-sql — CLI verb shape aligned with `agenterm-rh`/`agenterm-lua`/
//! `agenterm-qjs` (`crates/agenterm-{rh,lua,qjs}/src/main.rs`) where the verb
//! is real, and reserved-but-honest where it isn't:
//!
//! - `check`, `check-many`, `corpus-scan`, `version`, `--help` — **real**,
//!   same as the other three engines.
//! - `eval`, `run`, `pack`, `qualify`, `task` — **honest not-implemented
//!   stubs**. Each exits `2` (this crate's usage/configuration-error
//!   convention, matching `agenterm-qjs`'s own exit-code table) and prints a
//!   message naming the actual blocker: `eval.rs`'s open design question
//!   (what does executing `.sql` source even run against?), not a generic
//!   "unknown command". Reserving the verb table now means a wrapper script
//!   that already calls `agenterm-sql eval foo.sql` today gets a stable,
//!   greppable error instead of "unknown command `eval`" — and gets upgraded
//!   to real behavior later without a CLI shape change.
//!
//! See `lib.rs`'s module doc for the crate-wide "what's real vs. placeholder"
//! table and the open design question in full; `plan/design-script-engine-trait.md`
//! §2.6 is the SSOT this crate scaffolds against.
//!
//! Argv parsing goes through `agenterm_script_common::cli`'s slice-based
//! helpers (re-exported from `agenterm_sql`'s lib — `main.rs` is compiled as
//! a `[[bin]]` of the root `agenterm` package, which doesn't depend on
//! `agenterm-script-common` directly, see that re-export's doc in `lib.rs`).

use std::{
    env, fs,
    path::PathBuf,
    process::ExitCode,
};

use agenterm_sql::{
    SQL_VERSION, SqlError, check, has_flag, parse_check_many_cli, positional, read_manifest,
    require_flag_value, run_check_many,
};

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<u8, SqlError> {
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
                    .map_err(SqlError::Check)?,
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
        "eval" | "run" | "pack" | "qualify" | "task" => {
            return Ok(not_implemented_stub(command));
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(SqlError::Check(format!(
                "unknown command `{other}`; try check | check-many | corpus-scan | version \
                 (eval | run | pack | qualify | task are reserved, not implemented — see \
                 `agenterm-sql eval --help`-shaped output by just running them)"
            )));
        }
    }

    Ok(0)
}

/// `eval`/`run`/`pack`/`qualify`/`task` all share one honest-stub behavior:
/// print exactly why (the open execution-target design question — see
/// `lib.rs`'s and `eval.rs`'s module docs), point at the design doc, and
/// exit `2`. Kept as one function so the five verbs can never drift into
/// five slightly different messages.
fn not_implemented_stub(command: &str) -> u8 {
    eprintln!(
        "{command}: not implemented — the sql backend has no decided execution target yet \
         (embedded engine vs. external DB connection vs. host-state-as-virtual-tables); see \
         crates/agenterm-sql/src/lib.rs's module doc and \
         plan/design-script-engine-trait.md \u{a7}2.6. `check`/`check-many`/`corpus-scan` \
         (parse-only, no execution) work today."
    );
    2
}

fn run_check_many_command(args: &[String]) -> Result<u8, SqlError> {
    let parsed = parse_check_many_cli(args.iter().cloned())?;
    let manifest = read_manifest(&parsed.manifest_path)?;
    let report = run_check_many(manifest, parsed.options);
    if parsed.json {
        let encoded = serde_json::to_string_pretty(&report)
            .map_err(|err| SqlError::Check(err.to_string()))?;
        println!("{encoded}");
    } else if report.ok {
        println!("OK ({} files)", report.checked_files);
    } else {
        for failure in &report.failures {
            eprintln!(
                "{}: {}",
                failure.path,
                serde_json::json!({
                    "code": failure.code,
                    "message": failure.message,
                    "invocation_id": failure.invocation_id,
                    "exit_class": failure.exit_class,
                })
            );
        }
    }
    Ok(report.exit_code())
}

/// `corpus-scan [--dir <dir>]` — scan a directory for `.sql` files and check
/// syntax, mirroring `agenterm-qjs`'s `run_corpus_scan_command`. Uses
/// `has_flag` + `require_flag_value` (not the absent/no-value-collapsing
/// `find_flag_value`) so "no `--dir`" and "`--dir` with nothing after it"
/// are distinguishable — the latter is a hard error, matching qjs.
fn run_corpus_scan_command(args: &[String]) -> Result<u8, SqlError> {
    let dir = if has_flag(args, "--dir") {
        // `usage` is unreachable here: `has_flag` already guarantees the
        // flag is present, so the only way `require_flag_value` can fail
        // is the "no value follows" branch.
        PathBuf::from(require_flag_value(args, "--dir", "unreachable").map_err(SqlError::Check)?)
    } else {
        std::env::current_dir()
            .map_err(|err| SqlError::Check(format!("corpus_scan_cwd: {err}")))?
    };
    let report = agenterm_sql::scan_directory(&dir)
        .map_err(|err| SqlError::Check(format!("corpus_scan: {err}")))?;
    if report.failures == 0 {
        println!("corpus-scan: {} scripts ok", report.total_scripts);
        Ok(0)
    } else {
        eprintln!(
            "corpus-scan: {} scripts checked, {} failures",
            report.total_scripts, report.failures
        );
        for failed in &report.failed_files {
            eprintln!("  {} — {}", failed.path, failed.message);
        }
        Ok(1)
    }
}

fn read_source(path: &PathBuf) -> Result<String, SqlError> {
    fs::read_to_string(path).map_err(|err| SqlError::Check(format!("{}: {err}", path.display())))
}

fn print_usage() {
    println!(
        "agenterm-sql {SQL_VERSION} — SQL script engine scaffold (benchmark targets: SQL-92, \
         PostgreSQL), capability-aligned with agenterm-rh/agenterm-lua/agenterm-qjs\n\n\
Usage:\n  \
agenterm-sql check <file.sql>\n  \
agenterm-sql check-many --manifest <file.json> [--project-root DIR] [--timeout-ms N] [--json]\n  \
agenterm-sql corpus-scan [--dir <dir>]\n  \
agenterm-sql version\n\n\
Reserved, not yet implemented (exit 2 with an explanation, not \"unknown command\"):\n  \
agenterm-sql eval <file.sql>\n  \
agenterm-sql run <file.sql>\n  \
agenterm-sql pack ...\n  \
agenterm-sql qualify <file.sql>\n  \
agenterm-sql task ...\n\n\
Why: sql source has no decided execution target yet (embedded engine vs.\n\
external DB connection vs. host-state-as-virtual-tables) — see\n\
crates/agenterm-sql/src/lib.rs's module doc and\n\
plan/design-script-engine-trait.md \u{a7}2.6."
    );
}

#[cfg(test)]
mod tests {
    use agenterm_sql::find_flag_value;

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
