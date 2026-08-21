//! Root-lib entry point for the `agenterm-rh` CLI.
//!
//! Moved verbatim from `crates/agenterm-rh/src/main.rs` for SUB-M2 of
//! `plan/archive/design-script-engine-subcommands.md` (see §2/§5 there). Zero
//! behavior change — this is a location + visibility move, not a rewrite.
//!
//! `crates/agenterm-rh/src/main.rs` is a root-package `[[bin]]` whose CLI
//! logic reaches into `agenterm::` internals (the incremental-rustc-wrapper
//! probe, the legacy/framed worker stdio loops, and Script Runtime task
//! dispatch) that live in *this* crate, not in `agenterm_rh`. Unlike the
//! lua/qjs/sql engines (SUB-M1, whose CLI entry points moved into their own
//! crate's `cli` module because their `main.rs` only touched their own
//! engine crate), rh's entry point has to live in the root lib: putting it
//! in `agenterm_rh::cli` would make the engine crate depend on `agenterm`,
//! reversing the intended dependency direction (design doc §2, "抽到根 lib
//! 新模块 `agenterm::script_rh_cli_main::run(args)` — 不能进 agenterm-rh
//! crate——依赖方向反了").
//!
//! `crates/agenterm-rh/src/main.rs` is now a thin shell that collects argv
//! and forwards to [`run_main`].

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use agenterm_rh::{
    CallerInventoryOptions, CorpusScanOptions, RH_VERSION, RhError, build_pack_dir, check,
    check_with_project_validation, compile_native, hash_file, parse_check_many_cli,
    qualify_pack_dir, read_manifest, run_check_many, scan_caller_inventory, scan_rh_directory,
    static_evidence_declarations, transpile, write_receipt,
};

