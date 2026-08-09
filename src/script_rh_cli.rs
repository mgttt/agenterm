//! Resolve and forward dev-facing commands to the in-process rh engine.
//!
//! The standalone `agenterm-rh` binary is retired: `agenterm-rh` used to be
//! searched for as a file next to the current executable (or under
//! `dist`/`target/debug`); now the rh engine lives inside the main
//! `agenterm` PE itself, reached by prefixing its own path with
//! `RH_ENGINE_ARGS` (the `__agenterm-internal-engine rh` marker — see
//! `src/bin/agenterm.rs`'s `INTERNAL_ENGINE_SUBCOMMAND` dispatch, and
//! `worker_supervisor::SCRIPT_WORKER_ENGINE_ARGS` for the sibling constant
//! used by the hosted-worker spawn paths).

use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

const RH_DEV_COMMANDS: &[&str] = &[
    "check",
    "check-many",
    "transpile",
    "compile",
    "run-smoke",
    "pack",
    "qualify",
    "hash",
    "version",
    "corpus-scan",
    "caller-inventory",
];

/// Argv prefix that routes a `Command::new(<main agenterm PE>)` invocation
/// into the in-process rh engine. Duplicated from
/// `worker_supervisor::SCRIPT_WORKER_ENGINE_ARGS` rather than imported, to
/// keep this leaf module free of a `worker_supervisor` dependency; both must
/// stay in sync with `src/bin/agenterm.rs`'s `__agenterm-internal-engine`
/// dispatch.
const RH_ENGINE_ARGS: [&str; 2] = ["__agenterm-internal-engine", "rh"];

/// The rh engine now always lives inside the currently running main
/// `agenterm` PE — there is no separate `agenterm-rh` file to locate.
pub fn resolve_rh_cli() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

pub fn try_forward_version_flags(arguments: &[String]) -> Option<std::io::Result<ExitStatus>> {
    if arguments.len() != 1 {
        return None;
    }
    let flag = arguments[0].as_str();
    if flag != "--version" && flag != "-V" {
        return None;
    }
    let rh = resolve_rh_cli()?;
    Some(
        Command::new(rh)
            .args(RH_ENGINE_ARGS)
            .arg("version")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status(),
    )
}

pub fn try_forward_dev_cli(arguments: &[String]) -> Option<std::io::Result<ExitStatus>> {
    let rh = resolve_rh_cli()?;
    if arguments.is_empty() {
        return None;
    }
    let command = arguments[0].as_str();
    let forwarded = match command {
        "check" => forward_if_rh_path(arguments, 1, &["check"]),
        "eval" => forward_if_rh_path(arguments, 1, &["eval"]),
        "run" => forward_run_as_eval(arguments),
        cmd if RH_DEV_COMMANDS.contains(&cmd) => Some(arguments.to_vec()),
        _ => None,
    }?;
    Some(
        Command::new(rh)
            .args(RH_ENGINE_ARGS)
            .args(forwarded)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status(),
    )
}

pub fn check_many_requires_rh_error() -> String {
    "check-many requires the rh script engine; build with: cargo build --bin agenterm".into()
}

fn forward_if_rh_path(
    arguments: &[String],
    path_index: usize,
    prefix: &[&str],
) -> Option<Vec<String>> {
    let path = arguments.get(path_index)?;
    if !(path.ends_with(".rh") || path.ends_with(".rhai")) {
        return None;
    }
    let mut forwarded = prefix.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    forwarded.extend_from_slice(&arguments[path_index..]);
    Some(forwarded)
}

fn forward_run_as_eval(arguments: &[String]) -> Option<Vec<String>> {
    let path = arguments.get(1)?;
    if !path.ends_with(".rh") {
        return None;
    }
    let mut forwarded = vec!["eval".to_owned()];
    forwarded.extend_from_slice(&arguments[1..]);
    Some(forwarded)
}

#[cfg(test)]
mod tests {
    use super::{forward_if_rh_path, forward_run_as_eval, resolve_rh_cli};

    #[test]
    fn resolve_rh_cli_is_the_running_main_pe() {
        assert_eq!(
            resolve_rh_cli(),
            std::env::current_exe().ok(),
            "the rh engine now lives inside the currently running main agenterm PE"
        );
    }

    #[test]
    fn run_forwards_to_eval_for_rh_paths() {
        let args = vec![
            "run".to_owned(),
            "fixtures/rh/entry.rh".to_owned(),
            "--".to_owned(),
            "x".to_owned(),
        ];
        let forwarded = forward_run_as_eval(&args).expect("forward");
        assert_eq!(forwarded[0], "eval");
        assert_eq!(forwarded[1], "fixtures/rh/entry.rh");
    }

    #[test]
    fn interpreted_eval_is_not_forwarded() {
        // `run <path>.rh` DOES forward to eval (see
        // run_forwards_to_eval_for_rh_paths above); the old assertion here
        // claimed the opposite and contradicted that test in this same file.
        assert!(forward_if_rh_path(&["eval".into(), "40 + 2".into()], 1, &["eval"]).is_none());
    }
}
