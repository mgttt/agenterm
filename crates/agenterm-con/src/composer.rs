//! Pure single-line composer editing rules.
//!
//! Clipboard I/O stays in the host. This module only owns deterministic text
//! and selection changes so the behavior can later converge with the shared
//! frontend without importing terminal or window authority.

pub const PASTE_LIMIT_BYTES: usize = 64 * 1024;
const COMPOSER_LIMIT_BYTES: usize = PASTE_LIMIT_BYTES;

/// Complete state of the external single-line input surface.
///
/// Host adapters may inspect these fields for rendering and routing, while
/// transitions that must clear several fields stay atomic here.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ComposerState {
    pub text: String,
    pub preedit: String,
    pub focused: bool,
    pub select_all: bool,
    pub submit_error: Option<String>,
}

impl ComposerState {
    pub fn cancel_focus(&mut self) {
        self.focused = false;
        self.preedit.clear();
        self.select_all = false;
        self.submit_error = None;
    }

    /// Returns one terminal submission and resets all transient edit state.
    pub fn take_submission(&mut self) -> Option<String> {
        let mut submission = (!self.text.is_empty()).then(|| std::mem::take(&mut self.text));
        if let Some(text) = submission.as_mut() {
            text.push('\r');
        }
        self.text.clear();
        self.preedit.clear();
        self.select_all = false;
        self.submit_error = None;
        submission
    }

    pub fn restore_failed_submission(&mut self, mut submission: String, error: String) {
        if submission.ends_with('\r') {
            submission.pop();
        }
        self.text = submission;
        self.preedit.clear();
        self.select_all = false;
        self.focused = true;
        self.submit_error = Some(error);
    }
}

/// Where the painted window into a single-line composer begins.
///
/// The composer stays one line on purpose: its content is submitted to a
/// shell, where a newline means "run", so wrapping would misrepresent what
/// pressing Enter does. What is *not* on purpose is painting from the head
/// and clipping the tail, which is what the chrome painter does with any
/// string too wide for its box. Past that width the composer showed stale
/// leading text while every new character — and the caret with them — landed
/// outside the box. That is typing into a surface that cannot show what you
/// typed.
///
/// Every edit happens at the end (there is no caret position to move), so the
/// end is the only span worth showing. These offsets slide the window to keep
/// it there.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VisibleWindow {
    /// Byte offset into the committed text where painting starts.
    pub text: usize,
    /// Byte offset into the IME preedit where painting starts. Nonzero only
    /// when the preedit alone already fills the box.
    pub preedit: usize,
    /// Whether anything was scrolled out of view, so the painter can say so
    /// rather than letting the text appear to begin where it does not.
    pub truncated: bool,
}

/// The painter's own width rule, so the measurement here and the advance
/// there cannot disagree: a double-width character owns two cells.
fn character_cells(character: char) -> usize {
    if unicode_width::UnicodeWidthChar::width(character).unwrap_or(1) > 1 {
        2
    } else {
        1
    }
}

/// Total painted width of `text`, in cells.
pub fn cells(text: &str) -> usize {
    text.chars().map(character_cells).sum()
}

/// Walks backwards from the end of `text`, returning the byte offset of the
/// first character that still fits in `budget` and charging what it consumed.
fn tail_offset(text: &str, budget: &mut usize) -> usize {
    let mut offset = text.len();
    for (index, character) in text.char_indices().rev() {
        let cells = character_cells(character);
        if cells > *budget {
            break;
        }
        *budget -= cells;
        offset = index;
    }
    offset
}

/// `cells` is the box width in cells; `reserved` is what the caret and any
/// other fixed decoration already claim.
///
/// A truncated window is measured twice: the first pass discovers that
/// something was hidden, the second pays a cell for the marker that says so.
/// Reserving that cell unconditionally would shrink every short line for a
/// marker it will never show.
pub fn visible_window(text: &str, preedit: &str, reserved: usize, cells: usize) -> VisibleWindow {
    let measure = |extra: usize| {
        let mut budget = cells.saturating_sub(reserved).saturating_sub(extra);
        let preedit_offset = tail_offset(preedit, &mut budget);
        // A preedit that did not fit whole leaves no room for any committed
        // text, so the window starts past its end rather than mid-string.
        let text_offset = if preedit_offset > 0 {
            text.len()
        } else {
            tail_offset(text, &mut budget)
        };
        VisibleWindow {
            text: text_offset,
            preedit: preedit_offset,
            truncated: text_offset > 0 || preedit_offset > 0,
        }
    };
    let window = measure(0);
    if window.truncated { measure(1) } else { window }
}

