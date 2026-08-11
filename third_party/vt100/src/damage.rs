//! Conservative, allocation-free damage information for a terminal screen.

/// A conservative half-open range of visible terminal rows.
///
/// The range may contain unchanged rows when two disjoint mutations are
/// merged. It must never omit a row that may have changed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RowRange {
    first: u32,
    end: u32,
}

impl RowRange {
    /// Returns an empty range.
    pub const fn empty() -> Self {
        Self { first: 0, end: 0 }
    }

    /// Returns whether this range contains no rows.
    pub const fn is_empty(self) -> bool {
        self.first >= self.end
    }

    /// Returns the first row in the range.
    pub const fn first(self) -> u32 {
        self.first
    }

    /// Returns the exclusive end row in the range.
    pub const fn end(self) -> u32 {
        self.end
    }

    /// Returns whether a row is covered by this range.
    pub fn contains(self, row: u16) -> bool {
        let row = u32::from(row);
        row >= self.first && row < self.end
    }

    /// Adds one row to this conservative range.
    pub fn mark_row(&mut self, row: u16) {
        self.mark_range(row, u32::from(row) + 1);
    }

    /// Adds `[first, end)` to this conservative range.
    pub fn mark_range(&mut self, first: u16, end: u32) {
        let first = u32::from(first);
        if end <= first {
            return;
        }
        if self.is_empty() {
            self.first = first;
            self.end = end;
        } else {
            self.first = self.first.min(first);
            self.end = self.end.max(end);
        }
    }

    /// Merges another range into this range.
    pub fn union(&mut self, other: Self) {
        if other.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = other;
        } else {
            self.first = self.first.min(other.first);
            self.end = self.end.max(other.end);
        }
    }

    /// Clips this range to a row count.
    pub fn clip(self, row_count: u16) -> Self {
        let end = self.end.min(u32::from(row_count));
        if self.first >= end {
            Self::empty()
        } else {
            Self {
                first: self.first,
                end,
            }
        }
    }
}

/// Damage accumulated by one terminal screen.
///
/// `rows` is a conservative row range. `full` means that no finite row range
/// can prove the visible cell result is sufficient. `viewport_changed` means
/// that the visible-row mapping changed, and therefore also requires a full
/// terminal raster. `cursor_changed` covers cursor position, shape, blink, or
/// visibility. `cursor_before` and `cursor_after` let an overlay consumer
/// repaint both endpoints without keeping a second terminal snapshot.
/// `model_changed` is true for non-pixel state changes such as SGR or
/// input-mode changes, so callers can still produce a fresh snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScreenDamage {
    rows: RowRange,
    full: bool,
    cursor_changed: bool,
    cursor_before: Option<(u16, u16)>,
    cursor_after: Option<(u16, u16)>,
    viewport_changed: bool,
    model_changed: bool,
}

impl ScreenDamage {
    /// Returns an empty damage value.
    pub const fn empty() -> Self {
        Self {
            rows: RowRange::empty(),
            full: false,
            cursor_changed: false,
            cursor_before: None,
            cursor_after: None,
            viewport_changed: false,
            model_changed: false,
        }
    }

    /// Returns whether no screen state or visible pixel may have changed.
    pub const fn is_empty(self) -> bool {
        !self.full
            && !self.cursor_changed
            && !self.viewport_changed
            && !self.model_changed
            && self.rows.is_empty()
    }

    /// Returns the conservative changed row range.
    pub const fn rows(self) -> RowRange {
        self.rows
    }

    /// Returns whether a full terminal raster is required.
    pub const fn needs_full_raster(self) -> bool {
        self.full || self.viewport_changed
    }

    /// Returns whether the cursor overlay may have changed.
    pub const fn cursor_changed(self) -> bool {
        self.cursor_changed
    }

    /// Returns the cursor position before the first cursor-affecting mutation.
    pub const fn cursor_before(self) -> Option<(u16, u16)> {
        self.cursor_before
    }

    /// Returns the cursor position after the accumulated mutations.
    pub const fn cursor_after(self) -> Option<(u16, u16)> {
        self.cursor_after
    }

    /// Returns whether visible-row mapping changed.
    pub const fn viewport_changed(self) -> bool {
        self.viewport_changed
    }

    /// Returns whether any terminal model state changed.
    pub const fn model_changed(self) -> bool {
        self.model_changed
    }

    pub(crate) fn mark_model(&mut self) {
        self.model_changed = true;
    }

    pub(crate) fn mark_row(&mut self, row: u16) {
        self.rows.mark_row(row);
        self.model_changed = true;
    }

