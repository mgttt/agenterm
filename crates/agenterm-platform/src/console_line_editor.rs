//! Reusable console line-editor mechanism.
//!
//! [`ConsoleLineEditor`] switches the process console into per-key input mode
//! (keeping processed Ctrl-C delivery so callers can pair it with the console
//! interrupt observer) and normalizes key presses to [`ConsoleKey`]. The pure
//! [`LineBuffer`] / [`LineHistory`] types implement the editing state without
//! any operating-system dependency. [`EscapeParser`] decodes Unix terminal
//! byte streams (ESC sequences / UTF-8) and lives here so its behavior is
//! tested on every target, not only where the termios adapter compiles.

pub use crate::contract::console_line_editor::{ConsoleKey, ConsoleLineEditorError};

/// RAII per-key console input mode.
///
/// The editor keeps `ENABLE_PROCESSED_INPUT` / `ISIG` enabled so Ctrl-C keeps
/// flowing to the existing interrupt observer instead of arriving as a key
/// press. Dropping the editor restores the previous console mode.
#[must_use = "dropping the editor restores the previous console input mode"]
pub struct ConsoleLineEditor {
    inner: crate::selected::console_line_editor::Editor,
}

impl ConsoleLineEditor {
    /// Enter per-key input mode on the process console.
    pub fn enter() -> Result<Self, ConsoleLineEditorError> {
        crate::selected::console_line_editor::Editor::enter().map(|inner| Self { inner })
    }

    /// Read one normalized key press, or `None` when no key arrived within the
    /// adapter's polling window.
    pub fn read_key(&mut self) -> Result<Option<ConsoleKey>, ConsoleLineEditorError> {
        self.inner.read_key()
    }
}

/// A single in-progress input line with a byte cursor.
///
/// Cursor positions are byte offsets into the line; the movement helpers stay
/// on character boundaries. The type is deliberately small so the REPL can
/// test editing behavior without a terminal.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineBuffer {
    text: String,
    cursor: usize,
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replace the whole line and place the cursor at its end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
    }

    /// Insert a character at the cursor.
    pub fn insert_char(&mut self, ch: char) {
        let at = self.cursor;
        self.text.insert(at, ch);
        self.cursor = at + ch.len_utf8();
    }

    /// Remove the character before the cursor.
    pub fn backspace(&mut self) -> bool {
        let at = self.cursor;
        let Some((start, _)) = self.text[..at].char_indices().next_back() else {
            return false;
        };
        self.text.replace_range(start..at, "");
        self.cursor = start;
        true
    }

    /// Remove the character under the cursor.
    pub fn delete(&mut self) -> bool {
        let start = self.cursor;
        if start >= self.text.len() {
            return false;
        }
        let end = match self.text[start..].char_indices().nth(1) {
            Some((offset, _)) => start + offset,
            None => self.text.len(),
        };
        self.text.replace_range(start..end, "");
        true
    }

    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((start, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = start;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Take the line and reset the buffer.
    pub fn take(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        text
    }
}

/// In-memory up/down history for completed input lines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineHistory {
    entries: Vec<String>,
    /// 0 = not navigating; 1 = most recent entry, up to `entries.len()`.
    position: usize,
    /// The line that was being edited before the first Up navigation.
    draft: String,
}

impl LineHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed line. Consecutive duplicates collapse.
    pub fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if self.entries.last().map(String::as_str) == Some(line) {
            self.reset();
            return;
        }
        self.entries.push(line.to_owned());
        self.reset();
    }

    /// Navigate toward older entries, saving the current draft on the first
    /// Up press.
    pub fn previous(&mut self, buffer: &LineBuffer) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        if self.position == 0 {
            self.draft = buffer.text().to_owned();
        }
        if self.position >= self.entries.len() {
            return None;
        }
        self.position += 1;
        Some(self.entries[self.entries.len() - self.position].clone())
    }

    /// Navigate toward newer entries, restoring the draft at the bottom.
    pub fn next(&mut self, _buffer: &LineBuffer) -> Option<String> {
        if self.position == 0 {
            return None;
        }
        self.position -= 1;
        if self.position == 0 {
            return Some(std::mem::take(&mut self.draft));
        }
        Some(self.entries[self.entries.len() - self.position].clone())
    }

    /// Drop navigation state (used after a cancelled line).
    pub fn reset(&mut self) {
        self.position = 0;
        self.draft.clear();
    }
}

