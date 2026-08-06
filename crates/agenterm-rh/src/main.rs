use std::{env, fs, path::PathBuf, process::ExitCode};

use agenterm_rh::{check, compile_native, transpile, RhError, RH_VERSION};

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
            let source = fs::read_to_string(&path).map_err(|err| RhError::Parse(err.to_string()))?;
            check(&source)?;
            println!("rh check ok: {}", path.display());
        }
        "transpile" => {
            let path = require_path(&mut args, "transpile")?;
            let output = output_path(&mut args, &path)?;
            let source = fs::read_to_string(&path).map_err(|err| RhError::Parse(err.to_string()))?;
            let rust = transpile(&source)?;
            fs::write(&output, rust).map_err(|err| RhError::Transpile(err.to_string()))?;
            println!("rh transpile ok: {} -> {}", path.display(), output.display());
        }
        "compile" => {
            let path = require_path(&mut args, "compile")?;
            let output = output_path(&mut args, &path)?;
            let source = fs::read_to_string(&path).map_err(|err| RhError::Parse(err.to_string()))?;
            compile_native(&source, &output)?;
            println!("rh compile ok: {} -> {}", path.display(), output.display());
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(RhError::Parse(format!(
                "unknown command `{other}`; try check | transpile | compile | version"
            )));
        }
    }

    Ok(())
}

fn require_path(args: &mut impl Iterator<Item = String>, command: &str) -> Result<PathBuf, RhError> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| RhError::Parse(format!("usage: agenterm-rh {command} <file.rh> [-o out]")))
}

fn output_path(args: &mut impl Iterator<Item = String>, input: &PathBuf) -> Result<PathBuf, RhError> {
    let mut output = None;
    while let Some(arg) = args.next() {
        if arg == "-o" {
            output = args.next().map(PathBuf::from);
        } else {
            return Err(RhError::Parse(format!("unexpected argument `{arg}`")));
        }
    }
    Ok(output.unwrap_or_else(|| {
        input.with_extension(match input.extension().and_then(|s| s.to_str()) {
            Some("rh") => "rs".to_string(),
            _ => "rh.rs".to_string(),
        })
    }))
}

fn print_usage() {
    eprintln!(
        "agenterm-rh {RH_VERSION}\n\
         \n\
         commands:\n\
           check <file>              validate rh-0 subset\n\
           transpile <file> [-o rs]  emit Rust source for AOT\n\
           compile <file> [-o so]    native link (stub in rh-0)\n\
           version\n"
    );
}
