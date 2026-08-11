//! Pure single-line composer editing rules.
//!
//! Clipboard I/O stays in the host. This module only owns deterministic text
//! and selection changes so the behavior can later converge with the shared
//! frontend without importing terminal or window authority.

pub const PASTE_LIMIT_BYTES: usize = 64 * 1024;

pub fn select_all(buffer: &str, selected: &mut bool) {
    *selected = !buffer.is_empty();
}

pub fn insert(buffer: &mut String, selected: &mut bool, text: &str) {
    prepare_edit(buffer, selected);
    buffer.push_str(text);
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

    #[test]
    fn selected_input_replaces_the_whole_buffer() {
        let mut buffer = "old".to_owned();
        let mut selected = true;
        insert(&mut buffer, &mut selected, "new");
        assert_eq!(buffer, "new");
        assert!(!selected);
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
}
