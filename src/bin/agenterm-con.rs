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

// NOTE: windows_subsystem = "windows" is intentionally omitted. The ConPTY
// child (cmd.exe) needs a console-capable host process; a GUI-subsystem binary
// can cause the child to silently exit(0) without producing any output. The
// extra console window is acceptable for a fallback terminal.

#[path = "agenterm-con/font.rs"]
mod font;
#[path = "agenterm-con/palette.rs"]
mod palette;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use agenterm_platform::input::{KeyPressState, LogicalKey, NamedKey, NormalizedKeyEvent};
use agenterm_platform::pty::{ChildCommand, PtyChild, PtyMaster, TerminalSize};
use agenterm_platform::window_host::{
    run_pixel_window, GeometryChange, LogicalSize, PixelWindow, PixelWindowApplication,
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

/// Trailing-edge debounce for resize: drag storms produce dozens of geometry
/// events per second. We keep only the latest metrics and apply a single resize
/// once the stream has been quiet for this long, so TUI apps see one clean
/// SIGWINCH/ConPTY resize instead of a redraw storm.
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(60);

/// Read buffer for the PTY pump thread.
const READ_BUF: usize = 8192;

/// Scrollback retained by the vt100 model.
const SCROLLBACK: usize = 4000;

/// Logical (DIP) font size. Ctrl+wheel will adjust this in a later increment.
const DEFAULT_FONT_PX: f64 = 13.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(code) = offline_cli_exit(&args) {
        std::process::exit(code);
    }

    let mut no_activate = std::env::var_os("AGENTERM_NO_ACTIVATE").is_some();
    let mut working_dir: Option<String> = None;
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
            unknown => {
                let _ = agenterm_platform::process::write_parent_console_stderr(&format!(
                    "error: unknown argument '{unknown}'\n\n{USAGE}"
                ));
                std::process::exit(2);
            }
        }
    }

    let app = ConTerminal::new(working_dir);
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
       agenterm-con --version
       agenterm-con --help

