use crate::theme::{Rgb, ThemeId, ThemePalette};
use crate::ui_geometry::{
    PixelRect, TreeRowMode, sidebar_tree_row_geometry, tree_connector_segments, tree_row_at_y,
};
use unicode_width::UnicodeWidthChar;

use super::{
    font::GLYPH_HEIGHT,
    font::GLYPH_WIDTH,
    layout::{SCROLLBAR_WIDTH, u32_rect},
};

pub(super) const COMPOSER_HEIGHT: u32 = 48;
pub(super) const STATUS_HEIGHT: u32 = 26;
pub(super) const SETTINGS_MODAL_WIDTH: u32 = 480;
pub(super) const SETTINGS_MODAL_HEIGHT: u32 = 330;
pub(super) const NEW_TERMINAL_MODAL_MIN_WIDTH: u32 = 480;
pub(super) const NEW_TERMINAL_MODAL_MAX_WIDTH: u32 = 620;
pub(super) const NEW_TERMINAL_MODAL_MIN_HEIGHT: u32 = 390;
pub(super) const NEW_TERMINAL_MODAL_MAX_HEIGHT: u32 = 450;

/// Unix GUI currently rasterizes with the built-in 8×8 bitmap, not host TTF families.
pub(super) const RESOLVED_UNIX_FONT: &str = "bitmap-8x8";

pub(super) const CELL_WIDTH: u32 = 10;
pub(super) const CELL_HEIGHT: u32 = 16;
pub(super) const CELL_PADDING_X: u32 = 1;
pub(super) const CELL_PADDING_Y: u32 = 4;

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalCell {
    pub(super) ch: char,
    pub(super) fg: u8,
    pub(super) bg: u8,
}

impl TerminalCell {
    pub(super) const fn blank() -> Self {
        Self {
            ch: ' ',
            fg: 7,
            bg: 0,
        }
    }

    pub(super) fn with_defaults(ch: char, _palette: &ThemePalette) -> Self {
        Self { ch, fg: 7, bg: 0 }
    }
}

pub(super) struct TerminalGrid {
    pub(super) cols: u16,
    pub(super) rows: u16,
    cells: Vec<TerminalCell>,
    palette: &'static ThemePalette,
}

impl TerminalGrid {
    pub(super) fn new(cols: u16, rows: u16, palette: &'static ThemePalette) -> Self {
        Self {
            cols,
            rows,
            cells: vec![TerminalCell::blank(); usize::from(cols) * usize::from(rows)],
            palette,
        }
    }

    pub(super) fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.cells
            .resize(usize::from(cols) * usize::from(rows), TerminalCell::blank());
    }

    pub(super) fn sync_from_screen(&mut self, screen: &vt100::Screen) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = screen
                    .cell(row, col)
                    .map_or_else(TerminalCell::blank, |cell| {
                        if cell.is_wide_continuation() {
                            return TerminalCell::blank();
                        }
                        let text = cell.contents();
                        let ch = text.chars().next().unwrap_or(' ');
                        let ch = if text.is_empty() { ' ' } else { ch };
                        let mut fg = color_index(cell.fgcolor(), false);
                        let mut bg = color_index(cell.bgcolor(), true);
                        if cell.inverse() {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                        TerminalCell { ch, fg, bg }
                    });
                self.set_cell(col, row, cell);
            }
        }
    }

    pub(super) fn cell(&self, col: u16, row: u16) -> TerminalCell {
        self.cells[self.index(col, row)]
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

fn color_index(color: vt100::Color, background: bool) -> u8 {
    match color {
        vt100::Color::Default if background => 0,
        vt100::Color::Default => 7,
        vt100::Color::Idx(index) => index,
        vt100::Color::Rgb(_, _, _) => 7,
    }
}

pub(super) fn cell_metrics(font_size: u16) -> (u32, u32) {
    let pitch = u32::from(font_size.clamp(8, 36));
    let cell_h = pitch.max(8 + CELL_PADDING_Y * 2);
    let cell_w = GLYPH_WIDTH + CELL_PADDING_X * 2;
    (cell_w, cell_h)
}

pub(super) fn grid_dimensions_for_pixels(
    width: u32,
    height: u32,
    sidebar_width: u32,
    composer_height: u32,
    status_height: u32,
    cell_width: u32,
    cell_height: u32,
) -> (u16, u16) {
    let terminal_width = width
        .saturating_sub(sidebar_width)
        .saturating_sub(SCROLLBAR_WIDTH);
    let terminal_height = height
        .saturating_sub(composer_height)
        .saturating_sub(status_height);
    let cols = (terminal_width / cell_width.max(1)).max(1) as u16;
    let rows = (terminal_height / cell_height.max(1)).max(1) as u16;
    (cols, rows)
}

pub(super) struct ComposerView<'a> {
    pub(super) text: &'a str,
    pub(super) focused: bool,
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
pub(super) enum SettingsFocusView {
    FontFamily,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsHit {
    FontFamily,
    Dark,
    Light,
    SizeDecrease,
    SizeIncrease,
    Cancel,
    Apply,
}

#[derive(Clone, Debug)]
pub(super) struct SettingsModalView<'a> {
    pub(super) font_family: &'a str,
    pub(super) font_size: u16,
    pub(super) theme_draft: ThemeId,
    pub(super) focus: SettingsFocusView,
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) font_family_field: (u32, u32, u32, u32),
    pub(super) dark_button: (u32, u32, u32, u32),
    pub(super) light_button: (u32, u32, u32, u32),
    pub(super) cancel_button: (u32, u32, u32, u32),
    pub(super) apply_button: (u32, u32, u32, u32),
    pub(super) size_decrease_button: (u32, u32, u32, u32),
    pub(super) size_increase_button: (u32, u32, u32, u32),
}

