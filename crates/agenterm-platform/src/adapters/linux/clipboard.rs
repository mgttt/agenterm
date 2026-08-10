//! Linux clipboard capability bridge for platform migration slice-2
//! Adapter-private native mechanism selected only by platform::selected.
//! (contract revision 1).
//!
//! Uses process helpers (`wl-clipboard` / `xclip` / `xsel`). Failures are typed
//! [`CapabilityStatus::Failed`] / [`Unsupported`] — never a silent Available.
//! Shared contract already declares [`CapabilityKind::Clipboard`]; no new
//! shared fields are introduced in this slice.
//!
//! Hardening:
//! - read and write each require their matching helper (no `wl-copy || wl-paste`)
//! - Wayland helpers only count on Wayland; X11 helpers only on X11
//! - every helper call has an explicit wall timeout and must not block the GUI
//! - stdout is scanned with a live byte budget (never `read_to_end` then check)
//! - the `sh -c "command -v ..."` existence probe passes the program name as
//!   a positional parameter, never interpolated into the script text

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
enum CapabilityStatus {
    Available,
    Unsupported { reason: &'static str },
    Failed { code: &'static str, message: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DisplayBackendFacts {
    x11: bool,
    wayland: bool,
    headless: bool,
}

fn display_facts_from_env() -> DisplayBackendFacts {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    DisplayBackendFacts {
        x11,
        wayland,
        headless: !(x11 || wayland),
    }
}

/// Adapter ceiling for a caller-supplied clipboard deadline (GUI must not stall).
pub(crate) const HELPER_TIMEOUT: Duration = Duration::from_millis(1_500);

/// Bound for TARGETS / MIME-type probe output (not full paste payloads).
const TYPE_LIST_LIMIT_BYTES: usize = 64 * 1024;

/// Typed clipboard failure for Linux adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipboardError {
    Unavailable { message: String },
    TooLarge { limit: usize },
    Timeout { message: String },
    Backend { message: String },
}

impl ClipboardError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Unavailable { message }
            | Self::Backend { message }
            | Self::Timeout { message } => message.clone(),
            Self::TooLarge { limit } => {
                format!("clipboard text exceeds the {limit} byte terminal paste limit")
            }
        }
    }

    #[cfg(test)]
    fn to_capability_status(&self) -> CapabilityStatus {
        match self {
            Self::Unavailable { message } => CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                message: message.clone(),
            },
            Self::TooLarge { limit } => CapabilityStatus::Failed {
                code: "clipboard_too_large",
                message: format!("exceeds {limit} bytes"),
            },
            Self::Timeout { message } => CapabilityStatus::Failed {
                code: "clipboard_timeout",
                message: message.clone(),
            },
            Self::Backend { message } => CapabilityStatus::Failed {
                code: "clipboard_backend_error",
                message: message.clone(),
            },
        }
    }
}

/// Which clipboard helper binaries appear installed (discovery only).
///
/// `wl-copy` and `wl-paste` are tracked separately so a half-installed
/// wl-clipboard package cannot report Available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ClipboardBackendFacts {
    pub wl_copy: bool,
    pub wl_paste: bool,
    pub xclip: bool,
    pub xsel: bool,
}

impl ClipboardBackendFacts {
    pub(crate) fn probe() -> Self {
        Self {
            wl_copy: command_exists("wl-copy"),
            wl_paste: command_exists("wl-paste"),
            xclip: command_exists("xclip"),
            xsel: command_exists("xsel"),
        }
    }

    pub(crate) fn wayland_write(self) -> bool {
        self.wl_copy
    }

    pub(crate) fn wayland_read(self) -> bool {
        self.wl_paste
    }

    pub(crate) fn wayland_pair(self) -> bool {
        self.wl_copy && self.wl_paste
    }

    pub(crate) fn x11_pair(self) -> bool {
        // xclip and xsel each provide both read and write in one binary.
        self.xclip || self.xsel
    }
}