/// Entry point for the `agenterm-rh` CLI, callable from the thin
/// `crates/agenterm-rh/src/main.rs` shell (and, later, from the main
/// `agenterm` binary's `rh` subcommand — SUB-M3).
///
/// `os_args` must be the process argv with argv\[0\] already stripped, i.e.
/// exactly what `std::env::args_os().skip(1)` yields — callers must collect
/// argv the same way the original standalone `main()` did.
///
/// ## Never-returns note (incremental-rustc-wrapper path)
///
/// If `os_args` looks like an incremental-rustc-wrapper invocation, this
/// function does not return: it calls
/// [`agenterm::incremental_wrapper::run_incremental_rustc_wrapper`], whose
/// signature is `fn(Vec<OsString>) -> !` (it always terminates the process
/// itself, matching the previous behavior where `main()` also never
/// returned past that call site). This mirrors the original `main()`
/// exactly — the check happened first, before any other argv handling.
///
/// ## OsString/String fidelity
///
/// The original `main()` collected argv *twice* from the live process:
/// once as `env::args_os().skip(1)` (used only for the incremental-wrapper
/// probe, which must not panic on non-UTF-8 argv — using the `_os` variant
/// there is the whole point), and once as `env::args().skip(1)` (used for
/// every other code path). `env::args()` panics if *any* argument is not
/// valid UTF-8 — even ones after the ones actually used. Because both
/// collections read the same argv, they were always byte-identical as
/// `OsString`s; the only behavior that depended on which collector ran was
/// *when* a non-UTF-8 argument would panic (immediately for `env::args()`,
/// vs. after the wrapper probe for `env::args_os()`).
///
/// This function receives a single `Vec<OsString>` (the `_os` collection)
/// and must reproduce the same effect ordering without re-reading process
/// argv a second time. We do that by converting to `String` only *after*
/// the incremental-wrapper check, element-by-element, and panicking on the
/// first invalid one (`OsString::into_string` returns the offending
/// `OsString` back on failure, unlike `env::args()`'s internal panic
/// message, but the *panic itself* is preserved). We deliberately do NOT
/// lossy-convert (`to_string_lossy`) — that would silently accept argv the
/// original binary would have rejected by panicking, which is a behavior
/// change.
pub fn run_main(os_args: Vec<OsString>) -> ExitCode {
    if crate::incremental_wrapper::is_incremental_rustc_wrapper_process(&os_args) {
        crate::incremental_wrapper::run_incremental_rustc_wrapper(os_args);
    }
    let arguments: Vec<String> = os_args
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .unwrap_or_else(|invalid| panic!("invalid utf-8 sequence in argument: {invalid:?}"))
        })
        .collect();
    match arguments.as_slice() {
        [mode, rest @ ..] if mode == "check-many" => {
            return public_command_exit_code(run_check_many_command(&mut rest.iter().cloned()));
        }
        [mode, rest @ ..] if mode == "check" => {
            return public_command_exit_code(run_public_check_command(rest));
        }
        [mode, rest @ ..] if mode == "--internal-incremental-finalize" => {
            return worker_exit_code(crate::incremental_wrapper::finalize_incremental_manifest(
                rest,
            ));
        }
        [mode] if mode == "--worker" => {
            return worker_exit_code(crate::run_legacy_worker_stdio());
        }
        [mode] if mode == "--framed-worker" => {
            return worker_exit_code(crate::run_framed_worker_stdio());
        }
        [command, ..] if command == "task" => {
            return script_exit_code(crate::run_script_entry_with_args(arguments));
        }
        _ => {}
    }
    match run(arguments.into_iter()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn worker_exit_code(result: anyhow::Result<u8>) -> ExitCode {
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

fn script_exit_code(code: i32) -> ExitCode {
    u8::try_from(code)
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}

fn public_command_exit_code(result: Result<u8, RhError>) -> ExitCode {
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<(), RhError> {
    let Some(command) = args.next() else {
        print_usage();
        return Ok(());
    };

    match command.as_str() {
        "version" | "--version" | "-V" => {
            println!("agenterm-rh {RH_VERSION}");
        }
        "check" => {
            let path = require_path(&mut args, "check")?;
            let source = read_source(&path)?;
            check(&source)?;
            println!("rh check ok: {}", path.display());
        }
        "evidence-list" => {
            let path = require_path(&mut args, "evidence-list")?;
            if let Some(argument) = args.next() {
                return Err(RhError::Parse(format!(
                    "unexpected evidence-list argument `{argument}`"
                )));
            }
            let source = read_source(&path)?;
            for evidence in static_evidence_declarations(&source)? {
                println!("{evidence}");
            }
        }
        "corpus-scan" => {
            run_corpus_scan_command(&mut args)?;
        }
        "caller-inventory" => {
            run_caller_inventory_command(&mut args)?;
        }
        "transpile" => {
            let path = require_path(&mut args, "transpile")?;
            let output = text_output_path(&mut args, &path, "rs")?;
            let source = read_source(&path)?;
            let rust = transpile(&source)?;
            fs::write(&output, rust).map_err(|err| RhError::Transpile(err.to_string()))?;
            println!(
                "rh transpile ok: {} -> {}",
                path.display(),
                output.display()
            );
        }
        "compile" => {
            let path = require_path(&mut args, "compile")?;
            let output = native_output_path(&mut args, &path)?;
            let source = read_source(&path)?;
            let result = compile_native(&source, &output)?;
            println!(
                "rh compile ok: {} -> {}\n  source_hash={}\n  native_hash={}\n  manifest={}",
                path.display(),
                result.native_path.display(),
                result.source_hash,
                result.native_hash,
                result.manifest_path.display()
            );
        }
        "eval" => {
            let path = require_path(&mut args, "eval")?;
            let source = read_source(&path)?;
            // Evaluated by the interpreter. This used to build a native pack
            // first, which meant `eval` could not run without a Rust toolchain
            // and paid a compile before printing anything. The AOT pipeline is
            // still here for `compile` / `pack` / `qualify` / task gates; it is
            // just no longer on the path of "run this script once".
            let value = eval_with_interpreter(&source, Vec::new())?;
            println!(
                "rh eval ok: {} -> {}\n  source_hash={}",
                path.display(),
                value_display(&value),
                agenterm_rh::hash_bytes(source.as_bytes())
            );
        }
        "run" => {
            run_rh_script_command(&mut args)?;
        }
        "run-smoke" => {
            let path = require_path(&mut args, "run-smoke")?;
            let (value, output) = crate::script_rh_host::call_pack_entry_capturing_output(&path)?;
            print!("{output}");
            println!(
                "rh run-smoke ok: {} -> rh_entry() = {value}",
                path.display()
            );
        }
        "hash" => {
            let path = require_path(&mut args, "hash")?;
            let digest = hash_file(&path)?;
            println!("{digest}  {}", path.display());
        }
        "qualify" => {
            let path = require_path(&mut args, "qualify")?;
            let mut subargs = args;
            let (dir, rest) = pack_dir_flag(&mut subargs, "qualify")?;
            let output = parse_output_flag(&mut rest.into_iter())?
                .unwrap_or_else(|| dir.join("qualification.json"));
            let source = read_source(&path)?;
            let receipt = qualify_pack_dir(&source, &dir)?;
            write_receipt(&output, &receipt)?;
            println!(
                "rh qualify ok: {} -> {}\n  target={}\n  native_hash={}\n  entry={}",
                path.display(),
                output.display(),
                receipt.target,
                receipt.native_hash,
                receipt.entry_value
            );
        }
        "pack" => {
            let mut subargs = args;
            let Some(subcommand) = subargs.next() else {
                return Err(RhError::Parse(
                    "usage: agenterm rh pack build <file.rh> --dir PATH".into(),
                ));
            };
            if subcommand != "build" {
                return Err(RhError::Parse(format!(
                    "unknown pack subcommand `{subcommand}`"
                )));
            }
            let path = require_path(&mut subargs, "pack build")?;
            let (dir, rest) = pack_dir_flag(&mut subargs, "pack build")?;
            reject_leftover_args(rest)?;
            let source = read_source(&path)?;
            let output = build_pack_dir(&source, &dir)?;
            println!(
                "rh pack build ok: {} -> {}\n  native={}\n  manifest={}\n  native_hash={}",
                path.display(),
                dir.display(),
                output.native_path.display(),
                output.manifest_path.display(),
                output.compile.native_hash
            );
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(RhError::Parse(format!(
                "unknown command `{other}`; try check | check-many | corpus-scan | caller-inventory | transpile | compile | eval | run | run-smoke | pack | qualify | hash | version | task"
            )));
        }
    }

    Ok(())
}

fn run_check_many_command(args: &mut impl Iterator<Item = String>) -> Result<u8, RhError> {
    let parsed = parse_check_many_cli(args)?;
    let manifest = read_manifest(&parsed.manifest_path)?;
    let report = run_check_many(manifest, parsed.options);
    if parsed.json {
        let encoded =
            serde_json::to_string_pretty(&report).map_err(|err| RhError::Parse(err.to_string()))?;
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

fn run_rh_script_command(args: &mut impl Iterator<Item = String>) -> Result<(), RhError> {
    let (path, project_root, script_args, budgets) = parse_run_cli(args)?;
    let source = read_source(&path)?;
    // `run` executes through the interpreter. Project imports are still
    // resolved the workbench way first, so a script that `require`s a sibling
    // module keeps working.
    let bundled = agenterm_rh::bundle_project_source(&project_root, &source)
        .unwrap_or_else(|_| source.clone());
    let value = eval_with_budgets(&bundled, script_args, budgets)?;
    if value != agenterm_rh::Value::Unit {
        println!("rh run ok: {} -> {}", path.display(), value_display(&value));
    }
    Ok(())
}

/// Run Language-1 source on the interpreter with the workbench host attached.
///
/// `print` goes straight to stdout here: this is the interactive CLI, not the
/// worker protocol, so there is no JSON stream to corrupt.
fn eval_with_interpreter(
    source: &str,
    script_args: Vec<String>,
) -> Result<agenterm_rh::Value, RhError> {
    eval_with_budgets(source, script_args, None)
}

/// As above, honouring the `run` budget flags.
///
/// `--max-operations` becomes engine fuel and `--timeout-ms` becomes its
/// wall-clock deadline, so the flags keep meaning what they meant when a
/// worker enforced them.
fn eval_with_budgets(
    source: &str,
    script_args: Vec<String>,
    budgets: Option<crate::script_protocol::ScriptBudgets>,
) -> Result<agenterm_rh::Value, RhError> {
    let budgets = budgets.unwrap_or_default();
    let host = crate::script_rh_host::AgentermRhHost::new(script_args);
    let mut engine = agenterm_rh::Engine::new_with_host(host).with_options(agenterm_rh::Options {
        fuel: Some(budgets.operations),
        wall_time: Some(std::time::Duration::from_millis(budgets.wall_time_ms)),
        output_bytes: budgets.output_bytes,
        call_depth: budgets.call_depth,
        runtime_expression_depth: budgets.expression_depth,
        ..agenterm_rh::Options::embedder()
    });
    engine
        .eval(source)
        .map_err(|error| rh_error_from_lang(&error))
}

/// Language errors carry more shape than the pipeline's `RhError`; keep the
/// subset code, which scripts and gates match on.
fn rh_error_from_lang(error: &agenterm_rh::Error) -> RhError {
    match error {
        agenterm_rh::Error::Parse(message) => RhError::Parse(message.clone()),
        agenterm_rh::Error::Subset { code, detail } => RhError::Subset {
            code,
            detail: detail.clone(),
        },
        // `RhError::Runtime` already prints the `rh runtime:` prefix, so pass
        // the detail alone rather than the language error's own rendering.
        agenterm_rh::Error::Runtime(message) => RhError::Runtime(message.clone()),
        agenterm_rh::Error::Host(message) => RhError::Runtime(format!("host: {message}")),
        agenterm_rh::Error::Io(message) => RhError::Runtime(format!("io: {message}")),
        agenterm_rh::Error::Unsupported { feature } => {
            RhError::Runtime(format!("{feature} is unsupported"))
        }
        agenterm_rh::Error::OutOfFuel => {
            RhError::Runtime("out of fuel (raise --max-operations)".to_owned())
        }
        agenterm_rh::Error::Timeout => {
            RhError::Runtime("timed out (raise --timeout-ms)".to_owned())
        }
        agenterm_rh::Error::Cancelled => RhError::Runtime("cancelled".to_owned()),
        other => RhError::Runtime(other.to_string()),
    }
}

fn value_display(value: &agenterm_rh::Value) -> String {
    match value {
        agenterm_rh::Value::Unit => "()".to_owned(),
        agenterm_rh::Value::Bool(inner) => inner.to_string(),
        agenterm_rh::Value::Int(inner) => inner.to_string(),
        agenterm_rh::Value::String(inner) => inner.clone(),
        other => format!("{other:?}"),
    }
}

fn parse_run_cli(
    args: &mut impl Iterator<Item = String>,
) -> Result<
    (
        PathBuf,
        PathBuf,
        Vec<String>,
        Option<crate::script_protocol::ScriptBudgets>,
    ),
    RhError,
> {
    let path = require_path(args, "run")?;
    let mut project_root = PathBuf::from(".");
    let mut script_args = Vec::new();
    let mut budgets = crate::script_protocol::ScriptBudgets::default();
    let mut budgets_touched = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project-root" => {
                project_root =
                    PathBuf::from(args.next().ok_or_else(|| {
                        RhError::Parse("missing path after --project-root".into())
                    })?);
            }
            // Legacy inert compatibility flag from agenterm-rh task/run callers.
            "--profile" => {
                let _ = args
                    .next()
                    .ok_or_else(|| RhError::Parse("missing value after --profile".into()))?;
            }
            "--timeout-ms" => {
                let raw = args
                    .next()
                    .ok_or_else(|| RhError::Parse("missing value after --timeout-ms".into()))?;
                budgets.wall_time_ms = raw
                    .parse::<u64>()
                    .map_err(|_| RhError::Parse(format!("invalid --timeout-ms value `{raw}`")))?;
                budgets_touched = true;
            }
            "--max-operations" => {
                let raw = args
                    .next()
                    .ok_or_else(|| RhError::Parse("missing value after --max-operations".into()))?;
                budgets.operations = raw.parse::<u64>().map_err(|_| {
                    RhError::Parse(format!("invalid --max-operations value `{raw}`"))
                })?;
                budgets_touched = true;
            }
            "--max-output-bytes" => {
                let raw = args.next().ok_or_else(|| {
                    RhError::Parse("missing value after --max-output-bytes".into())
                })?;
                budgets.output_bytes = raw.parse::<usize>().map_err(|_| {
                    RhError::Parse(format!("invalid --max-output-bytes value `{raw}`"))
                })?;
                budgets_touched = true;
            }
            "--" => {
                script_args.extend(args.by_ref());
                break;
            }
            other => {
                return Err(RhError::Parse(format!(
                    "unknown run option `{other}`; expected --project-root, --profile, \
                     --timeout-ms, --max-operations, --max-output-bytes, or --"
                )));
            }
        }
    }
    Ok((
        path,
        project_root,
        script_args,
        budgets_touched.then_some(budgets),
    ))
}

fn run_public_check_command(arguments: &[String]) -> Result<u8, RhError> {
    let Some(path) = arguments.first() else {
        return Err(RhError::Parse("usage: agenterm rh check <file>".into()));
    };
    let mut project_root = PathBuf::from(".");
    let mut json = false;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--project-root" => {
                project_root =
                    PathBuf::from(arguments.get(index + 1).ok_or_else(|| {
                        RhError::Parse("missing path after --project-root".into())
                    })?);
                index += 2;
            }
            "--json" => {
                json = true;
                index += 1;
            }
            other => {
                return Err(RhError::Parse(format!("unknown check option `{other}`")));
            }
        }
    }
    let source = read_source(&PathBuf::from(path))?;
    match check_with_project_validation(&source, Some(&project_root)) {
        Ok(()) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "exit_class": "success",
                        "failure": serde_json::Value::Null,
                    })
                );
            } else {
                println!("rh check ok: {path}");
            }
            Ok(0)
        }
        Err(error) => {
            let (code, message) = public_check_failure(&error);
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "exit_class": "script",
                        "failure": {
                            "code": code,
                            "message": message,
                        },
                    })
                );
            } else {
                eprintln!("{error}");
            }
            Ok(1)
        }
    }
}