A minimal console host (conhost equivalent). No tabs, no server, no Fleet.";

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
            font_size_px: 13,
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
        #[cfg(unix)]
        {
            if let Some(login_arg) =
                agenterm_platform::pty::login_shell_argument(std::path::Path::new(&shell), 0)
            {
                command = command.arg(login_arg);
            }
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
    }

    /// Applies a settled geometry: resize PTY first, then the VT model. The PTY
    /// resize is allowed to fail (some backends reject transient bad sizes);
    /// the model still converges so the next event is consistent.
    fn apply_resize(&mut self, phys_w: u32, phys_h: u32, scale: f64) {
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

    fn forward_key(&mut self, event: &NormalizedKeyEvent) {
        if self.exit || self.child_gone {
            return;
        }
        if let Some(bytes) = key_to_bytes(event) {
            if let Some(master) = &self.master {
                let _ = master.write_all(&bytes);
            }
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
        window.set_title(&format!("agenterm-con — {}", font::resolved_name()));
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
            PixelWindowEvent::MouseWheel { delta, modifiers, .. } => {
                // Ctrl+wheel adjusts font size (wave 4); plain wheel scrolls.
                if !modifiers.control {
                    let lines = match delta {
                        WheelDelta::Lines { y, .. } => y,
                        WheelDelta::LogicalPixels { y, .. } => y as f32 / (self.cell_h as f32).max(1.0),
                        _ => 0.0,
                    };
                    self.wheel_accumulator += lines;
                    let whole = self.wheel_accumulator.trunc();
                    self.wheel_accumulator -= whole;
                    if whole != 0.0 && !self.parser.screen().alternate_screen() {
                        let max_sb = self.parser.screen().scrollback() + self.scroll_offset;
                        let target = self.scroll_offset.saturating_add(whole.round() as usize);
                        self.scroll_offset = target.min(max_sb);
                        self.parser.screen_mut().set_scrollback(self.scroll_offset);
                        window.request_redraw();
                    }
                }
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::PointerButton {
                button: PointerButton::Left,
                state: PointerButtonState::Pressed,
                ..
            } => {
                // Selection arrives in C2; a plain click still focuses/activates
                // without us stealing it, so there is nothing to do here yet.
                let _ = window;
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
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }

                // Wide (CJK/emoji) cells span 2 columns for background + glyph clip.
                let span_w = if cell.is_wide() { self.cell_w * 2 } else { self.cell_w };

                // Only repaint backgrounds that differ from the frame clear.
                if bg != self.default_bg {
                    fill_rect(pixels, fw, fh, x0, y0, span_w, self.cell_h, bg.to_xrgb());
                }

                if cell.has_contents() {
                    if let Some(glyph) = font::raster(first_grapheme(cell.contents()), self.font_size_px) {
                        blit_glyph(
                            pixels, fw, fh, &glyph, x0, y0, fg, span_w, self.cell_h,
                        );
                    }
                }
            }
        }

        // Solid block cursor at the current position. Hide when scrolled back.
        if !cursor_hidden && self.scroll_offset == 0 {
            let cx = u32::from(cursor.1) * self.cell_w;
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

fn key_to_bytes(event: &NormalizedKeyEvent) -> Option<Vec<u8>> {
    if event.state == KeyPressState::Released {
        return None;
    }

    // Compute the base byte sequence first, then apply Alt/Meta ESC prefix.
    let mut base: Option<Vec<u8>> = None;

    // Named keys produce fixed VT sequences. `NamedKey` is non-exhaustive, so
    // unmapped variants fall through to the text path below.
    if let LogicalKey::Named(named) = &event.logical {
        if let Some(bytes) = named_key_bytes(*named) {
            base = Some(bytes.to_vec());
        }
    }

    if base.is_none() {
        // Ctrl+letter → C0 control code (Ctrl+A = 0x01 .. Ctrl+Z = 0x1a).
        if event.modifiers.control {
            if let LogicalKey::Character(text) = &event.logical {
                if let Some(ch) = text.chars().next() {
                    let lower = ch.to_ascii_lowercase();
                    if ('a'..='z').contains(&lower) {
                        base = Some(vec![u8::wrapping_sub(lower as u8, b'a') + 1]);
                    }
                }
            }
        }
    }

    if base.is_none() {
        // Plain printable text (no control).
        if !event.modifiers.control {
            if let Some(text) = event.text.as_deref() {
                if !text.is_empty() {
                    base = Some(text.as_bytes().to_vec());
                }
            }
            if base.is_none() {
                if let LogicalKey::Character(text) = &event.logical {
                    if !text.is_empty() {
                        base = Some(text.as_bytes().to_vec());
                    }
                }
            }
        }
    }

    // Alt/Meta → prepend ESC so readline/emacs Meta bindings work.
    match base {
        Some(mut bytes) if event.modifiers.alt => {
            let mut prefixed = Vec::with_capacity(bytes.len() + 1);
            prefixed.push(0x1b);
            prefixed.append(&mut bytes);
            Some(prefixed)
        }
        other => other,
    }
}

/// Maps a named key to its VT input sequence, or `None` if the key has no
/// fixed sequence (callers fall back to text).
fn named_key_bytes(named: NamedKey) -> Option<&'static [u8]> {
    match named {
        NamedKey::Enter => Some(b"\r"),
        NamedKey::Tab => Some(b"\t"),
        NamedKey::Backspace => Some(b"\x7f"),
        NamedKey::Escape => Some(b"\x1b"),
        NamedKey::Space => Some(b" "),
        NamedKey::ArrowUp => Some(b"\x1b[A"),
        NamedKey::ArrowDown => Some(b"\x1b[B"),
        NamedKey::ArrowRight => Some(b"\x1b[C"),
        NamedKey::ArrowLeft => Some(b"\x1b[D"),
        NamedKey::Home => Some(b"\x1b[H"),
        NamedKey::End => Some(b"\x1b[F"),
        NamedKey::PageUp => Some(b"\x1b[5~"),
        NamedKey::PageDown => Some(b"\x1b[6~"),
        NamedKey::Delete => Some(b"\x1b[3~"),
        NamedKey::Insert => Some(b"\x1b[2~"),
        NamedKey::F1 => Some(b"\x1bOP"),
        NamedKey::F2 => Some(b"\x1bOQ"),
        NamedKey::F3 => Some(b"\x1bOR"),
        NamedKey::F4 => Some(b"\x1bOS"),
        NamedKey::F5 => Some(b"\x1b[15~"),
        NamedKey::F6 => Some(b"\x1b[17~"),
        NamedKey::F7 => Some(b"\x1b[18~"),
        NamedKey::F8 => Some(b"\x1b[19~"),
        NamedKey::F9 => Some(b"\x1b[20~"),
        NamedKey::F10 => Some(b"\x1b[21~"),
        NamedKey::F11 => Some(b"\x1b[23~"),
        NamedKey::F12 => Some(b"\x1b[24~"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenterm_platform::input::ModifierState;

    fn char_event(text: &str, logical: &str, mods: ModifierState) -> NormalizedKeyEvent {
        NormalizedKeyEvent {
            logical: LogicalKey::Character(logical.to_owned()),
            physical: agenterm_platform::input::PhysicalKeyCode::Other,
            text: Some(text.to_owned()),
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers: mods,
        }
    }

    #[test]
    fn arrow_keys_encode_to_csi_sequences() {
        let mut e = NormalizedKeyEvent {
            logical: LogicalKey::Named(NamedKey::ArrowUp),
            physical: agenterm_platform::input::PhysicalKeyCode::Other,
            text: None,
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers: ModifierState::default(),
        };
        assert_eq!(key_to_bytes(&e), Some(b"\x1b[A".to_vec()));

        e.logical = LogicalKey::Named(NamedKey::Delete);
        assert_eq!(key_to_bytes(&e), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn released_keys_produce_nothing() {
        let e = NormalizedKeyEvent {
            logical: LogicalKey::Named(NamedKey::Enter),
            physical: agenterm_platform::input::PhysicalKeyCode::Other,
            text: None,
            state: KeyPressState::Released,
            repeat: false,
            modifiers: ModifierState::default(),
        };
        assert_eq!(key_to_bytes(&e), None);
    }

    #[test]
    fn plain_text_is_forwarded_as_utf8() {
        let e = char_event("h", "h", ModifierState::default());
        assert_eq!(key_to_bytes(&e), Some(b"h".to_vec()));

        let e = char_event("é", "e", ModifierState::default());
        assert_eq!(key_to_bytes(&e), Some("é".as_bytes().to_vec()));
    }

    #[test]
    fn ctrl_letter_maps_to_control_code() {
        let e = char_event("", "c", {
            let mut m = ModifierState::default();
            m.control = true;
            m
        });
        assert_eq!(key_to_bytes(&e), Some(vec![0x03]));
    }

    #[test]
    fn alt_letter_prepends_esc() {
        let e = char_event("b", "b", {
            let mut m = ModifierState::default();
            m.alt = true;
            m
        });
        assert_eq!(key_to_bytes(&e), Some(vec![0x1b, b'b']));
    }

    #[test]
    fn alt_ctrl_letter_prepends_esc_before_control_code() {
        let e = char_event("", "c", {
            let mut m = ModifierState::default();
            m.control = true;
            m.alt = true;
            m
        });
        // ESC + Ctrl+C (0x03)
        assert_eq!(key_to_bytes(&e), Some(vec![0x1b, 0x03]));
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