/// Clipboard capability for the current display + helper backends.
///
/// Available only when at least one display-matched backend can both read and
/// write. A lone `wl-copy` or `wl-paste` is Failed, never Available.
fn clipboard_capability_status(
    display: DisplayBackendFacts,
    backends: ClipboardBackendFacts,
) -> CapabilityStatus {
    if display.headless {
        return CapabilityStatus::Unsupported {
            reason: "headless-display",
        };
    }

    if can_read_and_write(display, backends) {
        return CapabilityStatus::Available;
    }

    CapabilityStatus::Failed {
        code: "clipboard_unavailable",
        message: unavailable_detail(display, backends),
    }
}

fn clipboard_capability_status_from_env() -> CapabilityStatus {
    clipboard_capability_status(display_facts_from_env(), ClipboardBackendFacts::probe())
}

fn can_read_and_write(display: DisplayBackendFacts, backends: ClipboardBackendFacts) -> bool {
    can_write(display, backends) && can_read(display, backends)
}

fn can_write(display: DisplayBackendFacts, backends: ClipboardBackendFacts) -> bool {
    if display.headless {
        return false;
    }
    (display.wayland && backends.wayland_write()) || (display.x11 && backends.x11_pair())
}

fn can_read(display: DisplayBackendFacts, backends: ClipboardBackendFacts) -> bool {
    if display.headless {
        return false;
    }
    (display.wayland && backends.wayland_read()) || (display.x11 && backends.x11_pair())
}

fn unavailable_detail(display: DisplayBackendFacts, backends: ClipboardBackendFacts) -> String {
    if display.wayland && !backends.wayland_pair() && !display.x11 {
        if backends.wl_copy && !backends.wl_paste {
            return "wl-paste missing (wl-copy alone is not enough for clipboard)".to_string();
        }
        if backends.wl_paste && !backends.wl_copy {
            return "wl-copy missing (wl-paste alone is not enough for clipboard)".to_string();
        }
        return "no Wayland clipboard helpers (need both wl-copy and wl-paste)".to_string();
    }
    if display.x11 && !backends.x11_pair() && !display.wayland {
        return "no X11 clipboard helpers (need xclip or xsel)".to_string();
    }
    if display.wayland && backends.wl_copy != backends.wl_paste && !backends.x11_pair() {
        return "incomplete wl-clipboard pair and no usable X11 helper".to_string();
    }
    "no display-matched clipboard helper pair found".to_string()
}

fn require_capability_for_io() -> Result<(), ClipboardError> {
    match clipboard_capability_status_from_env() {
        CapabilityStatus::Available => Ok(()),
        CapabilityStatus::Unsupported { reason } => Err(ClipboardError::Unavailable {
            message: format!("clipboard unsupported ({reason})"),
        }),
        CapabilityStatus::Failed { code, message } => Err(ClipboardError::Unavailable {
            message: format!("{code}: {message}"),
        }),
    }
}

/// Write Unicode text to the system clipboard.
pub(crate) fn set_text(text: &str, timeout: std::time::Duration) -> Result<(), ClipboardError> {
    let timeout = bounded_helper_timeout(timeout)?;
    let deadline = Instant::now() + timeout;
    require_capability_for_io()?;
    let display = display_facts_from_env();
    let backends = ClipboardBackendFacts::probe();
    if !can_write(display, backends) {
        return Err(ClipboardError::Unavailable {
            message: "no display-matched clipboard write helper".to_string(),
        });
    }

    let mut errors = Vec::new();
    for (argv, label) in write_attempts(display, backends) {
        let remaining = remaining_budget(deadline, timeout)?;
        match write_via_command(argv, text, remaining) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("{label}: {}", error.message())),
        }
    }
    Err(classify_attempt_errors("write", &errors))
}

/// Read Unicode text from the system clipboard.
pub(crate) fn get_text(
    max_read_bytes: usize,
    timeout: std::time::Duration,
) -> Result<String, ClipboardError> {
    let timeout = bounded_helper_timeout(timeout)?;
    let deadline = Instant::now() + timeout;
    require_capability_for_io()?;
    let display = display_facts_from_env();
    let backends = ClipboardBackendFacts::probe();
    if !can_read(display, backends) {
        return Err(ClipboardError::Unavailable {
            message: "no display-matched clipboard read helper".to_string(),
        });
    }

    let mut errors = Vec::new();
    for (argv, label) in read_attempts(display, backends) {
        let remaining = remaining_budget(deadline, timeout)?;
        match read_via_command(argv, max_read_bytes, remaining) {
            Ok(text) => return Ok(text),
            Err(error) => {
                if matches!(error, ClipboardError::TooLarge { .. }) {
                    return Err(error);
                }
                errors.push(format!("{label}: {}", error.message()));
            }
        }
    }
    Err(classify_attempt_errors("read", &errors))
}

