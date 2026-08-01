//! Bounded macOS Unicode clipboard capability.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "macos")]

use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

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
}

pub(crate) fn set_text(text: &str, _timeout: std::time::Duration) -> Result<(), ClipboardError> {
    write_via_command("pbcopy", text, HELPER_TIMEOUT)
}

pub(crate) fn get_text(
    max_read_bytes: usize,
    _timeout: std::time::Duration,
) -> Result<String, ClipboardError> {
    read_via_command("pbpaste", max_read_bytes, HELPER_TIMEOUT)
}

pub(crate) fn has_unicode_text() -> bool {
    match read_via_command("pbpaste", 1, HELPER_TIMEOUT) {
        Ok(text) => !text.is_empty(),
        Err(ClipboardError::TooLarge { .. }) => true,
        Err(_) => false,
    }
}

pub(crate) fn map_error(error: ClipboardError) -> crate::contract::clipboard::ClipboardError {
    match &error {
        ClipboardError::Unavailable { .. } => {
            crate::contract::clipboard::ClipboardError::unsupported("clipboard-unavailable")
        }
        ClipboardError::TooLarge { .. } => crate::contract::clipboard::ClipboardError::failed(
            "clipboard_too_large",
            error.message(),
        ),
        ClipboardError::Timeout => {
            crate::contract::clipboard::ClipboardError::failed("clipboard_timeout", error.message())
        }
        ClipboardError::Backend { .. } => crate::contract::clipboard::ClipboardError::failed(
            "clipboard_backend_error",
            error.message(),
        ),
    }
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
    let text = text.as_bytes().to_vec();
    let writer = thread::spawn(move || {
        let result = stdin
            .write_all(&text)
            .map_err(|error| ClipboardError::Backend {
                message: error.to_string(),
            });
        drop(stdin);
        result
    });
    let deadline = Instant::now() + timeout;
    while !writer.is_finished() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            return Err(ClipboardError::Timeout);
        }
        thread::sleep(Duration::from_millis(5));
    }
    match writer.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ClipboardError::Backend {
                message: "clipboard writer thread panicked".to_owned(),
            });
        }
    }
    wait_child(
        &mut child,
        deadline.saturating_duration_since(Instant::now()),
    )
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
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn failures_have_stable_typed_codes() {
        assert_eq!(
            map_error(ClipboardError::TooLarge { limit: 32 }).to_capability_status(),
            crate::CapabilityStatus::Failed {
                code: "clipboard_too_large".into(),
                message: "clipboard text exceeds the 32 byte terminal paste limit".to_owned(),
            }
        );
        assert_eq!(
            map_error(ClipboardError::Timeout).to_capability_status(),
            crate::CapabilityStatus::Failed {
                code: "clipboard_timeout".into(),
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
    fn bounded_reader_reports_the_caller_supplied_limit() {
        let mut input = &b"12345"[..];
        assert_eq!(
            read_stdout_bounded(&mut input, 4),
            Err(ClipboardError::TooLarge { limit: 4 })
        );
    }

    #[test]
    fn blocked_writer_is_terminated_by_the_helper_deadline() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-macos-blocked-clipboard-{}.sh",
            std::process::id()
        ));
        std::fs::write(&path, "#!/bin/sh\nexec sleep 5\n").expect("write helper");
        let mut permissions = std::fs::metadata(&path)
            .expect("helper metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("make helper executable");

        let text = "x".repeat(2 * 1024 * 1024);
        let started = Instant::now();
        let result = write_via_command(
            path.to_str().expect("UTF-8 helper path"),
            &text,
            Duration::from_millis(50),
        );
        let _ = std::fs::remove_file(path);

        assert_eq!(result, Err(ClipboardError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
