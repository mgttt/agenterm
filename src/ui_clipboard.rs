//! Shared clipboard text normalization for terminal and composer surfaces.

pub(crate) const TERMINAL_PASTE_LIMIT_BYTES: usize = 256 * 1024;

/// Normalize clipboard text for terminal PTY input (CRLF/LF → CR, drop unsafe controls).
pub(crate) fn normalize_terminal_paste(text: &str) -> String {
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

/// Frame normalized terminal input according to the target terminal mode.
pub(crate) fn terminal_paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(text.as_bytes());
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

/// Normalize clipboard text for the composer strip (LF newlines, safe controls only).
#[allow(dead_code)] // Consumed by the Unix frontend adapter.
pub(crate) fn normalize_composer_paste(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push('\n');
            }
            '\n' => normalized.push('\n'),
            '\t' => normalized.push('\t'),
            value if !value.is_control() => normalized.push(value),
            _ => {}
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{normalize_composer_paste, normalize_terminal_paste, terminal_paste_bytes};

    #[test]
    fn normalize_terminal_paste_matches_win32_rules() {
        assert_eq!(normalize_terminal_paste("a\r\nb\nc\t\u{7}d"), "a\rb\rc\td");
        assert_eq!(normalize_terminal_paste(""), "");
    }

    #[test]
    fn normalize_composer_paste_keeps_newlines() {
        assert_eq!(normalize_composer_paste("a\r\nb\nc"), "a\nb\nc");
    }

    #[test]
    fn terminal_paste_framing_is_exact_and_mode_owned() {
        assert_eq!(terminal_paste_bytes("a\rb", false), b"a\rb");
        assert_eq!(
            terminal_paste_bytes("a\rb", true),
            b"\x1b[200~a\rb\x1b[201~"
        );
    }
}
