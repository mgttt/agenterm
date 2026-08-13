//! Chrome renderer accessibility set-value for AT-SPI `Text` nodes.
//!
//! Chromium 151's AuraLinux ATK layer implements `AtkText` (so `cu tree` can
//! read a named field) but never registers `AtkEditableText`. The write that
//! `AtkEditableText::set_text_contents` would have issued is AX `kSetValue`
//! on the renderer node. This module applies that same set-value through the
//! Chrome instance's existing remote-debugging port (already on for box
//! Chrome) and the caller confirms the result with AT-SPI `Text.GetText`.
//! Never XTest.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::contract::accessibility_tree::AccessibilityTreeError;

const CDP_TIMEOUT: Duration = Duration::from_millis(1500);

pub(super) fn set_named_field_value(
    pids: impl IntoIterator<Item = u32>,
    name: &str,
    value: &str,
) -> Result<(), AccessibilityTreeError> {
    let port = debug_port_from_pids(pids).ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            "node exposes AT-SPI Text but not EditableText, and no Chrome \
             remote-debugging port was found to apply AX set-value",
        )
    })?;
    set_named_field_on_port(port, name, value)
}

fn debug_port_from_pids(pids: impl IntoIterator<Item = u32>) -> Option<u16> {
    pids.into_iter().find_map(port_from_cmdline)
}

fn port_from_cmdline(pid: u32) -> Option<u16> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|arg| std::str::from_utf8(arg).ok())
        .find_map(|arg| {
            arg.strip_prefix("--remote-debugging-port=")
                .and_then(|port| port.parse().ok())
                .filter(|port| *port > 0)
        })
}

fn set_named_field_on_port(
    port: u16,
    name: &str,
    value: &str,
) -> Result<(), AccessibilityTreeError> {
    let list = http_get(port, "/json/list")?;
    let mut last_err = AccessibilityTreeError::failed(
        "a11y_text_unavailable",
        format!(
            "Chrome AX tree has no writable node named {name:?} \
             (AT-SPI Text is read-only; EditableText is absent)"
        ),
    );
    for ws_path in page_ws_paths(&list) {
        match set_named_field_on_target(port, &ws_path, name, value) {
            Ok(()) => return Ok(()),
            Err(error) => last_err = error,
        }
    }
    Err(last_err)
}

fn page_ws_paths(list_json: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut rest = list_json;
    while let Some(idx) = rest.find("\"webSocketDebuggerUrl\"") {
        rest = &rest[idx + 23..];
        let Some(url_start) = rest.find("ws://") else {
            continue;
        };
        let url = &rest[url_start..];
        let Some(url_end) = url.find('"') else {
            break;
        };
        let url = &url[..url_end];
        // Only page targets; browser_ui / iframe workers cannot take DOM value.
        if url.contains("/devtools/page/")
            && let Some(path) = url
                .split_once("://")
                .and_then(|(_, rest)| rest.find('/').map(|i| rest[i..].to_owned()))
        {
            paths.push(path);
        }
        rest = &rest[url_end..];
    }
    paths
}

fn set_named_field_on_target(
    port: u16,
    ws_path: &str,
    name: &str,
    value: &str,
) -> Result<(), AccessibilityTreeError> {
    let mut session = CdpSession::connect(port, ws_path)?;
    let _ = session.call("Accessibility.enable", "{}");
    let _ = session.call("DOM.enable", "{}");
    let tree = session.call("Accessibility.getFullAXTree", "{}")?;
    let Some(backend_id) = backend_dom_node_id_for_name(&tree, name) else {
        return Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            format!("Chrome AX tree has no DOM-backed node named {name:?}"),
        ));
    };
    let resolved = session.call(
        "DOM.resolveNode",
        &format!("{{\"backendNodeId\":{backend_id}}}"),
    )?;
    let object_id = json_string_field(&resolved, "objectId").ok_or_else(|| {
        AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            format!("Chrome DOM.resolveNode for {name:?} returned no objectId"),
        )
    })?;
    let setter = format!(
        "{{\
            \"objectId\":\"{}\",\
            \"functionDeclaration\":\"function(v){{\
                this.focus && this.focus();\
                if (this.isContentEditable) {{\
                    this.textContent = v;\
                }} else {{\
                    const proto = Object.getPrototypeOf(this);\
                    const desc = proto && Object.getOwnPropertyDescriptor(proto, 'value');\
                    if (desc && desc.set) desc.set.call(this, v); else this.value = v;\
                }}\
                this.dispatchEvent(new Event('input', {{bubbles:true}}));\
                this.dispatchEvent(new Event('change', {{bubbles:true}}));\
                return true;\
            }}\",\
            \"arguments\":[{{\"value\":\"{}\"}}]\
        }}",
        json_escape(&object_id),
        json_escape(value)
    );
    let result = session.call("Runtime.callFunctionOn", &setter)?;
    if result.contains("\"exceptionDetails\"") {
        return Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            format!("Chrome AX set-value for {name:?} raised an exception"),
        ));
    }
    Ok(())
}

