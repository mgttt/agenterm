//! Unix software-rendered frontend projection.

use crate::terminal_cursor::TerminalCursorShape;
use crate::theme::{Rgb, ThemeId, ThemePalette};
use crate::ui_geometry::{
    PixelRect, TreeRowActionDensity, TreeRowMode, sidebar_tree_row_geometry,
    tree_connector_segments, tree_row_at_y,
};
use unicode_width::UnicodeWidthChar;

use super::{
    font::{
        GLYPH_HEIGHT, GLYPH_WIDTH, RasterGlyph, primary_advance, primary_ascent, raster_glyph,
        resolved_font_name,
    },
    layout::{SCROLLBAR_WIDTH, u32_rect},
};

pub(super) const COMPOSER_HEIGHT: u32 = 64;
const COMPOSER_VISIBLE_LINES: usize = 2;
const COMPOSER_LINE_HEIGHT: u32 = 16;
pub(super) const STATUS_HEIGHT: u32 = 26;
pub(super) const SETTINGS_MODAL_WIDTH: u32 = 480;
pub(super) const SETTINGS_MODAL_HEIGHT: u32 = 330;
pub(super) const NEW_TERMINAL_MODAL_MIN_WIDTH: u32 = 480;
pub(super) const NEW_TERMINAL_MODAL_MAX_WIDTH: u32 = 620;
pub(super) const NEW_TERMINAL_MODAL_MIN_HEIGHT: u32 = 390;
pub(super) const NEW_TERMINAL_MODAL_MAX_HEIGHT: u32 = 450;

pub(super) const CELL_WIDTH: u32 = 10;
pub(super) const CELL_HEIGHT: u32 = 16;
pub(super) const CELL_PADDING_X: u32 = 1;
pub(super) const CELL_PADDING_Y: u32 = 2;

#[derive(Clone, Debug)]
pub(super) struct SidebarTabRow {
    pub(super) id: u64,
    pub(super) depth: usize,
    pub(super) is_last: bool,
    pub(super) guides: Vec<bool>,
    pub(super) title: String,
    pub(super) note: String,
    pub(super) active: bool,
    pub(super) collapsed: bool,
    pub(super) has_children: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TabEditorFocusView {
    Name,
    Note,
}

#[derive(Clone, Debug)]
pub(super) struct TabEditorView {
    pub(super) name_draft: String,
    pub(super) note_draft: String,
    pub(super) focus: TabEditorFocusView,
    pub(super) selected_all: bool,
}

const TERMINAL_CELL_TEXT_BYTES: usize = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalCellText {
    bytes: [u8; TERMINAL_CELL_TEXT_BYTES],
    len: u8,
}

impl TerminalCellText {
    const fn blank() -> Self {
        let mut bytes = [0; TERMINAL_CELL_TEXT_BYTES];
        bytes[0] = b' ';
        Self { bytes, len: 1 }
    }

    fn from_str(text: &str) -> Self {
        assert!(
            text.len() <= TERMINAL_CELL_TEXT_BYTES,
            "vt100 cell contents exceed the mirrored inline capacity"
        );
        let mut bytes = [0; TERMINAL_CELL_TEXT_BYTES];
        let normalized = if text.is_empty() { " " } else { text };
        bytes[..normalized.len()].copy_from_slice(normalized.as_bytes());
        Self {
            bytes,
            len: normalized.len() as u8,
        }
    }

