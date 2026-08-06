//! Caret and selection state for single- and multi-line text fields.
//!
//! The terminal grid has its own selection model in [`crate::frontend::selection`]
//! because it addresses cells by row/column. Text fields address *characters*,
//! so they need a separate model — but the two follow the same conventions
//! (anchor/focus, drag gestures, far-endpoint shift extension) so the two
//! surfaces behave alike from the user's point of view.
//!
//! Windows hosts the composer in a native `EDIT` control, which supplies caret
//! placement, drag selection, double-click word select and shift extension for
//! free. Hosts that draw their own text have none of that, so this module
//! provides it in platform-neutral form rather than each adapter inventing its
//! own rules and drifting apart.
//!
//! Offsets are **character** indices, not byte indices: text fields hold CJK and
//! emoji, and every public boundary here is expressed in `char` units so callers
//! cannot accidentally slice a multi-byte scalar in half.

/// Caret position and selection extent within a text buffer.
///
/// `anchor` is where the selection started and `focus` is where the caret is
/// now; `anchor == focus` means "no selection, just a caret". Keeping the two
/// separate (rather than storing an ordered range) is what lets shift-extension
/// and backwards drags grow the selection from the correct end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextCursor {
    anchor: usize,
    focus: usize,
}

impl TextCursor {
    /// Caret with no selection at `offset`.
    pub(crate) const fn at(offset: usize) -> Self {
        Self {
            anchor: offset,
            focus: offset,
        }
    }

    /// Selection spanning `anchor` to `focus`, preserving direction.
    pub(crate) const fn new(anchor: usize, focus: usize) -> Self {
        Self { anchor, focus }
    }

    pub(crate) const fn focus(self) -> usize {
        self.focus
    }

    pub(crate) const fn anchor(self) -> usize {
        self.anchor
    }

    /// Selection bounds in ascending order, regardless of drag direction.
    pub(crate) const fn range(self) -> (usize, usize) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    pub(crate) const fn has_selection(self) -> bool {
        self.anchor != self.focus
    }

    /// Clamp both ends into a buffer of `len` characters.
    ///
    /// Applied after every external mutation, because the buffer can shrink
    /// underneath a cursor (a server-driven composer rewrite, an undo) and a
    /// stale offset would otherwise panic the slicing helpers below.
    pub(crate) fn clamped(self, len: usize) -> Self {
        Self {
            anchor: self.anchor.min(len),
            focus: self.focus.min(len),
        }
    }

    /// Collapse to a caret at the given end of the current selection.
    #[allow(dead_code)] // public selection API for frontend adapters not yet fully wired
    pub(crate) const fn collapsed_to(self, offset: usize) -> Self {
        Self::at(offset)
    }

    /// Move the focus, keeping the anchor — the shift+arrow / drag behaviour.
    pub(crate) const fn extended_to(self, focus: usize) -> Self {
        Self {
            anchor: self.anchor,
            focus,
        }
    }

    /// Select everything in a buffer of `len` characters.
    #[allow(dead_code)] // public selection API for frontend adapters not yet fully wired
    pub(crate) const fn select_all(len: usize) -> Self {
        Self {
            anchor: 0,
            focus: len,
        }
    }
}

/// Byte range for a character range, so callers can slice `String` safely.
///
/// Returns `None` when the range is empty, which spares every caller an
/// `is_empty` check before it can act on a selection.
pub(crate) fn byte_range(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    if start >= end {
        return None;
    }
    let start_byte = byte_offset(text, start);
    let end_byte = byte_offset(text, end);
    (start_byte < end_byte).then_some((start_byte, end_byte))
}

/// Text covered by a cursor's selection, or `None` when nothing is selected.
pub(crate) fn selected_text(text: &str, cursor: TextCursor) -> Option<String> {
    let (start, end) = cursor.range();
    let (start_byte, end_byte) = byte_range(text, start, end)?;
    Some(text[start_byte..end_byte].to_owned())
}

