//! `agenterm-con` — a minimal console host (conhost equivalent).
//!
//! Like Windows `conhost.exe`, it owns the terminal window, renders cells
//! into a pixel surface, and forwards keyboard input to a shell running
//! inside a PTY. It does not implement tab/workspace/Fleet/server — it is a
//! lightweight, standalone console host for when the full agenterm GUI is
//! unavailable or being rebuilt.
//!
//! Design priority: **stability**. The terminal that TUI agents and CLI tools
//! crash inside most often dies during resize storms or VT-sequence floods, so
//! resize is trailing-edge debounced, the PTY reader runs on its own thread
//! (never blocking the render path), and the VT parser is the same one the
//! product terminal already hardened. See `plan/plan-v0.1.16.md` §C.

// GUI subsystem: prevents conhost from attaching a console window.
// Earlier this was omitted to work around cmd.exe exit(0), but that root
// cause turned out to be PtyChild drop (Job Object kill), now fixed by
// keeping the child handle alive for the session lifetime.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "agenterm-con/font.rs"]
mod font;
#[path = "agenterm-con/palette.rs"]
mod palette;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use agenterm_platform::input::{KeyPressState, LogicalKey, NamedKey, NormalizedKeyEvent};
use agenterm_platform::terminal_input::{self, TerminalKeyMode};
use agenterm_platform::pty::{ChildCommand, PtyChild, PtyMaster, TerminalSize};
use agenterm_platform::window_host::{
    run_pixel_window, GeometryChange, LogicalPoint, LogicalSize, PixelWindow, PixelWindowApplication,
    PixelWindowDirective, PixelWindowError, PixelWindowEvent, PixelWindowOptions,
    PointerButton, PointerButtonState, WheelDelta, XrgbPixelFrame,
};

use palette::Rgb;

/// VT callback storage for OSC sequences (window title, etc.).
#[derive(Default)]
struct ConCallbacks {
    title: Option<String>,
}

impl vt100::Callbacks for ConCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(String::from_utf8_lossy(title).trim().to_string());
    }
}

/// A terminal cell coordinate used for selection hit-testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalPoint {
    row: u16,
    col: u16,
}

impl TerminalPoint {
    /// Returns the normalized (top-left, bottom-right) bounds of a selection.
    fn normalize(a: TerminalPoint, b: TerminalPoint) -> (TerminalPoint, TerminalPoint) {
        if a.row < b.row || (a.row == b.row && a.col <= b.col) {
            (a, b)
        } else {
            (b, a)
        }
    }
}

/// Extracts text from the VT screen between two points (inclusive).
/// Produces Windows CRLF line joins, trims trailing whitespace per row.
fn selection_text(screen: &vt100::Screen, a: TerminalPoint, b: TerminalPoint) -> String {
    let (start, end) = TerminalPoint::normalize(a, b);
    let (_, cols) = screen.size();
    let mut result = String::new();
    for row in start.row..=end.row {
        let col_start = if row == start.row { start.col } else { 0 };
        let col_end = if row == end.row { end.col + 1 } else { cols };
        let mut row_text = String::new();
        for col in col_start..col_end {
            if let Some(cell) = screen.cell(row, col) {
                if cell.is_wide_continuation() {
                    continue;
                }
                if cell.has_contents() {
                    // A wide cell contributes its full text here; its
                    // continuation cell is skipped by the guard above.
                    row_text.push_str(cell.contents());
                } else {
                    row_text.push(' ');
                }
            } else {
                row_text.push(' ');
            }
        }
        // Trim trailing spaces on each row (conhost behavior).
        let trimmed = row_text.trim_end();
        if row > start.row {
            result.push_str("\r\n");
        }
        result.push_str(trimmed);
    }
    result
}

/// Trailing-edge debounce for resize: drag storms produce dozens of geometry
/// events per second. We keep only the latest metrics and apply a single resize
/// once the stream has been quiet for this long, so TUI apps see one clean
/// SIGWINCH/ConPTY resize instead of a redraw storm.
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(60);

/// Read buffer for the PTY pump thread.
const READ_BUF: usize = 8192;

/// Scrollback retained by the vt100 model.
const SCROLLBACK: usize = 4000;

/// Logical (DIP) font size. conhost defaults to ~12px at 96 DPI, but
/// ab_glyph outline metrics tend to produce taller cells than GDI, so we
/// use a slightly smaller value to match the visual size.
const DEFAULT_FONT_PX: f64 = 10.0;

/// Configuration loaded from `agenterm-con.json` (analogous to conhost
/// "Defaults" — persist font size, window geometry, etc. without a GUI dialog).
///
/// Location: `%APPDATA%/agenterm-con.json` on Windows,
/// `~/.config/agenterm-con.json` on Unix.
#[derive(Default)]
struct ConConfig {
    font_size: Option<f64>,
    cols: Option<u16>,
    rows: Option<u16>,
}

fn config_path() -> Option<std::path::PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(std::path::PathBuf::from(appdata).join("agenterm-con.json"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Some(
            std::path::PathBuf::from(home)
                .join(".config")
                .join("agenterm-con.json"),
        );
    }
    None
}