pub fn select_all(buffer: &str, selected: &mut bool) {
    *selected = !buffer.is_empty();
}

pub fn insert(buffer: &mut String, selected: &mut bool, text: &str) {
    prepare_edit(buffer, selected);
    let remaining = COMPOSER_LIMIT_BYTES.saturating_sub(buffer.len());
    let mut end = remaining.min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    buffer.push_str(&text[..end]);
}

pub fn backspace(buffer: &mut String, selected: &mut bool) {
    if !prepare_edit(buffer, selected) {
        buffer.pop();
    }
}

pub fn selected_text<'a>(buffer: &'a str, selected: &bool) -> Option<&'a str> {
    (*selected && !buffer.is_empty()).then_some(buffer)
}

pub fn cut(buffer: &mut String, selected: &mut bool) -> Option<String> {
    if *selected && !buffer.is_empty() {
        *selected = false;
        Some(std::mem::take(buffer))
    } else {
        None
    }
}

pub fn paste(buffer: &mut String, selected: &mut bool, text: &str) {
    let normalized = normalize_single_line(text);
    if !normalized.is_empty() {
        insert(buffer, selected, &normalized);
    }
}

/// Replace the buffer with AT-SPI `SetTextContents` / computed `InsertText`
/// result. Keeps the composer single-line so a bus write cannot inject
/// newlines into the painted input.
pub fn replace_text(buffer: &mut String, selected: &mut bool, text: &str) {
    *selected = false;
    *buffer = normalize_single_line(text);
}

fn prepare_edit(buffer: &mut String, selected: &mut bool) -> bool {
    if *selected {
        buffer.clear();
        *selected = false;
        true
    } else {
        false
    }
}

