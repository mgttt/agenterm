//! WebKitGTK toolkit set-value for AT-SPI `Text` nodes.
//!
//! WebKit 2.52's native AT-SPI backend registers `Text` on `<textarea>` /
//! contenteditable (so `cu tree` can read the field) but never
//! `EditableText`. The write that `SetTextContents` would have issued is
//! `AXCoreObject::setValue` inside the renderer. This module applies that
//! set-value through the eval helper loaded by
//! `scripts/reasonix-desktop-a11y.sh` (`LD_PRELOAD` + GTK-thread
//! `webkit_web_view_run_javascript`), using the AT-SPI `id` attribute
//! (`composer-input`) and accessible name. The caller confirms with
//! `Text.GetText`. Never XTest.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::contract::accessibility_tree::AccessibilityTreeError;

const EVAL_TIMEOUT: Duration = Duration::from_millis(1500);

pub(super) fn set_field_value(
    html_id: &str,
    name: &str,
    value: &str,
) -> Result<(), AccessibilityTreeError> {
    let path = eval_sock_path().ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            "node exposes AT-SPI Text but not EditableText, and no WebKit \
             eval helper socket was found (launch via scripts/reasonix-desktop-a11y.sh)",
        )
    })?;
    if !path.exists() {
        return Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            format!(
                "node exposes AT-SPI Text but not EditableText, and WebKit \
                 eval helper {} is absent (launch via scripts/reasonix-desktop-a11y.sh)",
                path.display()
            ),
        ));
    }
    let mut stream = UnixStream::connect(&path).map_err(|error| {
        AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            format!("WebKit eval helper connect failed: {error}"),
        )
    })?;
    stream
        .set_read_timeout(Some(EVAL_TIMEOUT))
        .map_err(|error| eval_failed(format!("WebKit eval set read timeout: {error}")))?;
    stream
        .set_write_timeout(Some(EVAL_TIMEOUT))
        .map_err(|error| eval_failed(format!("WebKit eval set write timeout: {error}")))?;
    write_frame(&mut stream, html_id, name, value)?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|error| eval_failed(format!("WebKit eval helper read failed: {error}")))?;
    if reply.starts_with("OK") {
        return Ok(());
    }
    Err(eval_failed(format!(
        "WebKit eval helper refused the write: {}",
        reply.trim()
    )))
}

pub(super) fn eval_sock_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PLATFORM_WEBKIT_EVAL_SOCK")
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR")
        && !runtime.is_empty()
    {
        return Some(Path::new(&runtime).join("agenterm-webkit-eval.sock"));
    }
    Some(PathBuf::from("/tmp/agenterm-webkit-eval.sock"))
}

fn write_frame(
    stream: &mut UnixStream,
    html_id: &str,
    name: &str,
    value: &str,
) -> Result<(), AccessibilityTreeError> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"A11YEVAL1\n");
    write_prefixed(&mut buf, html_id.as_bytes());
    write_prefixed(&mut buf, name.as_bytes());
    write_prefixed(&mut buf, value.as_bytes());
    stream
        .write_all(&buf)
        .map_err(|error| eval_failed(format!("WebKit eval helper write failed: {error}")))
}

fn write_prefixed(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(format!("{}\n", bytes.len()).as_bytes());
    buf.extend_from_slice(bytes);
}

fn eval_failed(message: impl ToString) -> AccessibilityTreeError {
    AccessibilityTreeError::failed("a11y_text_unavailable", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_socket_path_wins() {
        let previous = std::env::var_os("PLATFORM_WEBKIT_EVAL_SOCK");
        unsafe {
            std::env::set_var("PLATFORM_WEBKIT_EVAL_SOCK", "/tmp/agenterm-eval-test.sock");
        }
        assert_eq!(
            eval_sock_path().as_deref(),
            Some(Path::new("/tmp/agenterm-eval-test.sock"))
        );
        unsafe {
            match previous {
                Some(value) => std::env::set_var("PLATFORM_WEBKIT_EVAL_SOCK", value),
                None => std::env::remove_var("PLATFORM_WEBKIT_EVAL_SOCK"),
            }
        }
    }

    #[test]
    fn prefixed_frame_round_trip_shape() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"A11YEVAL1\n");
        write_prefixed(&mut buf, b"composer-input");
        write_prefixed(&mut buf, b"Message");
        write_prefixed(&mut buf, b"hi");
        assert_eq!(
            buf,
            b"A11YEVAL1\n14\ncomposer-input7\nMessage2\nhi".to_vec()
        );
    }
}
