use crate::theme::{Rgb, ThemeId, ThemePalette};

use super::{
    font::GLYPH_WIDTH,
    layout::{SCROLLBAR_WIDTH, u32_rect},
};

pub(super) const SIDEBAR_TAB_ROW_HEIGHT: u32 = 20;
pub(super) const COMPOSER_HEIGHT: u32 = 48;
pub(super) const SETTINGS_MODAL_WIDTH: u32 = 360;
pub(super) const SETTINGS_MODAL_HEIGHT: u32 = 220;

pub(super) const CELL_WIDTH: u32 = 10;
pub(super) const CELL_HEIGHT: u32 = 16;
pub(super) const CELL_PADDING_X: u32 = 1;
pub(super) const CELL_PADDING_Y: u32 = 4;

#[derive(Clone, Debug)]
pub(super) struct SidebarTabRow {
    pub(super) id: u64,
    pub(super) depth: usize,
    pub(super) title: String,
    pub(super) active: bool,
    pub(super) collapsed: bool,
    pub(super) has_children: bool,
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

pub(super) fn grid_dimensions_for_pixels(
    width: u32,
    height: u32,
    sidebar_width: u32,
    composer_height: u32,
) -> (u16, u16) {
    let terminal_width = width
        .saturating_sub(sidebar_width)
        .saturating_sub(SCROLLBAR_WIDTH);
    let terminal_height = height.saturating_sub(composer_height);
    let cols = (terminal_width / CELL_WIDTH).max(1) as u16;
    let rows = (terminal_height / CELL_HEIGHT).max(1) as u16;
    (cols, rows)
}

pub(super) struct ComposerView<'a> {
    pub(super) text: &'a str,
    pub(super) focused: bool,
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
    Cancel,
    Apply,
}

#[derive(Clone, Debug)]
pub(super) struct SettingsModalView<'a> {
    pub(super) font_family: &'a str,
    pub(super) font_size: u16,
    pub(super) theme_draft: ThemeId,
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) dark_button: (u32, u32, u32, u32),
    pub(super) light_button: (u32, u32, u32, u32),
    pub(super) cancel_button: (u32, u32, u32, u32),
    pub(super) apply_button: (u32, u32, u32, u32),
}