fn normalize_single_line(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len().min(PASTE_LIMIT_BYTES));
    let mut previous_was_space = false;
    for character in text.chars() {
        if normalized.len().saturating_add(character.len_utf8()) > PASTE_LIMIT_BYTES {
            break;
        }
        let character = match character {
            '\r' | '\n' | '\t' => ' ',
            value if value.is_control() => continue,
            value => value,
        };
        if character == ' ' && previous_was_space {
            continue;
        }
        previous_was_space = character == ' ';
        normalized.push(character);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect this exists for: a line that outgrows its box must keep
    /// showing its END, because the end is where every edit lands.
    #[test]
    fn a_line_wider_than_the_box_shows_its_end_not_its_beginning() {
        let text = "0123456789abcdefghij";
        // 10 cells of box, 1 reserved for the caret, 1 for the marker.
        let window = visible_window(text, "", 1, 10);
        assert!(window.truncated);
        assert_eq!(&text[window.text..], "cdefghij");
        assert_eq!(cells(&text[window.text..]), 8);
    }

    #[test]
    fn a_line_that_fits_is_shown_whole_and_unmarked() {
        let window = visible_window("echo ok", "", 1, 40);
        assert_eq!(window, VisibleWindow::default());
        assert!(!window.truncated);
    }

    /// The box is measured in cells, not characters: a CJK line that fits by
    /// character count would overflow by half its width again.
    #[test]
    fn double_width_characters_are_charged_two_cells() {
        let text = "中文中文中";
        assert_eq!(cells(text), 10);
        let window = visible_window(text, "", 1, 8);
        assert!(window.truncated);
        // 8 cells minus caret minus marker leaves 6, so three wide characters.
        assert_eq!(&text[window.text..], "文中文中".get(3..).unwrap_or_default());
        assert_eq!(cells(&text[window.text..]), 6);
        // Never split a character: every offset must be a char boundary.
        assert!(text.is_char_boundary(window.text));
    }

    /// The preedit is the tail, so it wins the space. A preedit long enough
    /// to fill the box on its own must hide the committed text entirely
    /// rather than paint a window starting inside it.
    #[test]
    fn a_preedit_filling_the_box_hides_the_committed_text_completely() {
        let text = "committed";
        let preedit = "中文中文中文";
        let window = visible_window(text, preedit, 1, 6);
        assert_eq!(window.text, text.len(), "no committed text remains visible");
        assert!(window.preedit > 0, "the preedit itself is windowed");
        assert!(preedit.is_char_boundary(window.preedit));
        assert!(cells(&preedit[window.preedit..]) <= 4);
    }

    /// A box too small for anything must still produce usable offsets rather
    /// than panic or point past the end of a string.
    #[test]
    fn a_box_with_no_room_yields_valid_empty_offsets() {
        let text = "text";
        let preedit = "pre";
        for width in 0..3 {
            let window = visible_window(text, preedit, 1, width);
            assert!(text.is_char_boundary(window.text));
            assert!(preedit.is_char_boundary(window.preedit));
            assert!(window.text <= text.len());
            assert!(window.preedit <= preedit.len());
        }
    }

    /// The visible span must never exceed the box, or the painter clips it
    /// again and the caret leaves the box exactly as before.
    #[test]
    fn the_visible_span_always_fits_the_box_it_was_measured_for() {
        let text = "a".repeat(500);
        let preedit = "中".repeat(20);
        for width in 3..40 {
            let window = visible_window(&text, &preedit, 1, width);
            let painted = usize::from(window.truncated)
                + cells(&text[window.text..])
                + cells(&preedit[window.preedit..])
                + 1; // the caret
            assert!(
                painted <= width,
                "width {width} painted {painted} cells: the box would clip again"
            );
        }
    }

    #[test]
    fn selected_input_replaces_the_whole_buffer() {
        let mut buffer = "old".to_owned();
        let mut selected = true;
        insert(&mut buffer, &mut selected, "new");
        assert_eq!(buffer, "new");
        assert!(!selected);
    }

    #[test]
    fn all_insert_paths_bound_total_bytes_at_a_utf8_boundary() {
        let mut buffer = "a".repeat(COMPOSER_LIMIT_BYTES - 2);
        let mut selected = false;
        insert(&mut buffer, &mut selected, "中文");
        assert_eq!(buffer.len(), COMPOSER_LIMIT_BYTES - 2);

        insert(&mut buffer, &mut selected, "bcdef");
        assert_eq!(buffer.len(), COMPOSER_LIMIT_BYTES);
        assert!(buffer.ends_with("bc"));

        paste(&mut buffer, &mut selected, "ignored");
        assert_eq!(buffer.len(), COMPOSER_LIMIT_BYTES);

        selected = true;
        insert(
            &mut buffer,
            &mut selected,
            &"中".repeat(COMPOSER_LIMIT_BYTES),
        );
        assert!(buffer.len() <= COMPOSER_LIMIT_BYTES);
        assert_eq!(buffer.len() % '中'.len_utf8(), 0);
    }

    #[test]
    fn backspace_deletes_selection_or_last_character() {
        let mut buffer = "中文A".to_owned();
        let mut selected = false;
        backspace(&mut buffer, &mut selected);
        assert_eq!(buffer, "中文");
        select_all(&buffer, &mut selected);
        backspace(&mut buffer, &mut selected);
        assert!(buffer.is_empty());
    }

    #[test]
    fn copy_and_cut_require_a_selection() {
        let mut buffer = "copy me".to_owned();
        let mut selected = false;
        assert_eq!(selected_text(&buffer, &selected), None);
        select_all(&buffer, &mut selected);
        assert_eq!(selected_text(&buffer, &selected), Some("copy me"));
        assert_eq!(cut(&mut buffer, &mut selected), Some("copy me".to_owned()));
        assert!(buffer.is_empty());
    }

    #[test]
    fn paste_is_single_line_bounded_and_filters_controls() {
        let mut buffer = "replace".to_owned();
        let mut selected = true;
        paste(&mut buffer, &mut selected, "a\r\nb\t\u{1b}c");
        assert_eq!(buffer, "a b c");
        assert!(!selected);
    }

    #[test]
    fn replace_text_is_single_line_and_clears_selection() {
        let mut buffer = "old".to_owned();
        let mut selected = true;
        replace_text(&mut buffer, &mut selected, "cu33o\nmarker");
        assert_eq!(buffer, "cu33o marker");
        assert!(!selected);
    }

    #[test]
    fn cancel_and_submit_clear_transient_state_atomically() {
        let mut state = ComposerState {
            text: "echo ok".to_owned(),
            preedit: "中".to_owned(),
            focused: true,
            select_all: true,
            submit_error: Some("old failure".to_owned()),
        };
        assert_eq!(state.take_submission().as_deref(), Some("echo ok\r"));
        assert_eq!(state.text, "");
        assert_eq!(state.preedit, "");
        assert!(!state.select_all);
        assert_eq!(state.submit_error, None);
        assert!(state.focused);

        state.restore_failed_submission("retry\r".to_owned(), "PTY is closed".to_owned());
        assert_eq!(state.text, "retry");
        assert_eq!(state.submit_error.as_deref(), Some("PTY is closed"));
        assert!(state.focused);

        state.preedit = "文".to_owned();
        state.select_all = true;
        state.cancel_focus();
        assert!(!state.focused);
        assert_eq!(state.preedit, "");
        assert!(!state.select_all);
        assert_eq!(state.submit_error, None);
    }
}