fn load_config() -> ConConfig {
    let Some(path) = config_path() else {
        return ConConfig::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ConConfig::default();
    };
    // Minimal JSON parsing without a serde dependency: look for known keys.
    let mut config = ConConfig::default();
    for line in text.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "\"font_size\"" => config.font_size = value.parse().ok(),
            "\"cols\"" => config.cols = value.parse().ok(),
            "\"rows\"" => config.rows = value.parse().ok(),
            _ => {}
        }
    }
    config
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(code) = offline_cli_exit(&args) {
        std::process::exit(code);
    }

    let mut no_activate = std::env::var_os("AGENTERM_NO_ACTIVATE").is_some();
    let mut working_dir: Option<String> = None;
    let mut font_size: Option<f64> = None;
    let mut initial_cols: Option<u16> = None;
    let mut initial_rows: Option<u16> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--no-activate" => no_activate = true,
            "--working-dir" => {
                working_dir = rest.next().cloned();
            }
            other if other.starts_with("--working-dir=") => {
                working_dir = Some(other["--working-dir=".len()..].to_owned());
            }
            "--font-size" => {
                font_size = rest.next().and_then(|v| v.parse().ok());
            }
            other if other.starts_with("--font-size=") => {
                font_size = other["--font-size=".len()..].parse().ok();
            }
            "--cols" => {
                initial_cols = rest.next().and_then(|v| v.parse().ok());
            }
            "--rows" => {
                initial_rows = rest.next().and_then(|v| v.parse().ok());
            }
            unknown => {
                let _ = agenterm_platform::process::write_parent_console_stderr(&format!(
                    "error: unknown argument '{unknown}'\n\n{USAGE}"
                ));
                std::process::exit(2);
            }
        }
    }

    // Load config file: CLI flags override config, config overrides defaults.
    let config = load_config();

    let mut app = ConTerminal::new(working_dir.clone());
    // Config values (lowest priority)
    if let Some(fs) = config.font_size {
        app.font_size_logical = fs.clamp(8.0, 36.0);
    }
    if let Some(cols) = config.cols {
        app.cols = cols.max(2);
    }
    if let Some(rows) = config.rows {
        app.rows = rows.max(2);
    }
    // CLI flags override config
    if let Some(fs) = font_size {
        app.font_size_logical = fs.clamp(8.0, 36.0);
    }
    if let Some(cols) = initial_cols {
        app.cols = cols.max(2);
    }
    if let Some(rows) = initial_rows {
        app.rows = rows.max(2);
    }
    // IME must stay on: without it CJK cannot be typed at all, which no
    // console host on Windows gets to call acceptable. An earlier fix disabled
    // it to recover keyboard input, but the actual cause was the missing
    // focus request in `opened` — see the Ime arm in `event` for the other
    // half (composed text never reached the PTY, which made IME look broken).
    let options = PixelWindowOptions::new("agenterm-con", LogicalSize::new(960.0, 600.0))
        .with_no_activate(no_activate)
        .with_ime_allowed(true);

    if let Err(error) = run_pixel_window(options, Box::new(app)) {
        let _ = agenterm_platform::process::write_parent_console_stderr(&format!(
            "agenterm-con: {error}"
        ));
        std::process::exit(1);
    }
}

const USAGE: &str = "\
Usage: agenterm-con [--no-activate] [--working-dir DIR]
                   [--font-size N] [--cols N] [--rows N]
       agenterm-con --version
       agenterm-con --help

A minimal console host (conhost equivalent). No tabs, no server, no Fleet.

Configuration: create agenterm-con.json in %APPDATA% (Windows) or
~/.config (Unix) with keys: font_size, cols, rows (all optional).
CLI flags override config; config overrides defaults.
Ctrl+wheel adjusts font size at runtime.";

/// Flags that must not open a window. Returns `Some(exit_code)` when handled.
fn offline_cli_exit(args: &[String]) -> Option<i32> {
    let alone = args.len() == 1;
    match args.first().map(String::as_str) {
        Some("--version" | "-V") if alone => {
            let _ = agenterm_platform::process::write_parent_console_stdout(&format!(
                "agenterm-con {}",
                env!("CARGO_PKG_VERSION")
            ));
            Some(0)
        }
        Some("--help" | "-h") if alone => {
            let _ = agenterm_platform::process::write_parent_console_stdout(USAGE);
            Some(0)
        }
        Some("--version" | "-V" | "--help" | "-h") => {
            let _ = agenterm_platform::process::write_parent_console_stderr(
                "error: --version/--help must be used alone",
            );
            Some(2)
        }
        _ => None,
    }
}

struct ConTerminal {
    working_dir: Option<String>,

    /// VT model. Resized in lock-step with the PTY (see `apply_resize`).
    parser: vt100::Parser<ConCallbacks>,

    /// PTY master (input writes + resize). `None` until `opened` spawns it.
    master: Option<PtyMaster>,

    /// PTY child handle. MUST stay alive for the session lifetime: dropping it
    /// closes the rmux_pty Job Object (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE),
    /// which kills the shell process immediately.
    child: Option<PtyChild>,

    /// Receives PTY output chunks from the reader thread.
    pty_rx: mpsc::Receiver<Vec<u8>>,

    /// Logical font size in DIPs. Adjusted by Ctrl+wheel.
    font_size_logical: f64,

    /// Physical cell metrics, recomputed whenever the font size or scale changes.
    cell_w: u32,
    cell_h: u32,
    font_size_px: u16,

    cols: u16,
    rows: u16,

    /// Latest un-applied geometry (coalesced). Applied once the stream settles.
    pending_geometry: Option<(u32, u32, f64)>,
    last_geometry_at: Instant,

    default_fg: Rgb,
    default_bg: Rgb,

    /// Set when the reader thread exits (PTY EOF or error).
    child_gone: bool,
    exit: bool,

    /// Scrollback scroll offset (0 = bottom/live). Positive = scrolled up.
    scroll_offset: usize,
    /// Accumulated wheel delta (fractional lines pending application).
    wheel_accumulator: f32,

    /// Text selection: anchor + focus in terminal cell coordinates.
    /// None = no selection; Some = active or completed selection.
    selection: Option<(TerminalPoint, TerminalPoint)>,
    /// True while left mouse button is held during a drag.
    selecting: bool,
    /// True while the application (not local selection) owns a button gesture.
    /// Keeps press/release paired so TUI buttons do not get a stuck-down state.
    mouse_dragging: bool,
    /// Last cell reported to the application, used to collapse motion spam.
    last_reported_cell: Option<TerminalPoint>,
    /// Button code of the in-flight application gesture, so the release
    /// reports the same button that was pressed.
    active_button: Option<u8>,

