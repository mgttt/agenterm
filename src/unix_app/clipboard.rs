#[cfg(not(target_os = "linux"))]
use std::io::{Read, Write};
#[cfg(not(target_os = "linux"))]
use std::process::{Command, Stdio};

#[cfg(not(target_os = "linux"))]
use crate::ui_clipboard::TERMINAL_PASTE_LIMIT_BYTES;

/// Best-effort Unicode clipboard write for Unix hosts without a GUI clipboard crate.
pub(super) fn set_clipboard_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::clipboard::set_text(text).map_err(|error| error.message())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let attempts: &[(&[&str], &str)] = &[(&["pbcopy"], "pbcopy")];
        let mut errors = Vec::new();
        for (argv, label) in attempts {
            match write_via_command(argv, text) {
                Ok(()) => return Ok(()),
                Err(error) => errors.push(format!("{label}: {error}")),
            }
        }
        Err(format!("could not write clipboard ({})", errors.join("; ")))
    }
}

/// Best-effort Unicode clipboard read for terminal paste.
pub(super) fn get_clipboard_text() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::clipboard::get_text().map_err(|error| error.message())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let attempts: &[(&[&str], &str)] = &[(&["pbpaste"], "pbpaste")];
        let mut errors = Vec::new();
        for (argv, label) in attempts {
            match read_via_command(argv) {
                Ok(text) => {
                    if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
                        return Err(format!(
                            "clipboard text exceeds the {TERMINAL_PASTE_LIMIT_BYTES} byte terminal paste limit"
                        ));
                    }
                    return Ok(text);
                }
                Err(error) => errors.push(format!("{label}: {error}")),
            }
        }
        Err(format!("could not read clipboard ({})", errors.join("; ")))
    }
}

/// Fast probe for Unicode clipboard text without reading the full payload when possible.
pub(super) fn clipboard_has_unicode_text() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::clipboard::has_unicode_text()
    }
    #[cfg(not(target_os = "linux"))]
    {
        probe_command_stdout_has_byte(&["pbpaste"])
    }
}

#[cfg(not(target_os = "linux"))]
fn probe_command_stdout_has_byte(argv: &[&str]) -> bool {
    let program = argv.first().copied().unwrap_or_default();
    let mut child = match Command::new(program)
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return false,
    };
    let mut buffer = [0u8; 1];
    let read_ok = stdout
        .read(&mut buffer)
        .map(|count| count > 0)
        .unwrap_or(false);
    let _ = child.wait();
    read_ok
}

#[cfg(not(target_os = "linux"))]
fn write_via_command(argv: &[&str], text: &str) -> Result<(), String> {
    let program = argv
        .first()
        .copied()
        .ok_or_else(|| "empty command".to_owned())?;
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

#[cfg(not(target_os = "linux"))]
fn read_via_command(argv: &[&str]) -> Result<String, String> {
    let program = argv
        .first()
        .copied()
        .ok_or_else(|| "empty command".to_owned())?;
    let mut child = Command::new(program)
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "missing stdout".to_owned())?;
    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let status = child.wait().map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("exit {status}"));
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "linux"))]
    use super::{read_via_command, write_via_command};

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn write_via_command_rejects_empty_argv() {
        assert!(write_via_command(&[], "hi").is_err());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn read_via_command_rejects_empty_argv() {
        assert!(read_via_command(&[]).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_clipboard_delegates_to_platform_capability_boundary() {
        use crate::platform::{CapabilityKind, CapabilityStatus};
        let status = crate::platform::linux::capability_status(CapabilityKind::Clipboard);
        assert!(matches!(
            status,
            CapabilityStatus::Available
                | CapabilityStatus::Failed {
                    code: "clipboard_unavailable",
                    ..
                }
                | CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
        ));
    }
}