fn public_check_failure(error: &RhError) -> (&str, &str) {
    match error {
        RhError::Parse(message) => ("rh_subset", message),
        RhError::Subset { code, detail } => (code, detail),
        RhError::Compile(message) => message
            .split_once(": ")
            .filter(|(code, _)| !code.is_empty())
            .unwrap_or(("rh_check", message)),
        RhError::Transpile(message) => ("rh_transpile", message),
        RhError::Runtime(message) => ("rh_runtime", message),
    }
}

fn run_corpus_scan_command(args: &mut impl Iterator<Item = String>) -> Result<(), RhError> {
    let mut project_root = PathBuf::from(".");
    let mut relative_dir = "scripts/rh".to_owned();
    let mut tasks_manifest = None::<PathBuf>;
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                project_root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| RhError::Parse("missing path after --root".into()))?,
                );
            }
            "--dir" => {
                relative_dir = args
                    .next()
                    .ok_or_else(|| RhError::Parse("missing path after --dir".into()))?;
            }
            "--tasks" => {
                tasks_manifest = Some(match args.next() {
                    Some(path) => PathBuf::from(path),
                    None => project_root.join("agenterm.tasks.json"),
                });
            }
            "--json" => json = true,
            other => {
                return Err(RhError::Parse(format!(
                    "unknown corpus-scan option `{other}`"
                )));
            }
        }
    }
    let report = scan_rh_directory(CorpusScanOptions {
        project_root,
        relative_dir,
        tasks_manifest,
    })?;
    if json {
        let encoded =
            serde_json::to_string_pretty(&report).map_err(|err| RhError::Parse(err.to_string()))?;
        println!("{encoded}");
    } else {
        println!(
            "corpus-scan: {}/{} passed ({} failed)",
            report.passed, report.scanned, report.failed
        );
        for entry in report.entries.iter().filter(|entry| entry.ok) {
            println!("  OK  {}", entry.path);
        }
    }
    Ok(())
}