impl SettingsModalView<'_> {
    pub(super) fn hit_test(&self, x: f64, y: f64) -> Option<SettingsHit> {
        let x = x as u32;
        let y = y as u32;
        if rect_contains(self.font_family_field, x, y) {
            return Some(SettingsHit::FontFamily);
        }
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
        font_family: &str,
        font_size: u16,
        theme_draft: ThemeId,
        focus: SettingsFocusView,
    ) -> SettingsModalView<'_> {
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
            font_family,
            font_size,
            theme_draft,
            focus,
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
    CommandPrompt,
    PowerShell,
}

impl NewShellChoice {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::CommandPrompt => "Command Prompt",
            Self::PowerShell => "PowerShell",
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
    CmdShell,
    PowerShell,
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
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) default_shell_button: (u32, u32, u32, u32),
    pub(super) cmd_shell_button: (u32, u32, u32, u32),
    pub(super) powershell_button: (u32, u32, u32, u32),
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
        if rect_contains(self.cmd_shell_button, x, y) {
            return Some(NewTerminalHit::CmdShell);
        }
        if rect_contains(self.powershell_button, x, y) {
            return Some(NewTerminalHit::PowerShell);
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
            bounds: (left, top, width, height),
            default_shell_button: shell_button(0),
            cmd_shell_button: shell_button(1),
            powershell_button: shell_button(2),
            initial_command_field: field(top + 142),
            http_proxy_field: field(top + 218),
            https_proxy_field: field(top + 294),
            create_button,
            cancel_button,
        }
    }
}

pub(super) struct TerminalPaint<'a> {
    pub(super) grid: &'a TerminalGrid,
    pub(super) selection: Option<crate::terminal_selection::TerminalSelection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToolbarHit {
    NewTab,
    ToggleTabs,
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

pub(super) struct FrameContent<'a> {
    pub(super) sidebar_width: u32,
    pub(super) content_height: u32,
    pub(super) tree_height: u32,
    pub(super) cell_width: u32,
    pub(super) cell_height: u32,
    pub(super) terminal: TerminalPaint<'a>,
    pub(super) sidebar_rows: &'a [SidebarTabRow],
    pub(super) sidebar_tree: PixelRect,
    pub(super) editing_tab_id: Option<u64>,
    pub(super) tab_editor: Option<TabEditorView>,
    pub(super) workspace_toolbar: Option<WorkspaceToolbarView>,
    pub(super) terminal_top: u32,
    pub(super) composer: ComposerView<'a>,
    pub(super) scrollbar: Option<ScrollbarView>,
    pub(super) sidebar_scrollbar: Option<ScrollbarView>,
    pub(super) settings: Option<SettingsModalView<'a>>,
    pub(super) new_terminal: Option<NewTerminalModalView<'a>>,
    pub(super) confirm_close: Option<ConfirmCloseView>,
    pub(super) window_close: Option<WindowCloseView>,
    pub(super) status: Option<StatusBarView<'a>>,
    pub(super) resize_grip: Option<(u32, u32, u32, u32)>,
}

