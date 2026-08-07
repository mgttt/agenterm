use std::process::ExitCode;

fn main() -> ExitCode {
    let os_arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if agenterm::incremental_wrapper::is_incremental_rustc_wrapper_process(&os_arguments) {
        agenterm::incremental_wrapper::run_incremental_rustc_wrapper(os_arguments);
    }
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<u8> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if let Some(status) = agenterm::script_rh_cli::try_forward_version_flags(&arguments) {
        let status = status?;
        return u8::try_from(status.code().unwrap_or(1))
            .map_err(|_| anyhow::anyhow!("agenterm-rh returned an invalid exit code"));
    }
    if let Some(status) = agenterm::script_rh_cli::try_forward_dev_cli(&arguments) {
        let status = status?;
        return u8::try_from(status.code().unwrap_or(1))
            .map_err(|_| anyhow::anyhow!("agenterm-rh returned an invalid exit code"));
    }
    match arguments.as_slice() {
        [mode, rest @ ..] if mode == "--internal-incremental-finalize" => {
            agenterm::incremental_wrapper::finalize_incremental_manifest(rest)
        }
        [mode] if mode == "--worker" => agenterm::run_legacy_worker_stdio(),
        [mode] if mode == "--framed-worker" => agenterm::run_framed_worker_stdio(),
        [mode, ..] if mode == "check-many" => {
            anyhow::bail!(
                "{}",
                agenterm::script_rh_cli::check_many_requires_rh_error()
            )
        }
        [mode] if mode == "--version" || mode == "-V" => {
            println!("agenterm-rhai {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        _ => u8::try_from(agenterm::run_script_entry_with_args(arguments))
            .map_err(|_| anyhow::anyhow!("script entry returned an invalid exit code")),
    }
}
