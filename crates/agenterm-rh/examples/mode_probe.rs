//! Ad-hoc Native/pack probe for project `.rh` entries.
//!
//! ```text
//! cargo run -p agenterm-rh --example mode_probe -- \
//!   --root /workspace scripts/rh/qualification-selftest.rh --pack
//! ```

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut entry: Option<PathBuf> = None;
    let mut pack = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = PathBuf::from(args.next().expect("--root PATH"));
            }
            "--pack" => pack = true,
            other if other.starts_with('-') => {
                eprintln!("unknown flag {other}");
                return ExitCode::from(2);
            }
            other => {
                if entry.is_some() {
                    eprintln!("unexpected argument {other}");
                    return ExitCode::from(2);
                }
                entry = Some(PathBuf::from(other));
            }
        }
    }
    let Some(entry) = entry else {
        eprintln!("usage: mode_probe [--root DIR] [--pack] <entry.rh>");
        return ExitCode::from(2);
    };
    let path = if entry.is_absolute() {
        entry
    } else {
        root.join(&entry)
    };
    let source = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read {}: {err}", path.display());
            return ExitCode::from(2);
        }
    };
    let output = match agenterm_rh::transpile_cdylib_with_project(&root, &source) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("transpile fail: {err}");
            return ExitCode::from(1);
        }
    };
    // Count call sites only (`fn rh_host_eval_int(snippet` is not a call).
    let he = output.rust.matches("rh_host_eval_int(\"").count();
    println!("entry={}", path.display());
    println!("mode={}", output.execution_mode.as_str());
    println!("host_eval_int={he}");
    println!(
        "compat_delegating={}",
        output.rust.contains("compat delegating")
    );
    if env::var_os("RH_DUMP_HOST_EVAL").is_some() {
        for (i, part) in output.rust.split("rh_host_eval_int(\"").enumerate() {
            if i == 0 {
                continue;
            }
            let snip = part.split('"').next().unwrap_or("");
            println!("host_eval[{i}]=\"{snip}\"");
        }
    }
    if pack {
        let bundled = match agenterm_rh::bundle_project_source(&root, &source) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("bundle fail: {err}");
                return ExitCode::from(1);
            }
        };
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(err) => {
                eprintln!("tempdir fail: {err}");
                return ExitCode::from(2);
            }
        };
        match agenterm_rh::build_pack_dir(&bundled, dir.path()) {
            Ok(built) => println!("pack=ok native={}", built.native_path.display()),
            Err(err) => {
                eprintln!("pack=fail {err}");
                return ExitCode::from(1);
            }
        }
    }
    if output.execution_mode.as_str() != "native" || he > 0 {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