/// Stateful decoder for terminal byte streams. Survives reads split at any
/// byte boundary (ESC sequences, UTF-8, and repeated poll windows).
// The parser is consumed by the Unix adapter only; on other targets it stays
// compiled (with its tests) but unused, so allow dead code rather than gating
// away cross-platform coverage.
#[allow(dead_code)]
#[derive(Default)]
pub(crate) struct EscapeParser {
    state: ParserState,
    /// Bytes of an incomplete ESC sequence waiting for more input.
    pending: Vec<u8>,
    /// Bytes of an incomplete UTF-8 code point waiting for continuation.
    utf8: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(dead_code)]
enum ParserState {
    #[default]
    Ground,
    Escape,
    Csi,
}

#[allow(dead_code)]
impl EscapeParser {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.state != ParserState::Ground
    }

    /// Resolve any buffered incomplete input as unknown keys.
    pub(crate) fn flush(&mut self) -> ConsoleKey {
        self.pending.clear();
        self.utf8.clear();
        self.state = ParserState::Ground;
        ConsoleKey::Unknown
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<ConsoleKey> {
        let mut keys = Vec::new();
        for &byte in bytes {
            match self.state {
                ParserState::Ground => self.ground_byte(byte, &mut keys),
                ParserState::Escape => match byte {
                    b'[' | b'O' => {
                        self.pending.clear();
                        self.state = ParserState::Csi;
                    }
                    _ => {
                        // Standalone ESC or ESC+printable: report one unknown
                        // and reprocess the byte from the ground state.
                        self.pending.clear();
                        self.state = ParserState::Ground;
                        keys.push(ConsoleKey::Unknown);
                        self.ground_byte(byte, &mut keys);
                    }
                },
                ParserState::Csi => {
                    self.pending.push(byte);
                    if (0x40..=0x7E).contains(&byte) {
                        keys.push(self.finish_csi());
                        self.state = ParserState::Ground;
                    } else if self.pending.len() > 16 {
                        self.pending.clear();
                        self.state = ParserState::Ground;
                        keys.push(ConsoleKey::Unknown);
                    }
                }
            }
        }
        keys
    }

    fn ground_byte(&mut self, byte: u8, keys: &mut Vec<ConsoleKey>) {
        match byte {
            0x1B => {
                self.state = ParserState::Escape;
            }
            0x0D | 0x0A => keys.push(ConsoleKey::Enter),
            0x04 => keys.push(ConsoleKey::Eof),
            0x7F => keys.push(ConsoleKey::Backspace),
            0x20..=0x7E => keys.push(ConsoleKey::Char(byte as char)),
            0x00..=0x1F => {}
            _ => {
                // UTF-8 lead or continuation byte.
                if (0x80..=0xBF).contains(&byte) && !self.utf8.is_empty() {
                    self.utf8.push(byte);
                    self.decode_utf8_if_complete(keys);
                } else {
                    let needed = utf8_sequence_len(byte);
                    if needed == 0 {
                        keys.push(ConsoleKey::Unknown);
                    } else {
                        self.utf8.clear();
                        self.utf8.push(byte);
                        self.decode_utf8_if_complete(keys);
                    }
                }
            }
        }
    }

    fn finish_csi(&mut self) -> ConsoleKey {
        let sequence = std::mem::take(&mut self.pending);
        match sequence.as_slice() {
            [b'A'] => ConsoleKey::Up,
            [b'B'] => ConsoleKey::Down,
            [b'C'] => ConsoleKey::Right,
            [b'D'] => ConsoleKey::Left,
            [b'H'] | [b'1', b'~'] | [b'7', b'~'] => ConsoleKey::Home,
            [b'F'] | [b'4', b'~'] | [b'8', b'~'] => ConsoleKey::End,
            [b'3', b'~'] => ConsoleKey::Delete,
            _ => ConsoleKey::Unknown,
        }
    }

    fn decode_utf8_if_complete(&mut self, keys: &mut Vec<ConsoleKey>) {
        let Some(&first) = self.utf8.first() else {
            return;
        };
        let needed = utf8_sequence_len(first);
        if self.utf8.len() < needed {
            return;
        }
        if let Ok(text) = std::str::from_utf8(&self.utf8) {
            if let Some(ch) = text.chars().next() {
                keys.push(ConsoleKey::Char(ch));
            }
        } else {
            keys.push(ConsoleKey::Unknown);
        }
        self.utf8.clear();
    }
}