fn run_caller_inventory_command(args: &mut impl Iterator<Item = String>) -> Result<(), RhError> {
    let mut project_root = PathBuf::from(".");
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                project_root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| RhError::Parse("missing path after --root".into()))?,
                );
            }
            "--json" => json = true,
            other => {
                return Err(RhError::Parse(format!(
                    "unknown caller-inventory option `{other}`"
                )));
            }
        }
    }
    let report = scan_caller_inventory(CallerInventoryOptions { project_root })?;
    if json {
        let encoded =
            serde_json::to_string_pretty(&report).map_err(|err| RhError::Parse(err.to_string()))?;
        println!("{encoded}");
    } else {
        println!(
            "caller-inventory: {} hits in {} scanned files",
            report.hit_count, report.scanned_files
        );
        for (category, count) in &report.categories {
            println!("  {category}: {count}");
        }
    }
    Ok(())
}

fn read_source(path: &PathBuf) -> Result<String, RhError> {
    fs::read_to_string(path).map_err(|err| RhError::Parse(err.to_string()))
}

fn require_path(
    args: &mut impl Iterator<Item = String>,
    command: &str,
) -> Result<PathBuf, RhError> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| RhError::Parse(format!("usage: agenterm rh {command} <file> [-o out]")))
}

fn parse_output_flag(args: &mut impl Iterator<Item = String>) -> Result<Option<PathBuf>, RhError> {
    let mut output = None;
    while let Some(arg) = args.next() {
        if arg == "-o" {
            output = Some(
                args.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| RhError::Parse("missing path after -o".into()))?,
            );
        } else {
            return Err(RhError::Parse(format!("unexpected argument `{arg}`")));
        }
    }
    Ok(output)
}