impl SettingsModalView<'_> {
    pub(super) fn hit_test(&self, x: f64, y: f64) -> Option<SettingsHit> {
        let x = x as u32;
        let y = y as u32;
        if rect_contains(self.dark_button, x, y) {
            return Some(SettingsHit::Dark);
        }
        if rect_contains(self.light_button, x, y) {
            return Some(SettingsHit::Light);
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
    ) -> SettingsModalView<'_> {
        let left = (client_width.saturating_sub(SETTINGS_MODAL_WIDTH)) / 2;
        let top = (client_height.saturating_sub(SETTINGS_MODAL_HEIGHT)) / 2;
        let button_y = top + 150;
        SettingsModalView {
            font_family,
            font_size,
            theme_draft,
            bounds: (left, top, SETTINGS_MODAL_WIDTH, SETTINGS_MODAL_HEIGHT),
            dark_button: (left + 16, button_y, 120, 28),
            light_button: (left + 148, button_y, 120, 28),
            cancel_button: (left + 16, button_y + 40, 120, 28),
            apply_button: (left + 148, button_y + 40, 120, 28),
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
}

#[derive(Clone, Debug)]
pub(super) struct SidebarToolbarView {
    pub(super) bounds: (u32, u32, u32, u32),
    pub(super) new_tab: (u32, u32, u32, u32),
    pub(super) tabs: (u32, u32, u32, u32),
    pub(super) settings: (u32, u32, u32, u32),
    pub(super) compact: bool,
}

impl SidebarToolbarView {
    pub(super) fn from_layout(
        toolbar: crate::ui_geometry::SidebarToolbarLayout,
    ) -> Self {
        Self {
            bounds: u32_rect(toolbar.bounds),
            new_tab: u32_rect(toolbar.new_tab),
            tabs: u32_rect(toolbar.tabs),
            settings: u32_rect(toolbar.settings),
            compact: matches!(
                toolbar.mode,
                crate::ui_geometry::SidebarToolbarMode::Compact
            ),
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
        None
    }
}

pub(super) struct FrameContent<'a> {
    pub(super) sidebar_width: u32,
    pub(super) content_height: u32,
    pub(super) tree_height: u32,
    pub(super) terminal: TerminalPaint<'a>,
    pub(super) sidebar_rows: &'a [SidebarTabRow],
    pub(super) sidebar_toolbar: Option<SidebarToolbarView>,
    pub(super) composer: ComposerView<'a>,
    pub(super) scrollbar: Option<ScrollbarView>,
    pub(super) settings: Option<SettingsModalView<'a>>,
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
            content.sidebar_rows,
            content.sidebar_width,
        );
        if let Some(toolbar) = content.sidebar_toolbar {
            render_sidebar_toolbar(buffer, stride, width, height, palette, toolbar);
        }
    }
    render_terminal_grid(
        buffer,
        stride,
        width,
        content.content_height,
        content.terminal,
        palette,
        content.sidebar_width,
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
        content.composer.text,
        content.composer.focused,
    );
    if let Some(settings) = content.settings {
        render_settings_modal(buffer, stride, width, height, palette, settings);
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

#[allow(clippy::too_many_arguments)]
fn render_composer(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    sidebar_width: u32,
    text: &str,
    focused: bool,
) {
    let top = height.saturating_sub(COMPOSER_HEIGHT);
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
    if focused {
        let ring = rgb_to_pixel(palette.focus_ring);
        fill_rect(buffer, stride, sidebar_width, top, composer_width, 2, ring);
    }
    let label = if text.is_empty() {
        "> ".to_owned()
    } else {
        format!("> {text}")
    };
    let max_chars = ((composer_width.saturating_sub(16)) / (GLYPH_WIDTH + 1)).max(1) as usize;
    draw_text(
        buffer,
        stride,
        width,
        height,
        sidebar_width + 8,
        top + 16,
        &truncate_chars(&label, max_chars),
        palette.text,
    );
}

fn render_sidebar_toolbar(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    toolbar: SidebarToolbarView,
) {
    let (bx, by, bw, bh) = toolbar.bounds;
    let bg = rgb_to_pixel(palette.sidebar);
    fill_rect(buffer, stride, bx, by, bw, bh, bg);
    let divider = rgb_to_pixel(palette.divider);
    fill_rect(buffer, stride, bx, by, bw, 1, divider);
    let button_bg = rgb_to_pixel(palette.composer);
    let labels = if toolbar.compact {
        ("+", "T", "S")
    } else {
        ("New", "Tabs", "Settings")
    };
    for (rect, label) in [
        (toolbar.new_tab, labels.0),
        (toolbar.tabs, labels.1),
        (toolbar.settings, labels.2),
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

fn render_sidebar(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    palette: &ThemePalette,
    rows: &[SidebarTabRow],
    sidebar_width: u32,
) {
    let sidebar_width = sidebar_width.min(width);
    let sidebar_bg = rgb_to_pixel(palette.sidebar);
    fill_rect(buffer, stride, 0, 0, sidebar_width, height, sidebar_bg);

    let divider = rgb_to_pixel(palette.divider);
    if sidebar_width > 0 && sidebar_width < width {
        fill_rect(
            buffer,
            stride,
            sidebar_width.saturating_sub(1),
            0,
            1,
            height,
            divider,
        );
    }

    for (index, row) in rows.iter().enumerate() {
        let top = index as u32 * SIDEBAR_TAB_ROW_HEIGHT;
        if top >= height {
            break;
        }
        let row_height = SIDEBAR_TAB_ROW_HEIGHT.min(height.saturating_sub(top));
        let indent = 8 + u32::try_from(row.depth).unwrap_or(0).saturating_mul(12);
        let text_x = indent.min(sidebar_width.saturating_sub(1));
        let marker = if row.has_children {
            if row.collapsed { "[+]" } else { "[-]" }
        } else {
            "   "
        };
        let label = format!("{marker} @{} {}", row.id, row.title);
        let max_chars =
            ((sidebar_width.saturating_sub(text_x)) / (GLYPH_WIDTH + 1)).max(1) as usize;
        let clipped = truncate_chars(&label, max_chars);

        if row.active {
            let active_bg = rgb_to_pixel(palette.active);
            fill_rect(buffer, stride, 0, top, sidebar_width, row_height, active_bg);
            draw_text(
                buffer,
                stride,
                width,
                height,
                text_x,
                top + 4,
                &clipped,
                palette.selection_foreground,
            );
        } else {
            draw_text(
                buffer,
                stride,
                width,
                height,
                text_x,
                top + 4,
                &clipped,
                palette.text,
            );
        }
    }
}

fn render_terminal_grid(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    terminal: TerminalPaint<'_>,
    palette: &ThemePalette,
    offset_x: u32,
) {
    let terminal_width = width
        .saturating_sub(offset_x)
        .saturating_sub(SCROLLBAR_WIDTH);
    let selection_fg = palette.selection_foreground;
    let selection_bg = palette.selection_background;
    for row in 0..terminal.grid.rows {
        for col in 0..terminal.grid.cols {
            let x = offset_x + u32::from(col) * CELL_WIDTH;
            if x + CELL_WIDTH > offset_x + terminal_width {
                break;
            }
            let cell = terminal.grid.cell(col, row);
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
                width,
                height,
                x,
                u32::from(row) * CELL_HEIGHT,
                cell.ch,
                fg,
                bg,
            );
        }
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
    let overlay = rgb_to_pixel(Rgb {
        red: 0,
        green: 0,
        blue: 0,
    });
    for y in 0..height {
        for x in 0..width {
            let index = (y * stride + x) as usize;
            if let Some(pixel) = buffer.get_mut(index) {
                let base = *pixel;
                let r = ((base >> 16) & 0xFF) * 2 / 5;
                let g = ((base >> 8) & 0xFF) * 2 / 5;
                let b = (base & 0xFF) * 2 / 5;
                *pixel = overlay & 0xFF00_0000 | (r << 16) | (g << 8) | b;
            }
        }
    }

    let (mx, my, mw, mh) = settings.bounds;
    let modal_bg = rgb_to_pixel(palette.modal);
    fill_rect(buffer, stride, mx, my, mw, mh, modal_bg);
    let border = rgb_to_pixel(palette.focus_ring);
    fill_rect(buffer, stride, mx, my, mw, 2, border);
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 12,
        my + 12,
        "Settings",
        palette.text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 12,
        my + 36,
        &format!("Font: {}", settings.font_family),
        palette.muted_text,
    );
    draw_text(
        buffer,
        stride,
        width,
        height,
        mx + 12,
        my + 52,
        &format!("Size: {}", settings.font_size),
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
    ch: char,
    fg: Rgb,
    bg: Rgb,
) {
    if origin_x >= width || origin_y >= height {
        return;
    }

    let cell_w = CELL_WIDTH.min(width - origin_x);
    let cell_h = CELL_HEIGHT.min(height - origin_y);
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
            if row_bits & (0x80 >> bit) == 0 {
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
                if row_bits & (0x80 >> bit) == 0 {
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
    0xFF00_0000 | (u32::from(rgb.red) << 16) | (u32::from(rgb.green) << 8) | u32::from(rgb.blue)
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
    Some((y / SIDEBAR_TAB_ROW_HEIGHT) as usize)
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
        assert_eq!(grid_dimensions_for_pixels(800, 480, 200, 48), (58, 27));
    }

    #[test]
    fn settings_hit_test_maps_buttons() {
        let modal = SettingsModalView::for_client(800, 600, "Consolas", 12, ThemeId::Dark);
        assert_eq!(
            modal.hit_test(
                f64::from(modal.apply_button.0 + 4),
                f64::from(modal.apply_button.1 + 4)
            ),
            Some(SettingsHit::Apply)
        );
    }

    #[test]
    fn hidden_sidebar_yields_wider_terminal_grid() {
        let without = grid_dimensions_for_pixels(800, 480, 0, 48).0;
        let with = grid_dimensions_for_pixels(800, 480, 200, 48).0;
        assert!(without > with);
        let _ = AppConfig::default();
    }
}