    fn from_char(ch: char) -> Self {
        let mut encoded = [0; 4];
        Self::from_str(ch.encode_utf8(&mut encoded))
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.len)])
            .expect("terminal cell text is constructed from valid UTF-8")
    }

    fn base_char(&self) -> char {
        self.as_str().chars().next().unwrap_or(' ')
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalCell {
    text: TerminalCellText,
    fg: TerminalColor,
    bg: TerminalColor,
    attributes: TerminalAttributes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalColor {
    Default,
    Indexed(u8),
    TrueColor(Rgb),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TerminalAttributes(u8);

impl TerminalAttributes {
    const BOLD: u8 = 1 << 0;
    const DIM: u8 = 1 << 1;
    const ITALIC: u8 = 1 << 2;
    const UNDERLINE: u8 = 1 << 3;

    fn from_cell(cell: &vt100::Cell) -> Self {
        let mut value = 0;
        value |= u8::from(cell.bold()) * Self::BOLD;
        value |= u8::from(cell.dim()) * Self::DIM;
        value |= u8::from(cell.italic()) * Self::ITALIC;
        value |= u8::from(cell.underline()) * Self::UNDERLINE;
        Self(value)
    }

    const fn bold(self) -> bool {
        self.0 & Self::BOLD != 0
    }

    const fn dim(self) -> bool {
        self.0 & Self::DIM != 0
    }

    const fn italic(self) -> bool {
        self.0 & Self::ITALIC != 0
    }

    const fn underline(self) -> bool {
        self.0 & Self::UNDERLINE != 0
    }
}

impl TerminalCell {
    pub(super) const fn blank() -> Self {
        Self {
            text: TerminalCellText::blank(),
            fg: TerminalColor::Default,
            bg: TerminalColor::Default,
            attributes: TerminalAttributes(0),
        }
    }

    pub(super) fn with_defaults(ch: char, _palette: &ThemePalette) -> Self {
        Self {
            text: TerminalCellText::from_char(ch),
            ..Self::blank()
        }
    }

    fn text(&self) -> &str {
        self.text.as_str()
    }

    fn base_char(&self) -> char {
        self.text.base_char()
    }
}

pub(super) struct TerminalGrid {
    pub(super) cols: u16,
    pub(super) rows: u16,
    cells: Vec<TerminalCell>,
    cursor: TerminalCursor,
    palette: &'static ThemePalette,
    /// Rows whose cells changed since the persistent terminal layer last
    /// consumed them; the layer repaints only these rows.
    dirty_rows: Vec<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalCursor {
    row: u16,
    col: u16,
    visible: bool,
}

impl TerminalGrid {
    pub(super) fn new(cols: u16, rows: u16, palette: &'static ThemePalette) -> Self {
        Self {
            cols,
            rows,
            cells: vec![TerminalCell::blank(); usize::from(cols) * usize::from(rows)],
            cursor: TerminalCursor {
                row: 0,
                col: 0,
                visible: cols > 0 && rows > 0,
            },
            palette,
            dirty_rows: vec![true; usize::from(rows)],
        }
    }

    pub(super) fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.cells
            .resize(usize::from(cols) * usize::from(rows), TerminalCell::blank());
        self.cursor.row = self.cursor.row.min(rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(cols.saturating_sub(1));
        self.cursor.visible &= cols > 0 && rows > 0;
        self.dirty_rows.clear();
        self.dirty_rows.resize(usize::from(rows), true);
    }

    pub(super) fn sync_from_screen(&mut self, screen: &vt100::Screen) {
        for row in 0..self.rows {
            let mut row_changed = false;
            for col in 0..self.cols {
                let cell = screen
                    .cell(row, col)
                    .map_or_else(TerminalCell::blank, |cell| {
                        if cell.is_wide_continuation() {
                            return TerminalCell::blank();
                        }
                        let mut fg = terminal_cell_color(cell.fgcolor());
                        let mut bg = terminal_cell_color(cell.bgcolor());
                        if cell.inverse() {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                        TerminalCell {
                            text: TerminalCellText::from_str(cell.contents()),
                            fg,
                            bg,
                            attributes: TerminalAttributes::from_cell(cell),
                        }
                    });
                let index = self.index(col, row);
                if self.cells[index] != cell {
                    self.cells[index] = cell;
                    row_changed = true;
                }
            }
            if row_changed && let Some(dirty) = self.dirty_rows.get_mut(usize::from(row)) {
                *dirty = true;
            }
        }
        let (row, col) = screen.cursor_position();
        let cursor = TerminalCursor {
            row: row.min(self.rows.saturating_sub(1)),
            col: col.min(self.cols.saturating_sub(1)),
            visible: !screen.hide_cursor() && self.cols > 0 && self.rows > 0,
        };
        if cursor != self.cursor {
            self.mark_row_dirty(self.cursor.row);
            self.mark_row_dirty(cursor.row);
            self.cursor = cursor;
        }
    }

    pub(super) fn row_dirty(&self, row: u16) -> bool {
        self.dirty_rows
            .get(usize::from(row))
            .copied()
            .unwrap_or(true)
    }

    pub(super) fn any_row_dirty(&self) -> bool {
        self.dirty_rows.iter().any(|dirty| *dirty)
    }

    pub(super) fn clear_dirty_rows(&mut self) {
        self.dirty_rows.fill(false);
    }

    fn mark_row_dirty(&mut self, row: u16) {
        if let Some(dirty) = self.dirty_rows.get_mut(usize::from(row)) {
            *dirty = true;
        }
    }

    pub(super) const fn cursor_key(&self) -> (u16, u16, bool) {
        (self.cursor.row, self.cursor.col, self.cursor.visible)
    }

    pub(super) fn cell(&self, col: u16, row: u16) -> TerminalCell {
        self.cells[self.index(col, row)]
    }

    pub(super) const fn cursor_visible(&self) -> bool {
        self.cursor.visible
    }

    pub(super) fn set_cell(&mut self, col: u16, row: u16, cell: TerminalCell) {
        if col < self.cols && row < self.rows {
            let index = usize::from(row) * usize::from(self.cols) + usize::from(col);
            self.cells[index] = cell;
        }
    }

    fn index(&self, col: u16, row: u16) -> usize {
        usize::from(row) * usize::from(self.cols) + usize::from(col)
    }
}

fn terminal_cell_color(color: vt100::Color) -> TerminalColor {
    match color {
        vt100::Color::Default => TerminalColor::Default,
        vt100::Color::Idx(index) => TerminalColor::Indexed(index),
        vt100::Color::Rgb(red, green, blue) => TerminalColor::TrueColor(Rgb::new(red, green, blue)),
    }
}

pub(super) fn cell_metrics(font_size: u16) -> (u32, u32) {
    let glyph_h = u32::from(font_size.clamp(8, 36));
    let cell_h = glyph_h + CELL_PADDING_Y * 2;
    let cell_w = primary_advance(font_size)
        .map(|advance| advance.ceil().max(1.0) as u32)
        .unwrap_or_else(|| {
            let glyph_w = (glyph_h * 2).div_ceil(3).max(GLYPH_WIDTH);
            glyph_w + CELL_PADDING_X * 2
        });
    (cell_w, cell_h)
}

pub(super) fn grid_dimensions_for_terminal(
    terminal_width: u32,
    terminal_height: u32,
    cell_width: u32,
    cell_height: u32,
) -> (u16, u16) {
    let terminal_width = terminal_width.saturating_sub(SCROLLBAR_WIDTH);
    let cols = (terminal_width / cell_width.max(1)).max(2) as u16;
    // A one-row grid underflows vt100's wrap bookkeeping (grid.rs
    // `prev_pos.row -= scrolled`) and aborts the process, so the grid never
    // shrinks below two rows even when the viewport is dragged that small.
    let rows = (terminal_height / cell_height.max(1)).max(2) as u16;
    (cols, rows)
}

pub(super) struct ComposerView<'a> {
    pub(super) text: &'a str,
    pub(super) focused: bool,
    pub(super) selected_all: bool,
    pub(super) top: u32,
    pub(super) label: &'a str,
    pub(super) send_button: (u32, u32, u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposerHit {
    Send,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ScrollbarView {
    pub(super) track: (u32, u32, u32, u32),
    pub(super) thumb: (u32, u32, u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsHit {
    Dark,
    Light,
    SizeDecrease,
    SizeIncrease,
    Cancel,
    Apply,
}

#[derive(Clone, Debug)]
pub(super) struct SettingsModalView {
    pub(super) font_size: u16,
    pub(super) theme_draft: ThemeId,
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) font_family_field: (u32, u32, u32, u32),
    pub(super) dark_button: (u32, u32, u32, u32),
    pub(super) light_button: (u32, u32, u32, u32),
    pub(super) cancel_button: (u32, u32, u32, u32),
    pub(super) apply_button: (u32, u32, u32, u32),
    pub(super) size_decrease_button: (u32, u32, u32, u32),
    pub(super) size_increase_button: (u32, u32, u32, u32),
}

impl SettingsModalView {
    pub(super) fn hit_test(&self, x: f64, y: f64) -> Option<SettingsHit> {
        let x = x as u32;
        let y = y as u32;
        if rect_contains(self.dark_button, x, y) {
            return Some(SettingsHit::Dark);
        }
        if rect_contains(self.light_button, x, y) {
            return Some(SettingsHit::Light);
        }
        if rect_contains(self.size_decrease_button, x, y) {
            return Some(SettingsHit::SizeDecrease);
        }
        if rect_contains(self.size_increase_button, x, y) {
            return Some(SettingsHit::SizeIncrease);
        }
        if rect_contains(self.cancel_button, x, y) {
            return Some(SettingsHit::Cancel);
        }
        if rect_contains(self.apply_button, x, y) {
            return Some(SettingsHit::Apply);
        }
        None
    }

    pub(super) fn for_client(
        client_width: u32,
        client_height: u32,
        font_size: u16,
        theme_draft: ThemeId,
    ) -> SettingsModalView {
        let width = SETTINGS_MODAL_WIDTH.min(client_width.saturating_sub(32).max(1));
        let left = (client_width.saturating_sub(width)) / 2;
        let top = (client_height.saturating_sub(SETTINGS_MODAL_HEIGHT)) / 2;
        let font_family_field = (left + 32, top + 92, width.saturating_sub(158), 32);
        let size_left = left + width.saturating_sub(110);
        let size_row_top = top + 92;
        let size_decrease_button = (size_left, size_row_top, 28, 32);
        let size_increase_button = (size_left + 36, size_row_top, 28, 32);
        let theme_row = top + 180;
        let action_row = top + 266;
        SettingsModalView {
            font_size,
            theme_draft,
            bounds: (left, top, width, SETTINGS_MODAL_HEIGHT),
            font_family_field,
            dark_button: (left + 32, theme_row, 146, 34),
            light_button: (left + 190, theme_row, 146, 34),
            cancel_button: (left + width.saturating_sub(238), action_row, 94, 36),
            apply_button: (left + width.saturating_sub(126), action_row, 94, 36),
            size_decrease_button,
            size_increase_button,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NewShellChoice {
    Default,
    Primary,
    Bash,
}

impl NewShellChoice {
    pub(super) fn label(self) -> String {
        match self {
            Self::Default => "Default".to_owned(),
            Self::Primary => {
                crate::platform::runtime::primary_terminal_shell().label.to_owned()
            }
            Self::Bash => "bash".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NewTerminalFocusView {
    InitialCommand,
    HttpProxy,
    HttpsProxy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NewTerminalHit {
    DefaultShell,
    PrimaryShell,
    BashShell,
    InitialCommand,
    HttpProxy,
    HttpsProxy,
    Create,
    Cancel,
}

#[derive(Clone, Debug)]
pub(super) struct NewTerminalModalView<'a> {
    pub(super) shell_choice: NewShellChoice,
    pub(super) initial_command: &'a str,
    pub(super) http_proxy: &'a str,
    pub(super) https_proxy: &'a str,
    pub(super) focus: NewTerminalFocusView,
    pub(super) selected_all: bool,
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) default_shell_button: (u32, u32, u32, u32),
    pub(super) primary_shell_button: (u32, u32, u32, u32),
    pub(super) bash_shell_button: (u32, u32, u32, u32),
    pub(super) initial_command_field: (u32, u32, u32, u32),
    pub(super) http_proxy_field: (u32, u32, u32, u32),
    pub(super) https_proxy_field: (u32, u32, u32, u32),
    pub(super) create_button: (u32, u32, u32, u32),
    pub(super) cancel_button: (u32, u32, u32, u32),
}

impl NewTerminalModalView<'_> {
    pub(super) fn hit_test(&self, x: f64, y: f64) -> Option<NewTerminalHit> {
        let x = x as u32;
        let y = y as u32;
        if rect_contains(self.default_shell_button, x, y) {
            return Some(NewTerminalHit::DefaultShell);
        }
        if rect_contains(self.primary_shell_button, x, y) {
            return Some(NewTerminalHit::PrimaryShell);
        }
        if rect_contains(self.bash_shell_button, x, y) {
            return Some(NewTerminalHit::BashShell);
        }
        if rect_contains(self.initial_command_field, x, y) {
            return Some(NewTerminalHit::InitialCommand);
        }
        if rect_contains(self.http_proxy_field, x, y) {
            return Some(NewTerminalHit::HttpProxy);
        }
        if rect_contains(self.https_proxy_field, x, y) {
            return Some(NewTerminalHit::HttpsProxy);
        }
        if rect_contains(self.create_button, x, y) {
            return Some(NewTerminalHit::Create);
        }
        if rect_contains(self.cancel_button, x, y) {
            return Some(NewTerminalHit::Cancel);
        }
        None
    }

    pub(super) fn for_client<'a>(
        client_width: u32,
        client_height: u32,
        shell_choice: NewShellChoice,
        initial_command: &'a str,
        http_proxy: &'a str,
        https_proxy: &'a str,
        focus: NewTerminalFocusView,
    ) -> NewTerminalModalView<'a> {
        let width = (client_width.saturating_sub(32))
            .clamp(NEW_TERMINAL_MODAL_MIN_WIDTH, NEW_TERMINAL_MODAL_MAX_WIDTH);
        let height = (client_height.saturating_sub(32))
            .clamp(NEW_TERMINAL_MODAL_MIN_HEIGHT, NEW_TERMINAL_MODAL_MAX_HEIGHT);
        let left = (client_width.saturating_sub(width)) / 2;
        let top = (client_height.saturating_sub(height)) / 2;
        let inner_left = left as i32 + 28;
        let inner_right = left as i32 + width as i32 - 28;
        let gap = 8i32;
        let shell_width = ((inner_right - inner_left - gap * 2) / 3).max(90) as u32;
        let shell_top = top + 74;
        let shell_button = |index: u32| {
            let x = inner_left + index as i32 * (shell_width as i32 + gap);
            (x as u32, shell_top, shell_width, 34)
        };
        let field = |field_top: u32| {
            (
                inner_left as u32,
                field_top,
                (inner_right - inner_left) as u32,
                30,
            )
        };
        let create_button = (
            inner_right.saturating_sub(96) as u32,
            top + height.saturating_sub(52),
            96,
            34,
        );
        let cancel_button = (
            create_button.0.saturating_sub(106),
            create_button.1,
            96,
            create_button.3,
        );
        NewTerminalModalView {
            shell_choice,
            initial_command,
            http_proxy,
            https_proxy,
            focus,
            selected_all: false,
            bounds: (left, top, width, height),
            default_shell_button: shell_button(0),
            primary_shell_button: shell_button(1),
            bash_shell_button: shell_button(2),
            initial_command_field: field(top + 142),
            http_proxy_field: field(top + 218),
            https_proxy_field: field(top + 294),
            create_button,
            cancel_button,
        }
    }

    pub(super) fn with_selected_all(mut self, selected_all: bool) -> Self {
        self.selected_all = selected_all;
        self
    }
}

pub(super) struct TerminalPaint<'a> {
    pub(super) grid: &'a TerminalGrid,
    pub(super) selection: Option<super::terminal_selection::TerminalSelection>,
    pub(super) cursor_style: TerminalCursorStyle,
    pub(super) cursor_shape: TerminalCursorShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TerminalCursorStyle {
    Hidden,
    Active,
    Inactive,
}

/// Workspace toolbar control hit from geometry hit-testing.
///
/// On Linux, clicks are mapped through `platform::linux` action ids (contract
/// revision 1) before product handlers run. Labels stay in render/locale so
/// `ui-snapshot` and the visible GUI cannot diverge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToolbarHit {
    NewTab,
    ToggleTabs,
    ControlCenter,
    Settings,
    ToggleLocale,
    FontDecrease,
    FontIncrease,
}

#[derive(Clone, Debug)]
pub(super) struct WorkspaceToolbarView {
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) new_tab: (u32, u32, u32, u32),
    pub(super) tabs: (u32, u32, u32, u32),
    pub(super) control_center: (u32, u32, u32, u32),
    pub(super) settings: (u32, u32, u32, u32),
    pub(super) locale: (u32, u32, u32, u32),
    pub(super) font_decrease: (u32, u32, u32, u32),
    pub(super) font_increase: (u32, u32, u32, u32),
    pub(super) compact: bool,
    pub(super) tabs_visible: bool,
    pub(super) locale_id: crate::locale::LocaleId,
}

impl WorkspaceToolbarView {
    pub(super) fn from_layout(
        toolbar: crate::ui_geometry::WorkspaceToolbarLayout,
        tabs_visible: bool,
        locale_id: crate::locale::LocaleId,
    ) -> Self {
        Self {
            bounds: u32_rect(toolbar.bounds),
            new_tab: u32_rect(toolbar.new_tab),
            tabs: u32_rect(toolbar.tabs),
            control_center: u32_rect(toolbar.control_center),
            settings: u32_rect(toolbar.settings),
            locale: u32_rect(toolbar.locale),
            font_decrease: u32_rect(toolbar.font_decrease),
            font_increase: u32_rect(toolbar.font_increase),
            compact: matches!(
                toolbar.mode,
                crate::ui_geometry::WorkspaceToolbarMode::Compact
            ),
            tabs_visible,
            locale_id,
        }
    }

    pub(super) fn hit_test(&self, x: f64, y: f64) -> Option<ToolbarHit> {
        let x = x as u32;
        let y = y as u32;
        if rect_contains(self.new_tab, x, y) {
            return Some(ToolbarHit::NewTab);
        }
        if rect_contains(self.tabs, x, y) {
            return Some(ToolbarHit::ToggleTabs);
        }
        if rect_contains(self.control_center, x, y) {
            return Some(ToolbarHit::ControlCenter);
        }
        if rect_contains(self.settings, x, y) {
            return Some(ToolbarHit::Settings);
        }
        if rect_contains(self.locale, x, y) {
            return Some(ToolbarHit::ToggleLocale);
        }
        if rect_contains(self.font_decrease, x, y) {
            return Some(ToolbarHit::FontDecrease);
        }
        if rect_contains(self.font_increase, x, y) {
            return Some(ToolbarHit::FontIncrease);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfirmCloseHit {
    Confirm,
    Cancel,
}

#[derive(Clone, Debug)]
pub(super) struct ConfirmCloseView {
    pub(super) tab_id: u64,
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) confirm_button: (u32, u32, u32, u32),
    pub(super) cancel_button: (u32, u32, u32, u32),
}

impl ConfirmCloseView {
    pub(super) fn for_client(client_width: u32, client_height: u32, tab_id: u64) -> Self {
        let width = 360u32;
        let height = 140u32;
        let left = client_width.saturating_sub(width) / 2;
        let top = client_height.saturating_sub(height) / 2;
        let button_y = top + 90;
        Self {
            tab_id,
            bounds: (left, top, width, height),
            confirm_button: (left + 40, button_y, 120, 28),
            cancel_button: (left + 200, button_y, 120, 28),
        }
    }

    pub(super) fn hit_test(&self, x: f64, y: f64) -> Option<ConfirmCloseHit> {
        let x = x as u32;
        let y = y as u32;
        if rect_contains(self.confirm_button, x, y) {
            Some(ConfirmCloseHit::Confirm)
        } else if rect_contains(self.cancel_button, x, y) {
            Some(ConfirmCloseHit::Cancel)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowCloseHit {
    KeepServer,
    StopServer,
    Cancel,
}

#[derive(Clone, Debug)]
pub(super) struct WindowCloseView {
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) keep_button: (u32, u32, u32, u32),
    pub(super) stop_button: (u32, u32, u32, u32),
    pub(super) cancel_button: (u32, u32, u32, u32),
}

impl WindowCloseView {
    pub(super) fn for_client(client_width: u32, client_height: u32) -> Self {
        let width = 520u32;
        let height = 160u32;
        let left = client_width.saturating_sub(width) / 2;
        let top = client_height.saturating_sub(height + STATUS_HEIGHT) / 2;
        let button_y = top + 110;
        Self {
            bounds: (left, top, width, height),
            keep_button: (left + 24, button_y, 150, 28),
            stop_button: (left + 186, button_y, 150, 28),
            cancel_button: (left + 348, button_y, 140, 28),
        }
    }

    pub(super) fn hit_test(&self, x: f64, y: f64) -> Option<WindowCloseHit> {
        let x = x as u32;
        let y = y as u32;
        if rect_contains(self.keep_button, x, y) {
            Some(WindowCloseHit::KeepServer)
        } else if rect_contains(self.stop_button, x, y) {
            Some(WindowCloseHit::StopServer)
        } else if rect_contains(self.cancel_button, x, y) {
            Some(WindowCloseHit::Cancel)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct StatusBarView<'a> {
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) cwd_bounds: (u32, u32, u32, u32),
    pub(super) provider_bounds: Option<(u32, u32, u32, u32)>,
    pub(super) tabs_recovery: Option<(u32, u32, u32, u32)>,
    pub(super) cwd_text: &'a str,
    pub(super) provider_text: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ImePreeditView<'a> {
    pub(super) text: &'a str,
    pub(super) cursor: Option<(usize, usize)>,
    pub(super) anchor: (u32, u32, u32, u32),
}

pub(super) struct FrameContent<'a> {
    pub(super) sidebar_width: u32,
    pub(super) content_height: u32,
    pub(super) tree_height: u32,
    pub(super) cell_width: u32,
    pub(super) cell_height: u32,
    pub(super) terminal: TerminalPaint<'a>,
    pub(super) terminal_at_logical_resolution: bool,
    pub(super) sidebar_rows: &'a [SidebarTabRow],
    pub(super) sidebar_tree: PixelRect,
    pub(super) editing_tab_id: Option<u64>,
    pub(super) tab_editor: Option<TabEditorView>,
    pub(super) workspace_toolbar: Option<WorkspaceToolbarView>,
    pub(super) terminal_top: u32,
    pub(super) composer: ComposerView<'a>,
    pub(super) scrollbar: Option<ScrollbarView>,
    pub(super) sidebar_scrollbar: Option<ScrollbarView>,
    pub(super) settings: Option<SettingsModalView>,
    pub(super) new_terminal: Option<NewTerminalModalView<'a>>,
    pub(super) confirm_close: Option<ConfirmCloseView>,
    pub(super) window_close: Option<WindowCloseView>,
    pub(super) status: Option<StatusBarView<'a>>,
    pub(super) ime_preedit: Option<ImePreeditView<'a>>,
    pub(super) resize_grip: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy, Debug)]
struct TerminalGridLayout {
    width: u32,
    height: u32,
    offset_x: u32,
    offset_y: u32,
    cell_width: u32,
    cell_height: u32,
    padding_x: u32,
    padding_y: u32,
    scrollbar_width: u32,
}

pub(super) fn render_frame(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    content: FrameContent<'_>,
) {
    let background = rgb_to_pixel(palette.terminal_background);
    for pixel in buffer.iter_mut().take((stride * height) as usize) {
        *pixel = background;
    }

    if content.sidebar_width > 0 {
        render_sidebar(
            buffer,
            stride,
            width,
            content.tree_height,
            palette,
            content.sidebar_tree,
            content.sidebar_rows,
            content.editing_tab_id,
            content.tab_editor.as_ref(),
        );
        if let Some(scrollbar) = content.sidebar_scrollbar {
            render_scrollbar(buffer, stride, palette, scrollbar);
        }
    }
    if let Some(toolbar) = content.workspace_toolbar {
        render_workspace_toolbar(buffer, stride, width, height, palette, toolbar);
    }
    if content.terminal_at_logical_resolution {
        render_terminal_grid(
            buffer,
            stride,
            content.terminal,
            palette,
            TerminalGridLayout {
                width,
                height: content.content_height,
                offset_x: content.sidebar_width,
                offset_y: content.terminal_top,
                cell_width: content.cell_width,
                cell_height: content.cell_height,
                padding_x: CELL_PADDING_X,
                padding_y: CELL_PADDING_Y,
                scrollbar_width: SCROLLBAR_WIDTH,
            },
        );
    }
    if let Some(scrollbar) = content.scrollbar {
        render_scrollbar(buffer, stride, palette, scrollbar);
    }
    render_composer(
        buffer,
        stride,
        width,
        height,
        palette,
        content.sidebar_width,
        content.composer,
    );
    if let Some(status) = content.status {
        render_status_bar(buffer, stride, width, height, palette, status);
    }
    if let Some(grip) = content.resize_grip {
        let (x, y, w, h) = grip;
        fill_rect(buffer, stride, x, y, w, h, rgb_to_pixel(palette.divider));
        fill_rect(
            buffer,
            stride,
            x + w.saturating_sub(1) / 2,
            y + 4,
            1,
            h.saturating_sub(8),
            rgb_to_pixel(palette.focus_ring),
        );
    }
    if let Some(settings) = content.settings {
        render_settings_modal(buffer, stride, width, height, palette, settings);
    }
    if let Some(new_terminal) = content.new_terminal {
        render_new_terminal_modal(buffer, stride, width, height, palette, new_terminal);
    }
    if let Some(confirm) = content.confirm_close {
        render_confirm_close(buffer, stride, width, height, palette, confirm);
    }
    if let Some(window_close) = content.window_close {
        render_window_close(buffer, stride, width, height, palette, window_close);
    }
    if let Some(preedit) = content.ime_preedit {
        render_ime_preedit(buffer, stride, width, height, palette, preedit);
    }
}

#[allow(clippy::too_many_arguments)]
/// Physical-resolution geometry of the persistent terminal layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalLayerGeometry {
    pub(super) offset_x: u32,
    pub(super) offset_y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) cell_width: u32,
    pub(super) cell_height: u32,
    pub(super) padding_x: u32,
    pub(super) padding_y: u32,
}

/// Maps the logical terminal viewport onto physical framebuffer pixels for
/// the persistent terminal layer. Returns `None` when no drawable cell area
/// remains.
#[allow(clippy::too_many_arguments)]
pub(super) fn terminal_layer_geometry(
    physical_width: u32,
    physical_height: u32,
    logical_width: u32,
    logical_height: u32,
    logical_content_height: u32,
    logical_offset_x: u32,
    logical_offset_y: u32,
    logical_cell_width: u32,
    logical_cell_height: u32,
    cols: u16,
    rows: u16,
) -> Option<TerminalLayerGeometry> {
    if logical_width == 0 || logical_height == 0 {
        return None;
    }
    let scale_w = |value: u32| {
        (u64::from(value) * u64::from(physical_width) / u64::from(logical_width)) as u32
    };
    let scale_h = |value: u32| {
        (u64::from(value) * u64::from(physical_height) / u64::from(logical_height)) as u32
    };
    let cell_width = scale_w(logical_cell_width).max(1);
    let cell_height = scale_h(logical_cell_height).max(1);
    let offset_x = scale_w(logical_offset_x);
    let offset_y = scale_h(logical_offset_y);
    let scrollbar_width = scale_w(SCROLLBAR_WIDTH).max(1);
    let content_height = scale_h(logical_content_height).min(physical_height);
    let available_width = physical_width
        .saturating_sub(offset_x)
        .saturating_sub(scrollbar_width);
    let available_height = content_height.saturating_sub(offset_y);
    let width = (u32::from(cols) * cell_width).min(available_width);
    let height = (u32::from(rows) * cell_height).min(available_height);
    (width > 0 && height > 0).then_some(TerminalLayerGeometry {
        offset_x,
        offset_y,
        width,
        height,
        cell_width,
        cell_height,
        padding_x: scale_w(CELL_PADDING_X).max(1),
        padding_y: scale_h(CELL_PADDING_Y).max(1),
    })
}

/// Repaints the persistent physical-resolution terminal layer.
///
/// Only rows flagged dirty on the grid (plus `extra_dirty_rows`, used for the
/// previous and current cursor rows) are repainted; clean rows keep their
/// pixels from earlier frames. `repaint_all` refreshes every row after
/// geometry, palette, or selection changes.
pub(super) fn render_terminal_layer(
    layer: &mut [u32],
    geometry: TerminalLayerGeometry,
    terminal: TerminalPaint<'_>,
    palette: &ThemePalette,
    repaint_all: bool,
    extra_dirty_rows: [Option<u16>; 2],
) {
    let layout = TerminalGridLayout {
        width: geometry.width,
        height: geometry.height,
        offset_x: 0,
        offset_y: 0,
        cell_width: geometry.cell_width,
        cell_height: geometry.cell_height,
        padding_x: geometry.padding_x,
        padding_y: geometry.padding_y,
        scrollbar_width: 0,
    };
    let background = rgb_to_pixel(palette.terminal_background);
    let cursor_row = terminal.grid.cursor_key().0;
    let mut cursor_row_repainted = false;
    for row in 0..terminal.grid.rows {
        let repaint = repaint_all
            || terminal.grid.row_dirty(row)
            || extra_dirty_rows.iter().flatten().any(|extra| *extra == row);
        if !repaint {
            continue;
        }
        let y = u32::from(row) * geometry.cell_height;
        if y >= geometry.height {
            break;
        }
        fill_rect(
            layer,
            geometry.width,
            0,
            y,
            geometry.width,
            geometry.cell_height.min(geometry.height - y),
            background,
        );
        render_terminal_grid_row(layer, geometry.width, &terminal, palette, layout, row);
        if row == cursor_row {
            cursor_row_repainted = true;
        }
    }
    if cursor_row_repainted {
        render_terminal_cursor(layer, geometry.width, &terminal, palette, layout);
    }
}

/// Copies the terminal layer into the output frame at its physical offset.
pub(super) fn blit_terminal_layer(
    destination: &mut [u32],
    stride: u32,
    destination_height: u32,
    layer: &[u32],
    geometry: TerminalLayerGeometry,
) {
    let copy_width = geometry.width.min(stride.saturating_sub(geometry.offset_x)) as usize;
    if copy_width == 0 {
        return;
    }
    let rows = geometry
        .height
        .min(destination_height.saturating_sub(geometry.offset_y));
    for row in 0..rows {
        let source_start = (row * geometry.width) as usize;
        let destination_start = ((geometry.offset_y + row) * stride + geometry.offset_x) as usize;
        let (Some(source), Some(target)) = (
            layer.get(source_start..source_start + copy_width),
            destination.get_mut(destination_start..destination_start + copy_width),
        ) else {
            continue;
        };
        target.copy_from_slice(source);
    }
}

fn render_ime_preedit(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    preedit: ImePreeditView<'_>,
) {
    let (anchor_x, anchor_y, _, anchor_height) = preedit.anchor;
    let text_columns = preedit
        .text
        .chars()
        .map(|ch| ch.width().unwrap_or(1).max(1) as u32)
        .sum::<u32>();
    let maximum_width = width.saturating_sub(8);
    let box_width = (text_columns * (GLYPH_WIDTH + 1) + 12)
        .min(maximum_width)
        .max(maximum_width.min(28));
    let box_height = (GLYPH_HEIGHT + 8).max(anchor_height);
    let x = anchor_x.min(width.saturating_sub(box_width + 4));
    let below = anchor_y.saturating_add(anchor_height);
    let y = if below.saturating_add(box_height) <= height {
        below
    } else {
        anchor_y.saturating_sub(box_height)
    };
    fill_rect(
        buffer,
        stride,
        x,
        y,
        box_width,
        box_height,
        rgb_to_pixel(palette.modal),
    );
    fill_rect(
        buffer,
        stride,
        x,
        y.saturating_add(box_height.saturating_sub(2)),
        box_width,
        2,
        rgb_to_pixel(palette.focus_ring),
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        x + 6,
        y + 4,
        preedit.text,
        palette.text,
    );
    if let Some((cursor, _)) = preedit.cursor
        && cursor <= preedit.text.len()
        && preedit.text.is_char_boundary(cursor)
    {
        let cursor_columns = preedit.text[..cursor]
            .chars()
            .map(|ch| ch.width().unwrap_or(1).max(1) as u32)
            .sum::<u32>();
        fill_rect(
            buffer,
            stride,
            (x + 6 + cursor_columns * (GLYPH_WIDTH + 1))
                .min(x.saturating_add(box_width).saturating_sub(2)),
            y + 3,
            1,
            box_height.saturating_sub(7),
            rgb_to_pixel(palette.focus_ring),
        );
    }
}

fn render_scrollbar(
    buffer: &mut [u32],
    stride: u32,
    palette: &ThemePalette,
    scrollbar: ScrollbarView,
) {
    let track_color = rgb_to_pixel(palette.scrollbar_track);
    let thumb_color = rgb_to_pixel(palette.scrollbar_thumb);
    let (tx, ty, tw, th) = scrollbar.track;
    fill_rect(buffer, stride, tx, ty, tw, th, track_color);
    let (ux, uy, uw, uh) = scrollbar.thumb;
    fill_rect(buffer, stride, ux, uy, uw, uh, thumb_color);
}

fn render_composer(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    sidebar_width: u32,
    composer: ComposerView<'_>,
) {
    let top = composer.top;
    if top >= height {
        return;
    }
    let composer_width = width.saturating_sub(sidebar_width);
    let composer_bg = rgb_to_pixel(palette.composer);
    fill_rect(
        buffer,
        stride,
        sidebar_width,
        top,
        composer_width,
        COMPOSER_HEIGHT,
        composer_bg,
    );
    let divider = rgb_to_pixel(palette.divider);
    fill_rect(
        buffer,
        stride,
        sidebar_width,
        top,
        composer_width,
        1,
        divider,
    );
    if composer.focused {
        let ring = rgb_to_pixel(palette.focus_ring);
        fill_rect(buffer, stride, sidebar_width, top, composer_width, 2, ring);
    }
    let (sx, sy, sw, sh) = composer.send_button;
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        (sx, sy, sw, sh),
        "Send",
        composer.focused,
    );
    let text_right = sx.saturating_sub(8);
    let text_width = text_right.saturating_sub(sidebar_width + 8);
    draw_text(
        buffer,
        stride,
        width,
        height,
        sidebar_width + 8,
        top + 6,
        "Composer",
        palette.muted_text,
    );
    let prefix = if composer.label.is_empty() {
        "> "
    } else {
        composer.label
    };
    let max_chars = (text_width / (GLYPH_WIDTH + 1)).max(1) as usize;
    let mut reverse_lines = composer.text.rsplit('\n');
    let last_line = reverse_lines.next().unwrap_or_default();
    let previous_line = reverse_lines.next();
    let earlier_lines_hidden = reverse_lines.next().is_some();
    let visible_lines = [previous_line.unwrap_or(last_line), last_line];
    let visible_line_count = if previous_line.is_some() {
        COMPOSER_VISIBLE_LINES
    } else {
        1
    };
    let mut caret = None;
    for (row, line) in visible_lines[..visible_line_count].iter().enumerate() {
        let row_prefix = if row == 0 {
            if earlier_lines_hidden { "… " } else { prefix }
        } else {
            "  "
        };
        let visible_prefix = truncate_chars(row_prefix, max_chars);
        let row_y = top + 20 + row as u32 * COMPOSER_LINE_HEIGHT;
        let text_x = sidebar_width + 8 + ui_text_width(&visible_prefix);
        let visible_text = truncate_tail_to_width(line, text_right.saturating_sub(text_x));
        draw_text(
            buffer,
            stride,
            width,
            height,
            sidebar_width + 8,
            row_y,
            &visible_prefix,
            palette.text,
        );
        let visible_text_width = ui_text_width(&visible_text);
        if composer.selected_all && visible_text_width > 0 {
            fill_rect(
                buffer,
                stride,
                text_x,
                row_y.saturating_sub(2),
                visible_text_width,
                COMPOSER_LINE_HEIGHT.min(height.saturating_sub(row_y.saturating_sub(2))),
                rgb_to_pixel(palette.selection_background),
            );
        }
        draw_text(
            buffer,
            stride,
            width,
            height,
            text_x,
            row_y,
            &visible_text,
            if composer.selected_all {
                palette.selection_foreground
            } else {
                palette.text
            },
        );
        caret = Some((text_x + ui_text_width(&visible_text), row_y));
    }
    // The composer edits at the end of the draft, so the caret marks exactly
    // that insertion point; without it the strip gives no editing feedback.
    if composer.focused
        && !composer.selected_all
        && let Some((caret_x, caret_y)) = caret
    {
        fill_rect(
            buffer,
            stride,
            caret_x.min(text_right.saturating_sub(2)),
            caret_y.saturating_sub(2),
            2,
            COMPOSER_LINE_HEIGHT,
            rgb_to_pixel(palette.focus_ring),
        );
    }
}

fn render_status_bar(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    status: StatusBarView<'_>,
) {
    let (x, y, w, h) = status.bounds;
    fill_rect(buffer, stride, x, y, w, h, rgb_to_pixel(palette.composer));
    fill_rect(buffer, stride, x, y, w, 1, rgb_to_pixel(palette.divider));
    if let Some((rx, ry, rw, rh)) = status.tabs_recovery {
        fill_rect(buffer, stride, rx, ry, rw, rh, rgb_to_pixel(palette.active));
        draw_text(
            buffer,
            stride,
            width,
            height,
            rx + 8,
            ry + 6,
            "Tabs",
            palette.text,
        );
    }
    let (cx, cy, cw, _ch) = status.cwd_bounds;
    let cwd = if status.cwd_text.is_empty() {
        "CWD: (unknown)"
    } else {
        status.cwd_text
    };
    let max_chars = ((cw.saturating_sub(8)) / (GLYPH_WIDTH + 1)).max(1) as usize;
    draw_text(
        buffer,
        stride,
        width,
        height,
        cx + 4,
        cy + 6,
        &truncate_chars(cwd, max_chars),
        palette.text,
    );
    if let Some((px, py, pw, ph)) = status.provider_bounds
        && !status.provider_text.is_empty()
    {
        let max_provider = (pw.saturating_sub(8) / (GLYPH_WIDTH + 1)).max(1) as usize;
        draw_text(
            buffer,
            stride,
            width,
            height,
            px + 4,
            py + (ph / 2).saturating_sub(4),
            &truncate_chars(status.provider_text, max_provider),
            palette.muted_text,
        );
    }
}

fn render_window_close(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    modal: WindowCloseView,
) {
    for y in 0..height {
        for x in 0..width {
            let index = (y * stride + x) as usize;
            if let Some(pixel) = buffer.get_mut(index) {
                *pixel = dim_pixel(*pixel, 2, 5);
            }
        }
    }
    let (mx, my, mw, mh) = modal.bounds;
    fill_rect(buffer, stride, mx, my, mw, mh, rgb_to_pixel(palette.modal));
    fill_rect(
        buffer,
        stride,
        mx,
        my,
        mw,
        2,
        rgb_to_pixel(palette.focus_ring),
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 16,
        my + 20,
        "Close AgenTerm window?",
        palette.text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 16,
        my + 48,
        "Keep server running hides the GUI; Stop exits.",
        palette.text,
    );
    for (rect, label) in [
        (modal.keep_button, "Keep server"),
        (modal.stop_button, "Stop & exit"),
        (modal.cancel_button, "Cancel"),
    ] {
        let (x, y, w, h) = rect;
        fill_rect(buffer, stride, x, y, w, h, rgb_to_pixel(palette.composer));
        draw_text(
            buffer,
            stride,
            width,
            height,
            x + 12,
            y + 8,
            label,
            palette.text,
        );
    }
}

fn render_workspace_toolbar(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    toolbar: WorkspaceToolbarView,
) {
    let (bx, by, bw, bh) = toolbar.bounds;
    let bg = rgb_to_pixel(palette.sidebar);
    fill_rect(buffer, stride, bx, by, bw, bh, bg);
    let divider = rgb_to_pixel(palette.divider);
    fill_rect(buffer, stride, bx, by, bw, 1, divider);
    let button_bg = rgb_to_pixel(palette.composer);
    let labels = if toolbar.compact {
        (
            "+",
            if toolbar.tabs_visible { "<T" } else { ">T" },
            "CC",
            "S",
        )
    } else {
        (
            "New",
            if toolbar.tabs_visible {
                "<Tabs"
            } else {
                ">Tabs"
            },
            "Control Center",
            "Settings",
        )
    };
    for (rect, label) in [
        (toolbar.new_tab, labels.0),
        (toolbar.tabs, labels.1),
        (toolbar.control_center, labels.2),
        (toolbar.settings, labels.3),
        (toolbar.locale, toolbar.locale_id.toolbar_label()),
        (toolbar.font_decrease, "A-"),
        (toolbar.font_increase, "A+"),
    ] {
        let (x, y, w, h) = rect;
        fill_rect(buffer, stride, x, y, w, h, button_bg);
        draw_text(
            buffer,
            stride,
            width,
            height,
            x + 8,
            y + h.saturating_sub(12) / 2,
            label,
            palette.text,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_sidebar(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    sidebar_tree: PixelRect,
    rows: &[SidebarTabRow],
    editing_tab_id: Option<u64>,
    tab_editor: Option<&TabEditorView>,
) {
    let sidebar_left = sidebar_tree.left.max(0) as u32;
    let sidebar_width = sidebar_tree.width().max(0) as u32;
    let sidebar_bg = rgb_to_pixel(palette.sidebar);
    fill_rect(
        buffer,
        stride,
        sidebar_left,
        0,
        sidebar_width,
        height,
        sidebar_bg,
    );

    let divider = rgb_to_pixel(palette.divider);
    let sidebar_right = sidebar_left + sidebar_width;
    if sidebar_width > 0 && sidebar_right < width {
        fill_rect(
            buffer,
            stride,
            sidebar_right.saturating_sub(1),
            0,
            1,
            height,
            divider,
        );
    }

    for (index, row) in rows.iter().enumerate() {
        let editing = editing_tab_id == Some(row.id);
        let mode = if editing {
            TreeRowMode::Editing
        } else {
            TreeRowMode::Normal
        };
        let geometry = sidebar_tree_row_geometry(sidebar_tree, index, row.depth, mode);
        let top = geometry.row.top.max(0) as u32;
        if top >= height {
            break;
        }
        let row_bottom = geometry.row.bottom.max(geometry.row.top) as u32;
        let row_height = row_bottom
            .saturating_sub(top)
            .min(height.saturating_sub(top));
        if row_height == 0 {
            continue;
        }

        render_tree_row_connectors(
            buffer,
            stride,
            palette,
            sidebar_tree,
            row,
            &geometry,
            top,
            row_bottom,
            mode,
        );

        if row.active {
            let active_bg = rgb_to_pixel(palette.active);
            fill_rect(
                buffer,
                stride,
                geometry.selection.left.max(0) as u32,
                geometry.selection.top.max(0) as u32,
                geometry.selection.width().max(0) as u32,
                geometry.selection.height().max(0) as u32,
                active_bg,
            );
        }

        let marker = if row.has_children {
            if row.collapsed { "[+]" } else { "[-]" }
        } else {
            "   "
        };
        let marker_x = geometry.node_x.max(0) as u32;
        let marker_y = geometry.node_y.max(0) as u32;
        let text_color = palette.text;
        draw_text(
            buffer, stride, width, height, marker_x, marker_y, marker, text_color,
        );

        if editing && let Some(editor) = tab_editor {
            let (save_label, cancel_label) = match geometry.actions.density {
                TreeRowActionDensity::Full => ("Save", "Cancel"),
                TreeRowActionDensity::Compact => ("Save", "x"),
            };
            let name_focused = editor.focus == TabEditorFocusView::Name;
            let note_focused = editor.focus == TabEditorFocusView::Note;
            if let Some(editors) = geometry.editors {
                render_inline_field(
                    buffer,
                    stride,
                    width,
                    height,
                    palette,
                    editors.name,
                    &editor.name_draft,
                    name_focused,
                    editor.selected_all && name_focused,
                    text_color,
                );
                render_inline_field(
                    buffer,
                    stride,
                    width,
                    height,
                    palette,
                    editors.note,
                    &editor.note_draft,
                    note_focused,
                    editor.selected_all && note_focused,
                    palette.muted_text,
                );
            }
            render_tree_action_button(
                buffer,
                stride,
                width,
                height,
                palette,
                geometry.actions.primary,
                save_label,
                true,
            );
            render_tree_action_button(
                buffer,
                stride,
                width,
                height,
                palette,
                geometry.actions.secondary,
                cancel_label,
                false,
            );
        } else {
            let name_x = geometry.name.left.max(0) as u32;
            let name_y = geometry.name.top.max(0) as u32;
            let name_chars =
                (geometry.name.width().max(0) as u32 / (GLYPH_WIDTH + 1)).max(1) as usize;
            let title = tab_row_title(row.id, &row.title, name_chars);
            draw_text(
                buffer, stride, width, height, name_x, name_y, &title, text_color,
            );

            let note_x = geometry.note.left.max(0) as u32;
            let note_y = geometry.note.top.max(0) as u32;
            let note_chars =
                (geometry.note.width().max(0) as u32 / (GLYPH_WIDTH + 1)).max(1) as usize;
            draw_text(
                buffer,
                stride,
                width,
                height,
                note_x,
                note_y,
                &truncate_chars(&row.note, note_chars),
                palette.muted_text,
            );
            if row.active {
                let (add_label, close_label) = match geometry.actions.density {
                    TreeRowActionDensity::Full => ("Add", "Close"),
                    TreeRowActionDensity::Compact => ("+", "x"),
                };
                if let Some(add_child) = geometry.actions.add_child {
                    render_tree_action_button(
                        buffer, stride, width, height, palette, add_child, add_label, false,
                    );
                }
                render_tree_action_button(
                    buffer,
                    stride,
                    width,
                    height,
                    palette,
                    geometry.actions.secondary,
                    close_label,
                    false,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_tree_row_connectors(
    buffer: &mut [u32],
    stride: u32,
    palette: &ThemePalette,
    sidebar_tree: PixelRect,
    row: &SidebarTabRow,
    geometry: &crate::ui_geometry::TreeRowGeometry,
    row_top: u32,
    row_bottom: u32,
    mode: TreeRowMode,
) {
    let line_color = rgb_to_pixel(palette.divider);
    for segment in tree_connector_segments(
        sidebar_tree,
        geometry,
        row.depth,
        &row.guides,
        row.is_last,
        mode,
    ) {
        let left = segment.left.max(0) as u32;
        let top = segment.top.max(row_top as i32).max(0) as u32;
        let right = segment.right.max(segment.left).max(0) as u32;
        let bottom = segment
            .bottom
            .min(row_bottom as i32)
            .max(segment.top)
            .max(0) as u32;
        fill_rect(
            buffer,
            stride,
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
            line_color,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_inline_field(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    bounds: PixelRect,
    text: &str,
    focused: bool,
    selected_all: bool,
    text_color: Rgb,
) {
    let x = bounds.left.max(0) as u32;
    let y = bounds.top.max(0) as u32;
    let w = bounds.width().max(0) as u32;
    let h = bounds.height().max(0) as u32;
    if w == 0 || h == 0 {
        return;
    }
    fill_rect(buffer, stride, x, y, w, h, rgb_to_pixel(palette.composer));
    let border = if focused {
        rgb_to_pixel(palette.focus_ring)
    } else {
        rgb_to_pixel(palette.divider)
    };
    fill_rect(buffer, stride, x, y, w, 1, border);
    fill_rect(
        buffer,
        stride,
        x,
        y.saturating_add(h.saturating_sub(1)),
        w,
        1,
        border,
    );
    fill_rect(buffer, stride, x, y, 1, h, border);
    fill_rect(
        buffer,
        stride,
        x.saturating_add(w.saturating_sub(1)),
        y,
        1,
        h,
        border,
    );
    let max_chars = (w.saturating_sub(4) / (GLYPH_WIDTH + 1)).max(1) as usize;
    let label = truncate_chars(text, max_chars);
    let visible_text_width =
        (label.chars().count() as u32 * (GLYPH_WIDTH + 1)).min(w.saturating_sub(4));
    if selected_all && visible_text_width > 0 {
        fill_rect(
            buffer,
            stride,
            x + 2,
            y + 2,
            visible_text_width,
            h.saturating_sub(4),
            rgb_to_pixel(palette.selection_background),
        );
    }
    draw_text(
        buffer,
        stride,
        width,
        height,
        x + 2,
        y + 2,
        &label,
        if selected_all {
            palette.selection_foreground
        } else {
            text_color
        },
    );
    if focused && !selected_all {
        let cursor_x = x + 2 + (label.chars().count() as u32 * (GLYPH_WIDTH + 1));
        if cursor_x + GLYPH_WIDTH < x + w {
            fill_rect(
                buffer,
                stride,
                cursor_x,
                y + 2,
                GLYPH_WIDTH,
                GLYPH_HEIGHT,
                rgb_to_pixel(text_color),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_tree_action_button(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    bounds: PixelRect,
    label: &str,
    primary: bool,
) {
    let x = bounds.left.max(0) as u32;
    let y = bounds.top.max(0) as u32;
    let w = bounds.width().max(0) as u32;
    let h = bounds.height().max(0) as u32;
    if w == 0 || h == 0 {
        return;
    }
    let bg = if primary {
        rgb_to_pixel(palette.active)
    } else {
        rgb_to_pixel(palette.composer)
    };
    fill_rect(buffer, stride, x, y, w, h, bg);
    fill_rect(buffer, stride, x, y, w, 1, rgb_to_pixel(palette.divider));
    let text_color = palette.text;
    draw_text(
        buffer,
        stride,
        width,
        height,
        x + 4,
        y + h.saturating_sub(12) / 2,
        label,
        text_color,
    );
}

fn render_terminal_grid(
    buffer: &mut [u32],
    stride: u32,
    terminal: TerminalPaint<'_>,
    palette: &ThemePalette,
    layout: TerminalGridLayout,
) {
    for row in 0..terminal.grid.rows {
        render_terminal_grid_row(buffer, stride, &terminal, palette, layout, row);
    }
    render_terminal_cursor(buffer, stride, &terminal, palette, layout);
}

fn render_terminal_grid_row(
    buffer: &mut [u32],
    stride: u32,
    terminal: &TerminalPaint<'_>,
    palette: &ThemePalette,
    layout: TerminalGridLayout,
    row: u16,
) {
    let terminal_width = layout
        .width
        .saturating_sub(layout.offset_x)
        .saturating_sub(layout.scrollbar_width);
    let selection_fg = palette.selection_foreground;
    let selection_bg = palette.selection_background;
    {
        let mut col = 0;
        while col < terminal.grid.cols {
            let x = layout.offset_x + u32::from(col) * layout.cell_width;
            if x + layout.cell_width > layout.offset_x + terminal_width {
                break;
            }
            let cell = terminal.grid.cell(col, row);
            let wide = cell.base_char().width() == Some(2);
            let cell_w = if wide {
                layout.cell_width * 2
            } else {
                layout.cell_width
            };
            let selected = terminal
                .selection
                .is_some_and(|selection| selection.contains(row, col));
            let fg = if selected {
                selection_fg
            } else {
                terminal_color(palette, cell.fg, false)
            };
            let bg = if selected {
                selection_bg
            } else {
                terminal_color(palette, cell.bg, true)
            };
            draw_cell(
                buffer,
                stride,
                layout.width,
                layout.height,
                x,
                layout.offset_y + u32::from(row) * layout.cell_height,
                cell_w,
                layout.cell_height,
                cell.text(),
                fg,
                bg,
                cell.attributes,
                layout.padding_x,
                layout.padding_y,
            );
            col += if wide { 2 } else { 1 };
        }
    }
}

fn render_terminal_cursor(
    buffer: &mut [u32],
    stride: u32,
    terminal: &TerminalPaint<'_>,
    palette: &ThemePalette,
    layout: TerminalGridLayout,
) {
    let terminal_width = layout
        .width
        .saturating_sub(layout.offset_x)
        .saturating_sub(layout.scrollbar_width);
    if terminal.cursor_style != TerminalCursorStyle::Hidden && terminal.grid.cursor.visible {
        let cursor = terminal.grid.cursor;
        let x = layout.offset_x + u32::from(cursor.col) * layout.cell_width;
        let y = layout.offset_y + u32::from(cursor.row) * layout.cell_height;
        if x + layout.cell_width <= layout.offset_x + terminal_width
            && y + layout.cell_height <= layout.height
        {
            let cell = terminal.grid.cell(cursor.col, cursor.row);
            let cursor_width = if cell.base_char().width() == Some(2) {
                layout.cell_width * 2
            } else {
                layout.cell_width
            }
            .min(layout.offset_x + terminal_width - x);
            match terminal.cursor_style {
                TerminalCursorStyle::Active => match terminal.cursor_shape {
                    TerminalCursorShape::Block => draw_cell(
                        buffer,
                        stride,
                        layout.width,
                        layout.height,
                        x,
                        y,
                        cursor_width,
                        layout.cell_height,
                        cell.text(),
                        palette.terminal_background,
                        palette.focus_ring,
                        cell.attributes,
                        layout.padding_x,
                        layout.padding_y,
                    ),
                    TerminalCursorShape::Underline => {
                        let cursor_height = (layout.cell_height / 8)
                            .clamp(layout.padding_y, layout.padding_y + layout.padding_y / 2);
                        fill_rect(
                            buffer,
                            stride,
                            x,
                            y + layout.cell_height - cursor_height,
                            cursor_width,
                            cursor_height,
                            rgb_to_pixel(palette.focus_ring),
                        );
                    }
                    TerminalCursorShape::Bar => {
                        let bar_width = (layout.cell_width / 5)
                            .clamp(layout.padding_x * 2, layout.padding_x * 3);
                        fill_rect(
                            buffer,
                            stride,
                            x,
                            y,
                            bar_width,
                            layout.cell_height,
                            rgb_to_pixel(palette.focus_ring),
                        );
                    }
                },
                TerminalCursorStyle::Inactive => {
                    let color = rgb_to_pixel(palette.muted_text);
                    let border_x = layout.padding_x.max(1);
                    let border_y = layout.padding_y.div_ceil(2).max(1);
                    fill_rect(buffer, stride, x, y, cursor_width, border_y, color);
                    fill_rect(
                        buffer,
                        stride,
                        x,
                        y + layout.cell_height - border_y,
                        cursor_width,
                        border_y,
                        color,
                    );
                    fill_rect(buffer, stride, x, y, border_x, layout.cell_height, color);
                    fill_rect(
                        buffer,
                        stride,
                        x + cursor_width - border_x,
                        y,
                        border_x,
                        layout.cell_height,
                        color,
                    );
                }
                TerminalCursorStyle::Hidden => {}
            }
        }
    }
}

fn render_confirm_close(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    confirm: ConfirmCloseView,
) {
    for y in 0..height {
        for x in 0..width {
            let index = (y * stride + x) as usize;
            if let Some(pixel) = buffer.get_mut(index) {
                *pixel = dim_pixel(*pixel, 2, 5);
            }
        }
    }
    let (mx, my, mw, mh) = confirm.bounds;
    fill_rect(buffer, stride, mx, my, mw, mh, rgb_to_pixel(palette.modal));
    fill_rect(
        buffer,
        stride,
        mx,
        my,
        mw,
        2,
        rgb_to_pixel(palette.focus_ring),
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 16,
        my + 20,
        &format!("Close live tab @{}?", confirm.tab_id),
        palette.text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 16,
        my + 48,
        "Process is still running.",
        palette.text,
    );
    for (rect, label) in [
        (confirm.confirm_button, "Close"),
        (confirm.cancel_button, "Cancel"),
    ] {
        let (x, y, w, h) = rect;
        fill_rect(buffer, stride, x, y, w, h, rgb_to_pixel(palette.composer));
        draw_text(
            buffer,
            stride,
            width,
            height,
            x + 28,
            y + 8,
            label,
            palette.text,
        );
    }
}

fn render_settings_modal(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    settings: SettingsModalView,
) {
    dim_full_frame(buffer, stride, width, height);

    let (mx, my, mw, mh) = settings.bounds;
    fill_rect(buffer, stride, mx, my, mw, mh, rgb_to_pixel(palette.modal));
    fill_rect(
        buffer,
        stride,
        mx,
        my,
        mw,
        2,
        rgb_to_pixel(palette.focus_ring),
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 28,
        my + 18,
        "Settings",
        palette.text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 12,
        my + 34,
        &format!("Renderer: {}", resolved_font_name()),
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 32,
        my + 58,
        "Terminal renderer (system)",
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + mw.saturating_sub(110),
        my + 58,
        &format!("Size {} pt", settings.font_size),
        palette.muted_text,
    );
    let (fx, fy, fw, fh) = settings.font_family_field;
    fill_rect(
        buffer,
        stride,
        fx,
        fy,
        fw,
        fh,
        rgb_to_pixel(palette.composer),
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        fx + 10,
        fy + fh.saturating_sub(GLYPH_HEIGHT) / 2,
        &format!("System {}", resolved_font_name()),
        palette.muted_text,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        settings.size_decrease_button,
        "-",
        false,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        settings.size_increase_button,
        "+",
        false,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 32,
        my + 144,
        "Color theme · preview is immediate; Apply persists",
        palette.muted_text,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        settings.dark_button,
        if settings.theme_draft == ThemeId::Dark {
            "Dark *"
        } else {
            "Dark"
        },
        settings.theme_draft == ThemeId::Dark,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        settings.light_button,
        if settings.theme_draft == ThemeId::Light {
            "Light *"
        } else {
            "Light"
        },
        settings.theme_draft == ThemeId::Light,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        settings.cancel_button,
        "Cancel",
        false,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        settings.apply_button,
        "Apply",
        true,
    );
}

fn render_new_terminal_modal(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    modal: NewTerminalModalView<'_>,
) {
    dim_full_frame(buffer, stride, width, height);

    let (mx, my, mw, mh) = modal.bounds;
    fill_rect(buffer, stride, mx, my, mw, mh, rgb_to_pixel(palette.modal));
    fill_rect(
        buffer,
        stride,
        mx,
        my,
        mw,
        2,
        rgb_to_pixel(palette.focus_ring),
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 28,
        my + 18,
        "New terminal",
        palette.text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 28,
        my + 48,
        "Shell profile",
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 28,
        my + 116,
        "Initial command · optional; leaves the selected shell open",
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 28,
        my + 192,
        "HTTP proxy · optional, applied only to this terminal",
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 28,
        my + 268,
        "HTTPS proxy · optional, values are never exposed in snapshots",
        palette.muted_text,
    );

    for (rect, choice) in [
        (modal.default_shell_button, NewShellChoice::Default),
        (modal.primary_shell_button, NewShellChoice::Primary),
        (modal.bash_shell_button, NewShellChoice::Bash),
    ] {
        let selected = modal.shell_choice == choice;
        let prefix = if selected { "● " } else { "○ " };
        render_button(
            buffer,
            stride,
            width,
            height,
            palette,
            rect,
            &format!("{prefix}{}", choice.label()),
            selected,
        );
    }

    let pixel_field = |rect: (u32, u32, u32, u32)| PixelRect {
        left: rect.0 as i32,
        top: rect.1 as i32,
        right: (rect.0 + rect.2) as i32,
        bottom: (rect.1 + rect.3) as i32,
    };
    render_inline_field(
        buffer,
        stride,
        width,
        height,
        palette,
        pixel_field(modal.initial_command_field),
        modal.initial_command,
        modal.focus == NewTerminalFocusView::InitialCommand,
        modal.selected_all && modal.focus == NewTerminalFocusView::InitialCommand,
        palette.text,
    );
    render_inline_field(
        buffer,
        stride,
        width,
        height,
        palette,
        pixel_field(modal.http_proxy_field),
        modal.http_proxy,
        modal.focus == NewTerminalFocusView::HttpProxy,
        modal.selected_all && modal.focus == NewTerminalFocusView::HttpProxy,
        palette.text,
    );
    render_inline_field(
        buffer,
        stride,
        width,
        height,
        palette,
        pixel_field(modal.https_proxy_field),
        modal.https_proxy,
        modal.focus == NewTerminalFocusView::HttpsProxy,
        modal.selected_all && modal.focus == NewTerminalFocusView::HttpsProxy,
        palette.text,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        modal.create_button,
        "Create",
        true,
    );
    render_button(
        buffer,
        stride,
        width,
        height,
        palette,
        modal.cancel_button,
        "Cancel",
        false,
    );
}

fn dim_full_frame(buffer: &mut [u32], stride: u32, width: u32, height: u32) {
    for y in 0..height {
        for x in 0..width {
            let index = (y * stride + x) as usize;
            if let Some(pixel) = buffer.get_mut(index) {
                *pixel = dim_pixel(*pixel, 2, 5);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_button(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    rect: (u32, u32, u32, u32),
    label: &str,
    primary: bool,
) {
    let (x, y, w, h) = rect;
    let bg = if primary {
        palette.accent
    } else {
        palette.control
    };
    fill_rect(buffer, stride, x, y, w, h, rgb_to_pixel(bg));
    draw_text(
        buffer,
        stride,
        width,
        height,
        x + 8,
        y + (h / 2).saturating_sub(4),
        label,
        if primary {
            palette.selection_foreground
        } else {
            palette.text
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_cell(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    origin_x: u32,
    origin_y: u32,
    cell_width: u32,
    cell_height: u32,
    text: &str,
    mut fg: Rgb,
    bg: Rgb,
    attributes: TerminalAttributes,
    padding_x: u32,
    padding_y: u32,
) {
    if origin_x >= width || origin_y >= height {
        return;
    }

    let cell_w = cell_width.min(width - origin_x);
    let cell_h = cell_height.min(height - origin_y);
    let bg_pixel = rgb_to_pixel(bg);
    fill_rect(buffer, stride, origin_x, origin_y, cell_w, cell_h, bg_pixel);
    if attributes.dim() {
        fg = mix_rgb(fg, bg, 2, 3);
    }
    if attributes.underline() && cell_w > 0 && cell_h > 0 {
        let underline_height = (cell_h / 16).max(1);
        fill_rect(
            buffer,
            stride,
            origin_x,
            origin_y + cell_h - underline_height,
            cell_w,
            underline_height,
            rgb_to_pixel(fg),
        );
    }

    let glyph_y = origin_y + padding_y;
    let glyph_h = cell_h.saturating_sub(padding_y * 2);
    if cell_w == 0 || glyph_h == 0 {
        return;
    }
    let raster_size = glyph_h.min(u32::from(u16::MAX)) as u16;
    let mut chars = text.chars();
    let Some(base) = chars.next() else {
        return;
    };
    if let (Some(glyph), Some(ascent)) =
        (raster_glyph(base, raster_size), primary_ascent(raster_size))
    {
        let centered_x = origin_x as f32 + (cell_w as f32 - glyph.advance).max(0.0) / 2.0;
        let baseline_y = glyph_y as f32 + ascent;
        draw_raster_glyph_styled(
            buffer,
            stride,
            width,
            height,
            &glyph,
            centered_x,
            baseline_y,
            fg,
            (origin_x, origin_y, cell_w, cell_h),
            attributes.bold(),
            attributes.italic(),
        );
        for combining in chars {
            if let Some(glyph) = raster_glyph(combining, raster_size) {
                draw_raster_glyph_styled(
                    buffer,
                    stride,
                    width,
                    height,
                    &glyph,
                    centered_x,
                    baseline_y,
                    fg,
                    (origin_x, origin_y, cell_w, cell_h),
                    attributes.bold(),
                    attributes.italic(),
                );
            }
        }
        return;
    }
    let glyph = super::font::glyph_rows(base).or_else(|| {
        (!base.is_whitespace())
            .then(|| super::font::glyph_rows('?'))
            .flatten()
    });
    let Some(glyph) = glyph else { return };
    let glyph_x = origin_x + padding_x;
    let glyph_w = cell_w.saturating_sub(padding_x * 2);
    if glyph_w == 0 {
        return;
    }
    let fg_pixel = rgb_to_pixel(fg);
    for target_y in 0..glyph_h {
        let source_y = (target_y * GLYPH_HEIGHT / glyph_h) as usize;
        let y = glyph_y + target_y;
        let row_bits = glyph[source_y.min(glyph.len() - 1)];
        for target_x in 0..glyph_w {
            let source_x = target_x * GLYPH_WIDTH / glyph_w;
            if !super::font::row_contains_pixel(row_bits, source_x) {
                continue;
            }
            let italic_shift =
                u32::from(attributes.italic()) * glyph_h.saturating_sub(target_y + 1).div_ceil(6);
            let x = glyph_x + target_x + italic_shift;
            if x < origin_x + cell_w && x < width {
                put_pixel(buffer, stride, x, y, fg_pixel);
                if attributes.bold() && x + 1 < origin_x + cell_w && x + 1 < width {
                    put_pixel(buffer, stride, x + 1, y, fg_pixel);
                }
            }
        }
    }
}

/// Pixel width `draw_text` will advance for `text`, glyph-accurate so carets
/// can sit flush against rendered text.
fn ui_text_width(text: &str) -> u32 {
    const UI_FONT_SIZE: u16 = 12;
    let mut x = 0f32;
    for ch in text.chars() {
        x += raster_glyph(ch, UI_FONT_SIZE).map_or((GLYPH_WIDTH + 1) as f32, |glyph| {
            glyph.advance.ceil().max(1.0)
        });
    }
    x as u32
}

/// Keeps the tail of `text` whose rendered width fits `max_width` pixels,
/// prefixing an ellipsis when anything was cut. Pixel-accurate so long lines
/// never overrun their field into neighbouring controls.
fn truncate_tail_to_width(text: &str, max_width: u32) -> String {
    if ui_text_width(text) <= max_width {
        return text.to_owned();
    }
    let ellipsis_width = ui_text_width("…");
    let budget = max_width.saturating_sub(ellipsis_width);
    let mut tail = String::new();
    let mut used = 0u32;
    for ch in text.chars().rev() {
        let advance = ui_text_width(ch.encode_utf8(&mut [0u8; 4]));
        if used + advance > budget {
            break;
        }
        used += advance;
        tail.insert(0, ch);
    }
    format!("…{tail}")
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    origin_x: u32,
    origin_y: u32,
    text: &str,
    fg: Rgb,
) {
    const UI_FONT_SIZE: u16 = 12;

    let fg_pixel = rgb_to_pixel(fg);
    let mut x = origin_x as f32;
    let baseline_y = primary_ascent(UI_FONT_SIZE).map(|ascent| origin_y as f32 + ascent);
    for ch in text.chars() {
        if let (Some(glyph), Some(baseline_y)) = (raster_glyph(ch, UI_FONT_SIZE), baseline_y) {
            draw_raster_glyph(
                buffer,
                stride,
                width,
                height,
                &glyph,
                x,
                baseline_y,
                fg,
                (0, 0, width, height),
            );
            x += glyph.advance.ceil().max(1.0);
            if x >= width as f32 {
                break;
            }
            continue;
        }
        let Some(glyph) = super::font::glyph_rows(ch) else {
            x += (GLYPH_WIDTH + 1) as f32;
            continue;
        };
        if origin_y >= height {
            break;
        }
        for (row_index, row_bits) in glyph.iter().enumerate() {
            let y = origin_y + row_index as u32;
            if y >= height {
                break;
            }
            for bit in 0..GLYPH_WIDTH {
                if !super::font::row_contains_pixel(*row_bits, bit) {
                    continue;
                }
                let pixel_x = x as u32 + bit;
                if pixel_x < width {
                    put_pixel(buffer, stride, pixel_x, y, fg_pixel);
                }
            }
        }
        x += (GLYPH_WIDTH + 1) as f32;
        if x >= width as f32 {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_raster_glyph(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    glyph: &RasterGlyph,
    baseline_x: f32,
    baseline_y: f32,
    foreground: Rgb,
    clip: (u32, u32, u32, u32),
) {
    draw_raster_glyph_styled(
        buffer, stride, width, height, glyph, baseline_x, baseline_y, foreground, clip, false,
        false,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_raster_glyph_styled(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    glyph: &RasterGlyph,
    baseline_x: f32,
    baseline_y: f32,
    foreground: Rgb,
    clip: (u32, u32, u32, u32),
    bold: bool,
    italic: bool,
) {
    let origin_x = baseline_x.round() as i32 + glyph.offset_x;
    let origin_y = baseline_y.round() as i32 + glyph.offset_y;
    let clip_right = clip.0.saturating_add(clip.2).min(width);
    let clip_bottom = clip.1.saturating_add(clip.3).min(height);
    for y in 0..glyph.height {
        let target_y = origin_y + y as i32;
        if target_y < clip.1 as i32 || target_y >= clip_bottom as i32 {
            continue;
        }
        for x in 0..glyph.width {
            let italic_shift = i32::from(italic)
                * i32::try_from(glyph.height.saturating_sub(y + 1).div_ceil(6)).unwrap_or(i32::MAX);
            let target_x = origin_x + x as i32 + italic_shift;
            if target_x < clip.0 as i32 || target_x >= clip_right as i32 {
                continue;
            }
            let alpha = glyph.alpha[(y * glyph.width + x) as usize];
            if alpha == 0 {
                continue;
            }
            blend_pixel(
                buffer,
                stride,
                target_x as u32,
                target_y as u32,
                foreground,
                alpha,
            );
            if bold && target_x + 1 < clip_right as i32 {
                blend_pixel(
                    buffer,
                    stride,
                    (target_x + 1) as u32,
                    target_y as u32,
                    foreground,
                    alpha,
                );
            }
        }
    }
}

fn mix_rgb(foreground: Rgb, background: Rgb, numerator: u16, denominator: u16) -> Rgb {
    let mix = |front: u8, back: u8| {
        let front = u16::from(front) * numerator;
        let back = u16::from(back) * (denominator - numerator);
        ((front + back + denominator / 2) / denominator) as u8
    };
    Rgb::new(
        mix(foreground.red, background.red),
        mix(foreground.green, background.green),
        mix(foreground.blue, background.blue),
    )
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 1 {
        return "…".to_owned();
    }
    let keep = max_chars - 1;
    format!("{}…", text.chars().take(keep).collect::<String>())
}

fn truncate_tail_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 1 {
        return "…".to_owned();
    }
    let keep = max_chars - 1;
    format!(
        "…{}",
        text.chars()
            .skip(count.saturating_sub(keep))
            .collect::<String>()
    )
}

fn tab_row_title(id: u64, title: &str, max_chars: usize) -> String {
    let identified = format!("@{id} {title}");
    if identified.chars().count() <= max_chars || title.is_empty() {
        truncate_chars(&identified, max_chars)
    } else {
        truncate_chars(title, max_chars)
    }
}

fn fill_rect(buffer: &mut [u32], stride: u32, x: u32, y: u32, width: u32, height: u32, color: u32) {
    for row in y..y + height {
        let start = (row * stride + x) as usize;
        let end = start + width as usize;
        if end <= buffer.len() {
            buffer[start..end].fill(color);
        }
    }
}

fn put_pixel(buffer: &mut [u32], stride: u32, x: u32, y: u32, color: u32) {
    let index = (y * stride + x) as usize;
    if let Some(pixel) = buffer.get_mut(index) {
        *pixel = color;
    }
}

fn blend_pixel(buffer: &mut [u32], stride: u32, x: u32, y: u32, foreground: Rgb, alpha: u8) {
    let index = (y * stride + x) as usize;
    let Some(background) = buffer.get_mut(index) else {
        return;
    };
    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    let blend = |front: u8, back: u32| (u32::from(front) * alpha + back * inverse + 127) / 255;
    let red = blend(foreground.red, (*background >> 16) & 0xFF);
    let green = blend(foreground.green, (*background >> 8) & 0xFF);
    let blue = blend(foreground.blue, *background & 0xFF);
    *background = (red << 16) | (green << 8) | blue;
}

fn rect_contains(rect: (u32, u32, u32, u32), x: u32, y: u32) -> bool {
    let (left, top, width, height) = rect;
    x >= left && y >= top && x < left + width && y < top + height
}

fn ansi_color(palette: &ThemePalette, index: u8) -> Rgb {
    palette.ansi[(index & 0x0F) as usize]
}

fn terminal_color(palette: &ThemePalette, color: TerminalColor, background: bool) -> Rgb {
    match color {
        TerminalColor::Default if background => palette.terminal_background,
        TerminalColor::Default => palette.terminal_foreground,
        TerminalColor::Indexed(index @ 0..=15) => ansi_color(palette, index),
        TerminalColor::Indexed(index @ 16..=231) => {
            let cube = index - 16;
            let component = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            Rgb::new(
                component(cube / 36),
                component((cube / 6) % 6),
                component(cube % 6),
            )
        }
        TerminalColor::Indexed(index) => {
            let value = 8 + (index - 232) * 10;
            Rgb::new(value, value, value)
        }
        TerminalColor::TrueColor(rgb) => rgb,
    }
}

fn rgb_to_pixel(rgb: Rgb) -> u32 {
    (u32::from(rgb.red) << 16) | (u32::from(rgb.green) << 8) | u32::from(rgb.blue)
}

fn dim_pixel(base: u32, numerator: u32, denominator: u32) -> u32 {
    let r = ((base >> 16) & 0xFF) * numerator / denominator;
    let g = ((base >> 8) & 0xFF) * numerator / denominator;
    let b = (base & 0xFF) * numerator / denominator;
    (r << 16) | (g << 8) | b
}

pub(super) fn effective_palette(
    configured: ThemeId,
    draft: ThemeId,
    settings_open: bool,
) -> &'static ThemePalette {
    if settings_open {
        draft.palette()
    } else {
        configured.palette()
    }
}

pub(super) fn sidebar_row_at_y(y: u32, tree_height: u32) -> Option<usize> {
    if y >= tree_height {
        return None;
    }
    tree_row_at_y(y as i32)
}

pub(super) fn scrollbar_view_from_geometry(
    geometry: crate::ui_geometry::TerminalScrollbarGeometry,
) -> ScrollbarView {
    ScrollbarView {
        track: u32_rect(geometry.track),
        thumb: u32_rect(geometry.thumb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppConfig;

    fn composer_test_view(text: &str, selected_all: bool) -> ComposerView<'_> {
        ComposerView {
            text,
            focused: true,
            selected_all,
            top: 0,
            label: "",
            send_button: (240, 7, 72, COMPOSER_HEIGHT - 14),
        }
    }

    #[test]
    fn composer_select_all_highlights_only_editable_text() {
        let palette = ThemeId::Dark.palette();
        let mut buffer = vec![0; 320 * COMPOSER_HEIGHT as usize];
        render_composer(
            &mut buffer,
            320,
            320,
            COMPOSER_HEIGHT,
            palette,
            20,
            composer_test_view("abc\ndef", true),
        );

        let prefix_x = 28;
        let text_x = prefix_x + 2 * (GLYPH_WIDTH + 1);
        assert_eq!(
            buffer[(18 * 320 + prefix_x) as usize],
            rgb_to_pixel(palette.composer)
        );
        assert_eq!(
            buffer[(18 * 320 + text_x) as usize],
            rgb_to_pixel(palette.selection_background)
        );
        assert_eq!(
            buffer[(34 * 320 + text_x) as usize],
            rgb_to_pixel(palette.selection_background)
        );
    }

    #[test]
    fn composer_renders_newline_on_a_second_visible_row() {
        let palette = ThemeId::Dark.palette();
        let mut single_line = vec![0; 320 * COMPOSER_HEIGHT as usize];
        let mut multiline = single_line.clone();
        for (buffer, text) in [
            (&mut single_line, "first"),
            (&mut multiline, "first\nsecond"),
        ] {
            render_composer(
                buffer,
                320,
                320,
                COMPOSER_HEIGHT,
                palette,
                20,
                composer_test_view(text, false),
            );
        }

        assert!(
            (34..52)
                .any(|y| { (28..200).any(|x| multiline[y * 320 + x] != single_line[y * 320 + x]) })
        );
    }

    #[test]
    fn composer_long_line_keeps_the_editing_tail_visible() {
        assert_eq!(truncate_tail_chars("abcdefgh", 5), "…efgh");
        assert_eq!(truncate_tail_chars("中文输入", 3), "…输入");
        assert_eq!(truncate_tail_chars("abc", 5), "abc");
    }

    #[test]
    fn selected_inline_field_paints_selection_and_hides_the_cursor() {
        let palette = ThemeId::Dark.palette();
        let bounds = PixelRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 24,
        };
        let mut selected = vec![0; 100 * 24];
        render_inline_field(
            &mut selected,
            100,
            100,
            24,
            palette,
            bounds,
            "abc",
            true,
            true,
            palette.text,
        );
        assert!(
            selected
                .iter()
                .any(|pixel| *pixel == rgb_to_pixel(palette.selection_background))
        );
        let cursor_x = 2 + 3 * (GLYPH_WIDTH + 1);
        assert_eq!(
            selected[(2 * 100 + cursor_x) as usize],
            rgb_to_pixel(palette.composer)
        );

        let mut unselected = vec![0; 100 * 24];
        render_inline_field(
            &mut unselected,
            100,
            100,
            24,
            palette,
            bounds,
            "abc",
            true,
            false,
            palette.text,
        );
        assert_eq!(
            unselected[(2 * 100 + cursor_x) as usize],
            rgb_to_pixel(palette.text)
        );
    }

    #[test]
    fn narrow_tab_rows_prioritize_the_terminal_title_over_the_id() {
        assert_eq!(tab_row_title(1, "bash", 7), "@1 bash");
        assert_eq!(tab_row_title(1, "bash", 5), "bash");
        assert_eq!(tab_row_title(42, "开发终端", 3), "开发…");
        assert_eq!(tab_row_title(42, "", 3), "@4…");
    }

    #[test]
    fn grid_dimensions_use_the_exact_terminal_viewport() {
        let (cell_w, cell_h) = cell_metrics(12);
        let terminal_width = 800 - 200;
        let terminal_height = 480 - 46 - 48 - 26;
        assert_eq!(
            grid_dimensions_for_terminal(terminal_width, terminal_height, cell_w, cell_h),
            (
                ((terminal_width - SCROLLBAR_WIDTH) / cell_w) as u16,
                (terminal_height / cell_h) as u16
            )
        );
    }

    #[test]
    fn settings_hit_test_maps_buttons() {
        let modal = SettingsModalView::for_client(800, 600, 12, ThemeId::Dark);
        assert_eq!(
            modal.hit_test(
                f64::from(modal.apply_button.0 + 4),
                f64::from(modal.apply_button.1 + 4)
            ),
            Some(SettingsHit::Apply)
        );
        assert_eq!(
            modal.hit_test(
                f64::from(modal.font_family_field.0 + 2),
                f64::from(modal.font_family_field.1 + 2)
            ),
            None
        );
    }

    #[test]
    fn new_terminal_hit_test_maps_create_and_fields() {
        let modal = NewTerminalModalView::for_client(
            800,
            600,
            NewShellChoice::Default,
            "",
            "",
            "",
            NewTerminalFocusView::InitialCommand,
        );
        assert_eq!(
            modal.hit_test(
                f64::from(modal.create_button.0 + 4),
                f64::from(modal.create_button.1 + 4)
            ),
            Some(NewTerminalHit::Create)
        );
        assert_eq!(
            modal.hit_test(
                f64::from(modal.default_shell_button.0 + 4),
                f64::from(modal.default_shell_button.1 + 4)
            ),
            Some(NewTerminalHit::DefaultShell)
        );
    }

    #[test]
    fn hidden_sidebar_yields_wider_terminal_grid() {
        let (cell_w, cell_h) = cell_metrics(12);
        let without = grid_dimensions_for_terminal(800, 360, cell_w, cell_h).0;
        let with = grid_dimensions_for_terminal(600, 360, cell_w, cell_h).0;
        assert!(without > with);
        let _ = AppConfig::default();
    }

    #[test]
    fn rgb_to_pixel_uses_softbuffer_zero_rgb_format() {
        let pixel = rgb_to_pixel(Rgb {
            red: 12,
            green: 14,
            blue: 18,
        });
        assert_eq!(pixel, 0x000C0E12);
        assert_eq!(pixel & 0xFF000000, 0);
    }

    #[test]
    fn terminal_defaults_follow_theme_instead_of_ansi_slots() {
        let blank = TerminalCell::blank();
        assert_eq!(blank.text(), " ");
        assert_eq!(
            terminal_color(ThemeId::Dark.palette(), blank.fg, false),
            ThemeId::Dark.palette().terminal_foreground
        );
        assert_eq!(
            terminal_color(ThemeId::Light.palette(), blank.bg, true),
            ThemeId::Light.palette().terminal_background
        );
        assert_eq!(
            terminal_color(ThemeId::Light.palette(), TerminalColor::Indexed(0), false),
            ThemeId::Light.palette().ansi[0]
        );
    }

    #[test]
    fn terminal_colors_preserve_truecolor_and_xterm_256_palette() {
        let palette = ThemeId::Dark.palette();
        assert_eq!(
            terminal_color(
                palette,
                TerminalColor::TrueColor(Rgb::new(12, 34, 56)),
                false
            ),
            Rgb::new(12, 34, 56)
        );
        assert_eq!(
            terminal_color(palette, TerminalColor::Indexed(16), false),
            Rgb::new(0, 0, 0)
        );
        assert_eq!(
            terminal_color(palette, TerminalColor::Indexed(202), false),
            Rgb::new(255, 95, 0)
        );
        assert_eq!(
            terminal_color(palette, TerminalColor::Indexed(232), false),
            Rgb::new(8, 8, 8)
        );
        assert_eq!(
            terminal_color(palette, TerminalColor::Indexed(255), false),
            Rgb::new(238, 238, 238)
        );
    }

    #[test]
    fn terminal_grid_keeps_parser_rgb_and_indexed_colors() {
        let palette = ThemeId::Dark.palette();
        let mut parser = vt100::Parser::new(1, 2, 0);
        parser.process(b"\x1b[38;2;12;34;56mR\x1b[48;5;202mX");
        let mut grid = TerminalGrid::new(2, 1, palette);

        grid.sync_from_screen(parser.screen());

        assert_eq!(
            grid.cell(0, 0).fg,
            TerminalColor::TrueColor(Rgb::new(12, 34, 56))
        );
        assert_eq!(grid.cell(1, 0).bg, TerminalColor::Indexed(202));
        assert!(std::mem::size_of::<TerminalCell>() <= 32);
    }

    #[test]
    fn terminal_grid_keeps_parser_text_attributes() {
        let palette = ThemeId::Dark.palette();
        let mut parser = vt100::Parser::new(1, 4, 0);
        parser.process(b"\x1b[1mB\x1b[0m\x1b[2mD\x1b[0m\x1b[3mI\x1b[0m\x1b[4mU\x1b[0m");
        let mut grid = TerminalGrid::new(4, 1, palette);

        grid.sync_from_screen(parser.screen());

        assert!(grid.cell(0, 0).attributes.bold());
        assert!(grid.cell(1, 0).attributes.dim());
        assert!(grid.cell(2, 0).attributes.italic());
        assert!(grid.cell(3, 0).attributes.underline());
        assert!(std::mem::size_of::<TerminalCell>() <= 32);
    }

    #[test]
    fn terminal_cell_renders_bold_dim_italic_and_underline() {
        let foreground = Rgb::new(240, 240, 240);
        let background = Rgb::new(12, 12, 12);
        let (width, height) = cell_metrics(18);
        let render = |attributes| {
            let mut buffer = vec![rgb_to_pixel(background); (width * height) as usize];
            draw_cell(
                &mut buffer,
                width,
                width,
                height,
                0,
                0,
                width,
                height,
                "M",
                foreground,
                background,
                attributes,
                CELL_PADDING_X,
                CELL_PADDING_Y,
            );
            buffer
        };
        let foreground_count = |buffer: &[u32]| {
            buffer
                .iter()
                .filter(|pixel| **pixel == rgb_to_pixel(foreground))
                .count()
        };

        let regular = render(TerminalAttributes::default());
        let bold = render(TerminalAttributes(TerminalAttributes::BOLD));
        let dim = render(TerminalAttributes(TerminalAttributes::DIM));
        let italic = render(TerminalAttributes(TerminalAttributes::ITALIC));
        let underline = render(TerminalAttributes(TerminalAttributes::UNDERLINE));

        assert!(foreground_count(&bold) > foreground_count(&regular));
        assert_ne!(italic, regular);
        assert!(
            dim.iter()
                .any(|pixel| { *pixel == rgb_to_pixel(mix_rgb(foreground, background, 2, 3)) })
        );
        assert!(
            underline[((height - 1) * width) as usize..(height * width) as usize]
                .iter()
                .all(|pixel| *pixel == rgb_to_pixel(foreground))
        );
    }

    #[test]
    fn terminal_cell_keeps_vt100_combining_sequences_inline() {
        let palette = ThemeId::Dark.palette();
        let mut parser = vt100::Parser::new(1, 4, 0);
        parser.process("e\u{301} ♥\u{fe0f}".as_bytes());
        let mut grid = TerminalGrid::new(4, 1, palette);

        grid.sync_from_screen(parser.screen());

        assert_eq!(grid.cell(0, 0).text(), "e\u{301}");
        assert_eq!(grid.cell(2, 0).text(), "♥\u{fe0f}");
        assert!(std::mem::size_of::<TerminalCell>() <= 32);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_cell_renders_combining_marks() {
        let foreground = Rgb {
            red: 240,
            green: 240,
            blue: 240,
        };
        let background = Rgb {
            red: 10,
            green: 10,
            blue: 10,
        };
        let (width, height) = cell_metrics(18);
        let render = |text| {
            let mut buffer = vec![rgb_to_pixel(background); (width * height) as usize];
            draw_cell(
                &mut buffer,
                width,
                width,
                height,
                0,
                0,
                width,
                height,
                text,
                foreground,
                background,
                TerminalAttributes::default(),
                CELL_PADDING_X,
                CELL_PADDING_Y,
            );
            buffer
        };

        assert_ne!(render("e"), render("e\u{301}"));
    }

    #[test]
    fn terminal_cell_draws_replacement_for_missing_base_glyph() {
        let foreground = Rgb {
            red: 240,
            green: 240,
            blue: 240,
        };
        let background = Rgb {
            red: 10,
            green: 10,
            blue: 10,
        };
        let (width, height) = cell_metrics(12);
        let mut buffer = vec![rgb_to_pixel(background); (width * height) as usize];

        draw_cell(
            &mut buffer,
            width,
            width,
            height,
            0,
            0,
            width,
            height,
            "\u{10ffff}",
            foreground,
            background,
            TerminalAttributes::default(),
            CELL_PADDING_X,
            CELL_PADDING_Y,
        );

        assert!(
            buffer
                .iter()
                .any(|pixel| *pixel != rgb_to_pixel(background))
        );
    }

    #[test]
    fn larger_row_pitch_yields_fewer_terminal_rows() {
        let (cell_w, small_h) = cell_metrics(12);
        let (large_w, large_h) = cell_metrics(24);
        assert!(cell_w > 0);
        assert_eq!(small_h, 16);
        assert!(large_w > cell_w);
        assert!(large_h > small_h);
        let small_rows = grid_dimensions_for_terminal(600, 360, cell_w, small_h).1;
        let large_rows = grid_dimensions_for_terminal(600, 360, large_w, large_h).1;
        assert!(small_rows > large_rows);
    }

    #[test]
    fn terminal_glyph_scales_with_logical_point_size() {
        let foreground = Rgb {
            red: 240,
            green: 240,
            blue: 240,
        };
        let background = Rgb {
            red: 10,
            green: 10,
            blue: 10,
        };
        let count_foreground = |font_size| {
            let (width, height) = cell_metrics(font_size);
            let mut buffer = vec![0; (width * height) as usize];
            draw_cell(
                &mut buffer,
                width,
                width,
                height,
                0,
                0,
                width,
                height,
                "M",
                foreground,
                background,
                TerminalAttributes::default(),
                CELL_PADDING_X,
                CELL_PADDING_Y,
            );
            buffer
                .into_iter()
                .filter(|pixel| *pixel == rgb_to_pixel(foreground))
                .count()
        };

        assert!(count_foreground(24) > count_foreground(12));
    }

    #[test]
    fn sync_marks_only_changed_rows_dirty_and_layer_repaints_them() {
        let palette = ThemeId::Dark.palette();
        let mut parser = vt100::Parser::new(3, 4, 0);
        parser.process(b"aaaa\r\nbbbb\r\ncccc\x1b[?25l");
        let mut grid = TerminalGrid::new(4, 3, palette);
        grid.sync_from_screen(parser.screen());
        assert!(grid.any_row_dirty());
        grid.clear_dirty_rows();

        // Change only the middle row; park the cursor back where it was so
        // only the cell change dirties a row.
        parser.process(b"\x1b[2;1HBBBB\x1b[3;5H");
        grid.sync_from_screen(parser.screen());
        assert!(!grid.row_dirty(0));
        assert!(grid.row_dirty(1));
        assert!(!grid.row_dirty(2));

        // A partial layer repaint updates the dirty row and leaves clean rows.
        let (cell_width, cell_height) = cell_metrics(12);
        let geometry = TerminalLayerGeometry {
            offset_x: 0,
            offset_y: 0,
            width: cell_width * 4,
            height: cell_height * 3,
            cell_width,
            cell_height,
            padding_x: CELL_PADDING_X,
            padding_y: CELL_PADDING_Y,
        };
        fn paint(grid: &TerminalGrid) -> TerminalPaint<'_> {
            TerminalPaint {
                grid,
                selection: None,
                cursor_style: TerminalCursorStyle::Hidden,
                cursor_shape: TerminalCursorShape::Block,
            }
        }
        let sentinel = 0x00ff_00ffu32;
        let mut layer = vec![sentinel; (geometry.width * geometry.height) as usize];
        render_terminal_layer(
            &mut layer,
            geometry,
            paint(&grid),
            palette,
            false,
            [None, None],
        );
        let row_pixels = (geometry.width * cell_height) as usize;
        assert!(layer[..row_pixels].iter().all(|pixel| *pixel == sentinel));
        assert!(
            layer[row_pixels..2 * row_pixels]
                .iter()
                .all(|pixel| *pixel != sentinel)
        );
        assert!(
            layer[2 * row_pixels..]
                .iter()
                .all(|pixel| *pixel == sentinel)
        );

        // A full repaint covers every row.
        render_terminal_layer(
            &mut layer,
            geometry,
            paint(&grid),
            palette,
            true,
            [None, None],
        );
        assert!(layer.iter().all(|pixel| *pixel != sentinel));
    }

    #[test]
    fn hidpi_terminal_grid_rasterizes_at_physical_resolution() {
        let palette = ThemeId::Dark.palette();
        let (cell_width, cell_height) = cell_metrics(14);
        let logical_width = cell_width + SCROLLBAR_WIDTH;
        let logical_height = cell_height;
        let mut parser = vt100::Parser::new(1, 1, 0);
        parser.process(b"M\x1b[?25l");
        let mut grid = TerminalGrid::new(1, 1, palette);
        grid.sync_from_screen(parser.screen());
        let paint = || TerminalPaint {
            grid: &grid,
            selection: None,
            cursor_style: TerminalCursorStyle::Hidden,
            cursor_shape: TerminalCursorShape::Block,
        };
        let mut logical = vec![
            rgb_to_pixel(palette.terminal_background);
            (logical_width * logical_height) as usize
        ];
        render_terminal_grid(
            &mut logical,
            logical_width,
            paint(),
            palette,
            TerminalGridLayout {
                width: logical_width,
                height: logical_height,
                offset_x: 0,
                offset_y: 0,
                cell_width,
                cell_height,
                padding_x: CELL_PADDING_X,
                padding_y: CELL_PADDING_Y,
                scrollbar_width: SCROLLBAR_WIDTH,
            },
        );

        let physical_width = logical_width * 2;
        let physical_height = logical_height * 2;
        let mut physical = vec![0; (physical_width * physical_height) as usize];
        for y in 0..physical_height {
            for x in 0..physical_width {
                physical[(y * physical_width + x) as usize] =
                    logical[((y / 2) * logical_width + x / 2) as usize];
            }
        }
        let nearest = physical.clone();

        let geometry = terminal_layer_geometry(
            physical_width,
            physical_height,
            logical_width,
            logical_height,
            logical_height,
            0,
            0,
            cell_width,
            cell_height,
            grid.cols,
            grid.rows,
        )
        .expect("layer geometry");
        let mut layer = vec![0; (geometry.width * geometry.height) as usize];
        render_terminal_layer(&mut layer, geometry, paint(), palette, true, [None, None]);
        blit_terminal_layer(
            &mut physical,
            physical_width,
            physical_height,
            &layer,
            geometry,
        );

        assert_ne!(physical, nearest);
        for row in 0..physical_height {
            let start = (row * physical_width + cell_width * 2) as usize;
            let end = ((row + 1) * physical_width) as usize;
            assert_eq!(&physical[start..end], &nearest[start..end]);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_cell_renders_cjk_system_fallback() {
        let foreground = Rgb {
            red: 240,
            green: 240,
            blue: 240,
        };
        let background = Rgb {
            red: 10,
            green: 10,
            blue: 10,
        };
        let (cell_width, cell_height) = cell_metrics(12);
        let width = cell_width * 2;
        let mut buffer = vec![rgb_to_pixel(background); (width * cell_height) as usize];
        draw_cell(
            &mut buffer,
            width,
            width,
            cell_height,
            0,
            0,
            width,
            cell_height,
            "繁",
            foreground,
            background,
            TerminalAttributes::default(),
            CELL_PADDING_X,
            CELL_PADDING_Y,
        );

        assert!(
            buffer
                .iter()
                .any(|pixel| *pixel != rgb_to_pixel(background))
        );
    }

    #[test]
    fn ime_preedit_draws_visible_composition_and_cursor() {
        let width = 180;
        let height = 80;
        let palette = ThemeId::Dark.palette();
        let background = rgb_to_pixel(palette.terminal_background);
        let mut buffer = vec![background; (width * height) as usize];

        render_ime_preedit(
            &mut buffer,
            width,
            width,
            height,
            palette,
            ImePreeditView {
                text: "中文",
                cursor: Some(("中".len(), "中".len())),
                anchor: (12, 12, 10, 16),
            },
        );

        assert!(buffer.iter().any(|pixel| *pixel != background));
        assert!(
            buffer
                .iter()
                .any(|pixel| *pixel == rgb_to_pixel(palette.focus_ring))
        );
    }

    #[test]
    fn terminal_cursor_tracks_parser_position_and_hide_mode() {
        let palette = ThemeId::Dark.palette();
        let (cell_width, cell_height) = cell_metrics(12);
        let cols = 4;
        let rows = 2;
        let width = u32::from(cols) * cell_width + SCROLLBAR_WIDTH;
        let height = u32::from(rows) * cell_height;
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(b"ab");
        let mut grid = TerminalGrid::new(cols, rows, palette);
        grid.sync_from_screen(parser.screen());
        let mut buffer = vec![rgb_to_pixel(palette.terminal_background); (width * height) as usize];
        let layout = TerminalGridLayout {
            width,
            height,
            offset_x: 0,
            offset_y: 0,
            cell_width,
            cell_height,
            padding_x: CELL_PADDING_X,
            padding_y: CELL_PADDING_Y,
            scrollbar_width: SCROLLBAR_WIDTH,
        };

        render_terminal_grid(
            &mut buffer,
            width,
            TerminalPaint {
                grid: &grid,
                selection: None,
                cursor_style: TerminalCursorStyle::Active,
                cursor_shape: TerminalCursorShape::Underline,
            },
            palette,
            layout,
        );
        let cursor_x = 2 * cell_width;
        let cursor_y = cell_height - 2;
        assert_eq!(
            buffer[(cursor_y * width + cursor_x) as usize],
            rgb_to_pixel(palette.focus_ring)
        );

        buffer.fill(rgb_to_pixel(palette.terminal_background));
        render_terminal_grid(
            &mut buffer,
            width,
            TerminalPaint {
                grid: &grid,
                selection: None,
                cursor_style: TerminalCursorStyle::Inactive,
                cursor_shape: TerminalCursorShape::Block,
            },
            palette,
            layout,
        );
        assert_eq!(buffer[cursor_x as usize], rgb_to_pixel(palette.muted_text));

        parser.process(b"\x1b[?25l");
        grid.sync_from_screen(parser.screen());
        buffer.fill(rgb_to_pixel(palette.terminal_background));
        render_terminal_grid(
            &mut buffer,
            width,
            TerminalPaint {
                grid: &grid,
                selection: None,
                cursor_style: TerminalCursorStyle::Active,
                cursor_shape: TerminalCursorShape::Underline,
            },
            palette,
            layout,
        );
        assert_ne!(
            buffer[(cursor_y * width + cursor_x) as usize],
            rgb_to_pixel(palette.focus_ring)
        );
    }

    #[test]
    fn terminal_cursor_renders_block_underline_and_bar_shapes() {
        let palette = ThemeId::Dark.palette();
        let (cell_width, cell_height) = cell_metrics(12);
        let width = cell_width + SCROLLBAR_WIDTH;
        let parser = vt100::Parser::new(1, 1, 0);
        let mut grid = TerminalGrid::new(1, 1, palette);
        grid.sync_from_screen(parser.screen());
        let layout = TerminalGridLayout {
            width,
            height: cell_height,
            offset_x: 0,
            offset_y: 0,
            cell_width,
            cell_height,
            padding_x: CELL_PADDING_X,
            padding_y: CELL_PADDING_Y,
            scrollbar_width: SCROLLBAR_WIDTH,
        };
        let background = rgb_to_pixel(palette.terminal_background);
        let accent = rgb_to_pixel(palette.focus_ring);
        let render_shape = |shape| {
            let mut buffer = vec![background; (width * cell_height) as usize];
            render_terminal_grid(
                &mut buffer,
                width,
                TerminalPaint {
                    grid: &grid,
                    selection: None,
                    cursor_style: TerminalCursorStyle::Active,
                    cursor_shape: shape,
                },
                palette,
                layout,
            );
            buffer
        };

        let block = render_shape(TerminalCursorShape::Block);
        assert_eq!(
            block[(cell_height / 2 * width + cell_width / 2) as usize],
            accent
        );

        let underline = render_shape(TerminalCursorShape::Underline);
        assert_eq!(underline[((cell_height - 2) * width) as usize], accent);
        assert_eq!(underline[(cell_height / 2 * width) as usize], background);

        let bar = render_shape(TerminalCursorShape::Bar);
        assert_eq!(bar[(cell_height / 2 * width) as usize], accent);
        assert_eq!(bar[(cell_height / 2 * width + 4) as usize], background);
    }
}