/// Number of bytes in the UTF-8 sequence starting with `lead`, or 0 when the
/// byte is not a valid lead (continuations are consumed after a lead).
#[allow(dead_code)]
fn utf8_sequence_len(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(parser: &mut EscapeParser, bytes: &[u8]) -> Vec<ConsoleKey> {
        parser.feed(bytes)
    }

    #[test]
    fn line_buffer_edits_at_character_boundaries() {
        let mut buffer = LineBuffer::new();
        for ch in "héllo".chars() {
            buffer.insert_char(ch);
        }
        assert_eq!(buffer.text(), "héllo");
        assert_eq!(buffer.cursor(), buffer.text().len());

        buffer.move_home();
        buffer.move_right(); // past 'h'
        buffer.backspace();
        assert_eq!(buffer.text(), "éllo");
        assert_eq!(buffer.cursor(), 0);

        buffer.move_end();
        buffer.backspace();
        assert_eq!(buffer.text(), "éll");

        buffer.insert_char('o');
        assert_eq!(buffer.text(), "éllo");

        buffer.move_home();
        buffer.move_right(); // past multi-byte 'é'
        buffer.backspace();
        assert_eq!(buffer.text(), "llo");
        assert_eq!(buffer.cursor(), 0);

        buffer.move_end();
        buffer.delete();
        assert_eq!(buffer.text(), "llo");
    }

    #[test]
    fn line_buffer_cursor_stays_in_bounds() {
        let mut buffer = LineBuffer::new();
        buffer.set_text("abc");
        buffer.move_left();
        buffer.move_left();
        buffer.move_left();
        buffer.move_left();
        assert_eq!(buffer.cursor(), 0);
        buffer.move_right();
        buffer.move_right();
        buffer.move_right();
        buffer.move_right();
        assert_eq!(buffer.cursor(), 3);
        buffer.backspace();
        assert_eq!(buffer.text(), "ab");
        buffer.backspace();
        buffer.backspace();
        assert!(!buffer.backspace());
        assert!(buffer.is_empty());
    }

    #[test]
    fn history_navigates_with_draft_restore() {
        let mut history = LineHistory::new();
        history.push("first");
        history.push("second");
        assert!(history.next(&LineBuffer::new()).is_none());

        let mut buffer = LineBuffer::new();
        buffer.set_text("draft");
        assert_eq!(history.previous(&buffer).as_deref(), Some("second"));
        assert_eq!(history.previous(&buffer).as_deref(), Some("first"));
        assert_eq!(history.previous(&buffer), None);
        assert_eq!(history.next(&buffer).as_deref(), Some("second"));
        assert_eq!(history.next(&buffer).as_deref(), Some("draft"));
        assert!(history.next(&buffer).is_none());
    }

    #[test]
    fn history_collapses_consecutive_duplicates() {
        let mut history = LineHistory::new();
        history.push("same");
        history.push("same");
        history.push("other");
        assert_eq!(history.entries.len(), 2);
    }

    #[test]
    fn decodes_ascii_and_control_bytes() {
        let mut parser = EscapeParser::new();
        assert_eq!(
            feed_all(&mut parser, b"ab\r\n\x7f"),
            vec![
                ConsoleKey::Char('a'),
                ConsoleKey::Char('b'),
                ConsoleKey::Enter,
                ConsoleKey::Enter,
                ConsoleKey::Backspace,
            ]
        );
    }

    #[test]
    fn decodes_arrow_and_edit_sequences_across_read_boundaries() {
        let mut parser = EscapeParser::new();
        assert_eq!(feed_all(&mut parser, b"\x1b"), vec![]);
        assert_eq!(feed_all(&mut parser, b"[A"), vec![ConsoleKey::Up]);
        assert_eq!(feed_all(&mut parser, b"\x1b[3~"), vec![ConsoleKey::Delete]);
        assert_eq!(feed_all(&mut parser, b"\x1b"), vec![]);
        assert_eq!(feed_all(&mut parser, b"[H"), vec![ConsoleKey::Home]);
        assert_eq!(feed_all(&mut parser, b"\x1b[F"), vec![ConsoleKey::End]);
    }

    #[test]
    fn standalone_escape_resolves_to_unknown() {
        let mut parser = EscapeParser::new();
        assert_eq!(feed_all(&mut parser, b"\x1b"), vec![]);
        assert_eq!(parser.flush(), ConsoleKey::Unknown);
    }

    #[test]
    fn utf8_input_decodes_to_chars() {
        let mut parser = EscapeParser::new();
        assert_eq!(
            feed_all(&mut parser, "你".as_bytes()),
            vec![ConsoleKey::Char('你')]
        );
        let mut parser = EscapeParser::new();
        assert_eq!(feed_all(&mut parser, &"你".as_bytes()[..2]), vec![]);
        assert_eq!(
            feed_all(&mut parser, &"你".as_bytes()[2..]),
            vec![ConsoleKey::Char('你')]
        );
    }

    #[test]
    fn eof_and_unknown_controls() {
        let mut parser = EscapeParser::new();
        assert_eq!(feed_all(&mut parser, b"\x04"), vec![ConsoleKey::Eof]);
        let mut parser = EscapeParser::new();
        assert_eq!(feed_all(&mut parser, b"\x1b[5~"), vec![ConsoleKey::Unknown]);
    }
}
