use std::io::{Read, Write};
use std::process::{Command, Stdio};

const TERMINAL_PASTE_LIMIT_BYTES: usize = 256 * 1024;

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

/// Match Win32 `normalize_terminal_paste`: CRLF/LF → CR, drop unsafe controls.
pub(super) fn normalize_terminal_paste(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push('\r');
            }
            '\n' => normalized.push('\r'),
            '\t' => normalized.push('\t'),
            value if !value.is_control() => normalized.push(value),
            _ => {}
        }
    }
    normalized
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
    use super::{normalize_terminal_paste, read_via_command, write_via_command};

    #[test]
    fn write_via_command_rejects_empty_argv() {
        assert!(write_via_command(&[], "hi").is_err());
    }

    #[test]
    fn read_via_command_rejects_empty_argv() {
        assert!(read_via_command(&[]).is_err());
    }

    #[test]
    fn normalize_terminal_paste_matches_win32_rules() {
        assert_eq!(normalize_terminal_paste("a\r\nb\nc\t\u{7}d"), "a\rb\rc\td");
        assert_eq!(normalize_terminal_paste(""), "");
    }
}
