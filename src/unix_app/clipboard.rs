use std::io::Write;
use std::process::{Command, Stdio};

/// Best-effort Unicode clipboard write for Unix hosts without a GUI clipboard crate.
pub(super) fn set_clipboard_text(text: &str) -> Result<(), String> {
    let attempts: &[(&[&str], &str)] = if cfg!(target_os = "macos") {
        &[(&["pbcopy"], "pbcopy")]
    } else {
        &[
            (&["wl-copy"], "wl-copy"),
            (&["xclip", "-selection", "clipboard"], "xclip"),
            (&["xsel", "--clipboard", "--input"], "xsel"),
        ]
    };

    let mut errors = Vec::new();
    for (argv, label) in attempts {
        match write_via_command(argv, text) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("{label}: {error}")),
        }
    }
    Err(format!(
        "could not write clipboard ({})",
        errors.join("; ")
    ))
}

fn write_via_command(argv: &[&str], text: &str) -> Result<(), String> {
    let program = argv.first().copied().ok_or_else(|| "empty command".to_owned())?;
    let mut child = Command::new(program)
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "missing stdin".to_owned())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exit {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::write_via_command;

    #[test]
    fn write_via_command_rejects_empty_argv() {
        assert!(write_via_command(&[], "hi").is_err());
    }
}
