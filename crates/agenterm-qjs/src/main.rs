//! agenterm-qjs — CLI verb shape aligned with `agenterm-rh`
//! (`crates/agenterm-rh/src/main.rs`) and `agenterm-lua`
//! (`crates/agenterm-lua/src/main.rs`): `version`, `check`, `eval`, `run`,
//! `hash`, `pack build`/`pack load`, `qualify`, `check-many`, `task`,
//! `--help`. Same typed exit-code convention: `0` success, `2` usage/argv
//! error, `1` for a runtime/qualify/pack failure (matching lua's
//! convention, not rh's `RhError`-typed one — qjs shares lua's untyped
//! `Result<u8, String>` dispatch shape here, see `dispatch`/`run` below).
//!
//! `task` is an honest stub, not a re-implementation: real task dispatch
//! for the qjs backend already goes through `agenterm::script_backend`'s
//! `try_execute_qjs_invocation` from the root `agenterm` binary/worker
//! (`src/script_worker.rs`), the same place lua's task dispatch lives —
//! `agenterm-lua`'s own `task` subcommand is the same kind of stub, not a
//! coincidence.

use std::{env, fs, path::PathBuf, process::ExitCode};

use agenterm_qjs::{
    QJS_VERSION, QjsError, QjsHostFunctions, check, eval_entry, parse_check_many_cli,
    read_manifest, run_check_many,
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
            let rendered = render_value(outcome.value.as_ref());
            if !outcome.stdout.is_empty() {
                print!("{}", outcome.stdout);
            }
            println!("qjs eval ok: {label} -> {rendered}");
        }
        "hash" => {
            let path = require_path(&mut args, "hash")?;
            let source = read_source(&path)?;
            println!("{}  {}", agenterm_qjs::hash_source(&source), path.display());
        }
        "run" => {
            return run_run_command(&mut args);
        }
        "pack" => {
            return run_pack_command(&mut args);
        }
        "qualify" => {
            return run_qualify_command(&mut args);
        }
        "check-many" => {
            return run_check_many_command(&mut args);
        }
        "task" => {
            let rest = args.collect::<Vec<_>>().join(" ");
            eprintln!(
                "task: use `agenterm task {rest}` (root binary; set \
                 AGENTERM_SCRIPT_BACKEND=qjs to route it through this engine) — \
                 agenterm-qjs itself doesn't re-implement task dispatch, see \
                 plan/plan-v0.1.16.md \u{a7}1 Rh, QJS-M3"
            );
            return Ok(0);
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(QjsError::Check(format!(
                "unknown command `{other}`; try check | eval | run | hash | pack | qualify | \
                 check-many | task | version"
            )));
        }
    }

    Ok(0)
}

/// `run <file.js> [-- <args>...]` — evaluate with CLI arguments wired to
/// `__host.args_len`/`__host.arg`, mirroring `agenterm-lua`'s `cmd_run`.
fn run_run_command(args: &mut impl Iterator<Item = String>) -> Result<u8, QjsError> {
    let collected = args.collect::<Vec<_>>();
    let sep = collected.iter().position(|a| a == "--");
    let (file_args, script_args): (&[String], &[String]) = match sep {
        Some(pos) => (&collected[..pos], &collected[pos + 1..]),
        None => (&collected[..], &[]),
    };
    let path = file_args
        .first()
        .map(PathBuf::from)
        .ok_or_else(|| QjsError::Check("usage: agenterm-qjs run <file.js> [-- <args>...]".into()))?;
    let source = read_source(&path)?;
    let label = path.display().to_string();

    let mut host = QjsHostFunctions::default();
    let script_args = script_args.to_vec();
    let args_for_len = script_args.clone();
    host.args_len = Some(std::sync::Arc::new(move || args_for_len.len() as i64));
    let args_for_arg = script_args.clone();
    host.arg = Some(std::sync::Arc::new(move |index: i64| {
        usize::try_from(index)
            .ok()
            .and_then(|i| args_for_arg.get(i))
            .cloned()
            .ok_or_else(|| format!("argument {index} is unavailable"))
    }));

    let outcome = agenterm_qjs::eval_entry_with_host(&source, &label, &host)?;
    if !outcome.stdout.is_empty() {
        print!("{}", outcome.stdout);
    }
    println!(
        "qjs run ok: {label} -> {}",
        render_value(outcome.value.as_ref())
    );
    Ok(0)
}