fn bounded_helper_timeout(timeout: Duration) -> Result<Duration, ClipboardError> {
    if timeout.is_zero() {
        return Err(timeout_error(timeout, "clipboard operation"));
    }
    Ok(timeout.min(HELPER_TIMEOUT))
}

fn remaining_budget(deadline: Instant, timeout: Duration) -> Result<Duration, ClipboardError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(timeout_error(timeout, "clipboard operation"))
    } else {
        Ok(remaining)
    }
}

fn timeout_error(timeout: Duration, label: &str) -> ClipboardError {
    ClipboardError::Timeout {
        message: format!(
            "clipboard_timeout: {label} exceeded {} ms",
            timeout.as_millis()
        ),
    }
}

/// Fast probe for Unicode clipboard text without reading the full payload when possible.
pub(crate) fn has_unicode_text() -> bool {
    let display = display_facts_from_env();
    let backends = ClipboardBackendFacts::probe();
    if !can_read(display, backends) {
        return false;
    }
    if display.wayland && backends.wl_paste && probe_wl_clipboard_has_text() {
        return true;
    }
    if display.x11 {
        if backends.xclip && probe_xclip_has_text() {
            return true;
        }
        if backends.xsel && probe_xsel_has_text() {
            return true;
        }
    }
    match get_text(1, HELPER_TIMEOUT) {
        Ok(text) => !text.is_empty(),
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
        ClipboardError::Timeout { .. } => {
            crate::contract::clipboard::ClipboardError::failed("clipboard_timeout", error.message())
        }
        ClipboardError::Backend { .. } => crate::contract::clipboard::ClipboardError::failed(
            "clipboard_backend_error",
            error.message(),
        ),
    }
}

const WL_COPY: &[&str] = &["wl-copy"];
const WL_PASTE: &[&str] = &["wl-paste", "--no-newline"];
const WL_PASTE_TYPES: &[&str] = &["wl-paste", "--list-types"];
const XCLIP_WRITE: &[&str] = &["xclip", "-selection", "clipboard", "-target", "UTF8_STRING"];
const XCLIP_READ: &[&str] = &[
    "xclip",
    "-selection",
    "clipboard",
    "-target",
    "UTF8_STRING",
    "-o",
];
const XCLIP_TARGETS: &[&str] = &["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"];
const XSEL_WRITE: &[&str] = &["xsel", "--clipboard", "--input"];
const XSEL_READ: &[&str] = &["xsel", "--clipboard", "--output"];
const XSEL_TARGETS: &[&str] = &["xsel", "--clipboard", "--targets"];

fn write_attempts(
    display: DisplayBackendFacts,
    backends: ClipboardBackendFacts,
) -> Vec<(&'static [&'static str], &'static str)> {
    let mut attempts = Vec::new();
    if display.wayland && backends.wl_copy {
        attempts.push((WL_COPY, "wl-copy"));
    }
    if display.x11 {
        if backends.xclip {
            attempts.push((XCLIP_WRITE, "xclip"));
        }
        if backends.xsel {
            attempts.push((XSEL_WRITE, "xsel"));
        }
    }
    attempts
}

fn read_attempts(
    display: DisplayBackendFacts,
    backends: ClipboardBackendFacts,
) -> Vec<(&'static [&'static str], &'static str)> {
    let mut attempts = Vec::new();
    if display.wayland && backends.wl_paste {
        attempts.push((WL_PASTE, "wl-paste"));
    }
    if display.x11 {
        if backends.xclip {
            attempts.push((XCLIP_READ, "xclip"));
        }
        if backends.xsel {
            attempts.push((XSEL_READ, "xsel"));
        }
    }
    attempts
}

