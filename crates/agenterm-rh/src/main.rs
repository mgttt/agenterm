use std::{env, fs, path::PathBuf, process::ExitCode};

use agenterm_rh::{
    CallerInventoryOptions, CorpusScanOptions, RH_VERSION, RhError, build_pack_dir, check,
    check_with_project_validation, compile_native, hash_file, load_and_call_entry,
    parse_check_many_cli, qualify_pack_dir, read_manifest, run_check_many, scan_caller_inventory,
    scan_rhai_directory, transpile, write_receipt,
};

fn main() -> ExitCode {
    let os_arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if agenterm::incremental_wrapper::is_incremental_rustc_wrapper_process(&os_arguments) {
        agenterm::incremental_wrapper::run_incremental_rustc_wrapper(os_arguments);
    }
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [mode, rest @ ..] if mode == "check-many" => {
            return public_command_exit_code(run_check_many_command(&mut rest.iter().cloned()));
        }
        [mode, rest @ ..] if mode == "check" => {
            return public_command_exit_code(run_public_check_command(rest));
        }
        [mode, rest @ ..] if mode == "--internal-incremental-finalize" => {
            return worker_exit_code(
                agenterm::incremental_wrapper::finalize_incremental_manifest(rest),
            );
        }
        [mode] if mode == "--worker" => {
            return worker_exit_code(agenterm::run_legacy_worker_stdio());
        }
        [mode] if mode == "--framed-worker" => {
            return worker_exit_code(agenterm::run_framed_worker_stdio());
        }
        [command, ..] if command == "task" => {
            return script_exit_code(agenterm::run_script_entry_with_args(arguments));
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
            check(&source)?;
            let scratch = tempfile::tempdir().map_err(|err| RhError::Compile(err.to_string()))?;
            let receipt = qualify_pack_dir(&source, scratch.path())?;
            let native = scratch
                .path()
                .join(format!("pack.{}", agenterm_rh::compile::native_extension()));
            let value = load_and_call_entry(&native)?;
            println!(
                "rh eval ok: {} -> {}\n  source_hash={}\n  native_hash={}\n  cc_lines={}",
                path.display(),
                value,
                receipt.source_hash,
                receipt.native_hash,
                receipt.cc_line_count
            );
        }
        "run-smoke" => {
            let path = require_path(&mut args, "run-smoke")?;
            let value = load_and_call_entry(&path)?;
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
            let dir = pack_dir_flag(&mut subargs)?;
            let output =
                parse_output_flag(&mut subargs)?.unwrap_or_else(|| dir.join("qualification.json"));
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
                    "usage: agenterm-rh pack build <file.rh> --dir PATH".into(),
                ));
            };
            if subcommand != "build" {
                return Err(RhError::Parse(format!(
                    "unknown pack subcommand `{subcommand}`"
                )));
            }
            let path = require_path(&mut subargs, "pack build")?;
            let dir = pack_dir_flag(&mut subargs)?;
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
                "unknown command `{other}`; try check | check-many | corpus-scan | caller-inventory | transpile | compile | eval | run-smoke | pack | qualify | hash | version | task"
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

fn run_public_check_command(arguments: &[String]) -> Result<u8, RhError> {
    let Some(path) = arguments.first() else {
        return Err(RhError::Parse("usage: agenterm-rh check <file>".into()));
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
    }
}

fn run_corpus_scan_command(args: &mut impl Iterator<Item = String>) -> Result<(), RhError> {
    let mut project_root = PathBuf::from(".");
    let mut relative_dir = "scripts/rhai".to_owned();
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
    let report = scan_rhai_directory(CorpusScanOptions {
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
        .ok_or_else(|| RhError::Parse(format!("usage: agenterm-rh {command} <file> [-o out]")))
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
    input: &PathBuf,
    extension: &str,
) -> Result<PathBuf, RhError> {
    Ok(parse_output_flag(args)?.unwrap_or_else(|| input.with_extension(extension)))
}

fn native_output_path(
    args: &mut impl Iterator<Item = String>,
    input: &PathBuf,
) -> Result<PathBuf, RhError> {
    Ok(parse_output_flag(args)?
        .unwrap_or_else(|| input.with_extension(agenterm_rh::compile::native_extension())))
}

fn pack_dir_flag(args: &mut impl Iterator<Item = String>) -> Result<PathBuf, RhError> {
    let mut dir = None;
    while let Some(arg) = args.next() {
        if arg == "--dir" {
            dir = Some(
                args.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| RhError::Parse("missing path after --dir".into()))?,
            );
        } else {
            return Err(RhError::Parse(format!("unexpected argument `{arg}`")));
        }
    }
    dir.ok_or_else(|| RhError::Parse("pack build requires --dir PATH".into()))
}

fn print_usage() {
    eprintln!(
        "agenterm-rh {RH_VERSION}\n\
         \n\
         commands:\n\
           check <file>                      validate rh subset\n\
           check-many --manifest FILE        bounded multi-file rh subset check\n\
           corpus-scan [--root PATH] [--dir REL|--tasks [MANIFEST]]  scan .rhai scripts or task entries\n\
           caller-inventory [--root PATH]            report agenterm-rhai operational references\n\
           transpile <file> [-o rs]            emit Rust source for AOT\n\
           compile <file> [-o native]          transpile + cargo -> native + manifest\n\
           eval <file>                         check + AOT pack + dlopen entry (dev loop)\n\
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
