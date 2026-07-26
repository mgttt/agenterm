#[derive(Clone, Copy, Debug)]
pub(crate) struct StatusWindow {
    pub(crate) index: u32,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) active: bool,
}

pub(crate) fn parse_status_windows(line: &str) -> Vec<StatusWindow> {
    let bytes = line.as_bytes();
    let mut windows = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        let bracketed = bytes[position] == b'[';
        let digit_start = position + usize::from(bracketed);
        if digit_start >= bytes.len() || !bytes[digit_start].is_ascii_digit() {
            position += 1;
            continue;
        }
        let mut colon = digit_start;
        while colon < bytes.len() && bytes[colon].is_ascii_digit() {
            colon += 1;
        }
        if colon >= bytes.len() || bytes[colon] != b':' {
            position = colon.max(position + 1);
            continue;
        }
        let name_start = colon + 1;
        let mut end = name_start;
        while end < bytes.len() && !bytes[end].is_ascii_whitespace() && bytes[end] != b']' {
            end += 1;
        }
        if end == name_start
            || bytes[name_start..end]
                .iter()
                .all(|byte| byte.is_ascii_digit())
        {
            position = end;
            continue;
        }
        if bracketed && end < bytes.len() && bytes[end] == b']' {
            end += 1;
        }
        if let Ok(index) = line[digit_start..colon].parse::<u32>() {
            windows.push(StatusWindow {
                index,
                start: position,
                end,
                active: bracketed,
            });
        }
        position = end;
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rmux_status_windows_and_active_marker() {
        let windows = parse_status_windows(" 0:cmd [1:cmd.exe] 2:logs ");
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].index, 0);
        assert!(!windows[0].active);
        assert_eq!(windows[1].index, 1);
        assert!(windows[1].active);
        assert_eq!(windows[2].index, 2);
        assert!(!windows[2].active);
    }

    #[test]
    fn ignores_numbers_that_are_not_window_labels() {
        let windows = parse_status_windows("cpu 41% 52G/249G Sun 2026-07-26 22:21");
        assert!(windows.is_empty());
    }

    #[test]
    fn records_clickable_utf8_byte_ranges() {
        let line = " [12:构建] 13:logs ";
        let windows = parse_status_windows(line);
        assert_eq!(&line[windows[0].start..windows[0].end], "[12:构建]");
        assert_eq!(&line[windows[1].start..windows[1].end], "13:logs");
    }
}
