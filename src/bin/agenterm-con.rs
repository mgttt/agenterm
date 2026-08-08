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

/// How long after a click a second one still counts as a double-click.
/// Matches the common Windows default rather than reading SPI_GETDBLCLKTIME,
/// which would drag a Win32 dependency into a platform-neutral binary.
const MULTI_CLICK_WINDOW: Duration = Duration::from_millis(500);

/// Horizontal lean per pixel of height for faux italic (SGR 3), roughly the
/// 12-degree slant real italic faces use.
const ITALIC_SHEAR: f32 = 0.21;

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

/// Command-line options, parsed out of `main` so the precedence and
/// passthrough rules are unit-testable rather than only observable by
/// launching a window.
#[derive(Debug, Default, PartialEq)]
struct ConArgs {
    no_activate: bool,
    working_dir: Option<String>,
    font_size: Option<f64>,
    cols: Option<u16>,
    rows: Option<u16>,
    command: Option<Vec<String>>,
}

/// Parses arguments, returning the message to print on failure.
fn parse_args(args: &[String]) -> Result<ConArgs, String> {
    let mut parsed = ConArgs::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--no-activate" => parsed.no_activate = true,
            "--working-dir" => {
                parsed.working_dir = Some(
                    rest.next()
                        .cloned()
                        .ok_or_else(|| "error: --working-dir requires a path
".to_owned())?,
                );
            }
            other if other.starts_with("--working-dir=") => {
                parsed.working_dir = Some(other["--working-dir=".len()..].to_owned());
            }
            "--font-size" => parsed.font_size = next_value(&mut rest, "--font-size")?,
            other if other.starts_with("--font-size=") => {
                parsed.font_size = Some(parse_value(&other["--font-size=".len()..], "--font-size")?);
            }
            "--cols" => parsed.cols = next_value(&mut rest, "--cols")?,
            "--rows" => parsed.rows = next_value(&mut rest, "--rows")?,
            // Everything after -e is the command line, verbatim. Consuming the
            // remainder is what lets `-e ssh host -p 22` pass `-p 22` through
            // rather than having this parser reject it as an unknown flag.
            "-e" | "--command" => {
                let argv: Vec<String> = rest.cloned().collect();
                if argv.is_empty() {
                    return Err("error: -e requires a program to run
".to_owned());
                }
                parsed.command = Some(argv);
                return Ok(parsed);
            }
            unknown => {
                return Err(format!("error: unknown argument '{unknown}'

{USAGE}"));
            }
        }
    }
    Ok(parsed)
}

/// Reads the next argument as `T`, reporting the flag name on failure rather
/// than silently ignoring a typo — the old parser dropped bad values on the
/// floor, so `--cols twenty` quietly did nothing.
fn next_value<'a, T: std::str::FromStr>(
    rest: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<Option<T>, String> {
    let raw = rest
        .next()
        .ok_or_else(|| format!("error: {flag} requires a value
"))?;
    parse_value(raw, flag).map(Some)
}

fn parse_value<T: std::str::FromStr>(raw: &str, flag: &str) -> Result<T, String> {
    raw.parse()
        .map_err(|_| format!("error: {flag} expects a number, got '{raw}'
"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(code) = offline_cli_exit(&args) {
        std::process::exit(code);
    }

    let parsed = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = agenterm_platform::process::write_parent_console_stderr(&message);
            std::process::exit(2);
        }
    };
    let ConArgs {
        mut no_activate,
        working_dir,
        font_size,
        cols: initial_cols,
        rows: initial_rows,
        command,
    } = parsed;
    no_activate |= std::env::var_os("AGENTERM_NO_ACTIVATE").is_some();

    // Load config file: CLI flags override config, config overrides defaults.
    let config = load_config();

    let mut app = ConTerminal::new(working_dir.clone());
    app.command = command;
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
                   [-e PROGRAM [ARGS...]]
       agenterm-con --version
       agenterm-con --help

A standalone console host (conhost equivalent). No tabs, no server, no Fleet.

  -e, --command  Run PROGRAM instead of the default shell. Everything after
                 -e is passed through verbatim, so it must come last:
                   agenterm-con -e pwsh -NoLogo
                   agenterm-con --working-dir C:\\src -e cargo test

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

    /// Program to host, from `-e`. `None` runs the user's default shell.
    command: Option<Vec<String>>,

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

    /// Time and place of the last left press, plus how many clicks it
    /// continued, for double/triple-click selection.
    last_click: Option<(Instant, TerminalPoint, u8)>,
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
            command: None,
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
            last_click: None,
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
        // `-e` hosts a chosen program; otherwise fall back to the user's shell.
        let (program, extra_args) = match self.command.as_ref().and_then(|argv| argv.split_first()) {
            Some((program, args)) => (program.clone(), args.to_vec()),
            None => (agenterm_platform::runtime::default_terminal_shell(), Vec::new()),
        };

        let mut command = ChildCommand::new(program.clone())
            .size(TerminalSize {
                rows: self.rows,
                cols: self.cols,
            })
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor");

        if self.command.is_some() {
            for argument in extra_args {
                command = command.arg(argument);
            }
        } else if let Some(login_arg) =
            // Platform-neutral: returns Some("-l") on Unix for bare shells,
            // None on Windows or when the shell already has explicit args.
            // Only meaningful for the default-shell path — a program given
            // via -e must receive exactly the arguments the user wrote.
            agenterm_platform::pty::login_shell_argument(std::path::Path::new(&program), 0)
        {
            command = command.arg(login_arg);
        }
        if let Some(dir) = &self.working_dir {
            command = command.current_dir(dir.clone());
        }

        let spawned = command.spawn().map_err(|error| {
            // Name the program: "failed to spawn" with no subject is the kind
            // of error message that costs a user ten minutes.
            PixelWindowError::failed("cmd_spawn_failed", format!("{program}: {error}"))
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

    /// Records a left press and returns the click count (1, 2, or 3).
    ///
    /// A repeat only counts when it lands on the same cell inside the
    /// multi-click window; moving to a different cell starts a fresh count, so
    /// a fast click in two places does not select a word by accident.
    fn register_click(&mut self, point: TerminalPoint) -> u8 {
        let now = Instant::now();
        let count = match self.last_click {
            Some((at, at_point, count))
                if at_point == point && now.duration_since(at) <= MULTI_CLICK_WINDOW =>
            {
                // Cycle 1 → 2 → 3 → 1 so a fourth click returns to character
                // selection rather than sticking on whole-line.
                count % 3 + 1
            }
            _ => 1,
        };
        self.last_click = Some((now, point, count));
        count
    }

    /// Expands to the word around `point`, or `None` if that cell is blank.
    fn word_at(&self, point: TerminalPoint) -> Option<(TerminalPoint, TerminalPoint)> {
        let screen = self.parser.screen();
        let (_, cols) = screen.size();
        if !self.is_word_cell(point.row, point.col) {
            return None;
        }
        let mut start = point.col;
        while start > 0 && self.is_word_cell(point.row, start - 1) {
            start -= 1;
        }
        let mut end = point.col;
        while end + 1 < cols && self.is_word_cell(point.row, end + 1) {
            end += 1;
        }
        Some((
            TerminalPoint {
                row: point.row,
                col: start,
            },
            TerminalPoint {
                row: point.row,
                col: end,
            },
        ))
    }

    /// Whether a cell participates in a double-click word.
    ///
    /// Deliberately more permissive than conhost's space-only rule: `/`, `.`,
    /// `-`, and `:` stay inside the word so a path or URL selects in one click,
    /// which is the common case in a terminal.
    fn is_word_cell(&self, row: u16, col: u16) -> bool {
        let Some(cell) = self.parser.screen().cell(row, col) else {
            return false;
        };
        if !cell.has_contents() {
            return false;
        }
        cell.contents().chars().next().is_some_and(|character| {
            !character.is_whitespace()
                && !matches!(
                    character,
                    '(' | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '\''
                        | '"'
                        | '`'
                        | '|'
                        | ';'
                        | ','
                )
        })
    }

    /// Expands to the whole *logical* line, following soft wrapping, so a
    /// triple-click on a long wrapped command selects all of it rather than
    /// just the visual row the pointer happened to land on.
    fn line_at(&self, point: TerminalPoint) -> (TerminalPoint, TerminalPoint) {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let mut start = point.row;
        while start > 0 && screen.row_wrapped(start - 1) {
            start -= 1;
        }
        let mut end = point.row;
        while end + 1 < rows && screen.row_wrapped(end) {
            end += 1;
        }
        (
            TerminalPoint {
                row: start,
                col: 0,
            },
            TerminalPoint {
                row: end,
                col: cols.saturating_sub(1),
            },
        )
    }

    /// Draws the in-progress composition starting at the cursor cell and
    /// returns how many cells it occupied, so the caller can push the cursor
    /// past it. Wide (CJK) characters take two cells, matching the grid.
    fn draw_preedit(&self, surface: &mut Surface<'_>, cursor: (u16, u16)) -> u32 {
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
            if x0 >= surface.width || y0 >= surface.height {
                break;
            }
            let span = self.cell_w * cells;
            surface.fill_rect(x0, y0, span, self.cell_h, bg.to_xrgb());
            if let Some(glyph) = font::raster(character, self.font_size_px) {
                surface.blit_glyph(&glyph, CellRect { x: x0, y: y0, w: span, h: self.cell_h }, fg, 0.0);
            }
            // Underline: the standard "this is not committed yet" affordance.
            let underline_y = y0 + self.cell_h.saturating_sub(1);
            surface.fill_rect(x0, underline_y, span, 1, fg.to_xrgb());
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
                match self.register_click(point) {
                    1 => {
                        self.selection = Some((point, point));
                        self.selecting = true;
                    }
                    2 => {
                        self.selection = self.word_at(point);
                        self.selecting = false;
                    }
                    // Third click and beyond select the whole logical line.
                    _ => {
                        self.selection = Some(self.line_at(point));
                        self.selecting = false;
                    }
                }
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
        let bg_word = self.default_bg.to_xrgb();
        let mut surface = Surface {
            pixels: frame.pixels_mut(),
            width: fw,
            height: fh,
        };
        surface.pixels.fill(bg_word);

        let screen = self.parser.screen();
        let cursor = screen.cursor_position();
        let cursor_hidden = screen.hide_cursor();
        paint_cells(&mut surface, screen, self.selection, self.cell_w, self.cell_h, self.default_fg, self.default_bg, self.font_size_px);

        // IME composition, drawn over the cells to the right of the cursor and
        // underlined so it reads as provisional rather than committed text.
        // conhost cannot do this — it leaves composition to a floating OS
        // window that does not line up with the terminal grid.
        let preedit_cells = if self.ime_preedit.is_empty() {
            0
        } else {
            self.draw_preedit(&mut surface, cursor)
        };

        // Block cursor, drawn as a properly inverted cell rather than an opaque
        // fill: the character under the cursor stays readable, as it does in
        // conhost. Hidden while scrolled back, where it would point at a cell
        // the application is no longer writing to.
        if !cursor_hidden && self.scroll_offset == 0 {
            let cursor_col = u32::from(cursor.1) + preedit_cells;
            let cx = cursor_col * self.cell_w;
            let cy = u32::from(cursor.0) * self.cell_h;
            if cx < fw && cy < fh {
                // A wide (CJK) glyph under the cursor must be covered whole,
                // otherwise the cursor bisects it.
                let under = (preedit_cells == 0)
                    .then(|| screen.cell(cursor.0, cursor.1))
                    .flatten();
                let span = match under {
                    Some(cell) if cell.is_wide() => self.cell_w * 2,
                    _ => self.cell_w,
                };
                surface.fill_rect(cx, cy, span, self.cell_h, self.default_fg.to_xrgb());

                let glyph = under
                    .filter(|cell| cell.has_contents())
                    .and_then(|cell| {
                        font::raster(first_grapheme(cell.contents()), self.font_size_px)
                    });
                if let Some(glyph) = glyph {
                    surface.blit_glyph(
                        &glyph,
                        CellRect { x: cx, y: cy, w: span, h: self.cell_h },
                        self.default_bg,
                        0.0,
                    );
                }
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

/// A cell's pixel rectangle. The four values are always derived together from
/// the grid position, so passing them separately only invited transposition.
#[derive(Clone, Copy)]
struct CellRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// The pixel target for one frame: the buffer and its dimensions, which always
/// travel together. Bundling them keeps the drawing calls readable — the free
/// functions this replaced took nine positional arguments, most of them the
/// same three values threaded through every call.
struct Surface<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
}

impl Surface<'_> {
    fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let max_x = x.saturating_add(w).min(self.width);
        let max_y = y.saturating_add(h).min(self.height);
        for py in y..max_y {
            let base = (py * self.width) as usize;
            // The slice must start at column `x` within the row, not at the
            // row's own start — `base` alone is column 0. Omitting `+ x` here
            // was a real, shipped bug: every non-default background color,
            // the underline, text selection, the block cursor, and the IME
            // preedit background all rendered flush against the left edge of
            // their row instead of at their actual column. Glyphs were
            // unaffected (blit_glyph already offsets correctly), which is why
            // text always looked right and this went unnoticed until a
            // pixel-level test checked fills specifically.
            let start = base + x as usize;
            let row = &mut self.pixels[start..(start + (max_x - x) as usize)];
            row.fill(color);
        }
    }

    /// Blits a rasterized glyph into a cell, clipped to that cell.
    ///
    /// `shear` slants the glyph for faux italic: a per-row horizontal offset
    /// proportional to height above the baseline. Synthesizing the slant beats
    /// loading a real italic face, which would have a different advance width
    /// and break the fixed cell grid.
    fn blit_glyph(&mut self, glyph: &font::RasterGlyph, cell: CellRect, fg: Rgb, shear: f32) {
        let start_x = cell.x as i32 + glyph.offset_x;
        let start_y = cell.y as i32 + glyph.offset_y;
        let clip_x0 = cell.x as i32;
        let clip_y0 = cell.y as i32;
        let clip_x1 = cell.x as i32 + cell.w as i32;
        let clip_y1 = cell.y as i32 + cell.h as i32;

        for gy in 0..glyph.height {
            let py = start_y + gy as i32;
            if py < clip_y0 || py >= clip_y1 || py < 0 || py as u32 >= self.height {
                continue;
            }
            // Rows nearer the top lean further right, pivoting on the bottom
            // of the cell so the glyph stays seated on its baseline.
            let slant = if shear == 0.0 {
                0
            } else {
                ((clip_y1 - py) as f32 * shear).round() as i32
            };
            let row_base = (py as u32 * self.width) as usize;
            for gx in 0..glyph.width {
                let alpha = glyph.alpha[(gy * glyph.width + gx) as usize];
                if alpha == 0 {
                    continue;
                }
                let px = start_x + gx as i32 + slant;
                if px < clip_x0 || px >= clip_x1 || px < 0 || px as u32 >= self.width {
                    continue;
                }
                let idx = row_base + px as usize;
                if idx >= self.pixels.len() {
                    continue;
                }
                self.pixels[idx] = blend_xrgb(self.pixels[idx], fg, alpha);
            }
        }
    }
}

/// Paints every cell of one screen into `surface`. Pure with respect to
/// window/frame types so it is directly unit-testable — see the `tests`
/// module, which renders into a plain `Vec<u32>` and asserts on pixel colors.
#[allow(clippy::too_many_arguments)]
fn paint_cells(
    surface: &mut Surface<'_>,
    screen: &vt100::Screen,
    selection: Option<(TerminalPoint, TerminalPoint)>,
    cell_w: u32,
    cell_h: u32,
    default_fg: Rgb,
    default_bg: Rgb,
    font_size_px: u16,
) {
    let (rows, cols) = screen.size();
    for row in 0..rows {
        let y0 = u32::from(row) * cell_h;
        if y0 >= surface.height {
            break;
        }
        for col in 0..cols {
            let x0 = u32::from(col) * cell_w;
            if x0 >= surface.width {
                break;
            }
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }

            let mut fg = palette::resolve(cell.fgcolor(), default_fg, cell.bold());
            let mut bg = palette::resolve(cell.bgcolor(), default_bg, false);

            // Selection highlight: invert fg/bg for selected cells.
            if let Some((sa, sb)) = selection {
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

            // Dim (SGR 2) is a real attribute tools use for secondary text;
            // ignoring it renders de-emphasized output at full strength.
            // Blending toward the background is how terminals express it.
            if cell.dim() {
                fg = palette::blend(fg, bg, 0.55);
            }

            // Wide (CJK/emoji) cells span 2 columns for background + glyph clip.
            let span_w = if cell.is_wide() { cell_w * 2 } else { cell_w };

            // Only repaint backgrounds that differ from the frame clear.
            if bg != default_bg {
                surface.fill_rect(x0, y0, span_w, cell_h, bg.to_xrgb());
            }

            let glyph = cell
                .has_contents()
                .then(|| font::raster(first_grapheme(cell.contents()), font_size_px))
                .flatten();
            if let Some(glyph) = glyph {
                // Faux italic: shear the glyph rather than loading a second
                // face, which would break the fixed cell advance.
                let shear = if cell.italic() { ITALIC_SHEAR } else { 0.0 };
                surface.blit_glyph(
                    &glyph,
                    CellRect { x: x0, y: y0, w: span_w, h: cell_h },
                    fg,
                    shear,
                );
            }

            // Underline (SGR 4). conhost draws this; skipping it silently
            // drops emphasis that tools rely on to mark links and headings.
            if cell.underline() {
                let y = y0 + cell_h.saturating_sub(2);
                surface.fill_rect(x0, y, span_w, 1, fg.to_xrgb());
            }
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
    fn double_click_selects_a_word_and_keeps_paths_whole() {
        let mut app = ConTerminal::new(None);
        app.parser.screen_mut().set_size(4, 40);
        app.parser.process(b"cd /usr/local/bin (note)");

        // Inside the path: the whole path is one word, because '/', '.', '-'
        // and ':' are word characters here — more useful than conhost's
        // space-only rule.
        let hit = TerminalPoint { row: 0, col: 8 };
        let (start, end) = app.word_at(hit).expect("word under a path cell");
        assert_eq!((start.col, end.col), (3, 16));

        // Parentheses are delimiters, so "note" selects without them.
        let hit = TerminalPoint { row: 0, col: 19 };
        let (start, end) = app.word_at(hit).expect("word inside parens");
        assert_eq!((start.col, end.col), (19, 22));

        // A blank cell yields no word rather than an empty selection.
        assert!(app.word_at(TerminalPoint { row: 0, col: 2 }).is_none());
    }

    #[test]
    fn triple_click_follows_soft_wrapping_to_the_whole_logical_line() {
        let mut app = ConTerminal::new(None);
        app.parser.screen_mut().set_size(4, 10);
        // 15 characters over a 10-column grid soft-wraps onto row 1.
        app.parser.process(b"abcdefghijklmno");
        assert!(app.parser.screen().row_wrapped(0), "row 0 should be wrapped");

        // Clicking the continuation row still selects from the start of the
        // logical line, not just the visual row under the pointer.
        let (start, end) = app.line_at(TerminalPoint { row: 1, col: 2 });
        assert_eq!((start.row, start.col), (0, 0));
        assert_eq!((end.row, end.col), (1, 9));
    }

    #[test]
    fn click_counting_requires_the_same_cell_within_the_window() {
        let mut app = ConTerminal::new(None);
        let here = TerminalPoint { row: 1, col: 1 };
        let elsewhere = TerminalPoint { row: 5, col: 5 };

        assert_eq!(app.register_click(here), 1);
        assert_eq!(app.register_click(here), 2);
        assert_eq!(app.register_click(here), 3);
        // A fourth click cycles back to character selection.
        assert_eq!(app.register_click(here), 1);

        // Moving restarts the count, so a fast click in two places cannot
        // accidentally select a word.
        assert_eq!(app.register_click(here), 2);
        assert_eq!(app.register_click(elsewhere), 1);
    }

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn dash_e_takes_the_rest_of_the_line_verbatim() {
        // Flags belonging to the hosted program must reach it untouched, not
        // be parsed (or rejected) by this host.
        let parsed = parse_args(&argv(&["-e", "ssh", "host", "-p", "22"])).expect("parses");
        assert_eq!(
            parsed.command,
            Some(argv(&["ssh", "host", "-p", "22"]))
        );

        // Host flags before -e still apply.
        let parsed =
            parse_args(&argv(&["--cols", "100", "-e", "pwsh", "-NoLogo"])).expect("parses");
        assert_eq!(parsed.cols, Some(100));
        assert_eq!(parsed.command, Some(argv(&["pwsh", "-NoLogo"])));
    }

    #[test]
    fn dash_e_without_a_program_is_an_error() {
        assert!(parse_args(&argv(&["-e"])).is_err());
    }

    #[test]
    fn bad_numeric_values_are_reported_rather_than_silently_dropped() {
        // The previous parser used `.ok()`, so `--cols twenty` was ignored and
        // the user got a default-sized window with no explanation.
        let error = parse_args(&argv(&["--cols", "twenty"])).expect_err("should reject");
        assert!(error.contains("--cols"), "{error}");
        assert!(error.contains("twenty"), "{error}");

        assert!(parse_args(&argv(&["--font-size"])).is_err());
        assert!(parse_args(&argv(&["--working-dir"])).is_err());
    }

    #[test]
    fn unknown_flags_are_rejected_with_usage() {
        let error = parse_args(&argv(&["--nope"])).expect_err("should reject");
        assert!(error.contains("--nope"), "{error}");
        assert!(error.contains("Usage:"), "{error}");
    }

    /// Renders one screen and returns (pixel buffer, cell_w, cell_h) for exact
    /// pixel assertions — the deterministic alternative to eyeballing a
    /// screenshot, which is what actually caught this bug: a screenshot
    /// suggested underline/background/inverse were shifted by a couple of
    /// columns, but that could just as easily have been the screenshot
    /// harness. This settles it in-process.
    fn render_to_buffer(bytes: &[u8], cols: u16, rows: u16) -> (Vec<u32>, u32, u32) {
        let cell_w = 10u32;
        let cell_h = 20u32;
        let mut screen_parser =
            vt100::Parser::<ConCallbacks>::new_with_callbacks(rows, cols, 0, ConCallbacks::default());
        screen_parser.process(bytes);
        let fw = u32::from(cols) * cell_w;
        let fh = u32::from(rows) * cell_h;
        let mut pixels = vec![Rgb(0, 0, 0).to_xrgb(); (fw * fh) as usize];
        let mut surface = Surface {
            pixels: &mut pixels,
            width: fw,
            height: fh,
        };
        paint_cells(
            &mut surface,
            screen_parser.screen(),
            None,
            cell_w,
            cell_h,
            Rgb(0xCC, 0xCC, 0xCC),
            Rgb(0, 0, 0),
            10,
        );
        (pixels, cell_w, cell_h)
    }

    #[test]
    fn underline_paints_under_the_correct_columns_not_shifted() {
        // "AA" plain, then underlined "BB". If underline were misplaced (the
        // shift a screenshot seemed to show), it would land under "AA".
        let (pixels, cell_w, cell_h) = render_to_buffer(b"AA\x1b[4mBB\x1b[0m", 10, 1);
        let underline_y = cell_h - 2;
        let bg = Rgb(0, 0, 0).to_xrgb();

        // No underline under the plain run (cols 0-1).
        for col in 0..2u32 {
            let x = col * cell_w + cell_w / 2;
            assert_eq!(
                pixels[(underline_y * cell_w * 10 + x) as usize],
                bg,
                "col {col} must not be underlined"
            );
        }
        // Underline present under the attributed run (cols 2-3).
        for col in 2..4u32 {
            let x = col * cell_w + cell_w / 2;
            assert_ne!(
                pixels[(underline_y * cell_w * 10 + x) as usize],
                bg,
                "col {col} must be underlined"
            );
        }
    }

    #[test]
    fn background_fill_spans_exactly_the_attributed_columns() {
        // "XX" plain, then red-background "RR", then plain "YY" again — the
        // fill must start exactly at column 2 and end exactly at column 3.
        let (pixels, cell_w, cell_h) = render_to_buffer(b"XX\x1b[41mRR\x1b[0mYY", 10, 1);
        let mid_y = cell_h / 2;
        let row_base = (mid_y * cell_w * 10) as usize;
        let red = palette::resolve(vt100::Color::Idx(1), Rgb(0, 0, 0), false).to_xrgb();

        let sample = |col: u32| pixels[row_base + (col * cell_w + cell_w / 2) as usize];
        assert_ne!(sample(0), red, "col 0 (plain) must not be red");
        assert_ne!(sample(1), red, "col 1 (plain) must not be red");
        assert_eq!(sample(2), red, "col 2 must be red");
        assert_eq!(sample(3), red, "col 3 must be red");
        assert_ne!(sample(4), red, "col 4 (plain again) must not be red");
    }

    #[test]
    fn inverse_swaps_the_full_attributed_span_not_one_cell() {
        let (pixels, cell_w, cell_h) = render_to_buffer(b"NN\x1b[7mIIII\x1b[0m", 10, 1);
        let mid_y = cell_h / 2;
        let row_base = (mid_y * cell_w * 10) as usize;
        let fg = Rgb(0xCC, 0xCC, 0xCC).to_xrgb();

        // Inverse fills the background with the swapped color across all 4
        // attributed cells (2..6), not just the first one.
        for col in 2..6u32 {
            assert_eq!(
                pixels[row_base + (col * cell_w + cell_w / 2) as usize],
                fg,
                "col {col} must show the inverted background"
            );
        }
    }

    #[test]
    fn stress_apply_resize_across_extreme_scale_and_window_sizes() {
        // Reproduce a reported crash: "font grows past a certain size and the
        // program exits." Sweep scale factors (simulating high-DPI displays
        // this dev machine does not have) crossed with window sizes from tiny
        // to large, at every font size in the allowed range, and confirm
        // apply_resize never panics and never produces a zero-sized grid.
        for scale_tenths in 5..=40 {
            let scale = f64::from(scale_tenths) / 10.0;
            for logical in [8.0, 20.0, 36.0] {
                for &(w, h) in &[(1u32, 1u32), (50, 50), (960, 600), (3840, 2160)] {
                    let mut app = ConTerminal::new(None);
                    app.font_size_logical = logical;
                    app.apply_resize(w, h, scale);
                    assert!(app.cols >= 2, "cols degenerated at scale={scale} logical={logical} w={w} h={h}");
                    assert!(app.rows >= 2, "rows degenerated at scale={scale} logical={logical} w={w} h={h}");
                    assert!(app.cell_w > 0);
                    assert!(app.cell_h > 0);
                }
            }
        }
    }

    #[test]
    fn stress_raster_every_printable_ascii_and_cjk_at_every_clamped_size() {
        // font::raster clamps internally to [8,72], but sweep the full clamped
        // range against a wide character set in case some specific glyph's
        // outline panics ab_glyph's rasterizer at a particular size — the kind
        // of bug that would only show up for "large font + this app's prompt
        // happens to contain that glyph," matching a real-use-only report.
        let mut chars: Vec<char> = (32u8..=126).map(char::from).collect();
        chars.extend(['中', '文', '字', '形', '日', '本', '語', '한', '국', '어', '➜', '★', '你']);
        for size in 8u16..=72 {
            for &ch in &chars {
                let _ = font::raster(ch, size);
            }
        }
    }

    #[test]
    fn stress_paint_cells_with_shell_like_output_across_font_sizes() {
        // End-to-end: real PTY-shaped bytes (prompt, CJK, colors) through the
        // full paint path at every clamped font size, at a window size small
        // enough to force the grid toward its floor while the font is large.
        let bytes: &[u8] =
            b"C:/dev/agenterm> echo \xe4\xbd\xa0\xe5\xa5\xbd \x1b[1;32mok\x1b[0m\r\n\x1b[4munderline\x1b[0m ";
        for size in 8u16..=72 {
            let cell_w = 10u32.max(u32::from(size) / 2);
            let cell_h = 20u32.max(u32::from(size));
            let cols = (200u32 / cell_w).clamp(2, 512) as u16;
            let rows = (200u32 / cell_h).clamp(2, 512) as u16;
            let mut parser =
                vt100::Parser::<ConCallbacks>::new_with_callbacks(rows, cols, 0, ConCallbacks::default());
            parser.process(bytes);
            let fw = u32::from(cols) * cell_w;
            let fh = u32::from(rows) * cell_h;
            let mut pixels = vec![0u32; (fw * fh) as usize];
            let mut surface = Surface { pixels: &mut pixels, width: fw, height: fh };
            paint_cells(
                &mut surface,
                parser.screen(),
                None,
                cell_w,
                cell_h,
                Rgb(0xCC, 0xCC, 0xCC),
                Rgb(0, 0, 0),
                size,
            );
        }
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
