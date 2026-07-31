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
    use super::{normalize_composer_paste, normalize_terminal_paste};

    #[test]
    fn normalize_terminal_paste_matches_win32_rules() {
        assert_eq!(normalize_terminal_paste("a\r\nb\nc\t\u{7}d"), "a\rb\rc\td");
        assert_eq!(normalize_terminal_paste(""), "");
    }

    #[test]
    fn normalize_composer_paste_keeps_newlines() {
        assert_eq!(normalize_composer_paste("a\r\nb\nc"), "a\nb\nc");
    }
}
