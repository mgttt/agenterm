use agenterm_cc_web::{CONTRACT_VERSION, LauncherReceipt, asset_manifest, direct_host_path};
use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.as_slice() == ["--asset-manifest"] {
        return print_json(&asset_manifest(), ExitCode::SUCCESS);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "agenterm-cc-web (experimental)\n\nUSAGE:\n  agenterm-cc-web --probe\n  agenterm-cc-web --asset-manifest\n  agenterm-cc-web [--smoke] [--no-activate]\n\nThis fallback-safe launcher never links a browser runtime. It delegates only to the sibling direct-WRY experiment."
        );
        return ExitCode::SUCCESS;
    }
    if args
        .iter()
        .any(|arg| !matches!(arg.as_str(), "--probe" | "--smoke" | "--no-activate"))
    {
        eprintln!("unsupported argument; use --help");
        return ExitCode::from(64);
    }

    let current_exe = match env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            return unavailable("current_exe_failed", None, None, error.to_string(), None);
        }
    };
    let host_path = direct_host_path(&current_exe);
    let mut command = Command::new(&host_path);
    command.args(&args);
    if env::var_os("AGENTERM_NO_ACTIVATE").is_some()
        && !args.iter().any(|arg| arg == "--no-activate")
    {
        command.arg("--no-activate");
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            return unavailable(
                "host_unavailable",
                Some(host_path),
                None,
                error.to_string(),
                None,
            );
        }
    };

    let parsed: Option<serde_json::Value> = serde_json::from_slice(&output.stdout).ok();
    if output.status.success() {
        if let Some(value) = parsed {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).expect("JSON value serializes")
            );
        } else {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        return ExitCode::SUCCESS;
    }
    let detail = parsed
        .as_ref()
        .and_then(|value| value.get("reason"))
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| std::str::from_utf8(&output.stderr).unwrap_or("host failed"))
        .trim()
        .to_owned();
    unavailable(
        "runtime_or_host_unavailable",
        Some(host_path),
        output.status.code(),
        detail,
        parsed,
    )
}

fn unavailable(
    reason: &str,
    host_path: Option<std::path::PathBuf>,
    code: Option<i32>,
    detail: String,
    host_receipt: Option<serde_json::Value>,
) -> ExitCode {
    let receipt = LauncherReceipt {
        schema: CONTRACT_VERSION,
        implementation: "fallback-launcher",
        status: "unavailable",
        reason: format!("{reason}: {detail}"),
        host_path: host_path.unwrap_or_default(),
        host_exit_code: code,
        host_receipt,
        active_renderer: "native",
    };
    print_json(&receipt, ExitCode::from(69))
}

fn print_json(value: &impl serde::Serialize, code: ExitCode) -> ExitCode {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("receipt serializes")
    );
    code
}
