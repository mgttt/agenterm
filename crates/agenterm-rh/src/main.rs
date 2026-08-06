use std::{env, fs, path::PathBuf, process::ExitCode};

use agenterm_rh::{
    check, compile_native, hash_file, load_and_call_entry, transpile, RhError, RH_VERSION,
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
        "transpile" => {
            let path = require_path(&mut args, "transpile")?;
            let output = text_output_path(&mut args, &path, "rs")?;
            let source = read_source(&path)?;
            let rust = transpile(&source)?;
            fs::write(&output, rust).map_err(|err| RhError::Transpile(err.to_string()))?;
            println!("rh transpile ok: {} -> {}", path.display(), output.display());
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
        "run-smoke" => {
            let path = require_path(&mut args, "run-smoke")?;
            let value = load_and_call_entry(&path)?;
            println!("rh run-smoke ok: {} -> rh_entry() = {value}", path.display());
        }
        "hash" => {
            let path = require_path(&mut args, "hash")?;
            let digest = hash_file(&path)?;
            println!("{digest}  {}", path.display());
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(RhError::Parse(format!(
                "unknown command `{other}`; try check | transpile | compile | run-smoke | hash | version"
            )));
        }
    }

    Ok(())
}

fn read_source(path: &PathBuf) -> Result<String, RhError> {
    fs::read_to_string(path).map_err(|err| RhError::Parse(err.to_string()))
}

fn require_path(args: &mut impl Iterator<Item = String>, command: &str) -> Result<PathBuf, RhError> {
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
    Ok(parse_output_flag(args)?.unwrap_or_else(|| {
        input.with_extension(agenterm_rh::compile::native_extension())
    }))
}

fn print_usage() {
    eprintln!(
        "agenterm-rh {RH_VERSION}\n\
         \n\
         commands:\n\
           check <file>                 validate rh-0 subset\n\
           transpile <file> [-o rs]     emit Rust source for AOT\n\
           compile <file> [-o native]   transpile + cargo -> .so/.dylib/.dll + manifest\n\
           run-smoke <native>           dlopen and call rh_entry()\n\
           hash <file>                  sha256 for source/native receipt\n\
           version\n"
    );
}
