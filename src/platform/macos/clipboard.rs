//! Bounded macOS Unicode clipboard capability.

#![cfg(target_os = "macos")]

use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::{platform::CapabilityStatus, ui_clipboard::TERMINAL_PASTE_LIMIT_BYTES};

pub(crate) const HELPER_TIMEOUT: Duration = Duration::from_millis(1_500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipboardError {
    Unavailable { message: String },
    TooLarge { limit: usize },
    Timeout,
    Backend { message: String },
}

impl ClipboardError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Unavailable { message } | Self::Backend { message } => message.clone(),
            Self::TooLarge { limit } => {
                format!("clipboard text exceeds the {limit} byte terminal paste limit")
            }
            Self::Timeout => {
                format!(
                    "clipboard helper exceeded the {} ms deadline",
                    HELPER_TIMEOUT.as_millis()
                )
            }
        }
    }

    pub(crate) fn to_capability_status(&self) -> CapabilityStatus {
        let code = match self {
            Self::Unavailable { .. } => "clipboard_unavailable",
            Self::TooLarge { .. } => "clipboard_too_large",
            Self::Timeout => "clipboard_timeout",
            Self::Backend { .. } => "clipboard_backend_error",
        };
        CapabilityStatus::Failed {
            code,
            message: self.message(),
        }
    }
}

pub(crate) fn capability_status() -> CapabilityStatus {
    if command_exists("pbcopy") && command_exists("pbpaste") {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Failed {
            code: "clipboard_unavailable",
            message: "pbcopy and pbpaste are required".to_owned(),
        }
    }
}

pub(crate) fn set_text(text: &str) -> Result<(), ClipboardError> {
    if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
        return Err(ClipboardError::TooLarge {
            limit: TERMINAL_PASTE_LIMIT_BYTES,
        });
    }
    write_via_command("pbcopy", text, HELPER_TIMEOUT)
}

pub(crate) fn get_text() -> Result<String, ClipboardError> {
    read_via_command("pbpaste", TERMINAL_PASTE_LIMIT_BYTES, HELPER_TIMEOUT)
}

pub(crate) fn has_unicode_text() -> bool {
    match read_via_command("pbpaste", 1, HELPER_TIMEOUT) {
        Ok(text) => !text.is_empty(),
        Err(ClipboardError::TooLarge { .. }) => true,
        Err(_) => false,
    }
}

fn command_exists(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
}

fn write_via_command(program: &str, text: &str, timeout: Duration) -> Result<(), ClipboardError> {
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| unavailable_or_backend(program, error))?;
    let mut stdin = child.stdin.take().ok_or_else(|| ClipboardError::Backend {
        message: "clipboard helper stdin is unavailable".to_owned(),
    })?;
    stdin
        .write_all(text.as_bytes())
        .map_err(|error| ClipboardError::Backend {
            message: error.to_string(),
        })?;
    drop(stdin);
    wait_child(&mut child, timeout)
}

fn read_via_command(
    program: &str,
    limit: usize,
    timeout: Duration,
) -> Result<String, ClipboardError> {
    let mut child = Command::new(program)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| unavailable_or_backend(program, error))?;
    let mut stdout = child.stdout.take().ok_or_else(|| ClipboardError::Backend {
        message: "clipboard helper stdout is unavailable".to_owned(),
    })?;
    let reader = thread::spawn(move || read_stdout_bounded(&mut stdout, limit));
    let deadline = Instant::now() + timeout;
    while !reader.is_finished() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(ClipboardError::Timeout);
        }
        thread::sleep(Duration::from_millis(5));
    }
    let bytes = match reader.join() {
        Ok(result) => result,
        Err(_) => Err(ClipboardError::Backend {
            message: "clipboard reader thread panicked".to_owned(),
        }),
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    wait_child(
        &mut child,
        deadline.saturating_duration_since(Instant::now()),
    )?;
    String::from_utf8(bytes).map_err(|error| ClipboardError::Backend {
        message: error.to_string(),
    })
}

fn read_stdout_bounded(stdout: &mut impl Read, limit: usize) -> Result<Vec<u8>, ClipboardError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8_192];
    loop {
        let count = stdout
            .read(&mut chunk)
            .map_err(|error| ClipboardError::Backend {
                message: error.to_string(),
            })?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(count) > limit {
            return Err(ClipboardError::TooLarge { limit });
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn wait_child(child: &mut Child, timeout: Duration) -> Result<(), ClipboardError> {
    let deadline = Instant::now() + timeout.max(Duration::from_millis(1));
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(ClipboardError::Backend {
                    message: format!("clipboard helper exited with {status}"),
                });
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClipboardError::Timeout);
            }
            Err(error) => {
                return Err(ClipboardError::Backend {
                    message: error.to_string(),
                });
            }
        }
    }
}

fn unavailable_or_backend(program: &str, error: std::io::Error) -> ClipboardError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ClipboardError::Unavailable {
            message: format!("{program} is unavailable"),
        }
    } else {
        ClipboardError::Backend {
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_have_stable_typed_codes() {
        assert_eq!(
            ClipboardError::TooLarge { limit: 32 }.to_capability_status(),
            CapabilityStatus::Failed {
                code: "clipboard_too_large",
                message: "clipboard text exceeds the 32 byte terminal paste limit".to_owned(),
            }
        );
        assert_eq!(
            ClipboardError::Timeout.to_capability_status(),
            CapabilityStatus::Failed {
                code: "clipboard_timeout",
                message: "clipboard helper exceeded the 1500 ms deadline".to_owned(),
            }
        );
    }

    #[test]
    fn bounded_reader_stops_before_retaining_excess_bytes() {
        let mut input = &b"12345"[..];
        assert_eq!(
            read_stdout_bounded(&mut input, 4),
            Err(ClipboardError::TooLarge { limit: 4 })
        );
    }

    #[test]
    fn writes_reject_oversized_text_before_spawning() {
        let text = "x".repeat(TERMINAL_PASTE_LIMIT_BYTES + 1);
        assert_eq!(
            set_text(&text),
            Err(ClipboardError::TooLarge {
                limit: TERMINAL_PASTE_LIMIT_BYTES,
            })
        );
    }
}
