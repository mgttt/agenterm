//! WebKitGTK toolkit scroll for AT-SPI `Component.ScrollTo`.
//!
//! WebKit 2.52's native AT-SPI `Component.ScrollTo` returns true without
//! moving geometry. The scroll that `ScrollTo(TopEdge)` would have issued
//! is `Element.scrollIntoView({block:'start'})` inside the renderer. This
//! module applies that through the eval helper loaded by
//! `scripts/reasonix-desktop-a11y.sh` (`LD_PRELOAD` + GTK-thread
//! `webkit_web_view_run_javascript`), using the AT-SPI `id` attribute and
//! accessible name. The caller confirms with independent
//! `Component.GetExtents(Screen)`. Never XTest / wheel / Action `scroll*`.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::contract::accessibility_tree::AccessibilityTreeError;

use super::webkit_ax_set_value::eval_sock_path;

const EVAL_TIMEOUT: Duration = Duration::from_millis(1500);

pub(super) fn helper_present() -> bool {
    eval_sock_path().is_some_and(|path| path.exists())
}

pub(super) fn scroll_named_node(html_id: &str, name: &str) -> Result<(), AccessibilityTreeError> {
    if html_id.trim().is_empty() && name.trim().is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_scroll_unavailable",
            "WebKit scroll helper needs an accessible name or id attribute",
        ));
    }
    let path = eval_sock_path().ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_scroll_unavailable",
            "AT-SPI Component.ScrollTo is a no-op on this toolkit, and no \
             WebKit eval helper socket was found (launch via scripts/reasonix-desktop-a11y.sh)",
        )
    })?;
    if !path.exists() {
        return Err(AccessibilityTreeError::failed(
            "a11y_scroll_unavailable",
            format!(
                "AT-SPI Component.ScrollTo is a no-op on this toolkit, and WebKit \
                 eval helper {} is absent (launch via scripts/reasonix-desktop-a11y.sh)",
                path.display()
            ),
        ));
    }
    let mut stream = UnixStream::connect(&path).map_err(|error| {
        AccessibilityTreeError::failed(
            "a11y_scroll_unavailable",
            format!("WebKit eval helper connect failed: {error}"),
        )
    })?;
    stream
        .set_read_timeout(Some(EVAL_TIMEOUT))
        .map_err(|error| eval_failed(format!("WebKit eval set read timeout: {error}")))?;
    stream
        .set_write_timeout(Some(EVAL_TIMEOUT))
        .map_err(|error| eval_failed(format!("WebKit eval set write timeout: {error}")))?;
    write_frame(&mut stream, html_id, name)?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .map_err(|error| eval_failed(format!("WebKit eval helper read failed: {error}")))?;
    if reply.starts_with("OK") {
        return Ok(());
    }
    Err(eval_failed(format!(
        "WebKit eval helper refused the scroll: {}",
        reply.trim()
    )))
}

fn write_frame(
    stream: &mut UnixStream,
    html_id: &str,
    name: &str,
) -> Result<(), AccessibilityTreeError> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"A11YSCROLL1\n");
    write_prefixed(&mut buf, html_id.as_bytes());
    write_prefixed(&mut buf, name.as_bytes());
    stream
        .write_all(&buf)
        .map_err(|error| eval_failed(format!("WebKit eval helper write failed: {error}")))
}

fn write_prefixed(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(format!("{}\n", bytes.len()).as_bytes());
    buf.extend_from_slice(bytes);
}

fn eval_failed(message: impl ToString) -> AccessibilityTreeError {
    AccessibilityTreeError::failed("a11y_scroll_unavailable", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_frame_round_trip_shape() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"A11YSCROLL1\n");
        write_prefixed(&mut buf, b"composer-input");
        write_prefixed(&mut buf, b"Skip to composer");
        assert_eq!(
            buf,
            b"A11YSCROLL1\n14\ncomposer-input16\nSkip to composer".to_vec()
        );
    }

    #[test]
    fn empty_identity_fails_typed() {
        let error = scroll_named_node("", "").expect_err("empty identity");
        let AccessibilityTreeError::Failed { code, message } = error else {
            panic!("expected failed");
        };
        assert_eq!(code, "a11y_scroll_unavailable");
        assert!(message.contains("name or id"), "{message}");
        assert!(!message.contains("XTest"), "{message}");
        assert!(!message.to_lowercase().contains("wheel"), "{message}");
    }
}