fn classify_attempt_errors(op: &str, errors: &[String]) -> ClipboardError {
    let joined = errors.join("; ");
    if errors
        .iter()
        .any(|error| error.contains("clipboard_timeout"))
    {
        ClipboardError::Timeout {
            message: format!("clipboard {op} timed out ({joined})"),
        }
    } else if errors.is_empty() {
        ClipboardError::Unavailable {
            message: format!("no clipboard {op} helper attempted"),
        }
    } else {
        ClipboardError::Backend {
            message: format!("could not {op} clipboard ({joined})"),
        }
    }
}

fn command_exists(program: &str) -> bool {
    // `command -v` is a shell builtin, so it must run under `sh -c`, but
    // `program` reaches this function as a plain argument rather than a
    // literal today. Pass it as a positional parameter ($1) instead of
    // interpolating it into the script text so this stays injection-proof
    // even if a future caller ever derives `program` from configuration.
    Command::new("sh")
        .arg("-c")
        .arg("command -v \"$1\" >/dev/null 2>&1")
        .arg("sh")
        .arg(program)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn probe_wl_clipboard_has_text() -> bool {
    match read_via_command(WL_PASTE_TYPES, TYPE_LIST_LIMIT_BYTES, HELPER_TIMEOUT) {
        Ok(types) => clipboard_types_indicate_unicode_text(&types),
        Err(_) => false,
    }
}

fn probe_xclip_has_text() -> bool {
    match read_via_command(XCLIP_TARGETS, TYPE_LIST_LIMIT_BYTES, HELPER_TIMEOUT) {
        Ok(types) => clipboard_types_indicate_unicode_text(&types),
        Err(_) => false,
    }
}

fn probe_xsel_has_text() -> bool {
    match read_via_command(XSEL_TARGETS, TYPE_LIST_LIMIT_BYTES, HELPER_TIMEOUT) {
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

fn write_via_command(argv: &[&str], text: &str, timeout: Duration) -> Result<(), ClipboardError> {
    let program = argv
        .first()
        .copied()
        .ok_or_else(|| ClipboardError::Backend {
            message: "empty command".to_owned(),
        })?;
    let mut child = Command::new(program)
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| ClipboardError::Backend {
            message: error.to_string(),
        })?;
    let mut stdin = child.stdin.take().ok_or_else(|| ClipboardError::Backend {
        message: "missing stdin".to_owned(),
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
            return Err(timeout_error(timeout, "clipboard write"));
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
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(timeout_error(timeout, "clipboard write"));
    }
    wait_child_with_timeout(&mut child, remaining, "clipboard write")
}

fn read_via_command(
    argv: &[&str],
    limit: usize,
    timeout: Duration,
) -> Result<String, ClipboardError> {
    let program = argv
        .first()
        .copied()
        .ok_or_else(|| ClipboardError::Backend {
            message: "empty command".to_owned(),
        })?;
    let mut child = Command::new(program)
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| ClipboardError::Backend {
            message: error.to_string(),
        })?;
    let mut stdout = child.stdout.take().ok_or_else(|| ClipboardError::Backend {
        message: "missing stdout".to_owned(),
    })?;

    let reader = thread::spawn(move || read_stdout_bounded(&mut stdout, limit));
    let deadline = Instant::now() + timeout;
    loop {
        if reader.is_finished() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(ClipboardError::Timeout {
                message: format!(
                    "clipboard_timeout: helper exceeded {} ms",
                    timeout.as_millis()
                ),
            });
        }
        // Reap early exits so a finished helper does not leave a zombie while
        // the reader thread drains the last pipe bytes.
        match child.try_wait() {
            Ok(Some(_)) => {
                // Process exited; keep waiting briefly for the reader to finish.
            }
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = reader.join();
                return Err(ClipboardError::Backend {
                    message: error.to_string(),
                });
            }
        }
        thread::sleep(Duration::from_millis(5));
    }

    let bytes = match reader.join() {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ClipboardError::Backend {
                message: "clipboard reader thread panicked".to_owned(),
            });
        }
    };

    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(timeout_error(timeout, "clipboard read"));
    }
    wait_child_with_timeout(&mut child, remaining, "clipboard read")?;
    String::from_utf8(bytes).map_err(|error| ClipboardError::Backend {
        message: error.to_string(),
    })
}

