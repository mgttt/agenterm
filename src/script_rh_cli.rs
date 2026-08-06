//! Resolve and forward dev-facing commands to `agenterm-rh`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

const RH_DEV_COMMANDS: &[&str] = &[
    "check",
    "check-many",
    "transpile",
    "compile",
    "eval",
    "run-smoke",
    "pack",
    "qualify",
    "hash",
    "version",
    "corpus-scan",
    "caller-inventory",
];

fn non_empty_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

pub fn resolve_adjacent_rh_cli() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    let parent = current.parent()?;
    let candidate = parent.join(crate::platform::filesystem::executable_name("agenterm-rh"));
    non_empty_file(&candidate).then_some(candidate)
}

pub fn resolve_rh_cli() -> Option<PathBuf> {
    if let Some(adjacent) = resolve_adjacent_rh_cli() {
        return Some(adjacent);
    }
    let repo = std::env::var("AGENTERM_PROJECT_ROOT")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let name = crate::platform::filesystem::executable_name("agenterm-rh");
    for candidate in [
        repo.join("dist").join(&name),
        repo.join("target/debug").join(&name),
    ] {
        if non_empty_file(&candidate) {
            return Some(candidate);
        }
    }
    None
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
        "run" => forward_run_as_eval(arguments),
        cmd if RH_DEV_COMMANDS.contains(&cmd) => Some(arguments.to_vec()),
        _ => None,
    }?;
    Some(
        Command::new(rh)
            .args(forwarded)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status(),
    )
}

pub fn check_many_requires_rh_error() -> String {
    "check-many requires agenterm-rh; build with: cargo build -p agenterm-rh --bin agenterm-rh".into()
}

fn forward_if_rh_path(arguments: &[String], path_index: usize, prefix: &[&str]) -> Option<Vec<String>> {
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
    if !(path.ends_with(".rh") || path.ends_with(".rhai")) {
        return None;
    }
    let mut forwarded = vec!["eval".to_owned()];
    forwarded.extend_from_slice(&arguments[1..]);
    Some(forwarded)
}

#[cfg(test)]
mod tests {
    use super::{forward_run_as_eval, resolve_adjacent_rh_cli, resolve_rh_cli};

    #[test]
    fn adjacent_rh_cli_resolves_next_to_current_exe() {
        assert!(resolve_adjacent_rh_cli().is_some());
    }

    #[test]
    fn resolve_rh_cli_falls_back_to_target_debug() {
        assert!(resolve_rh_cli().is_some());
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
}