fn backend_dom_node_id_for_name(body: &str, name: &str) -> Option<u64> {
    // Chrome AX nodes serialize `name` (with nested relatedNodes that also
    // carry backendDOMNodeId) before the node's own `backendDOMNodeId`.
    // Skip ids that sit inside `relatedNodes` so we write the field, not
    // its label.
    let needle = format!("\"value\":\"{}\"", json_escape(name));
    let key = "\"backendDOMNodeId\"";
    let mut from = 0;
    while let Some(rel) = body[from..].find(key) {
        let idx = from + rel;
        let before = &body[..idx];
        let tail = if before.len() > 24 {
            &before[before.len() - 24..]
        } else {
            before
        };
        from = idx + key.len();
        // relatedNodes serialize as `relatedNodes":[{"backendDOMNodeId":N`.
        if tail.contains("[{") {
            continue;
        }
        let lookback = if before.len() > 2500 {
            &before[before.len() - 2500..]
        } else {
            before
        };
        if lookback.contains(&needle)
            && let Some(id) = json_u64_field(&body[idx..], "backendDOMNodeId")
        {
            return Some(id);
        }
    }
    None
}

fn json_u64_field(body: &str, field: &str) -> Option<u64> {
    let key = format!("\"{field}\"");
    let idx = body.find(&key)?;
    let rest = body[idx + key.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn json_string_field(body: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\"");
    let idx = body.find(&needle)?;
    let rest = body[idx + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                other => out.push(other),
            }
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}

fn http_get(port: u16, path: &str) -> Result<String, AccessibilityTreeError> {
    let mut stream = connect_tcp(port)?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|error| cdp_failed(format!("Chrome CDP HTTP write failed: {error}")))?;
    let buf = read_http_response(&mut stream)?;
    let response = String::from_utf8_lossy(&buf);
    if !response.contains("200") {
        return Err(cdp_failed("Chrome CDP /json/list did not return HTTP 200"));
    }
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_else(|| response.into_owned());
    Ok(body)
}

fn read_http_response(stream: &mut TcpStream) -> Result<Vec<u8>, AccessibilityTreeError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut header_end = None;
    let mut content_len = None;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if header_end.is_none()
                    && let Some(idx) = find_double_crlf(&buf)
                {
                    header_end = Some(idx);
                    content_len = parse_content_length(&buf[..idx]);
                }
                if let (Some(end), Some(len)) = (header_end, content_len)
                    && buf.len() >= end + len
                {
                    buf.truncate(end + len);
                    break;
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                if header_end.is_some() {
                    break;
                }
                return Err(cdp_failed(format!(
                    "Chrome CDP HTTP read timed out: {error}"
                )));
            }
            Err(error) => {
                return Err(cdp_failed(format!("Chrome CDP HTTP read failed: {error}")));
            }
        }
        if buf.len() > 4 * 1024 * 1024 {
            return Err(cdp_failed("Chrome CDP HTTP response too large"));
        }
    }
    if buf.is_empty() {
        return Err(cdp_failed("Chrome CDP HTTP response was empty"));
    }
    Ok(buf)
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(headers).ok()?;
    for line in text.split("\r\n") {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

struct CdpSession {
    stream: TcpStream,
    next_id: u32,
}

impl CdpSession {
    fn connect(port: u16, path: &str) -> Result<Self, AccessibilityTreeError> {
        let mut stream = connect_tcp(port)?;
        let key = ws_key();
        let req = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).map_err(|error| {
            cdp_failed(format!("Chrome CDP WS handshake write failed: {error}"))
        })?;
        let mut header = Vec::new();
        let mut byte = [0u8; 1];
        while !header.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut byte).map_err(|error| {
                cdp_failed(format!("Chrome CDP WS handshake read failed: {error}"))
            })?;
            if n == 0 {
                return Err(cdp_failed("Chrome CDP WS handshake closed"));
            }
            header.push(byte[0]);
            if header.len() > 8192 {
                return Err(cdp_failed("Chrome CDP WS handshake too large"));
            }
        }
        let header = String::from_utf8_lossy(&header);
        if !header.contains("101") {
            return Err(cdp_failed(
                "Chrome CDP WS handshake was not 101 Switching Protocols",
            ));
        }
        Ok(Self { stream, next_id: 1 })
    }

    fn call(&mut self, method: &str, params: &str) -> Result<String, AccessibilityTreeError> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = format!("{{\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}");
        write_ws_text(&mut self.stream, payload.as_bytes())?;
        loop {
            let msg = read_ws_text(&mut self.stream)?;
            if msg.contains(&format!("\"id\":{id}")) {
                return Ok(msg);
            }
        }
    }
}

fn connect_tcp(port: u16) -> Result<TcpStream, AccessibilityTreeError> {
    let stream = TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        CDP_TIMEOUT,
    )
    .map_err(|error| cdp_failed(format!("Chrome CDP connect :{port} failed: {error}")))?;
    stream
        .set_read_timeout(Some(CDP_TIMEOUT))
        .map_err(|error| cdp_failed(format!("Chrome CDP set read timeout: {error}")))?;
    stream
        .set_write_timeout(Some(CDP_TIMEOUT))
        .map_err(|error| cdp_failed(format!("Chrome CDP set write timeout: {error}")))?;
    Ok(stream)
}