fn read_stdout_bounded(stdout: &mut impl Read, limit: usize) -> Result<Vec<u8>, ClipboardError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8_192];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if bytes.len().saturating_add(n) > limit {
                    // Stop reading immediately — do not buffer past the budget.
                    return Err(ClipboardError::TooLarge { limit });
                }
                bytes.extend_from_slice(&chunk[..n]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(ClipboardError::Backend {
                    message: error.to_string(),
                });
            }
        }
    }
    Ok(bytes)
}

fn wait_child_with_timeout(
    child: &mut Child,
    timeout: Duration,
    label: &str,
) -> Result<(), ClipboardError> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err(ClipboardError::Backend {
                    message: format!("exit {status}"),
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ClipboardError::Timeout {
                        message: format!(
                            "clipboard_timeout: {label} exceeded {} ms",
                            timeout.as_millis()
                        ),
                    });
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(ClipboardError::Backend {
                    message: error.to_string(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x11_display() -> DisplayBackendFacts {
        DisplayBackendFacts {
            x11: true,
            wayland: false,
            headless: false,
        }
    }

    fn wayland_display() -> DisplayBackendFacts {
        DisplayBackendFacts {
            x11: false,
            wayland: true,
            headless: false,
        }
    }

    #[test]
    fn headless_clipboard_is_unsupported() {
        let status = clipboard_capability_status(
            DisplayBackendFacts {
                x11: false,
                wayland: false,
                headless: true,
            },
            ClipboardBackendFacts {
                wl_copy: true,
                wl_paste: true,
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
        let status = clipboard_capability_status(x11_display(), ClipboardBackendFacts::default());
        assert!(matches!(
            status,
            CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                ..
            }
        ));
    }

    #[test]
    fn xclip_on_x11_is_available() {
        let status = clipboard_capability_status(
            x11_display(),
            ClipboardBackendFacts {
                wl_copy: false,
                wl_paste: false,
                xclip: true,
                xsel: false,
            },
        );
        assert_eq!(status, CapabilityStatus::Available);
    }

    #[test]
    fn wl_copy_alone_is_not_available_on_wayland() {
        let status = clipboard_capability_status(
            wayland_display(),
            ClipboardBackendFacts {
                wl_copy: true,
                wl_paste: false,
                xclip: false,
                xsel: false,
            },
        );
        assert!(matches!(
            status,
            CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                ..
            }
        ));
        let message = match status {
            CapabilityStatus::Failed { message, .. } => message,
            other => panic!("expected Failed, got {other:?}"),
        };
        assert!(
            message.contains("wl-paste"),
            "detail should mention missing wl-paste: {message}"
        );
    }

    #[test]
    fn wl_paste_alone_is_not_available_on_wayland() {
        let status = clipboard_capability_status(
            wayland_display(),
            ClipboardBackendFacts {
                wl_copy: false,
                wl_paste: true,
                xclip: false,
                xsel: false,
            },
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
    fn wayland_pair_is_available() {
        let status = clipboard_capability_status(
            wayland_display(),
            ClipboardBackendFacts {
                wl_copy: true,
                wl_paste: true,
                xclip: false,
                xsel: false,
            },
        );
        assert_eq!(status, CapabilityStatus::Available);
    }

    #[test]
    fn wayland_helpers_do_not_count_on_x11_only_display() {
        let status = clipboard_capability_status(
            x11_display(),
            ClipboardBackendFacts {
                wl_copy: true,
                wl_paste: true,
                xclip: false,
                xsel: false,
            },
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
    fn x11_helpers_do_not_count_on_wayland_only_display() {
        let status = clipboard_capability_status(
            wayland_display(),
            ClipboardBackendFacts {
                wl_copy: false,
                wl_paste: false,
                xclip: true,
                xsel: true,
            },
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
    fn incomplete_wayland_falls_back_to_x11_when_both_present() {
        let status = clipboard_capability_status(
            DisplayBackendFacts {
                x11: true,
                wayland: true,
                headless: false,
            },
            ClipboardBackendFacts {
                wl_copy: true,
                wl_paste: false,
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
        let timeout = ClipboardError::Timeout {
            message: "clipboard_timeout: helper exceeded 1500 ms".to_string(),
        };
        assert!(matches!(
            timeout.to_capability_status(),
            CapabilityStatus::Failed {
                code: "clipboard_timeout",
                ..
            }
        ));
    }

    #[test]
    fn caller_timeout_is_preserved_and_capped_for_helpers() {
        assert_eq!(
            bounded_helper_timeout(Duration::from_millis(75)),
            Ok(Duration::from_millis(75))
        );
        assert_eq!(
            bounded_helper_timeout(Duration::from_secs(30)),
            Ok(HELPER_TIMEOUT)
        );
        assert_eq!(
            bounded_helper_timeout(Duration::ZERO),
            Err(ClipboardError::Timeout {
                message: "clipboard_timeout: clipboard operation exceeded 0 ms".to_owned(),
            })
        );
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
    fn xclip_commands_negotiate_the_same_unicode_target() {
        assert_eq!(
            XCLIP_WRITE,
            ["xclip", "-selection", "clipboard", "-target", "UTF8_STRING"]
        );
        assert_eq!(
            XCLIP_READ,
            [
                "xclip",
                "-selection",
                "clipboard",
                "-target",
                "UTF8_STRING",
                "-o"
            ]
        );
    }

    #[test]
    fn write_via_command_rejects_empty_argv() {
        assert!(matches!(
            write_via_command(&[], "hi", HELPER_TIMEOUT),
            Err(ClipboardError::Backend { .. })
        ));
    }

    #[test]
    fn read_via_command_rejects_empty_argv() {
        assert!(matches!(
            read_via_command(&[], 16, HELPER_TIMEOUT),
            Err(ClipboardError::Backend { .. })
        ));
    }

    #[test]
    fn read_via_command_enforces_byte_limit_while_streaming() {
        // Emit more than the budget; the reader must fail TooLarge without
        // buffering the entire stream first.
        let result = read_via_command(
            &[
                "python3",
                "-c",
                "import sys; sys.stdout.write('x' * 200000); sys.stdout.flush()",
            ],
            4_096,
            Duration::from_secs(3),
        );
        assert!(
            matches!(result, Err(ClipboardError::TooLarge { limit: 4_096 })),
            "expected TooLarge, got {result:?}"
        );
    }

    #[test]
    fn read_via_command_times_out_instead_of_blocking() {
        let started = Instant::now();
        let result = read_via_command(&["sleep", "5"], 1_024, Duration::from_millis(200));
        let elapsed = started.elapsed();
        assert!(
            matches!(result, Err(ClipboardError::Timeout { .. })),
            "expected Timeout, got {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "timeout path took too long: {elapsed:?}"
        );
    }

    #[test]
    fn desktop_clipboard_round_trip_when_available() {
        let status = clipboard_capability_status_from_env();
        if !matches!(status, CapabilityStatus::Available) {
            // Typed non-Available is success for this probe; skip IO round-trip.
            return;
        }
        let marker = format!(
            "agenterm-linux-clipboard-rt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        set_text(&marker, HELPER_TIMEOUT).expect("clipboard set_text");
        let got = get_text(marker.len(), HELPER_TIMEOUT).expect("clipboard get_text");
        assert_eq!(got, marker);
        assert!(has_unicode_text());
    }

    #[test]
    fn desktop_probe_facts_split_wl_helpers() {
        let facts = ClipboardBackendFacts::probe();
        // Discovery must not collapse wl-copy/wl-paste into one OR bit.
        if command_exists("wl-copy") != command_exists("wl-paste") {
            assert_ne!(facts.wl_copy, facts.wl_paste);
            let display = display_facts_from_env();
            if display.wayland && !display.x11 && !facts.x11_pair() {
                assert_ne!(
                    clipboard_capability_status(display, facts),
                    CapabilityStatus::Available
                );
            }
        }
    }
}