/// Delete the selection, returning the caret position left behind.
///
/// Returns `None` when there was no selection, so callers can fall through to
/// their "no selection" behaviour (backspace deleting one character, say)
/// without inspecting the cursor twice.
pub(crate) fn delete_selection(text: &mut String, cursor: TextCursor) -> Option<TextCursor> {
    let (start, end) = cursor.range();
    let (start_byte, end_byte) = byte_range(text, start, end)?;
    text.replace_range(start_byte..end_byte, "");
    Some(TextCursor::at(start))
}

/// Insert `insertion` at the caret, replacing any selection.
///
/// This is the single entry point for typing, pasting and IME commits, so all
/// three treat an active selection the same way users expect: it is replaced.
pub(crate) fn insert(text: &mut String, cursor: TextCursor, insertion: &str) -> TextCursor {
    let cursor = cursor.clamped(text.chars().count());
    let (start, end) = cursor.range();
    let start_byte = byte_offset(text, start);
    if let Some((selection_start, selection_end)) = byte_range(text, start, end) {
        text.replace_range(selection_start..selection_end, insertion);
    } else {
        text.insert_str(start_byte, insertion);
    }
    TextCursor::at(start + insertion.chars().count())
}

/// Byte offset of a character index, saturating at the end of the string.
pub(crate) fn byte_offset(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(index, _)| index)
}

/// Character index of a byte offset, for callers that already hold one.
#[allow(dead_code)] // inverse of `byte_offset`; kept for adapter parity
pub(crate) fn char_index(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())].chars().count()
}

/// Word boundaries around `offset`, for double-click select-word.
///
/// "Word" means a run of alphanumerics and underscores, matching what users get
/// from terminals and editors. Double-clicking whitespace or punctuation
/// selects that single character rather than nothing, so the gesture always
/// produces visible feedback.
pub(crate) fn word_bounds(text: &str, offset: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let index = offset.min(chars.len().saturating_sub(1));
    if !is_word_char(chars[index]) {
        return (index, index + 1);
    }
    let mut start = index;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    (start, end)
}

fn is_word_char(value: char) -> bool {
    value.is_alphanumeric() || value == '_'
}

/// Bounds of the logical line containing `offset`, for triple-click and Home/End.
pub(crate) fn line_bounds(text: &str, offset: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let index = offset.min(chars.len());
    let mut start = index;
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = index;
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    (start, end)
}

