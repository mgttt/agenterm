use std::{env, fs, path::PathBuf, process::ExitCode};

use agenterm_rh::{
    RH_VERSION, RhError, build_pack_dir, check, compile_native, hash_file, load_and_call_entry,
    qualify_pack_dir, read_manifest, run_check_many, scan_rhai_directory, transpile, write_receipt,
    CheckManyOptions, CorpusScanOptions,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), RhError> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        return Ok(());
    };

    match command.as_str() {
        "version" => {
            println!("agenterm-rh {RH_VERSION}");
        }
        "check" => {
            let path = require_path(&mut args, "check")?;
            let source = read_source(&path)?;
            check(&source)?;
            println!("rh check ok: {}", path.display());
        }
        "check-many" => {
            run_check_many_command(&mut args)?;
        }
        "corpus-scan" => {
            run_corpus_scan_command(&mut args)?;
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
            let scratch =
                tempfile::tempdir().map_err(|err| RhError::Compile(err.to_string()))?;
            let receipt = qualify_pack_dir(&source, scratch.path())?;
            let native = scratch.path().join(format!(
                "pack.{}",
                agenterm_rh::compile::native_extension()
            ));
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
                "unknown command `{other}`; try check | check-many | corpus-scan | transpile | compile | eval | run-smoke | pack | qualify | hash | version"
            )));
        }
    }

    Ok(())
}

fn run_check_many_command(args: &mut impl Iterator<Item = String>) -> Result<(), RhError> {
    let mut manifest_path = None::<PathBuf>;
    let mut project_root = PathBuf::from(".");
    let mut wall_time_ms = agenterm_rh::check_many::DEFAULT_WALL_TIME_MS;
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                manifest_path = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| RhError::Parse("missing path after --manifest".into()))?,
                ));
            }
            "--project-root" => {
                project_root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| RhError::Parse("missing path after --project-root".into()))?,
                );
            }
            "--timeout-ms" => {
                wall_time_ms = args
                    .next()
                    .ok_or_else(|| RhError::Parse("missing value after --timeout-ms".into()))?
                    .parse()
                    .map_err(|err| RhError::Parse(format!("timeout-ms: {err}")))?;
            }
            "--json" => json = true,
            other => return Err(RhError::Parse(format!("unknown check-many option `{other}`"))),
        }
    }
    let manifest_path =
        manifest_path.ok_or_else(|| RhError::Parse("check-many requires --manifest FILE".into()))?;
    let manifest = read_manifest(&manifest_path)?;
    let report = run_check_many(
        manifest,
        CheckManyOptions {
            project_root,
            wall_time_ms,
            ..CheckManyOptions::default()
        },
    );
    if json {
        let encoded = serde_json::to_string_pretty(&report)
            .map_err(|err| RhError::Parse(err.to_string()))?;
        println!("{encoded}");
    } else if report.ok {
        println!("OK ({} files)", report.checked_files);
    } else {
        for failure in &report.failures {
            eprintln!("{}: {} ({})", failure.path, failure.message, failure.code);
        }
    }
    if report.ok {
        Ok(())
    } else {
        Err(RhError::Parse("check-many reported failures".into()))
    }
}

fn run_corpus_scan_command(args: &mut impl Iterator<Item = String>) -> Result<(), RhError> {
    let mut project_root = PathBuf::from(".");
    let mut relative_dir = "scripts/rhai".to_owned();
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
            "--json" => json = true,
            other => return Err(RhError::Parse(format!("unknown corpus-scan option `{other}`"))),
        }
    }
    let report = scan_rhai_directory(CorpusScanOptions {
        project_root,
        relative_dir,
    })?;
    if json {
        let encoded = serde_json::to_string_pretty(&report)
            .map_err(|err| RhError::Parse(err.to_string()))?;
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
           corpus-scan [--root PATH] [--dir REL]  scan .rhai scripts for rh subset fit\n\
           transpile <file> [-o rs]            emit Rust source for AOT\n\
           compile <file> [-o native]          transpile + cargo -> native + manifest\n\
           eval <file>                         check + AOT pack + dlopen entry (dev loop)\n\
           run-smoke <native>                  dlopen and call rh_entry()\n\
           pack build <file> --dir PATH        build pack dir (native + manifest + entry.rh)\n\
           qualify <file> --dir PATH [-o json] build + load + write qualification receipt\n\
           hash <file>                         sha256 receipt\n\
           version\n"
    );
}