fn text_output_path(
    args: &mut impl Iterator<Item = String>,
    input: &Path,
    extension: &str,
) -> Result<PathBuf, RhError> {
    Ok(parse_output_flag(args)?.unwrap_or_else(|| input.with_extension(extension)))
}

fn native_output_path(
    args: &mut impl Iterator<Item = String>,
    input: &Path,
) -> Result<PathBuf, RhError> {
    Ok(parse_output_flag(args)?
        .unwrap_or_else(|| input.with_extension(agenterm_rh::compile::native_extension())))
}

/// Consumes `--dir PATH` and returns it together with the arguments that were
/// not part of that flag, so a command accepting further flags (`qualify` also
/// takes `-o`) can keep parsing. A command that accepts nothing else has to
/// reject the leftovers itself -- see `reject_leftover_args`.
fn pack_dir_flag(
    args: &mut impl Iterator<Item = String>,
    command: &str,
) -> Result<(PathBuf, Vec<String>), RhError> {
    let mut dir = None;
    let mut rest = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--dir" {
            dir = Some(
                args.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| RhError::Parse("missing path after --dir".into()))?,
            );
        } else {
            rest.push(arg);
        }
    }
    let dir = dir.ok_or_else(|| RhError::Parse(format!("{command} requires --dir PATH")))?;
    Ok((dir, rest))
}

