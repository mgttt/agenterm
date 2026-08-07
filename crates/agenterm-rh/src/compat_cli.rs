//! Explicit compatibility bridge for legacy Script Runtime commands not yet hosted by rh.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::RhError;

const COMPAT_COMMANDS: &[&str] = &["task"];

fn non_empty_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn executable_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

pub fn resolve_compat_cli() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("AGENTERM_RHAI_COMPAT_CLI")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        if non_empty_file(&configured) {
            return Some(configured);
        }
    }

    let name = executable_name("agenterm-rhai");
    if let Some(adjacent) = std::env::current_exe()
        .ok()
        .and_then(|current| current.parent().map(|parent| parent.join(&name)))
        .filter(|candidate| non_empty_file(candidate))
    {
        return Some(adjacent);
    }

    let root = std::env::var_os("AGENTERM_PROJECT_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    [
        root.join("dist").join(&name),
        root.join("target/debug").join(name),
    ]
    .into_iter()
    .find(|candidate| non_empty_file(candidate))
}

pub fn try_forward_compat_cli(arguments: &[String]) -> Option<Result<ExitStatus, RhError>> {
    let command = arguments.first()?;
    if !COMPAT_COMMANDS.contains(&command.as_str()) {
        return None;
    }
    let Some(compat) = resolve_compat_cli() else {
        return Some(Err(RhError::Compile(format!(
            "`agenterm-rh {command}` requires the temporary agenterm-rhai compatibility binary; \
             set AGENTERM_RHAI_COMPAT_CLI or stage both executables together"
        ))));
    };
    Some(
        Command::new(compat)
            .args(arguments)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|error| RhError::Compile(format!("compat CLI launch failed: {error}"))),
    )
}

#[cfg(test)]
mod tests {
    use super::try_forward_compat_cli;

    #[test]
    fn non_compat_commands_are_not_forwarded() {
        assert!(try_forward_compat_cli(&["check".to_owned()]).is_none());
    }
}
