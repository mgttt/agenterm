//! Linux clipboard capability bridge for platform migration slice-2
//! (contract revision 1).
//!
//! Uses process helpers (`wl-clipboard` / `xclip` / `xsel`). Failures are typed
//! [`CapabilityStatus::Failed`] / [`Unsupported`] — never a silent Available.
//! Shared contract already declares [`CapabilityKind::Clipboard`]; no new
//! shared fields are introduced in this slice.

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use crate::platform::{CapabilityStatus, DisplayBackendFacts};
use crate::ui_clipboard::TERMINAL_PASTE_LIMIT_BYTES;

use super::display_facts_from_env;

/// Typed clipboard failure for Linux adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipboardError {
    Unavailable { message: String },
    TooLarge { limit: usize },
    Backend { message: String },
}

impl ClipboardError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Unavailable { message } | Self::Backend { message } => message.clone(),
            Self::TooLarge { limit } => {
                format!("clipboard text exceeds the {limit} byte terminal paste limit")
            }
        }
    }

    pub(crate) fn to_capability_status(&self) -> CapabilityStatus {
        match self {
            Self::Unavailable { message } => CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                message: message.clone(),
            },
            Self::TooLarge { limit } => CapabilityStatus::Failed {
                code: "clipboard_too_large",
                message: format!("exceeds {limit} bytes"),
            },
            Self::Backend { message } => CapabilityStatus::Failed {
                code: "clipboard_backend_error",
                message: message.clone(),
            },
        }
    }
}

/// Which clipboard helper binaries appear installed (discovery only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ClipboardBackendFacts {
    pub wl_clipboard: bool,
    pub xclip: bool,
    pub xsel: bool,
}

impl ClipboardBackendFacts {
    pub(crate) fn any(self) -> bool {
        self.wl_clipboard || self.xclip || self.xsel
    }

    pub(crate) fn probe() -> Self {
        Self {
            wl_clipboard: command_exists("wl-copy") || command_exists("wl-paste"),
            xclip: command_exists("xclip"),
            xsel: command_exists("xsel"),
        }
    }
}

/// Clipboard capability for the current display + helper backends.
pub(crate) fn clipboard_capability_status(
    display: DisplayBackendFacts,
    backends: ClipboardBackendFacts,
) -> CapabilityStatus {
    if display.headless {
        return CapabilityStatus::Unsupported {
            reason: "headless-display",
        };
    }
    if backends.any() {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Failed {
            code: "clipboard_unavailable",
            message: "no wl-clipboard/xclip/xsel helper found".to_string(),
        }
    }
}

pub(crate) fn clipboard_capability_status_from_env() -> CapabilityStatus {
    clipboard_capability_status(display_facts_from_env(), ClipboardBackendFacts::probe())
}

/// Write Unicode text to the system clipboard.
pub(crate) fn set_text(text: &str) -> Result<(), ClipboardError> {
    match clipboard_capability_status_from_env() {
        CapabilityStatus::Available => {}
        CapabilityStatus::Unsupported { reason } => {
            return Err(ClipboardError::Unavailable {
                message: format!("clipboard unsupported ({reason})"),
            });
        }
        CapabilityStatus::Failed { code, message } => {
            return Err(ClipboardError::Unavailable {
                message: format!("{code}: {message}"),
            });
        }
    }

    let attempts: &[(&[&str], &str)] = &[
        (&["wl-copy"], "wl-copy"),
        (&["xclip", "-selection", "clipboard"], "xclip"),
        (&["xsel", "--clipboard", "--input"], "xsel"),
    ];
    let mut errors = Vec::new();
    for (argv, label) in attempts {
        match write_via_command(argv, text) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("{label}: {error}")),
        }
    }
    Err(ClipboardError::Backend {
        message: format!("could not write clipboard ({})", errors.join("; ")),
    })
}

/// Read Unicode text from the system clipboard.
pub(crate) fn get_text() -> Result<String, ClipboardError> {
    match clipboard_capability_status_from_env() {
        CapabilityStatus::Available => {}
        CapabilityStatus::Unsupported { reason } => {
            return Err(ClipboardError::Unavailable {
                message: format!("clipboard unsupported ({reason})"),
            });
        }
        CapabilityStatus::Failed { code, message } => {
            return Err(ClipboardError::Unavailable {
                message: format!("{code}: {message}"),
            });
        }
    }

    let attempts: &[(&[&str], &str)] = &[
        (&["wl-paste", "--no-newline"], "wl-paste"),
        (&["xclip", "-selection", "clipboard", "-o"], "xclip"),
        (&["xsel", "--clipboard", "--output"], "xsel"),
    ];
    let mut errors = Vec::new();
    for (argv, label) in attempts {
        match read_via_command(argv) {
            Ok(text) => {
                if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
                    return Err(ClipboardError::TooLarge {
                        limit: TERMINAL_PASTE_LIMIT_BYTES,
                    });
                }
                return Ok(text);
            }
            Err(error) => errors.push(format!("{label}: {error}")),
        }
    }
    Err(ClipboardError::Backend {
        message: format!("could not read clipboard ({})", errors.join("; ")),
    })
}

/// Fast probe for Unicode clipboard text without reading the full payload when possible.
pub(crate) fn has_unicode_text() -> bool {
    if probe_wl_clipboard_has_text() || probe_xclip_has_text() || probe_xsel_has_text() {
        return true;
    }
    match get_text() {
        Ok(text) => !text.is_empty(),
        Err(_) => false,
    }
}

fn command_exists(program: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {program} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
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
    use super::*;

    #[test]
    fn headless_clipboard_is_unsupported() {
        let status = clipboard_capability_status(
            DisplayBackendFacts {
                x11: false,
                wayland: false,
                headless: true,
            },
            ClipboardBackendFacts {
                wl_clipboard: true,
                xclip: true,
                xsel: true,
            },
        );
        assert!(matches!(
            status,
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        ));
    }

    #[test]
    fn missing_helpers_are_failed_not_available() {
        let status = clipboard_capability_status(
            DisplayBackendFacts {
                x11: true,
                wayland: false,
                headless: false,
            },
            ClipboardBackendFacts::default(),
        );
        assert!(matches!(
            status,
            CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                ..
            }
        ));
    }

    #[test]
    fn helper_present_with_display_is_available() {
        let status = clipboard_capability_status(
            DisplayBackendFacts {
                x11: true,
                wayland: false,
                headless: false,
            },
            ClipboardBackendFacts {
                wl_clipboard: false,
                xclip: true,
                xsel: false,
            },
        );
        assert_eq!(status, CapabilityStatus::Available);
    }

    #[test]
    fn clipboard_error_maps_to_typed_capability_status() {
        let err = ClipboardError::Unavailable {
            message: "no helper".to_string(),
        };
        assert!(matches!(
            err.to_capability_status(),
            CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                ..
            }
        ));
        let too_large = ClipboardError::TooLarge { limit: 12 };
        assert!(matches!(
            too_large.to_capability_status(),
            CapabilityStatus::Failed {
                code: "clipboard_too_large",
                ..
            }
        ));
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

    #[test]
    fn write_via_command_rejects_empty_argv() {
        assert!(write_via_command(&[], "hi").is_err());
    }

    #[test]
    fn read_via_command_rejects_empty_argv() {
        assert!(read_via_command(&[]).is_err());
    }
}