fn write_ws_text(stream: &mut TcpStream, payload: &[u8]) -> Result<(), AccessibilityTreeError> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x81);
    let mask = [0x11, 0x22, 0x33, 0x44];
    let len = payload.len();
    if len <= 125 {
        frame.push(0x80 | len as u8);
    } else if len <= 65535 {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    for (i, byte) in payload.iter().enumerate() {
        frame.push(byte ^ mask[i % 4]);
    }
    stream
        .write_all(&frame)
        .map_err(|error| cdp_failed(format!("Chrome CDP WS write failed: {error}")))
}

fn read_ws_text(stream: &mut TcpStream) -> Result<String, AccessibilityTreeError> {
    loop {
        let mut header = [0u8; 2];
        stream
            .read_exact(&mut header)
            .map_err(|error| cdp_failed(format!("Chrome CDP WS header read failed: {error}")))?;
        let opcode = header[0] & 0x0f;
        let mut len = (header[1] & 0x7f) as u64;
        if len == 126 {
            let mut ext = [0u8; 2];
            stream
                .read_exact(&mut ext)
                .map_err(|error| cdp_failed(format!("Chrome CDP WS len16 read failed: {error}")))?;
            len = u16::from_be_bytes(ext) as u64;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            stream
                .read_exact(&mut ext)
                .map_err(|error| cdp_failed(format!("Chrome CDP WS len64 read failed: {error}")))?;
            len = u64::from_be_bytes(ext);
        }
        if len > 4 * 1024 * 1024 {
            return Err(cdp_failed("Chrome CDP WS frame too large"));
        }
        let mut payload = vec![0u8; len as usize];
        if !payload.is_empty() {
            stream.read_exact(&mut payload).map_err(|error| {
                cdp_failed(format!("Chrome CDP WS payload read failed: {error}"))
            })?;
        }
        match opcode {
            0x1 => {
                return String::from_utf8(payload)
                    .map_err(|_| cdp_failed("Chrome CDP WS text frame was not UTF-8"));
            }
            0x8 => return Err(cdp_failed("Chrome CDP WS closed")),
            0x9 => {
                // ping → pong
                let _ = write_ws_pong(stream, &payload);
            }
            _ => {}
        }
    }
}

fn write_ws_pong(stream: &mut TcpStream, payload: &[u8]) -> Result<(), AccessibilityTreeError> {
    let mut frame = Vec::with_capacity(payload.len() + 6);
    frame.push(0x8a);
    let mask = [0x55, 0x66, 0x77, 0x88];
    frame.push(0x80 | payload.len() as u8);
    frame.extend_from_slice(&mask);
    for (i, byte) in payload.iter().enumerate() {
        frame.push(byte ^ mask[i % 4]);
    }
    stream
        .write_all(&frame)
        .map_err(|error| cdp_failed(format!("Chrome CDP WS pong failed: {error}")))
}

fn ws_key() -> String {
    // 16 arbitrary bytes, base64. CDP only checks the upgrade, not uniqueness.
    base64_16([
        0x61, 0x67, 0x65, 0x6e, 0x74, 0x65, 0x72, 0x6d, 0x2d, 0x63, 0x64, 0x70, 0x2d, 0x6b, 0x65,
        0x79,
    ])
}

fn base64_16(bytes: [u8; 16]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(24);
    let mut i = 0;
    while i < 16 {
        let b0 = bytes[i];
        let b1 = bytes.get(i + 1).copied().unwrap_or(0);
        let b2 = bytes.get(i + 2).copied().unwrap_or(0);
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < 16 {
            out.push(T[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < 16 {
            out.push(T[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn cdp_failed(message: impl ToString) -> AccessibilityTreeError {
    AccessibilityTreeError::failed("a11y_text_unavailable", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escape_quotes_and_controls() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("a\nb"), "a\\nb");
    }

    #[test]
    fn parses_backend_dom_node_id_for_name() {
        let body = r#"{"result":{"nodes":[{"role":{"value":"textbox"},"name":{"type":"computedString","value":"FixtureField","sources":[{"nativeSourceValue":{"relatedNodes":[{"backendDOMNodeId":15}]}}]},"backendDOMNodeId":16}]}}"#;
        assert_eq!(backend_dom_node_id_for_name(body, "FixtureField"), Some(16));
        assert_eq!(backend_dom_node_id_for_name(body, "other"), None);
    }

    #[test]
    fn parses_page_ws_paths() {
        let list = r#"[{"type":"page","webSocketDebuggerUrl":"ws://127.0.0.1:9224/devtools/page/ABC"},{"type":"browser_ui","webSocketDebuggerUrl":"ws://127.0.0.1:9224/devtools/browser/XYZ"}]"#;
        assert_eq!(page_ws_paths(list), vec!["/devtools/page/ABC".to_owned()]);
    }

    #[test]
    fn json_string_field_unescapes() {
        assert_eq!(
            json_string_field(r#"{"objectId":"node\n1"}"#, "objectId").as_deref(),
            Some("node\n1")
        );
    }
}
