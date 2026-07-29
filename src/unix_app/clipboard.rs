use std::io::{Read, Write};
use std::process::{Command, Stdio};

use crate::ui_clipboard::TERMINAL_PASTE_LIMIT_BYTES;

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
    Err(format!("could not write clipboard ({})", errors.join("; ")))
}

/// Best-effort Unicode clipboard read for terminal paste.
pub(super) fn get_clipboard_text() -> Result<String, String> {
    let attempts: &[(&[&str], &str)] = if cfg!(target_os = "macos") {
        &[(&["pbpaste"], "pbpaste")]
    } else {
        &[
            (&["wl-paste", "--no-newline"], "wl-paste"),
            (&["xclip", "-selection", "clipboard", "-o"], "xclip"),
            (&["xsel", "--clipboard", "--output"], "xsel"),
        ]
    };

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

/// Fast probe for Unicode clipboard text without reading the full payload when possible.
pub(super) fn clipboard_has_unicode_text() -> bool {
    if cfg!(target_os = "macos") {
        return probe_command_stdout_has_byte(&["pbpaste"]);
    }

    if probe_wl_clipboard_has_text() || probe_xclip_has_text() || probe_xsel_has_text() {
        return true;
    }

    match get_clipboard_text() {
        Ok(text) => !text.is_empty(),
        Err(_) => false,
    }
}

fn probe_wl_clipboard_has_text() -> bool {
    match read_via_command(&["wl-paste", "--list-types"]) {
        Ok(types) => clipboard_types_indicate_unicode_text(&types),
        Err(_) => false,
    }
}

fn probe_xclip_has_text() -> bool {
    match read_via_command(&["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"]) {
        Ok(types) => clipboard_types_indicate_unicode_text(&types),
        Err(_) => false,
    }
}

fn probe_xsel_has_text() -> bool {
    match read_via_command(&["xsel", "--clipboard", "--targets"]) {
        Ok(types) => clipboard_types_indicate_unicode_text(&types),
        Err(_) => false,
    }
}

fn clipboard_types_indicate_unicode_text(types: &str) -> bool {
    types.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() {
            return false;
        }
        let lower = line.to_ascii_lowercase();
        lower.starts_with("text/")
            || matches!(
                lower.as_str(),
                "utf8_string" | "string" | "text" | "compound_text" | "text/plain"
            )
    })
}

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
    use super::{clipboard_types_indicate_unicode_text, read_via_command, write_via_command};

    #[test]
    fn write_via_command_rejects_empty_argv() {
        assert!(write_via_command(&[], "hi").is_err());
    }

    #[test]
    fn read_via_command_rejects_empty_argv() {
        assert!(read_via_command(&[]).is_err());
    }

    #[test]
    fn clipboard_types_indicate_unicode_text_recognizes_common_targets() {
        let types = "UTF8_STRING\nSTRING\nTIMESTAMP\n";
        assert!(clipboard_types_indicate_unicode_text(types));

        let wl_types = "text/plain;charset=utf-8\n";
        assert!(clipboard_types_indicate_unicode_text(wl_types));

        assert!(!clipboard_types_indicate_unicode_text("TIMESTAMP\n"));
        assert!(!clipboard_types_indicate_unicode_text(""));
    }
}