    /// In-progress IME composition, drawn inline at the cursor. While this is
    /// non-empty the keystrokes feeding the composition must not also be sent
    /// to the PTY — the IME delivers the result once, as a commit.
    ime_preedit: String,

    /// Whether an input method is attached (between Enabled and Disabled).
    /// Gates the logical-key fallback, which would otherwise double-type keys
    /// the IME consumed. See `TerminalKeyMode::ime_active`.
    ime_attached: bool,
    /// Current scale factor (for pointer hit-test DIP→pixel conversion).
    scale: f64,
}

impl ConTerminal {
    fn new(working_dir: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        // Drop the sender on the main side; the reader thread owns the only
        // surviving copy, so Disconnected reliably signals PTY EOF.
        drop(tx);
        Self {
            working_dir,
            parser: vt100::Parser::new_with_callbacks(24, 80, SCROLLBACK, ConCallbacks::default()),
            master: None,
            child: None,
            pty_rx: rx,
            font_size_logical: DEFAULT_FONT_PX,
            cell_w: 8,
            cell_h: 16,
            font_size_px: 10,
            cols: 80,
            rows: 24,
            pending_geometry: None,
            last_geometry_at: Instant::now(),
            default_fg: Rgb(0xCC, 0xCC, 0xCC),
            default_bg: Rgb(0x00, 0x00, 0x00),
            child_gone: false,
            exit: false,
            scroll_offset: 0,
            wheel_accumulator: 0.0,
            selection: None,
            selecting: false,
            mouse_dragging: false,
            last_reported_cell: None,
            active_button: None,
            ime_preedit: String::new(),
            ime_attached: false,
            scale: 1.0,
        }
    }

    /// Computes grid dimensions from physical pixels and current cell metrics.
    fn compute_grid(phys_w: u32, phys_h: u32, cell_w: u32, cell_h: u32) -> (u16, u16) {
        let cols = (phys_w / cell_w.max(1)).clamp(2, 512) as u16;
        let rows = (phys_h / cell_h.max(1)).clamp(2, 512) as u16;
        (cols, rows)
    }

    /// (Re)computes physical cell metrics from the logical font size and scale.
    fn recompute_metrics(&mut self, scale: f64) {
        self.font_size_px = (self.font_size_logical * scale).round().max(8.0) as u16;
        let m = font::cell_metrics(self.font_size_px);
        self.cell_w = m.width.max(1);
        self.cell_h = m.height.max(1);
    }