/// `pack build <file.js> --dir <out>` / `pack load <dir>`.
fn run_pack_command(args: &mut impl Iterator<Item = String>) -> Result<u8, QjsError> {
    let Some(subcommand) = args.next() else {
        return Err(QjsError::Check(
            "usage: agenterm-qjs pack build <file.js> --dir <out> | pack load <dir>".into(),
        ));
    };
    match subcommand.as_str() {
        "build" => {
            let path = require_path(args, "pack build")?;
            let dir = require_flag_value(args, "--dir", "pack build requires --dir <out>")?;
            let source = read_source(&path)?;
            agenterm_qjs::build_pack_dir(&source, &dir).map_err(QjsError::Check)?;
            println!("pack build ok: {}", path.display());
            println!("  output: {}", dir.display());
            Ok(0)
        }
        "load" => {
            let dir = args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| QjsError::Check("usage: agenterm-qjs pack load <dir>".into()))?;
            let pack = agenterm_qjs::QjsPack::load(&dir).map_err(QjsError::Check)?;
            let host = QjsHostFunctions::default();
            let result = pack.eval(&host)?;
            if !result.stdout.is_empty() {
                print!("{}", result.stdout);
            }
            println!(
                "pack load ok: {} -> {}",
                dir.display(),
                render_value(result.value.as_ref())
            );
            Ok(0)
        }
        other => Err(QjsError::Check(format!(
            "pack: unknown subcommand `{other}`; try build or load"
        ))),
    }
}

/// `qualify <file.js> --dir <out>` — build + load + entry → receipt.
fn run_qualify_command(args: &mut impl Iterator<Item = String>) -> Result<u8, QjsError> {
    let path = require_path(args, "qualify")?;
    let dir = require_flag_value(args, "--dir", "qualify requires --dir <out>")?;
    let source = read_source(&path)?;
    let host = QjsHostFunctions::default();
    let receipt =
        agenterm_qjs::qualify_pack_dir(&source, &dir, &host).map_err(QjsError::Check)?;
    let receipt_path = dir.join("receipt.json");
    receipt.write(&receipt_path).map_err(QjsError::Check)?;
    println!(
        "qualify ok: {} -> {}",
        path.display(),
        render_value(receipt.entry_value.as_ref())
    );
    println!("  receipt: {}", receipt_path.display());
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

fn render_value(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "undefined".to_owned(),
    }
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

fn require_flag_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    usage: &str,
) -> Result<PathBuf, QjsError> {
    let collected = args.collect::<Vec<_>>();
    let pos = collected
        .iter()
        .position(|a| a == flag)
        .ok_or_else(|| QjsError::Check(usage.to_owned()))?;
    collected
        .get(pos + 1)
        .map(PathBuf::from)
        .ok_or_else(|| QjsError::Check(format!("{flag} requires a value")))
}

fn print_usage() {
    println!(
        "agenterm-qjs {QJS_VERSION} — QuickJS script engine, capability-aligned with agenterm-rh\n\n\
Usage:\n  \
agenterm-qjs check <file.js>\n  \
agenterm-qjs eval <file.js>\n  \
agenterm-qjs run <file.js> [-- <args>...]\n  \
agenterm-qjs hash <file.js>\n  \
agenterm-qjs pack build <file.js> --dir <out>\n  \
agenterm-qjs pack load <dir>\n  \
agenterm-qjs qualify <file.js> --dir <out>\n  \
agenterm-qjs check-many --manifest <file.json> [--project-root DIR] [--timeout-ms N] [--json]\n  \
agenterm-qjs task ...  (stub — see plan/plan-v0.1.16.md \u{a7}1 Rh, QJS-M3)\n  \
agenterm-qjs version\n\n\
Not yet implemented: project-level import-graph validation in `check` (rh has\n\
one, see check.rs); see plan/plan-v0.1.16.md \u{a7}1 Rh, QJS-M3."
    );
}
