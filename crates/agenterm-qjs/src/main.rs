//! agenterm-qjs — CLI verb shape aligned with `agenterm-rh`
//! (`crates/agenterm-rh/src/main.rs`): `version`, `check`, `eval`,
//! `check-many`, `--help`. Same typed exit-code convention: `0` success,
//! `2` usage/argv error (`public_command_exit_code`-equivalent path below),
//! and `check-many`'s own `exit_code()` mapping for its typed failures.
//!
//! Not yet present (tracked, see PRD "qjs execution backend" / plan
//! QJS-M1/M2): `pack`, `qualify`, `task`, `run` — those need L2 facade/
//! Fleet wiring (`agenterm::script_backend`) that requires root-workspace
//! integration, deliberately deferred (see Cargo.toml `[workspace]` header
//! comment).

use std::{env, fs, path::PathBuf, process::ExitCode};

use agenterm_qjs::{
    QJS_VERSION, QjsError, check, eval_entry, parse_check_many_cli, read_manifest, run_check_many,
};

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match run(arguments.into_iter()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<u8, QjsError> {
    let Some(command) = args.next() else {
        print_usage();
        return Ok(0);
    };

    match command.as_str() {
        "version" | "--version" | "-V" => {
            println!("agenterm-qjs {QJS_VERSION}");
        }
        "check" => {
            let path = require_path(&mut args, "check")?;
            let source = read_source(&path)?;
            let label = path.display().to_string();
            check(&source, &label)?;
            println!("qjs check ok: {label}");
        }
        "eval" => {
            let path = require_path(&mut args, "eval")?;
            let source = read_source(&path)?;
            let label = path.display().to_string();
            let outcome = eval_entry(&source, &label)?;
            let rendered = match &outcome.value {
                Some(value) => value.to_string(),
                None => "undefined".to_owned(),
            };
            println!("qjs eval ok: {label} -> {rendered}");
        }
        "check-many" => {
            return run_check_many_command(&mut args);
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(QjsError::Check(format!(
                "unknown command `{other}`; try check | eval | check-many | version"
            )));
        }
    }

    Ok(0)
}

fn run_check_many_command(args: &mut impl Iterator<Item = String>) -> Result<u8, QjsError> {
    let parsed = parse_check_many_cli(args)?;
    let manifest = read_manifest(&parsed.manifest_path)?;
    let report = run_check_many(manifest, parsed.options);
    if parsed.json {
        let encoded = serde_json::to_string_pretty(&report)
            .map_err(|err| QjsError::Check(err.to_string()))?;
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

fn read_source(path: &PathBuf) -> Result<String, QjsError> {
    fs::read_to_string(path).map_err(|err| QjsError::Check(format!("{}: {err}", path.display())))
}

fn require_path(
    args: &mut impl Iterator<Item = String>,
    command: &str,
) -> Result<PathBuf, QjsError> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| QjsError::Check(format!("usage: agenterm-qjs {command} <file.js>")))
}

fn print_usage() {
    println!(
        "agenterm-qjs {QJS_VERSION} — QuickJS script engine, capability-aligned with agenterm-rh\n\n\
Usage:\n  \
agenterm-qjs check <file.js>\n  \
agenterm-qjs eval <file.js>\n  \
agenterm-qjs check-many --manifest <file.json> [--project-root DIR] [--timeout-ms N] [--json]\n  \
agenterm-qjs version\n\n\
Not yet implemented: pack, qualify, task, run (need L2 facade/Fleet wiring — see\n\
plan/plan-v0.1.16.md \u{a7}1 Rh, QJS-M2)."
    );
}