    /// Spawns the shell PTY and the reader thread. Called once from `opened`.
    fn spawn_pty(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let shell = agenterm_platform::runtime::default_terminal_shell();
        let mut command = ChildCommand::new(shell.clone())
            .size(TerminalSize {
                rows: self.rows,
                cols: self.cols,
            })
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor");
        // Platform-neutral: returns Some("-l") on Unix for bare shells,
        // None on Windows or when the shell already has explicit args.
        if let Some(login_arg) =
            agenterm_platform::pty::login_shell_argument(std::path::Path::new(&shell), 0)
        {
            command = command.arg(login_arg);
        }
        if let Some(dir) = &self.working_dir {
            command = command.current_dir(dir.clone());
        }

        let spawned = command.spawn().map_err(|error| {
            PixelWindowError::failed("cmd_spawn_failed", format!("{error}"))
        })?;
        let (mut master, child) = spawned.into_parts();

        // Reader thread: blocking read loop (the platform read polls internally),
        // forwarding chunks over the channel and waking the window loop.
        let reader = master
            .try_clone_for_startup_reader()
            .map_err(|error| PixelWindowError::failed("cmd_reader_clone_failed", format!("{error}")))?;
        let (tx, rx) = mpsc::channel();
        let waker = window.waker();
        thread::Builder::new()
            .name("agenterm-con-reader".into())
            .spawn(move || {
                let mut buf = [0u8; READ_BUF];
                loop {
                    match reader.io().read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                            let _ = waker.wake();
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                let _ = waker.wake();
            })
            .map_err(|error| PixelWindowError::failed("cmd_reader_spawn_failed", format!("{error}")))?;

        self.master = Some(master);
        self.child = Some(child);
        self.pty_rx = rx;

        Ok(())
    }

    fn drain_pty(&mut self) {
        let mut got_output = false;
        loop {
            match self.pty_rx.try_recv() {
                Ok(bytes) => {
                    self.parser.process(&bytes);
                    got_output = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.child_gone = true;
                    break;
                }
            }
        }
        // New output snaps scrollback to bottom.
        if got_output && self.scroll_offset > 0 {
            self.scroll_offset = 0;
            self.parser.screen_mut().set_scrollback(0);
        }
        // New output clears stale selection.
        if got_output && !self.selecting {
            self.selection = None;
        }
    }

    /// Applies a settled geometry: resize PTY first, then the VT model. The PTY
    /// resize is allowed to fail (some backends reject transient bad sizes);
    /// the model still converges so the next event is consistent.
    fn apply_resize(&mut self, phys_w: u32, phys_h: u32, scale: f64) {
        self.scale = scale;
        self.recompute_metrics(scale);
        let (cols, rows) = Self::compute_grid(phys_w, phys_h, self.cell_w, self.cell_h);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        if let Some(master) = &self.master {
            let _ = master.resize(TerminalSize { rows, cols });
        }
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Handles one IME composition event.
    ///
    /// Without this, a Chinese/Japanese/Korean user can compose in the OS
    /// candidate window but the result never reaches the shell — which is what
    /// made "IME enabled" look like "keyboard broken" and led to IME being
    /// switched off entirely.
    fn handle_ime(&mut self, window: &PixelWindow, event: agenterm_platform::ime::ImeEvent) {
        use agenterm_platform::ime::{classify_event, ImeAction};

        // The terminal grid is always a valid composition anchor: we place the
        // candidate window at the cursor cell below.
        match &event {
            agenterm_platform::ime::ImeEvent::Enabled => self.ime_attached = true,
            agenterm_platform::ime::ImeEvent::Disabled => self.ime_attached = false,
            _ => {}
        }

        match classify_event(event, true) {
            ImeAction::UpdatePreedit { text, .. } => {
                self.ime_preedit = text;
                self.update_ime_anchor(window);
            }
            ImeAction::ClearPreedit => {
                self.ime_preedit.clear();
            }
            ImeAction::CommitText(text) => {
                self.ime_preedit.clear();
                if !self.exit && !self.child_gone {
                    self.scroll_to_bottom();
                    self.write_pty(text.as_bytes());
                }
            }
            // `ImeAction` is non-exhaustive; an unknown future action must not
            // silently drop a composition, so clear rather than guess.
            ImeAction::None => {}
            _ => self.ime_preedit.clear(),
        }
        window.request_redraw();
    }

    /// Draws the in-progress composition starting at the cursor cell and
    /// returns how many cells it occupied, so the caller can push the cursor
    /// past it. Wide (CJK) characters take two cells, matching the grid.
    fn draw_preedit(
        &self,
        pixels: &mut [u32],
        fw: u32,
        fh: u32,
        cursor: (u16, u16),
    ) -> u32 {
        let y0 = u32::from(cursor.0) * self.cell_h;
        let mut advance = 0u32;
        // Inverted so the provisional text is unmistakable against committed
        // output, plus an underline in the conventional IME style.
        let fg = self.default_bg;
        let bg = self.default_fg;

        for character in self.ime_preedit.chars() {
            let wide = unicode_width::UnicodeWidthChar::width(character).unwrap_or(1) > 1;
            let cells = if wide { 2 } else { 1 };
            let x0 = (u32::from(cursor.1) + advance) * self.cell_w;
            if x0 >= fw || y0 >= fh {
                break;
            }
            let span = self.cell_w * cells;
            fill_rect(pixels, fw, fh, x0, y0, span, self.cell_h, bg.to_xrgb());
            if let Some(glyph) = font::raster(character, self.font_size_px) {
                blit_glyph(pixels, fw, fh, &glyph, x0, y0, fg, span, self.cell_h);
            }
            // Underline: the standard "this is not committed yet" affordance.
            let underline_y = y0 + self.cell_h.saturating_sub(1);
            fill_rect(pixels, fw, fh, x0, underline_y, span, 1, fg.to_xrgb());
            advance += cells;
        }
        advance
    }

    /// Anchors the OS candidate window to the cursor cell, so it does not
    /// appear at an arbitrary corner of the screen.
    fn update_ime_anchor(&self, window: &PixelWindow) {
        let (row, col) = self.parser.screen().cursor_position();
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        let x = f64::from(u32::from(col) * self.cell_w) / scale;
        let y = f64::from(u32::from(row) * self.cell_h) / scale;
        let _ = window.set_ime_cursor_area(agenterm_platform::window_host::LogicalRect::new(
            x,
            y,
            f64::from(self.cell_w) / scale,
            f64::from(self.cell_h) / scale,
        ));
    }

    fn forward_key(&mut self, event: &NormalizedKeyEvent) {
        if self.exit || self.child_gone {
            return;
        }

        // Keys that feed an active composition belong to the IME, not the PTY.
        // Forwarding them too would double-type the Latin keys behind a
        // Chinese/Japanese composition.
        if !self.ime_preedit.is_empty() {
            return;
        }

        // Host shortcuts are resolved before the application sees the key.
        if let LogicalKey::Character(text) = &event.logical {
            let control = event.modifiers.control;
            if control && event.modifiers.shift {
                if text.eq_ignore_ascii_case("c") {
                    self.copy_selection();
                    return;
                }
                if text.eq_ignore_ascii_case("v") {
                    self.paste_clipboard();
                    return;
                }
            }
            // Bare Ctrl+C copies when there is a selection, matching conhost;
            // with no selection it falls through to SIGINT (0x03).
            if control
                && !event.modifiers.alt
                && !event.modifiers.shift
                && text.eq_ignore_ascii_case("c")
                && self.selection.is_some()
            {
                self.copy_selection();
                self.selection = None;
                return;
            }
        }

        // Shift+PageUp/PageDown scroll the local viewport, matching conhost —
        // but not on the alternate screen, where those keys are the app's.
        let scrollable = event.modifiers.shift
            && event.state == KeyPressState::Pressed
            && !self.parser.screen().alternate_screen();
        if let LogicalKey::Named(named) = &event.logical
            && scrollable
        {
            {
                let page = usize::from(self.rows).saturating_sub(1).max(1) as isize;
                match named {
                    NamedKey::PageUp => {
                        self.scroll_by(page);
                        return;
                    }
                    NamedKey::PageDown => {
                        self.scroll_by(-page);
                        return;
                    }
                    _ => {}
                }
            }
        }

        let mode = TerminalKeyMode {
            application_cursor: self.parser.screen().application_cursor(),
            ime_active: self.ime_attached,
        };
        if let Some(bytes) = terminal_input::key_event_to_bytes(event, mode) {
            // Typing returns to the live view, as every terminal does.
            self.scroll_to_bottom();
            self.write_pty(&bytes);
        }
    }

    /// Writes bytes to the PTY, ignoring errors from a shell that already exited.
    fn write_pty(&self, bytes: &[u8]) {
        if let Some(master) = &self.master {
            let _ = master.write_all(bytes);
        }
    }

    /// Scrolls the viewport by `lines` (positive = toward older output).
    fn scroll_by(&mut self, lines: isize) {
        if self.parser.screen().alternate_screen() {
            return;
        }
        let max = self.parser.screen().scrollback() + self.scroll_offset;
        let target = self.scroll_offset as isize + lines;
        let clamped = target.clamp(0, max as isize) as usize;
        if clamped != self.scroll_offset {
            self.scroll_offset = clamped;
            self.parser.screen_mut().set_scrollback(clamped);
        }
    }

    fn scroll_to_bottom(&mut self) {
        if self.scroll_offset != 0 {
            self.scroll_offset = 0;
            self.parser.screen_mut().set_scrollback(0);
        }
    }

    /// Converts a logical (DIP) pointer position to terminal cell coordinates.
    fn hit_test(&self, pos: &LogicalPoint) -> TerminalPoint {
        let phys_x = pos.x * self.scale;
        let phys_y = pos.y * self.scale;
        TerminalPoint {
            row: (phys_y / self.cell_h as f64) as u16,
            col: (phys_x / self.cell_w as f64) as u16,
        }
    }

    fn copy_selection(&self) {
        let Some((start, end)) = self.selection else {
            return;
        };
        let text = selection_text(self.parser.screen(), start, end);
        if !text.is_empty() {
            let _ = agenterm_platform::clipboard::set_text(&text);
        }
    }

    fn paste_clipboard(&mut self) {
        let Ok(text) =
            agenterm_platform::clipboard::get_text(terminal_input::TERMINAL_PASTE_LIMIT_BYTES)
        else {
            return;
        };
        // Normalization drops ESC, so a payload cannot close the bracketed
        // guard early and have its tail executed as keystrokes.
        let normalized = terminal_input::normalize_terminal_paste(&text);
        if normalized.is_empty() {
            return;
        }
        let bracketed = self.parser.screen().bracketed_paste();
        self.scroll_to_bottom();
        self.write_pty(&terminal_input::terminal_paste_bytes(&normalized, bracketed));
    }

    /// Current mouse reporting contract negotiated by the running application.
    fn mouse_mode(&self) -> (terminal_input::ApplicationMouseMode, terminal_input::MouseReportEncoding) {
        let screen = self.parser.screen();
        let mode = match screen.mouse_protocol_mode() {
            vt100::MouseProtocolMode::None => terminal_input::ApplicationMouseMode::None,
            vt100::MouseProtocolMode::Press => terminal_input::ApplicationMouseMode::Press,
            vt100::MouseProtocolMode::PressRelease => {
                terminal_input::ApplicationMouseMode::PressRelease
            }
            vt100::MouseProtocolMode::ButtonMotion => {
                terminal_input::ApplicationMouseMode::ButtonMotion
            }
            vt100::MouseProtocolMode::AnyMotion => terminal_input::ApplicationMouseMode::AnyMotion,
        };
        let encoding = match screen.mouse_protocol_encoding() {
            vt100::MouseProtocolEncoding::Default => terminal_input::MouseReportEncoding::Default,
            vt100::MouseProtocolEncoding::Utf8 => terminal_input::MouseReportEncoding::Utf8,
            vt100::MouseProtocolEncoding::Sgr => terminal_input::MouseReportEncoding::Sgr,
        };
        (mode, encoding)
    }

    /// Attempts to deliver a pointer event to the application. Returns true
    /// when the application consumed it, so the caller skips local selection.
    fn report_mouse(
        &mut self,
        button: u8,
        point: TerminalPoint,
        pressed: bool,
        motion: bool,
        modifiers: &agenterm_platform::input::ModifierState,
    ) -> bool {
        let (mode, encoding) = self.mouse_mode();
        let delivery = terminal_input::mouse_delivery(
            mode,
            modifiers.shift,
            self.scroll_offset > 0,
            motion,
            self.mouse_dragging,
            pressed,
        );
        if delivery != terminal_input::MouseDelivery::Application {
            return false;
        }
        // Motion reports repeat per pixel; collapse them to one per cell.
        if motion && self.last_reported_cell == Some(point) {
            return true;
        }
        let code = terminal_input::mouse_code_with_modifiers(button, motion, *modifiers);
        let Some(bytes) =
            terminal_input::mouse_report_bytes(encoding, code, point.col, point.row, pressed)
        else {
            return false;
        };
        self.last_reported_cell = Some(point);
        self.write_pty(&bytes);
        true
    }

    /// Routes a wheel notch: application report → alternate-screen cursor keys
    /// → local scrollback, in that order of precedence.
    fn handle_wheel(
        &mut self,
        notches: f32,
        modifiers: &agenterm_platform::input::ModifierState,
        position: Option<LogicalPoint>,
    ) {
        let up = notches > 0.0;
        let count = (notches.abs().round() as usize).clamp(1, 32);

        // An application that grabbed the mouse gets buttons 64/65.
        let (mode, _) = self.mouse_mode();
        if mode != terminal_input::ApplicationMouseMode::None && !modifiers.shift {
            let point = position.map(|p| self.hit_test(&p)).unwrap_or(TerminalPoint {
                row: 0,
                col: 0,
            });
            let button = if up {
                terminal_input::MOUSE_WHEEL_UP
            } else {
                terminal_input::MOUSE_WHEEL_DOWN
            };
            for _ in 0..count {
                // Wheel is press-only; never emit a matching release.
                self.report_mouse(button, point, true, false, modifiers);
            }
            return;
        }

        // Alternate screen has no local scrollback to move, so translate the
        // gesture into cursor keys the way xterm does — this is what makes the
        // wheel scroll inside less/man/vim.
        if self.parser.screen().alternate_screen() {
            let application_cursor = self.parser.screen().application_cursor();
            let sequence: &[u8] = match (up, application_cursor) {
                (true, true) => b"\x1bOA",
                (false, true) => b"\x1bOB",
                (true, false) => b"\x1b[A",
                (false, false) => b"\x1b[B",
            };
            self.write_pty(&sequence.repeat(count.min(120)));
            return;
        }

        self.scroll_by(if up { count as isize } else { -(count as isize) });
    }

    /// Routes a pointer button press/release, preferring the application.
    fn handle_pointer_button(
        &mut self,
        window: &PixelWindow,
        button: PointerButton,
        state: PointerButtonState,
        position: Option<LogicalPoint>,
        modifiers: &agenterm_platform::input::ModifierState,
    ) {
        let pressed = state == PointerButtonState::Pressed;
        let point = match position {
            Some(pos) => self.hit_test(&pos),
            // A release with no position still has to close an open gesture.
            None => self.last_reported_cell.unwrap_or(TerminalPoint { row: 0, col: 0 }),
        };
        let code = match button {
            PointerButton::Left => 0,
            PointerButton::Middle => 1,
            PointerButton::Right => 2,
            _ => return,
        };

        if pressed {
            if self.report_mouse(code, point, true, false, modifiers) {
                self.mouse_dragging = true;
                self.active_button = Some(code);
                // The application owns this gesture; drop any stale selection
                // so the highlight does not linger over its UI.
                self.selection = None;
                window.request_redraw();
                return;
            }
        } else if self.mouse_dragging {
            let held = self.active_button.unwrap_or(code);
            self.report_mouse(held, point, false, false, modifiers);
            self.mouse_dragging = false;
            self.active_button = None;
            return;
        }

        // Local handling.
        match (button, pressed) {
            (PointerButton::Left, true) => {
                self.selection = Some((point, point));
                self.selecting = true;
                window.request_redraw();
            }
            (PointerButton::Left, false) => {
                self.selecting = false;
            }
            (PointerButton::Right, true) => {
                // Right-click: copy if a selection exists, else paste.
                if self.selection.is_some() {
                    self.copy_selection();
                    self.selection = None;
                    window.request_redraw();
                } else {
                    self.paste_clipboard();
                }
            }
            _ => {}
        }
    }
}

impl PixelWindowApplication for ConTerminal {
    fn opened(&mut self, window: &PixelWindow) -> Result<PixelWindowDirective, PixelWindowError> {
        let metrics = window.metrics()?;
        let scale = if metrics.scale_factor.is_finite() && metrics.scale_factor > 0.0 {
            metrics.scale_factor
        } else {
            1.0
        };
        self.recompute_metrics(scale);
        self.scale = scale;
        window.set_title(&format!("agenterm-con — {}", font::resolved_name()));
        // Request keyboard focus so winit delivers KeyboardInput events on Windows.
        window.focus();
        let (cols, rows) =
            Self::compute_grid(metrics.physical_width, metrics.physical_height, self.cell_w, self.cell_h);
        self.cols = cols;
        self.rows = rows;
        self.parser.screen_mut().set_size(rows, cols);

        self.spawn_pty(window)?;
        Ok(PixelWindowDirective::Continue)
    }

    fn event(
        &mut self,
        window: &PixelWindow,
        event: PixelWindowEvent,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        match event {
            PixelWindowEvent::CloseRequested => {
                self.exit = true;
                Ok(PixelWindowDirective::Exit)
            }
            PixelWindowEvent::GeometryChanged { change, metrics } => {
                if matches!(change, GeometryChange::Resized | GeometryChange::ScaleFactorChanged)
                    && metrics.is_drawable()
                {
                    // Coalesce: keep only the freshest metrics; the resize fires
                    // once the stream has been quiet for RESIZE_DEBOUNCE.
                    self.pending_geometry =
                        Some((metrics.physical_width, metrics.physical_height, metrics.scale_factor));
                    self.last_geometry_at = Instant::now();
                }
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::Keyboard(key) => {
                self.forward_key(&key);
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::Ime(ime) => {
                self.handle_ime(window, ime);
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::MouseWheel {
                delta,
                modifiers,
                position,
                ..
            } => {
                // Ctrl+wheel adjusts font size (wave 4); plain wheel scrolls.
                if modifiers.control {
                    let dir = match delta {
                        WheelDelta::Lines { y, .. } => y,
                        _ => 0.0,
                    };
                    if dir.abs() > 0.0 {
                        let delta_size = if dir > 0.0 { 1.0 } else { -1.0 };
                        self.font_size_logical = (self.font_size_logical + delta_size).clamp(8.0, 36.0);
                        let metrics = window.metrics().ok();
                        if let Some(m) = metrics {
                            self.apply_resize(m.physical_width, m.physical_height, m.scale_factor);
                        }
                        window.request_redraw();
                    }
                } else {
                    let lines = match delta {
                        WheelDelta::Lines { y, .. } => y,
                        WheelDelta::LogicalPixels { y, .. } => y as f32 / (self.cell_h as f32).max(1.0),
                        _ => 0.0,
                    };
                    self.wheel_accumulator += lines;
                    let whole = self.wheel_accumulator.trunc();
                    self.wheel_accumulator -= whole;
                    if whole != 0.0 {
                        self.handle_wheel(whole, &modifiers, position);
                        window.request_redraw();
                    }
                }
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::PointerButton {
                button,
                state,
                position,
                modifiers,
            } => {
                self.handle_pointer_button(window, button, state, position, &modifiers);
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::PointerMoved {
                position,
                modifiers,
                ..
            } => {
                let pt = self.hit_test(&position);
                // An application gesture in flight keeps ownership so its
                // press/release stay paired; otherwise extend local selection.
                if self.mouse_dragging {
                    let button = self.active_button.unwrap_or(0);
                    self.report_mouse(button, pt, true, true, &modifiers);
                } else if self.selecting {
                    if let Some((anchor, _)) = self.selection {
                        self.selection = Some((anchor, pt));
                        window.request_redraw();
                    }
                } else if self.mouse_mode().0 == terminal_input::ApplicationMouseMode::AnyMotion {
                    // 1003: report motion with no button held (button 3 = none).
                    self.report_mouse(3, pt, true, true, &modifiers);
                }
                Ok(PixelWindowDirective::Continue)
            }
            _ => Ok(PixelWindowDirective::Continue),
        }
    }

    fn render(
        &mut self,
        window: &PixelWindow,
        frame: &mut XrgbPixelFrame<'_>,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        self.drain_pty();

        // Apply OSC title changes (shell emits \e]0;title\a).
        if let Some(title) = self.parser.callbacks_mut().title.take() {
            window.set_title(&title);
        }

        let fw = frame.width();
        let fh = frame.height();
        let pixels = frame.pixels_mut();
        let bg_word = self.default_bg.to_xrgb();
        pixels.fill(bg_word);

        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let cursor = screen.cursor_position();
        let cursor_hidden = screen.hide_cursor();

        for row in 0..rows {
            let y0 = u32::from(row) * self.cell_h;
            if y0 >= fh {
                break;
            }
            for col in 0..cols {
                let x0 = u32::from(col) * self.cell_w;
                if x0 >= fw {
                    break;
                }
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }

                let mut fg = palette::resolve(cell.fgcolor(), self.default_fg, cell.bold());
                let mut bg = palette::resolve(cell.bgcolor(), self.default_bg, false);

                // Selection highlight: invert fg/bg for selected cells.
                if let Some((sa, sb)) = self.selection {
                    let (lo, hi) = TerminalPoint::normalize(sa, sb);
                    if row >= lo.row && row <= hi.row {
                        let col_start = if row == lo.row { lo.col } else { 0 };
                        let col_end = if row == hi.row { hi.col } else { u16::MAX };
                        if col >= col_start && col <= col_end {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                    }
                }

                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }

                // Wide (CJK/emoji) cells span 2 columns for background + glyph clip.
                let span_w = if cell.is_wide() { self.cell_w * 2 } else { self.cell_w };

                // Only repaint backgrounds that differ from the frame clear.
                if bg != self.default_bg {
                    fill_rect(pixels, fw, fh, x0, y0, span_w, self.cell_h, bg.to_xrgb());
                }

                let glyph = cell
                    .has_contents()
                    .then(|| font::raster(first_grapheme(cell.contents()), self.font_size_px))
                    .flatten();
                if let Some(glyph) = glyph {
                    blit_glyph(pixels, fw, fh, &glyph, x0, y0, fg, span_w, self.cell_h);
                }
            }
        }

        // IME composition, drawn over the cells to the right of the cursor and
        // underlined so it reads as provisional rather than committed text.
        // conhost cannot do this — it leaves composition to a floating OS
        // window that does not line up with the terminal grid.
        let preedit_cells = if self.ime_preedit.is_empty() {
            0
        } else {
            self.draw_preedit(pixels, fw, fh, cursor)
        };

        // Solid block cursor at the current position. Hide when scrolled back.
        if !cursor_hidden && self.scroll_offset == 0 {
            let cx = (u32::from(cursor.1) + preedit_cells) * self.cell_w;
            let cy = u32::from(cursor.0) * self.cell_h;
            if cx < fw && cy < fh {
                fill_rect(
                    pixels,
                    fw,
                    fh,
                    cx,
                    cy,
                    self.cell_w,
                    self.cell_h,
                    self.default_fg.to_xrgb(),
                );
            }
        }

        Ok(PixelWindowDirective::Continue)
    }

    fn about_to_wait(
        &mut self,
        window: &PixelWindow,
        now: Instant,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        // (see impl ConTerminal::draw_preedit for the composition renderer)
        if self.exit || self.child_gone {
            return Ok(PixelWindowDirective::Exit);
        }

        if let Some((pw, ph, scale)) = self.pending_geometry {
            if now.duration_since(self.last_geometry_at) >= RESIZE_DEBOUNCE {
                self.apply_resize(pw, ph, scale);
                self.pending_geometry = None;
                window.request_redraw();
                return Ok(PixelWindowDirective::Continue);
            }
            return Ok(PixelWindowDirective::WaitUntil(
                self.last_geometry_at + RESIZE_DEBOUNCE,
            ));
        }

        Ok(PixelWindowDirective::Wait)
    }
}

// ---------------------------------------------------------------------------
// Pixel helpers
// ---------------------------------------------------------------------------

fn fill_rect(pixels: &mut [u32], fw: u32, fh: u32, x: u32, y: u32, w: u32, h: u32, color: u32) {
    if x >= fw || y >= fh {
        return;
    }
    let max_x = x.saturating_add(w).min(fw);
    let max_y = y.saturating_add(h).min(fh);
    for py in y..max_y {
        let base = (py * fw) as usize;
        let row = &mut pixels[base..(base + (max_x - x) as usize)];
        row.fill(color);
    }
}

fn blit_glyph(
    pixels: &mut [u32],
    fw: u32,
    fh: u32,
    glyph: &font::RasterGlyph,
    cell_x: u32,
    cell_y: u32,
    fg: Rgb,
    cell_w: u32,
    cell_h: u32,
) {
    let start_x = cell_x as i32 + glyph.offset_x;
    let start_y = cell_y as i32 + glyph.offset_y;
    let clip_x0 = cell_x as i32;
    let clip_y0 = cell_y as i32;
    let clip_x1 = cell_x as i32 + cell_w as i32;
    let clip_y1 = cell_y as i32 + cell_h as i32;

    for gy in 0..glyph.height {
        let py = start_y + gy as i32;
        if py < clip_y0 || py >= clip_y1 || py < 0 || py as u32 >= fh {
            continue;
        }
        let row_base = (py as u32 * fw) as usize;
        for gx in 0..glyph.width {
            let alpha = glyph.alpha[(gy * glyph.width + gx) as usize];
            if alpha == 0 {
                continue;
            }
            let px = start_x + gx as i32;
            if px < clip_x0 || px >= clip_x1 || px < 0 || px as u32 >= fw {
                continue;
            }
            let idx = row_base + px as usize;
            if idx >= pixels.len() {
                continue;
            }
            pixels[idx] = blend_xrgb(pixels[idx], fg, alpha);
        }
    }
}

#[inline]
fn blend_xrgb(existing: u32, fg: Rgb, alpha: u8) -> u32 {
    let a = u32::from(alpha);
    let inv = 255 - a;
    let er = (existing >> 16) & 0xFF;
    let eg = (existing >> 8) & 0xFF;
    let eb = existing & 0xFF;
    let r = (er * inv + u32::from(fg.0) * a) / 255;
    let g = (eg * inv + u32::from(fg.1) * a) / 255;
    let b = (eb * inv + u32::from(fg.2) * a) / 255;
    r << 16 | g << 8 | b
}

/// Returns the first non-combining character so combining marks do not each
/// rasterize into their own cell-wide glyph.
fn first_grapheme(contents: &str) -> char {
    contents.chars().next().unwrap_or(' ')
}

// ---------------------------------------------------------------------------
// Keyboard → PTY byte encoding
// ---------------------------------------------------------------------------
//
// The encoding tables themselves live in `agenterm_platform::terminal_input`
// so the GUI terminal and this console host cannot drift apart again. Only the
// host-specific policy (what counts as a local shortcut) stays here.

#[cfg(test)]
mod tests {
    use super::*;
    use agenterm_platform::input::ModifierState;

    fn parser() -> vt100::Parser<ConCallbacks> {
        vt100::Parser::<ConCallbacks>::new_with_callbacks(24, 80, 0, ConCallbacks::default())
    }

    /// The escape-sequence tables are covered exhaustively in
    /// `agenterm_platform::contract::terminal_input`. What matters here is this
    /// host's own policy: that it reads the modes the application negotiated
    /// and hands the shared encoder the right ones.
    #[test]
    fn key_encoding_is_driven_by_live_screen_mode() {
        let mut parser = parser();
        let up = NormalizedKeyEvent {
            logical: LogicalKey::Named(NamedKey::ArrowUp),
            physical: agenterm_platform::input::PhysicalKeyCode::Other,
            text: None,
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers: ModifierState::default(),
        };

        let mode = TerminalKeyMode {
            application_cursor: parser.screen().application_cursor(),
            ime_active: false,
        };
        assert_eq!(
            terminal_input::key_event_to_bytes(&up, mode),
            Some(b"\x1b[A".to_vec()),
            "default mode must use CSI"
        );

        // The application turns on DECCKM; the same keypress must now encode as
        // SS3. Ignoring this is what made vim/less misread arrow keys.
        parser.process(b"\x1b[?1h");
        let mode = TerminalKeyMode {
            application_cursor: parser.screen().application_cursor(),
            ime_active: false,
        };
        assert_eq!(
            terminal_input::key_event_to_bytes(&up, mode),
            Some(b"\x1bOA".to_vec()),
            "DECCKM must switch cursor keys to SS3"
        );
    }

    #[test]
    fn paste_framing_follows_the_application_bracketed_paste_mode() {
        let mut parser = parser();
        assert!(!parser.screen().bracketed_paste());
        let text = terminal_input::normalize_terminal_paste("a\nb");
        assert_eq!(
            terminal_input::terminal_paste_bytes(&text, parser.screen().bracketed_paste()),
            b"a\rb".to_vec()
        );

        parser.process(b"\x1b[?2004h");
        assert!(parser.screen().bracketed_paste());
        assert_eq!(
            terminal_input::terminal_paste_bytes(&text, parser.screen().bracketed_paste()),
            b"\x1b[200~a\rb\x1b[201~".to_vec()
        );
    }

    #[test]
    fn mouse_mode_maps_the_vt100_variants_a_tui_actually_requests() {
        let mut app = ConTerminal::new(None);
        assert_eq!(
            app.mouse_mode(),
            (
                terminal_input::ApplicationMouseMode::None,
                terminal_input::MouseReportEncoding::Default
            )
        );

        // ?1002h + ?1006h is what a modern TUI asks for.
        app.parser.process(b"\x1b[?1002h\x1b[?1006h");
        assert_eq!(
            app.mouse_mode(),
            (
                terminal_input::ApplicationMouseMode::ButtonMotion,
                terminal_input::MouseReportEncoding::Sgr
            )
        );
    }

    #[test]
    fn selection_text_joins_rows_with_crlf_and_trims_trailing_blanks() {
        let mut parser = parser();
        parser.process(b"ab\r\ncd");
        let text = selection_text(
            &parser.screen(),
            TerminalPoint { row: 0, col: 0 },
            TerminalPoint { row: 1, col: 79 },
        );
        assert_eq!(text, "ab\r\ncd");
    }

    #[test]
    fn scrolling_clamps_to_available_scrollback() {
        let mut app = ConTerminal::new(None);
        // Nothing scrolled off yet, so the viewport cannot move up...
        app.scroll_by(10);
        assert_eq!(app.scroll_offset, 0);
        // ...and scrolling down from the bottom must not underflow.
        app.scroll_by(-10);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn offline_help_and_version_are_solo() {
        assert_eq!(offline_cli_exit(&["--version".to_owned()]), Some(0));
        assert_eq!(offline_cli_exit(&["--help".to_owned()]), Some(0));
        assert_eq!(
            offline_cli_exit(&["--version".to_owned(), "x".to_owned()]),
            Some(2)
        );
    }
}
