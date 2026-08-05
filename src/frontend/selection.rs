// This module owns linear local-selection and shared remote selection gesture
// arbitration and rectangular selection are intentionally outside this slice.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TerminalPoint {
    pub(crate) row: u16,
    pub(crate) col: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalSelection {
    pub(crate) tab_id: u64,
    pub(crate) anchor: TerminalPoint,
    pub(crate) focus: TerminalPoint,
    pub(crate) dragging: bool,
    pub(crate) moved: bool,
}

impl TerminalSelection {
    pub(crate) fn bounds(self) -> (TerminalPoint, TerminalPoint) {
        normalize_endpoints(self.anchor, self.focus)
    }

    pub(crate) fn contains(self, row: u16, col: u16) -> bool {
        let (start, end) = self.bounds();
        TerminalPoint { row, col } >= start && TerminalPoint { row, col } <= end
    }

    pub(crate) fn is_empty(self) -> bool {
        self.anchor == self.focus
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionGesturePhase {
    Prepared,
    Dragging,
    Completed,
    Cancelled,
}

impl SelectionGesturePhase {
    #[allow(dead_code)]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Dragging => "dragging",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectionGestureState<TabId, Point> {
    tab_id: TabId,
    anchor: Point,
    focus: Point,
    phase: SelectionGesturePhase,
}

impl<TabId: Clone, Point: Copy + Eq + Ord> SelectionGestureState<TabId, Point> {
    pub(crate) fn begin(tab_id: TabId, anchor: Point) -> Self {
        Self {
            tab_id,
            anchor,
            focus: anchor,
            phase: SelectionGesturePhase::Prepared,
        }
    }
    pub(crate) fn completed_unchecked(tab_id: TabId, anchor: Point, focus: Point) -> Self {
        Self {
            tab_id,
            anchor,
            focus,
            phase: SelectionGesturePhase::Completed,
        }
    }

    pub(crate) const fn phase(&self) -> SelectionGesturePhase {
        self.phase
    }

    pub(crate) const fn active(&self) -> bool {
        matches!(
            self.phase,
            SelectionGesturePhase::Prepared | SelectionGesturePhase::Dragging
        )
    }

    pub(crate) fn bounds(&self) -> (Point, Point) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    pub(crate) fn drag_to(mut self, focus: Point) -> Self {
        if !self.active() {
            return self;
        }
        self.focus = focus;
        self.phase = if self.is_empty() {
            SelectionGesturePhase::Prepared
        } else {
            SelectionGesturePhase::Dragging
        };
        self
    }

    pub(crate) fn complete(mut self) -> Self {
        self.phase = match self.phase {
            SelectionGesturePhase::Prepared => SelectionGesturePhase::Cancelled,
            SelectionGesturePhase::Dragging => SelectionGesturePhase::Completed,
            phase => phase,
        };
        self
    }

    pub(crate) fn cancel(mut self) -> Self {
        self.phase = SelectionGesturePhase::Cancelled;
        self
    }
}

impl SelectionGestureState<u64, TerminalPoint> {
    pub(crate) fn prepare(
        tab_id: u64,
        anchor: TerminalPoint,
        rows: u16,
        cols: u16,
    ) -> Option<Self> {
        Some(Self {
            tab_id,
            anchor: clamp_point(anchor, rows, cols)?,
            focus: clamp_point(anchor, rows, cols)?,
            phase: SelectionGesturePhase::Prepared,
        })
    }

    pub(crate) fn drag_to_clamped(self, focus: TerminalPoint, rows: u16, cols: u16) -> Self {
        if !self.active() {
            return self;
        }
        let Some(focus) = clamp_point(focus, rows, cols) else {
            return self.cancel();
        };
        self.drag_to(focus)
    }

    pub(crate) fn completed(
        tab_id: u64,
        anchor: TerminalPoint,
        focus: TerminalPoint,
        rows: u16,
        cols: u16,
    ) -> Option<Self> {
        Some(Self {
            tab_id,
            anchor: clamp_point(anchor, rows, cols)?,
            focus: clamp_point(focus, rows, cols)?,
            phase: SelectionGesturePhase::Completed,
        })
    }

    pub(crate) fn completed_selection(&self) -> Option<TerminalSelection> {
        (self.phase == SelectionGesturePhase::Completed).then_some(TerminalSelection {
            tab_id: self.tab_id,
            anchor: self.anchor,
            focus: self.focus,
            dragging: false,
            moved: true,
        })
    }

    pub(crate) fn selection(&self) -> Option<TerminalSelection> {
        (self.phase != SelectionGesturePhase::Cancelled).then_some(TerminalSelection {
            tab_id: self.tab_id,
            anchor: self.anchor,
            focus: self.focus,
            dragging: self.active(),
            moved: self.anchor != self.focus,
        })
    }

    pub(crate) const fn tab_id(&self) -> u64 {
        self.tab_id
    }
}

pub(crate) type SelectionGesture = SelectionGestureState<u64, TerminalPoint>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RemotePoint {
    pub(crate) row: u32,
    pub(crate) column: u32,
}

pub(crate) type RemoteSelectionGesture = SelectionGestureState<String, RemotePoint>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AutoScrollDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AutoScrollStep {
    pub(crate) direction: AutoScrollDirection,
    pub(crate) rows: usize,
}

const MAX_AUTOSCROLL_ROWS_PER_TICK: usize = 8;

pub(crate) fn autoscroll_step(
    pointer_y: i32,
    viewport_top: i32,
    viewport_bottom: i32,
    cell_height: i32,
) -> Option<AutoScrollStep> {
    if viewport_bottom <= viewport_top || cell_height <= 0 {
        return None;
    }
    let (direction, distance) = if pointer_y < viewport_top {
        (
            AutoScrollDirection::Up,
            viewport_top.saturating_sub(pointer_y),
        )
    } else if pointer_y >= viewport_bottom {
        (
            AutoScrollDirection::Down,
            pointer_y.saturating_sub(viewport_bottom).saturating_add(1),
        )
    } else {
        return None;
    };
    let rows = usize::try_from(
        distance
            .saturating_add(cell_height - 1)
            .saturating_div(cell_height),
    )
    .unwrap_or(MAX_AUTOSCROLL_ROWS_PER_TICK)
    .clamp(1, MAX_AUTOSCROLL_ROWS_PER_TICK);
    Some(AutoScrollStep { direction, rows })
}

pub(crate) const fn normalize_endpoints(
    first: TerminalPoint,
    second: TerminalPoint,
) -> (TerminalPoint, TerminalPoint) {
    if first.row < second.row || (first.row == second.row && first.col <= second.col) {
        (first, second)
    } else {
        (second, first)
    }
}

pub(crate) fn clamp_point(point: TerminalPoint, rows: u16, cols: u16) -> Option<TerminalPoint> {
    if rows == 0 || cols == 0 {
        return None;
    }
    Some(TerminalPoint {
        row: point.row.min(rows - 1),
        col: point.col.min(cols - 1),
    })
}

trait TerminalCellSource {
    fn rows(&self) -> u32;
    fn cols(&self) -> u32;
    fn cell(&self, row: u32, col: u32) -> Option<(&str, bool)>;
}

impl TerminalCellSource for &vt100::Screen {
    fn rows(&self) -> u32 {
        u32::from(self.size().0)
    }

    fn cols(&self) -> u32 {
        u32::from(self.size().1)
    }

    fn cell(&self, row: u32, col: u32) -> Option<(&str, bool)> {
        let row = u16::try_from(row).ok()?;
        let col = u16::try_from(col).ok()?;
        let cell = (*self).cell(row, col)?;
        Some((cell.contents(), cell.is_wide_continuation()))
    }
}

impl TerminalCellSource for &[Vec<Option<String>>] {
    fn rows(&self) -> u32 {
        u32::try_from(self.len()).unwrap_or_default()
    }

    fn cols(&self) -> u32 {
        self.first()
            .map_or(0, |row| u32::try_from(row.len()).unwrap_or_default())
    }

    fn cell(&self, row: u32, col: u32) -> Option<(&str, bool)> {
        let text = self
            .get(usize::try_from(row).ok()?)?
            .get(usize::try_from(col).ok()?)?
            .as_deref()?;
        Some((text, text.is_empty()))
    }
}

pub(crate) fn word_selection(
    screen: &vt100::Screen,
    point: TerminalPoint,
) -> Option<(TerminalPoint, TerminalPoint)> {
    let (start, end) =
        word_selection_bounds(&screen, (u32::from(point.row), u32::from(point.col)))?;
    Some((
        TerminalPoint {
            row: u16::try_from(start.0).ok()?,
            col: u16::try_from(start.1).ok()?,
        },
        TerminalPoint {
            row: u16::try_from(end.0).ok()?,
            col: u16::try_from(end.1).ok()?,
        },
    ))
}

pub(crate) fn remote_word_selection(
    cells: &[Vec<Option<String>>],
    point: RemotePoint,
) -> Option<(RemotePoint, RemotePoint)> {
    let (start, end) = word_selection_bounds(&cells, (point.row, point.column))?;
    Some((
        RemotePoint {
            row: start.0,
            column: start.1,
        },
        RemotePoint {
            row: end.0,
            column: end.1,
        },
    ))
}

fn word_selection_bounds(
    source: &dyn TerminalCellSource,
    point: (u32, u32),
) -> Option<((u32, u32), (u32, u32))> {
    let rows = source.rows();
    let cols = source.cols();
    if rows == 0 || cols == 0 {
        return None;
    }
    let row = point.0.min(rows - 1);
    let mut clicked_col = point.1.min(cols - 1);
    while clicked_col > 0
        && source
            .cell(row, clicked_col)
            .is_some_and(|(_, continuation)| continuation)
    {
        clicked_col -= 1;
    }

    let clicked_class = cell_word_class(source, row, clicked_col);
    let mut start = clicked_col;
    while start > 0 {
        let previous = previous_cell_start(source, row, start);
        if cell_word_class(source, row, previous) != clicked_class {
            break;
        }
        start = previous;
    }

    let mut end_start = clicked_col;
    while let Some(next) = next_cell_start(source, row, end_start, cols) {
        if cell_word_class(source, row, next) != clicked_class {
            break;
        }
        end_start = next;
    }
    let end = cell_end(source, row, end_start, cols);
    Some(((row, start), (row, end)))
}

pub(crate) fn visible_row_selection(
    screen: &vt100::Screen,
    row: u16,
) -> Option<(TerminalPoint, TerminalPoint)> {
    let (rows, cols) = screen.size();
    let point = clamp_point(TerminalPoint { row, col: 0 }, rows, cols)?;
    Some((
        point,
        TerminalPoint {
            row: point.row,
            col: cols - 1,
        },
    ))
}

pub(crate) fn remote_visible_row_selection(
    rows: u32,
    cols: u32,
    row: u32,
) -> Option<(RemotePoint, RemotePoint)> {
    if rows == 0 || cols == 0 || row >= rows {
        return None;
    }
    Some((
        RemotePoint { row, column: 0 },
        RemotePoint {
            row,
            column: cols - 1,
        },
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellWordClass {
    Whitespace,
    Word,
    Punctuation(char),
}

fn cell_word_class(source: &dyn TerminalCellSource, row: u32, col: u32) -> CellWordClass {
    let contents = source
        .cell(row, col)
        .map(|(contents, _)| contents)
        .unwrap_or_default();
    let Some(first) = contents.chars().next() else {
        return CellWordClass::Whitespace;
    };
    if contents.chars().all(char::is_whitespace) {
        CellWordClass::Whitespace
    } else if contents.chars().any(is_terminal_word_character) {
        CellWordClass::Word
    } else {
        CellWordClass::Punctuation(first)
    }
}

fn is_terminal_word_character(character: char) -> bool {
    character.is_alphanumeric()
        || matches!(character, '_' | '-' | '.' | '/' | '\\' | ':' | '@' | '~')
}

/// Walks left from `col` to the start of its continuation-cell run.
///
/// `col` must be greater than zero: the only call site is already gated by
/// `start > 0`, and `col - 1` below relies on that to avoid underflowing.
fn previous_cell_start(source: &dyn TerminalCellSource, row: u32, col: u32) -> u32 {
    debug_assert!(col > 0, "previous_cell_start requires col > 0");
    let mut previous = col - 1;
    while previous > 0
        && source
            .cell(row, previous)
            .is_some_and(|(_, continuation)| continuation)
    {
        previous -= 1;
    }
    previous
}

fn next_cell_start(source: &dyn TerminalCellSource, row: u32, col: u32, cols: u32) -> Option<u32> {
    let next = cell_end(source, row, col, cols).saturating_add(1);
    (next < cols).then_some(next)
}

fn cell_end(source: &dyn TerminalCellSource, row: u32, col: u32, cols: u32) -> u32 {
    if col + 1 < cols
        && source
            .cell(row, col + 1)
            .is_some_and(|(_, continuation)| continuation)
    {
        col + 1
    } else {
        col
    }
}

pub(crate) fn terminal_selection_text(
    screen: &vt100::Screen,
    selection: TerminalSelection,
) -> String {
    let (rows, cols) = screen.size();
    if rows == 0 || cols == 0 {
        return String::new();
    }
    let (mut start, mut end) = selection.bounds();
    start.row = start.row.min(rows - 1);
    start.col = start.col.min(cols - 1);
    end.row = end.row.min(rows - 1);
    end.col = end.col.min(cols - 1);

    let mut selected = String::new();
    for row in start.row..=end.row {
        let first_col = if row == start.row { start.col } else { 0 };
        let last_col = if row == end.row { end.col } else { cols - 1 };
        let mut line = String::new();
        for col in first_col..=last_col {
            match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if !cell.contents().is_empty() => line.push_str(cell.contents()),
                _ => line.push(' '),
            }
        }
        selected.push_str(line.trim_end_matches(' '));
        if row != end.row {
            selected.push_str("\r\n");
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_empty_when_anchor_matches_focus() {
        let selection = TerminalSelection {
            tab_id: 1,
            anchor: TerminalPoint { row: 2, col: 3 },
            focus: TerminalPoint { row: 2, col: 3 },
            dragging: false,
            moved: false,
        };
        assert!(selection.is_empty());
    }

    #[test]
    fn gesture_transitions_are_explicit_and_terminal() {
        let anchor = TerminalPoint { row: 1, col: 2 };
        let prepared = SelectionGesture::prepare(7, anchor, 4, 8).unwrap();
        assert_eq!(prepared.phase(), SelectionGesturePhase::Prepared);

        let dragging = prepared.drag_to_clamped(TerminalPoint { row: 2, col: 5 }, 4, 8);
        assert_eq!(dragging.phase(), SelectionGesturePhase::Dragging);
        let completed = dragging.complete();
        assert_eq!(completed.phase(), SelectionGesturePhase::Completed);
        assert_eq!(
            completed.completed_selection(),
            Some(TerminalSelection {
                tab_id: 7,
                anchor,
                focus: TerminalPoint { row: 2, col: 5 },
                dragging: false,
                moved: true,
            })
        );
        assert_eq!(
            completed
                .drag_to_clamped(TerminalPoint { row: 0, col: 0 }, 4, 8)
                .phase(),
            SelectionGesturePhase::Completed
        );

        let cancelled = SelectionGesture::prepare(7, anchor, 4, 8).unwrap().cancel();
        assert_eq!(cancelled.phase(), SelectionGesturePhase::Cancelled);
        assert_eq!(cancelled.completed_selection(), None);
        assert_eq!(
            SelectionGesture::prepare(7, anchor, 4, 8)
                .unwrap()
                .complete()
                .phase(),
            SelectionGesturePhase::Cancelled
        );
    }

    #[test]
    fn endpoints_normalize_and_points_clamp_to_nonempty_grids() {
        let earlier = TerminalPoint { row: 1, col: 7 };
        let later = TerminalPoint { row: 2, col: 1 };
        assert_eq!(normalize_endpoints(later, earlier), (earlier, later));
        assert_eq!(
            clamp_point(TerminalPoint { row: 99, col: 99 }, 3, 8),
            Some(TerminalPoint { row: 2, col: 7 })
        );
        assert_eq!(clamp_point(earlier, 0, 8), None);
        assert_eq!(clamp_point(earlier, 3, 0), None);
        assert_eq!(SelectionGesture::prepare(1, earlier, 0, 8), None);
    }

    #[test]
    fn word_selection_uses_terminal_cells_and_wide_character_spans() {
        let mut parser = vt100::Parser::new(2, 24, 0);
        parser.process("alpha.rs 你好 ok!".as_bytes());
        assert_eq!(
            word_selection(parser.screen(), TerminalPoint { row: 0, col: 3 }),
            Some((
                TerminalPoint { row: 0, col: 0 },
                TerminalPoint { row: 0, col: 7 },
            ))
        );
        assert_eq!(
            word_selection(parser.screen(), TerminalPoint { row: 0, col: 10 }),
            Some((
                TerminalPoint { row: 0, col: 9 },
                TerminalPoint { row: 0, col: 12 },
            ))
        );
        assert_eq!(
            word_selection(parser.screen(), TerminalPoint { row: 0, col: 16 }),
            Some((
                TerminalPoint { row: 0, col: 16 },
                TerminalPoint { row: 0, col: 16 },
            ))
        );
    }

    #[test]
    fn remote_word_selection_uses_snapshot_cell_grid_and_wide_spans() {
        let mut cells = vec![vec![None; 24]; 1];
        for (column, text) in [
            "a", "l", "p", "h", "a", ".", "r", "s", " ", "你", "", "好", "", " ", "o", "k", "!",
        ]
        .into_iter()
        .enumerate()
        {
            cells[0][column] = Some(text.to_owned());
        }
        assert_eq!(
            remote_word_selection(&cells, RemotePoint { row: 0, column: 3 }),
            Some((
                RemotePoint { row: 0, column: 0 },
                RemotePoint { row: 0, column: 7 },
            ))
        );
        assert_eq!(
            remote_word_selection(&cells, RemotePoint { row: 0, column: 10 }),
            Some((
                RemotePoint { row: 0, column: 9 },
                RemotePoint { row: 0, column: 12 },
            ))
        );
        assert_eq!(
            remote_word_selection(&cells, RemotePoint { row: 0, column: 16 }),
            Some((
                RemotePoint { row: 0, column: 16 },
                RemotePoint { row: 0, column: 16 },
            ))
        );
        assert_eq!(
            remote_visible_row_selection(3, 8, 1),
            Some((
                RemotePoint { row: 1, column: 0 },
                RemotePoint { row: 1, column: 7 },
            ))
        );
        assert_eq!(remote_visible_row_selection(3, 8, 99), None);
    }

    #[test]
    fn remote_gesture_can_complete_without_a_drag() {
        let start = RemotePoint { row: 2, column: 3 };
        let end = RemotePoint { row: 2, column: 9 };
        let gesture = RemoteSelectionGesture::completed_unchecked("tab".to_owned(), start, end);
        assert_eq!(gesture.phase(), SelectionGesturePhase::Completed);
        assert_eq!(gesture.bounds(), (start, end));
    }

    #[test]
    fn visible_rows_and_autoscroll_are_bounded() {
        let parser = vt100::Parser::new(3, 8, 0);
        assert_eq!(
            visible_row_selection(parser.screen(), 99),
            Some((
                TerminalPoint { row: 2, col: 0 },
                TerminalPoint { row: 2, col: 7 },
            ))
        );
        assert_eq!(
            autoscroll_step(9, 10, 90, 10),
            Some(AutoScrollStep {
                direction: AutoScrollDirection::Up,
                rows: 1,
            })
        );
        assert_eq!(autoscroll_step(50, 10, 90, 10), None);
        assert_eq!(
            autoscroll_step(109, 10, 90, 10),
            Some(AutoScrollStep {
                direction: AutoScrollDirection::Down,
                rows: 2,
            })
        );
        assert_eq!(
            autoscroll_step(i32::MAX, 10, 90, 1),
            Some(AutoScrollStep {
                direction: AutoScrollDirection::Down,
                rows: MAX_AUTOSCROLL_ROWS_PER_TICK,
            })
        );
        assert_eq!(autoscroll_step(0, 10, 10, 10), None);
        assert_eq!(autoscroll_step(0, 10, 90, 0), None);
    }

    #[test]
    fn terminal_selection_extracts_forward_reverse_and_wide_text() {
        let mut parser = vt100::Parser::new(3, 8, 0);
        parser.process(b"alpha\r\nbeta");
        let forward = TerminalSelection {
            tab_id: 1,
            anchor: TerminalPoint { row: 0, col: 1 },
            focus: TerminalPoint { row: 1, col: 2 },
            dragging: false,
            moved: true,
        };
        let reverse = TerminalSelection {
            anchor: forward.focus,
            focus: forward.anchor,
            ..forward
        };
        assert_eq!(
            terminal_selection_text(parser.screen(), forward),
            "lpha\r\nbet"
        );
        assert_eq!(
            terminal_selection_text(parser.screen(), reverse),
            "lpha\r\nbet"
        );

        let mut wide_parser = vt100::Parser::new(1, 8, 0);
        wide_parser.process("你A".as_bytes());
        let wide = TerminalSelection {
            tab_id: 1,
            anchor: TerminalPoint { row: 0, col: 0 },
            focus: TerminalPoint { row: 0, col: 2 },
            dragging: false,
            moved: true,
        };
        assert_eq!(terminal_selection_text(wide_parser.screen(), wide), "你A");
    }

    /// An all-CJK selection is the case users actually hit, and it is the one
    /// where a byte/column confusion would show up: every glyph occupies two
    /// grid columns, so the column span is twice the character count.
    #[test]
    fn all_wide_selection_returns_whole_characters_and_counts_chars_not_bytes() {
        let mut parser = vt100::Parser::new(1, 12, 0);
        parser.process("中文测试".as_bytes());

        // Four characters span eight columns; select all of them.
        let whole = TerminalSelection {
            tab_id: 1,
            anchor: TerminalPoint { row: 0, col: 0 },
            focus: TerminalPoint { row: 0, col: 7 },
            dragging: false,
            moved: true,
        };
        let text = terminal_selection_text(parser.screen(), whole);
        assert_eq!(text, "中文测试");
        // The status line reports characters, so it must not report the 12
        // UTF-8 bytes this string occupies.
        assert_eq!(text.chars().count(), 4);
        assert_eq!(text.len(), 12);

        // Ending on a wide character's continuation column must still yield
        // that whole character rather than half of it or nothing.
        let ends_mid_glyph = TerminalSelection {
            focus: TerminalPoint { row: 0, col: 2 },
            ..whole
        };
        assert_eq!(
            terminal_selection_text(parser.screen(), ends_mid_glyph),
            "中文"
        );
    }
}