/// Anchor to keep when shift-clicking or shift-arrowing to `target`.
///
/// Mirrors the terminal grid's convention (and xterm's): the selection grows
/// from whichever endpoint is further from the new position, so shift-clicking
/// past either edge extends rather than inverting the selection.
pub(crate) fn shift_extend_anchor(cursor: TextCursor, target: usize) -> usize {
    if !cursor.has_selection() {
        return cursor.anchor();
    }
    let (start, end) = cursor.range();
    let start_distance = target.abs_diff(start);
    let end_distance = target.abs_diff(end);
    if start_distance >= end_distance {
        start
    } else {
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_has_no_selection_and_reports_its_offset() {
        let cursor = TextCursor::at(3);
        assert!(!cursor.has_selection());
        assert_eq!(cursor.range(), (3, 3));
        assert_eq!(selected_text("hello", cursor), None);
    }

    #[test]
    fn backwards_drag_reports_ascending_range_but_keeps_its_anchor() {
        let cursor = TextCursor::new(4, 1);
        assert_eq!(cursor.range(), (1, 4));
        assert_eq!(cursor.anchor(), 4);
        assert_eq!(cursor.focus(), 1);
        assert_eq!(selected_text("abcdef", cursor).as_deref(), Some("bcd"));
    }

    /// Char-indexed offsets are the whole point: a byte-indexed implementation
    /// would slice these multi-byte scalars apart and panic.
    #[test]
    fn offsets_are_characters_not_bytes_for_cjk_and_emoji() {
        let text = "中文abc🎉";
        assert_eq!(text.chars().count(), 6);
        // 3 + 3 bytes of CJK, 3 of ASCII, 4 for the emoji.
        assert_eq!(text.len(), 13);

        let cursor = TextCursor::new(0, 2);
        assert_eq!(selected_text(text, cursor).as_deref(), Some("中文"));

        let emoji = TextCursor::new(5, 6);
        assert_eq!(selected_text(text, emoji).as_deref(), Some("🎉"));

        let mut buffer = text.to_owned();
        let after = insert(&mut buffer, TextCursor::new(0, 2), "X");
        assert_eq!(buffer, "Xabc🎉");
        assert_eq!(after.focus(), 1);
    }

    #[test]
    fn insert_replaces_a_selection_and_leaves_a_caret_after_it() {
        let mut text = "hello world".to_owned();
        let cursor = TextCursor::new(6, 11);
        let after = insert(&mut text, cursor, "there");
        assert_eq!(text, "hello there");
        assert!(!after.has_selection());
        assert_eq!(after.focus(), 11);
    }

    #[test]
    fn insert_without_a_selection_splices_at_the_caret() {
        let mut text = "helloworld".to_owned();
        let after = insert(&mut text, TextCursor::at(5), " ");
        assert_eq!(text, "hello world");
        assert_eq!(after.focus(), 6);
    }

    #[test]
    fn delete_selection_reports_absence_so_callers_can_fall_through() {
        let mut text = "abc".to_owned();
        assert_eq!(delete_selection(&mut text, TextCursor::at(1)), None);
        assert_eq!(text, "abc");

        let after = delete_selection(&mut text, TextCursor::new(2, 0)).expect("selection");
        assert_eq!(text, "c");
        assert_eq!(after.focus(), 0);
    }

    #[test]
    fn word_bounds_cover_runs_and_give_punctuation_a_single_cell() {
        let text = "let value_1 = 2";
        assert_eq!(word_bounds(text, 5), (4, 11));
        assert_eq!(word_bounds(text, 4), (4, 11));
        // The space between words selects just itself, so a double-click there
        // still shows the user something happened.
        assert_eq!(word_bounds(text, 3), (3, 4));
        assert_eq!(word_bounds("", 0), (0, 0));
    }

    #[test]
    fn word_bounds_treat_cjk_as_word_characters() {
        let text = "运行 test";
        assert_eq!(word_bounds(text, 0), (0, 2));
        assert_eq!(word_bounds(text, 1), (0, 2));
    }

    #[test]
    fn line_bounds_stop_at_newlines_without_including_them() {
        let text = "first\nsecond\nthird";
        assert_eq!(line_bounds(text, 0), (0, 5));
        assert_eq!(line_bounds(text, 8), (6, 12));
        assert_eq!(line_bounds(text, 18), (13, 18));
    }

    #[test]
    fn shift_extension_grows_from_the_far_endpoint() {
        let cursor = TextCursor::new(4, 8);
        // Clicking before the selection keeps the far (right) edge anchored.
        assert_eq!(shift_extend_anchor(cursor, 1), 8);
        // Clicking past the end keeps the far (left) edge anchored.
        assert_eq!(shift_extend_anchor(cursor, 12), 4);
        // With only a caret there is nothing to extend from but itself.
        assert_eq!(shift_extend_anchor(TextCursor::at(3), 9), 3);
    }

    #[test]
    fn clamping_survives_a_buffer_that_shrank_underneath_the_cursor() {
        let cursor = TextCursor::new(10, 20).clamped(4);
        assert_eq!(cursor.range(), (4, 4));
        assert!(!cursor.has_selection());
    }

    #[test]
    fn select_all_spans_the_whole_buffer() {
        let text = "中文abc";
        let cursor = TextCursor::select_all(text.chars().count());
        assert_eq!(selected_text(text, cursor).as_deref(), Some("中文abc"));
    }

    #[test]
    fn char_and_byte_offsets_round_trip() {
        let text = "中文abc";
        assert_eq!(byte_offset(text, 2), 6);
        assert_eq!(char_index(text, 6), 2);
        assert_eq!(byte_offset(text, 99), text.len());
    }
}