fn reject_leftover_args(rest: Vec<String>) -> Result<(), RhError> {
    match rest.first() {
        Some(arg) => Err(RhError::Parse(format!("unexpected argument `{arg}`"))),
        None => Ok(()),
    }
}

fn print_usage() {
    eprintln!(
        "agenterm rh {RH_VERSION}\n\
         \n\
         commands:\n\
           check <file>                      validate rh subset\n\
           check-many --manifest FILE        bounded multi-file rh subset check\n\
           evidence-list <file>              list static task evidence declarations\n\
           corpus-scan [--root PATH] [--dir REL|--tasks [MANIFEST]]  scan .rh scripts or task entries\n\
           caller-inventory [--root PATH]            report retired Script CLI references\n\
           transpile <file> [-o rs]            emit Rust source for AOT\n\
           compile <file> [-o native]          transpile + cargo -> native + manifest\n\
           eval <file>                         check + AOT pack + dlopen entry (dev loop)\n\
           run <file> [OPTIONS] [--] [ARGS...]  execute rh entry with args\n\
             options: --project-root DIR --profile NAME --timeout-ms N\n\
                      --max-operations N --max-output-bytes N\n\
           run-smoke <native>                  dlopen and call rh_entry()\n\
           pack build <file> --dir PATH        build pack dir (native + manifest + entry.rh)\n\
           qualify <file> --dir PATH [-o json] build + load + write qualification receipt\n\
           hash <file>                         sha256 receipt\n\
           version\n\
           task list|show|check|run ...        run a Script Runtime task\n\
           --worker                           run the legacy JSON worker protocol\n\
           --framed-worker                    run the framed worker protocol\n"
    );
}