struct TerminalGridLayout {
    width: u32,
    height: u32,
    offset_x: u32,
    offset_y: u32,
    cell_width: u32,
    cell_height: u32,
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
        },
    );
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
    let label = if composer.text.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}{}", composer.text)
    };
    let max_chars = (text_width / (GLYPH_WIDTH + 1)).max(1) as usize;
    draw_text(
        buffer,
        stride,
        width,
        height,
        sidebar_width + 8,
        top + 20,
        &truncate_chars(&label, max_chars),
        palette.text,
    );
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
            palette.selection_foreground,
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
        ("+", if toolbar.tabs_visible { "<T" } else { ">T" }, "S")
    } else {
        (
            "New",
            if toolbar.tabs_visible {
                "<Tabs"
            } else {
                ">Tabs"
            },
            "Settings",
        )
    };
    for (rect, label) in [
        (toolbar.new_tab, labels.0),
        (toolbar.tabs, labels.1),
        (toolbar.settings, labels.2),
        (toolbar.locale, toolbar.locale_id.toolbar_label()),
        (toolbar.font_decrease, "z"),
        (toolbar.font_increase, "Z"),
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
        let text_color = if row.active {
            palette.selection_foreground
        } else {
            palette.text
        };
        draw_text(
            buffer, stride, width, height, marker_x, marker_y, marker, text_color,
        );

        if editing && let Some(editor) = tab_editor {
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
                "Save",
                true,
            );
            render_tree_action_button(
                buffer,
                stride,
                width,
                height,
                palette,
                geometry.actions.secondary,
                "Cancel",
                false,
            );
        } else {
            let name_x = geometry.name.left.max(0) as u32;
            let name_y = geometry.name.top.max(0) as u32;
            let name_chars =
                (geometry.name.width().max(0) as u32 / (GLYPH_WIDTH + 1)).max(1) as usize;
            let title = truncate_chars(&format!("@{} {}", row.id, row.title), name_chars);
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
                if let Some(add_child) = geometry.actions.add_child {
                    render_tree_action_button(
                        buffer, stride, width, height, palette, add_child, "Add", false,
                    );
                }
                render_tree_action_button(
                    buffer,
                    stride,
                    width,
                    height,
                    palette,
                    geometry.actions.primary,
                    "Edit",
                    false,
                );
                render_tree_action_button(
                    buffer,
                    stride,
                    width,
                    height,
                    palette,
                    geometry.actions.secondary,
                    "Close",
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
    draw_text(
        buffer,
        stride,
        width,
        height,
        x + 2,
        y + 2,
        &label,
        text_color,
    );
    if focused {
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
    let text_color = if primary {
        palette.selection_foreground
    } else {
        palette.text
    };
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
    let terminal_width = layout
        .width
        .saturating_sub(layout.offset_x)
        .saturating_sub(SCROLLBAR_WIDTH);
    let selection_fg = palette.selection_foreground;
    let selection_bg = palette.selection_background;
    for row in 0..terminal.grid.rows {
        let mut col = 0;
        while col < terminal.grid.cols {
            let x = layout.offset_x + u32::from(col) * layout.cell_width;
            if x + layout.cell_width > layout.offset_x + terminal_width {
                break;
            }
            let cell = terminal.grid.cell(col, row);
            let wide = cell.ch.width() == Some(2);
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
                ansi_color(palette, cell.fg)
            };
            let bg = if selected {
                selection_bg
            } else {
                ansi_color(palette, cell.bg)
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
                if wide && cell.ch != ' ' {
                    if cell.ch.is_ascii() {
                        cell.ch
                    } else {
                        '\u{25A0}'
                    }
                } else {
                    cell.ch
                },
                fg,
                bg,
            );
            col += if wide { 2 } else { 1 };
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
    settings: SettingsModalView<'_>,
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
        &format!("Renderer: {}", RESOLVED_UNIX_FONT),
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 32,
        my + 58,
        "Terminal font family",
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + mw.saturating_sub(110),
        my + 58,
        "Size",
        palette.muted_text,
    );
    let (fx, fy, fw, fh) = settings.font_family_field;
    render_inline_field(
        buffer,
        stride,
        width,
        height,
        palette,
        PixelRect {
            left: fx as i32,
            top: fy as i32,
            right: (fx + fw) as i32,
            bottom: (fy + fh) as i32,
        },
        settings.font_family,
        settings.focus == SettingsFocusView::FontFamily,
        palette.text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 32,
        my + 66,
        &format!("{} px", settings.font_size),
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
        (modal.cmd_shell_button, NewShellChoice::CommandPrompt),
        (modal.powershell_button, NewShellChoice::PowerShell),
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
    ch: char,
    fg: Rgb,
    bg: Rgb,
) {
    if origin_x >= width || origin_y >= height {
        return;
    }

    let cell_w = cell_width.min(width - origin_x);
    let cell_h = cell_height.min(height - origin_y);
    let bg_pixel = rgb_to_pixel(bg);
    fill_rect(buffer, stride, origin_x, origin_y, cell_w, cell_h, bg_pixel);

    let Some(glyph) = super::font::glyph_rows(ch) else {
        return;
    };

    let glyph_x = origin_x + CELL_PADDING_X;
    let glyph_y = origin_y + CELL_PADDING_Y;
    let fg_pixel = rgb_to_pixel(fg);
    for (row_index, row_bits) in glyph.iter().enumerate() {
        let y = glyph_y + row_index as u32;
        if y >= origin_y + cell_h {
            break;
        }
        for bit in 0..GLYPH_WIDTH {
            if !super::font::row_contains_pixel(*row_bits, bit) {
                continue;
            }
            let x = glyph_x + bit;
            if x < width {
                put_pixel(buffer, stride, x, y, fg_pixel);
            }
        }
    }
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
    let fg_pixel = rgb_to_pixel(fg);
    let mut x = origin_x;
    for ch in text.chars() {
        let Some(glyph) = super::font::glyph_rows(ch) else {
            x = x.saturating_add(GLYPH_WIDTH + 1);
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
                let pixel_x = x + bit;
                if pixel_x < width {
                    put_pixel(buffer, stride, pixel_x, y, fg_pixel);
                }
            }
        }
        x = x.saturating_add(GLYPH_WIDTH + 1);
        if x >= width {
            break;
        }
    }
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

fn rect_contains(rect: (u32, u32, u32, u32), x: u32, y: u32) -> bool {
    let (left, top, width, height) = rect;
    x >= left && y >= top && x < left + width && y < top + height
}

fn ansi_color(palette: &ThemePalette, index: u8) -> Rgb {
    palette.ansi[(index & 0x0F) as usize]
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

    #[test]
    fn grid_dimensions_account_for_sidebar_scrollbar_and_composer() {
        let (cell_w, cell_h) = cell_metrics(12);
        assert_eq!(
            grid_dimensions_for_pixels(800, 480, 200, 48, 26, cell_w, cell_h),
            (58, 25)
        );
    }

    #[test]
    fn settings_hit_test_maps_buttons() {
        let modal = SettingsModalView::for_client(
            800,
            600,
            "Consolas",
            12,
            ThemeId::Dark,
            SettingsFocusView::FontFamily,
        );
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
            Some(SettingsHit::FontFamily)
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
        let without = grid_dimensions_for_pixels(800, 480, 0, 48, 26, cell_w, cell_h).0;
        let with = grid_dimensions_for_pixels(800, 480, 200, 48, 26, cell_w, cell_h).0;
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
    fn larger_row_pitch_yields_fewer_terminal_rows() {
        let (cell_w, small_h) = cell_metrics(12);
        let (large_w, large_h) = cell_metrics(24);
        assert_eq!(cell_w, large_w);
        assert!(large_h > small_h);
        let small_rows = grid_dimensions_for_pixels(800, 480, 200, 48, 26, cell_w, small_h).1;
        let large_rows = grid_dimensions_for_pixels(800, 480, 200, 48, 26, large_w, large_h).1;
        assert!(small_rows > large_rows);
    }
}