    pub(crate) fn mark_rows(&mut self, rows: RowRange) {
        self.rows.union(rows);
        self.model_changed = true;
    }

    pub(crate) fn mark_cursor(&mut self, before: (u16, u16)) {
        self.cursor_changed = true;
        if self.cursor_before.is_none() {
            self.cursor_before = Some(before);
        }
        self.model_changed = true;
    }

    pub(crate) fn set_cursor_after(&mut self, after: (u16, u16)) {
        if self.cursor_changed {
            self.cursor_after = Some(after);
        }
    }

    pub(crate) fn mark_viewport(&mut self) {
        self.viewport_changed = true;
        self.model_changed = true;
    }

    pub(crate) fn mark_full(&mut self) {
        self.full = true;
        self.model_changed = true;
    }

    pub(crate) fn merge_grid(&mut self, damage: GridDamage) {
        if damage.full {
            self.mark_full();
        }
        if damage.viewport_changed {
            self.mark_viewport();
        }
        if !damage.rows.is_empty() {
            self.mark_rows(damage.rows);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GridDamage {
    pub(crate) rows: RowRange,
    pub(crate) full: bool,
    pub(crate) viewport_changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Cell, Color, CursorShape, MouseProtocolEncoding, MouseProtocolMode, Parser, Screen,
    };

    #[derive(Clone, Eq, PartialEq)]
    struct VisibleSnapshot {
        rows: u16,
        cols: u16,
        cells: Vec<Option<Cell>>,
        wrapped: Vec<bool>,
        cursor: (u16, u16),
        shape: CursorShape,
        blinking: bool,
        hidden: bool,
        alternate: bool,
        scrollback: usize,
        application_keypad: bool,
        application_cursor: bool,
        bracketed_paste: bool,
        mouse_mode: MouseProtocolMode,
        mouse_encoding: MouseProtocolEncoding,
        fg: Color,
        bg: Color,
        bold: bool,
        dim: bool,
        italic: bool,
        underline: bool,
        inverse: bool,
    }

    impl VisibleSnapshot {
        fn capture(screen: &Screen) -> Self {
            let (rows, cols) = screen.size();
            let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(cols));
            let mut wrapped = Vec::with_capacity(usize::from(rows));
            for row in 0..rows {
                wrapped.push(screen.row_wrapped(row));
                for col in 0..cols {
                    cells.push(screen.cell(row, col).cloned());
                }
            }
            Self {
                rows,
                cols,
                cells,
                wrapped,
                cursor: screen.cursor_position(),
                shape: screen.cursor_shape(),
                blinking: screen.cursor_blinking(),
                hidden: screen.hide_cursor(),
                alternate: screen.alternate_screen(),
                scrollback: screen.scrollback(),
                application_keypad: screen.application_keypad(),
                application_cursor: screen.application_cursor(),
                bracketed_paste: screen.bracketed_paste(),
                mouse_mode: screen.mouse_protocol_mode(),
                mouse_encoding: screen.mouse_protocol_encoding(),
                fg: screen.fgcolor(),
                bg: screen.bgcolor(),
                bold: screen.bold(),
                dim: screen.dim(),
                italic: screen.italic(),
                underline: screen.underline(),
                inverse: screen.inverse(),
            }
        }

        fn cell(&self, row: u16, col: u16) -> Option<&Cell> {
            self.cells[usize::from(row) * usize::from(self.cols) + usize::from(col)].as_ref()
        }

        fn changed_rows(&self, after: &Self) -> RowRange {
            if self.rows != after.rows || self.cols != after.cols {
                return RowRange::empty();
            }
            let mut rows = RowRange::empty();
            for row in 0..self.rows {
                let cells_changed =
                    (0..self.cols).any(|col| self.cell(row, col) != after.cell(row, col));
                if cells_changed
                    || self.wrapped[usize::from(row)] != after.wrapped[usize::from(row)]
                {
                    rows.mark_row(row);
                }
            }
            rows
        }

        fn cursor_changed(&self, after: &Self) -> bool {
            self.cursor != after.cursor
                || self.shape != after.shape
                || self.blinking != after.blinking
                || self.hidden != after.hidden
        }
    }

    fn assert_damage_covers(
        before: &VisibleSnapshot,
        after: &VisibleSnapshot,
        damage: ScreenDamage,
    ) {
        if before.rows != after.rows || before.cols != after.cols {
            assert!(damage.needs_full_raster());
        }
        if before.alternate != after.alternate {
            assert!(damage.needs_full_raster());
        }
        if before.scrollback != after.scrollback {
            assert!(damage.viewport_changed());
        }
        if before.cursor_changed(after) {
            assert!(damage.cursor_changed());
        }
        if damage.cursor_changed() {
            assert_eq!(damage.cursor_before(), Some(before.cursor));
            assert_eq!(damage.cursor_after(), Some(after.cursor));
        }
        if before != after {
            assert!(damage.model_changed());
        }
        if !damage.needs_full_raster() {
            let changed = before.changed_rows(after);
            let mut row = changed.first();
            while row < changed.end() {
                let row_u16 = u16::try_from(row).expect("test row fits u16");
                assert!(
                    damage.rows().contains(row_u16),
                    "row {row} was not reported"
                );
                row += 1;
            }
        }
    }

    fn process_and_assert(parser: &mut Parser, bytes: &[u8]) -> ScreenDamage {
        let before = VisibleSnapshot::capture(parser.screen());
        parser.process(bytes);
        let after = VisibleSnapshot::capture(parser.screen());
        let damage = parser.take_damage();
        assert_damage_covers(&before, &after, damage);
        damage
    }

    #[test]
    fn ascii_single_line_is_row_damage_with_cursor_endpoints() {
        let mut parser = Parser::new(3, 8, 0);
        let damage = process_and_assert(&mut parser, b"ASCII");

        assert!(!damage.needs_full_raster());
        assert_eq!(damage.rows(), RowRange { first: 0, end: 1 });
        assert_eq!(damage.cursor_before(), Some((0, 0)));
        assert_eq!(damage.cursor_after(), Some((0, 5)));
        assert!(parser.take_damage().is_empty());
    }

    #[test]
    fn cursor_motion_only_reports_cursor_damage() {
        let mut parser = Parser::new(3, 8, 0);
        process_and_assert(&mut parser, b"A");
        let damage = process_and_assert(&mut parser, b"\x1b[1;1H");

        assert!(damage.cursor_changed());
        assert_eq!(damage.cursor_before(), Some((0, 1)));
        assert_eq!(damage.cursor_after(), Some((0, 0)));
        assert!(damage.rows().is_empty());
        assert!(!damage.needs_full_raster());
        assert!(!damage.viewport_changed());
    }

    #[test]
    fn cjk_combining_and_wrap_cover_their_visual_rows_without_hashes() {
        let mut parser = Parser::new(4, 8, 0);
        let cjk = process_and_assert(&mut parser, "你".as_bytes());
        assert!(!cjk.needs_full_raster());
        assert!(cjk.rows().contains(0));

        let combining = process_and_assert(&mut parser, "e\u{301}".as_bytes());
        assert!(!combining.needs_full_raster());
        assert!(combining.rows().contains(0));

        let mut parser = Parser::new(4, 4, 0);
        let wrap = process_and_assert(&mut parser, b"abcdX");
        assert!(!wrap.needs_full_raster());
        assert!(wrap.rows().contains(0));
        assert!(wrap.rows().contains(1));
    }

    #[test]
    fn zero_and_one_column_wide_input_is_ignored_without_damage() {
        for cols in [0, 1] {
            let mut parser = Parser::new(2, cols, 0);
            process_and_assert(&mut parser, b"A");
            for glyph in ["你", "😀"] {
                for byte in glyph.as_bytes() {
                    let damage = process_and_assert(&mut parser, std::slice::from_ref(byte));
                    assert!(damage.is_empty(), "unrepresentable {glyph:?} changed damage");
                }
            }
        }
    }

    #[test]
    fn exact_visible_cell_oracle_covers_terminal_mutations() {
        let mut parser = Parser::new(4, 8, 16);
        process_and_assert(&mut parser, b"ASCII");
        process_and_assert(&mut parser, "你e\u{301}".as_bytes());
        process_and_assert(&mut parser, b"\r\nwrapwrap");
        process_and_assert(&mut parser, b"\x1b[2K\x1b[1;3H\x1b[@\x1b[P\x1b[X");
        process_and_assert(&mut parser, b"\x1b[2;1H\x1b[L\x1b[M");
        process_and_assert(&mut parser, b"\x1b[2S\x1b[2T\x1b[1;1H\x1bM");
        process_and_assert(&mut parser, b"\x1b[3;4H\x1b7\x1b[1;1H\x1b8");
        process_and_assert(&mut parser, b"\x1b[2;3r\x1b[?6h\x1b[1;1H\x1b[?6l");
        process_and_assert(&mut parser, b"\x1b[31;1mA\x1b[0m");
        process_and_assert(&mut parser, b"\x1b[?25l\x1b[6 q\x1b[1;1H\x1b[?25h");
        process_and_assert(&mut parser, b"\x1b[?1049hALT\x1b[?1049l");
        process_and_assert(&mut parser, b"\x1b[9J");
        process_and_assert(&mut parser, b"\x1b[?9999h");
    }

    #[test]
    fn direct_resize_and_scrollback_are_full_or_viewport_damage() {
        let mut parser = Parser::new(3, 6, 8);
        process_and_assert(&mut parser, b"one\r\ntwo\r\nthree\r\nfour");

        let before = VisibleSnapshot::capture(parser.screen());
        parser.screen_mut().set_scrollback(1);
        let after = VisibleSnapshot::capture(parser.screen());
        let damage = parser.take_damage();
        assert_damage_covers(&before, &after, damage);
        assert!(damage.viewport_changed());

        let before = VisibleSnapshot::capture(parser.screen());
        parser.screen_mut().set_size(5, 9);
        let after = VisibleSnapshot::capture(parser.screen());
        let damage = parser.take_damage();
        assert_damage_covers(&before, &after, damage);
        assert!(damage.needs_full_raster());

        let before = VisibleSnapshot::capture(parser.screen());
        parser.screen_mut().set_size(5, 9);
        let after = VisibleSnapshot::capture(parser.screen());
        let damage = parser.take_damage();
        assert_damage_covers(&before, &after, damage);
        assert!(damage.needs_full_raster());
        assert!(parser.take_damage().is_empty());
    }

    #[test]
    fn exact_oracle_covers_il_dl_inside_and_outside_scroll_region() {
        let mut parser = Parser::new(6, 8, 0);
        process_and_assert(&mut parser, b"r0\r\nr1\r\nr2\r\nr3\r\nr4\r\nr5");
        process_and_assert(&mut parser, b"\x1b[2;5r");

        process_and_assert(&mut parser, b"\x1b[1;1H");
        let damage = process_and_assert(&mut parser, b"\x1b[L");
        assert!(damage.is_empty(), "IL outside scroll region changed state");

        process_and_assert(&mut parser, b"\x1b[6;1H");
        let damage = process_and_assert(&mut parser, b"\x1b[M");
        assert!(damage.is_empty(), "DL outside scroll region changed state");

        process_and_assert(&mut parser, b"\x1b[3;1H");
        let damage = process_and_assert(&mut parser, b"\x1b[L");
        assert!(!damage.needs_full_raster());
        assert!(damage.rows().contains(2));
        assert!(damage.rows().contains(4));

        let damage = process_and_assert(&mut parser, b"\x1b[M");
        assert!(!damage.needs_full_raster());
        assert!(damage.rows().contains(2));
        assert!(damage.rows().contains(4));
    }

    #[test]
    fn scroll_alternate_and_inactive_grid_consumption_are_conservative() {
        let mut parser = Parser::new(2, 4, 8);
        let damage = process_and_assert(&mut parser, b"1\n2\n3");
        assert!(damage.needs_full_raster());

        let before = VisibleSnapshot::capture(parser.screen());
        parser.process(b"primary");
        parser.process(b"\x1b[?1049hALT");
        let after = VisibleSnapshot::capture(parser.screen());
        let damage = parser.take_damage();
        assert_damage_covers(&before, &after, damage);
        assert!(damage.needs_full_raster());

        let damage = process_and_assert(&mut parser, b"\x1b[?1049l");
        assert!(damage.needs_full_raster());
        assert!(parser.take_damage().is_empty());
    }

    #[test]
    fn unknown_and_explicit_callback_fallback_are_full_and_take_clears() {
        let mut parser = Parser::new(2, 4, 0);
        let damage = process_and_assert(&mut parser, b"\x1b[9J");
        assert!(damage.needs_full_raster());

        parser.screen_mut().mark_full_damage();
        let damage = parser.take_damage();
        assert!(damage.needs_full_raster());
        assert!(parser.take_damage().is_empty());
    }

    #[test]
    fn every_parser_byte_boundary_preserves_damage_coverage() {
        let mut parser = Parser::new(3, 10, 8);
        let bytes = b"\x1b[2J\x1b[1;1H\xe4\xbd\xa0e\xcc\x81\r\nnext";
        for byte in bytes {
            process_and_assert(&mut parser, std::slice::from_ref(byte));
        }
    }

    #[test]
    fn damage_take_is_monotonic_until_consumed() {
        let mut parser = Parser::new(2, 4, 0);
        parser.process(b"a");
        parser.process(b"b");
        let damage = parser.take_damage();
        assert!(damage.model_changed());
        assert!(damage.rows().contains(0));
        assert!(parser.take_damage().is_empty());
    }
}
