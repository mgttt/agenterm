use agenterm_cc_web::{
    CONTRACT_VERSION, LauncherReceipt, asset_manifest, direct_host_path, tauri_host_path,
};
use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.as_slice() == ["--asset-manifest"] {
        return print_json(&asset_manifest(), ExitCode::SUCCESS);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "agenterm-cc-web (experimental)\n\nUSAGE:\n  agenterm-cc-web --asset-manifest\n  agenterm-cc-web [--implementation direct-wry|tauri] --probe\n  agenterm-cc-web [--implementation direct-wry|tauri] [--smoke] [--no-activate]\n\nThis fallback-safe launcher never links a browser runtime. It delegates only to an explicit sibling experiment."
        );
        return ExitCode::SUCCESS;
    }
    let (implementation, forwarded) = match parse_arguments(&args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}; use --help");
            return ExitCode::from(64);
        }
    };

    let current_exe = match env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            return unavailable(
                implementation,
                "current_exe_failed",
                None,
                None,
                error.to_string(),
                None,
            );
        }
    };
    let host_path = match implementation {
        "direct-wry" => direct_host_path(&current_exe),
        "tauri" => tauri_host_path(&current_exe),
        _ => unreachable!("argument parser restricts implementation"),
    };
    let mut command = Command::new(&host_path);
    command.args(&forwarded);
    if env::var_os("AGENTERM_NO_ACTIVATE").is_some()
        && !forwarded.iter().any(|arg| arg == "--no-activate")
    {
        command.arg("--no-activate");
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            return unavailable(
                implementation,
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
        implementation,
        "runtime_or_host_unavailable",
        Some(host_path),
        output.status.code(),
        detail,
        parsed,
    )
}

fn unavailable(
    implementation: &'static str,
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
        requested_implementation: implementation,
        host_path: host_path.unwrap_or_default(),
        host_exit_code: code,
        host_receipt,
        active_renderer: "native",
    };
    print_json(&receipt, ExitCode::from(69))
}

fn parse_arguments(arguments: &[String]) -> Result<(&'static str, Vec<String>), String> {
    let mut implementation = "direct-wry";
    let mut forwarded = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--implementation" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| "--implementation requires a value".to_string())?;
                implementation = match value.as_str() {
                    "direct-wry" => "direct-wry",
                    "tauri" => "tauri",
                    _ => return Err(format!("unsupported implementation {value}")),
                };
                index += 2;
            }
            "--probe" | "--smoke" | "--no-activate" => {
                forwarded.push(arguments[index].clone());
                index += 1;
            }
            value => return Err(format!("unsupported argument {value}")),
        }
    }
    Ok((implementation, forwarded))
}

fn print_json(value: &impl serde::Serialize, code: ExitCode) -> ExitCode {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("receipt serializes")
    );
    code
}
