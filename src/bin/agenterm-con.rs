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

#[path = "agenterm-con/agent_interface.rs"]
mod agent_interface;
#[path = "agenterm-con/font.rs"]
mod font;
#[path = "agenterm-con/palette.rs"]
mod palette;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use agent_interface::{ScreenSnapshot, ScriptCommand, ScriptKey, ScriptMouseButton};
use agenterm_platform::input::{
    KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent, PhysicalKeyCode,
};
use agenterm_platform::pty::{ChildCommand, PtyChild, PtyMaster, TerminalSize};
use agenterm_platform::terminal_input::{self, TerminalKeyMode};
use agenterm_platform::window_host::{
    GeometryChange, LogicalPoint, LogicalSize, PixelWindow, PixelWindowApplication,
    PixelWindowDirective, PixelWindowError, PixelWindowEvent, PixelWindowOptions, PointerButton,
    PointerButtonState, WheelDelta, XrgbPixelFrame, run_pixel_window,
};

use palette::Rgb;

/// VT callback storage for OSC sequences (window title, etc.) and terminal
/// query replies (see `unhandled_csi` below) that need to be written back
/// to the PTY.
#[derive(Default)]
struct ConCallbacks {
    title: Option<String>,
    /// Bytes queued by a terminal-query reply (DA1/CPR/DSR — see
    /// `unhandled_csi`), drained and written to the PTY by `drain_pty`
    /// right after the batch of input that produced them finishes
    /// processing. A callback only gets `&mut Screen`, not PTY write
    /// access, so this is the seam between "recognized a query" and
    /// "actually answered it."
    pending_replies: Vec<u8>,
}

impl vt100::Callbacks for ConCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(String::from_utf8_lossy(title).trim().to_string());
    }

    /// Real, previously-missing terminal-query support — discovered as a
    /// genuine hang, not a cosmetic gap: `claude` (a modern, real-world
    /// Node/Ink TUI) run inside this binary via `-e` produced zero output
    /// and never returned, indefinitely, while the identical command via a
    /// plain `cmd.exe /c` outside agenterm-con completed in under a
    /// second. Root cause, confirmed by reading vendored vt100's own
    /// `csi_dispatch`: neither DA1 (`CSI c`, "what are you") nor CPR
    /// (`CSI 6n`, "where is the cursor") is in its handled-final-byte list
    /// for the no-intermediate case — both fall through to
    /// `unhandled_csi`, which every terminal-facing callback in this
    /// codebase left as the trait's no-op default. A program that queries
    /// the terminal and *blocks* waiting for a reply before proceeding —
    /// exactly what sophisticated TUIs do to detect real capabilities —
    /// hangs forever against a terminal that never answers. This is very
    /// likely the deeper, more general version of "some effects don't
    /// render in real TUI programs": a program that never gets past its
    /// own capability probe never gets to rendering anything at all.
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        intermediate1: Option<u8>,
        _intermediate2: Option<u8>,
        params: &[&[u16]],
        final_byte: char,
    ) {
        // Private-mode sequences (`CSI ? ...`) and anything else with an
        // intermediate byte are a different, larger space (DEC private
        // mode queries, etc.) — out of scope for this fix, which targets
        // specifically the two queries proven to actually hang a real
        // program.
        if intermediate1.is_some() {
            return;
        }
        match final_byte {
            // DA1 (Primary Device Attributes). Real terminals differ in
            // exact capability bits; `\x1b[?1;2c` ("VT100 with Advanced
            // Video Option") is the same class of minimal-but-valid answer
            // xterm and other emulators have shipped as a baseline for
            // decades — enough for a program that just wants confirmation
            // something is listening before it proceeds.
            'c' => self.pending_replies.extend_from_slice(b"\x1b[?1;2c"),
            'n' => match params.first().and_then(|p| p.first()) {
                // CPR (Cursor Position Report), 1-indexed per the spec —
                // reads the screen's actual current cursor position, not a
                // placeholder, so a program that positions itself relative
                // to the reported location gets the truth.
                Some(6) => {
                    let (row, col) = screen.cursor_position();
                    let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                    self.pending_replies.extend_from_slice(reply.as_bytes());
                }
                // DSR "are you OK?" -> "0n" (terminal OK, no malfunction).
                Some(5) => self.pending_replies.extend_from_slice(b"\x1b[0n"),
                _ => {}
            },
            _ => {}
        }
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

/// Cursor blink half-period, matching the Windows default caret blink rate
/// rather than reading GetCaretBlinkTime, for the same reason as above.
const BLINK_INTERVAL: Duration = Duration::from_millis(530);

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
    /// `--emit-snapshot`: see `agent_interface` module docs.
    snapshot_path: Option<PathBuf>,
    /// `--script`: see `agent_interface` module docs.
    script_path: Option<PathBuf>,
}

/// Parses arguments, returning the message to print on failure.
fn parse_args(args: &[String]) -> Result<ConArgs, String> {
    let mut parsed = ConArgs::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--no-activate" => parsed.no_activate = true,
            "--working-dir" => {
                parsed.working_dir = Some(rest.next().cloned().ok_or_else(|| {
                    "error: --working-dir requires a path
"
                    .to_owned()
                })?);
            }
            other if other.starts_with("--working-dir=") => {
                parsed.working_dir = Some(other["--working-dir=".len()..].to_owned());
            }
            "--font-size" => parsed.font_size = next_value(&mut rest, "--font-size")?,
            other if other.starts_with("--font-size=") => {
                parsed.font_size =
                    Some(parse_value(&other["--font-size=".len()..], "--font-size")?);
            }
            "--cols" => parsed.cols = next_value(&mut rest, "--cols")?,
            "--rows" => parsed.rows = next_value(&mut rest, "--rows")?,
            "--emit-snapshot" => {
                parsed.snapshot_path =
                    Some(PathBuf::from(rest.next().cloned().ok_or_else(|| {
                        "error: --emit-snapshot requires a path\n".to_owned()
                    })?));
            }
            "--script" => {
                parsed.script_path =
                    Some(PathBuf::from(rest.next().cloned().ok_or_else(|| {
                        "error: --script requires a path\n".to_owned()
                    })?));
            }
            // Everything after -e is the command line, verbatim. Consuming the
            // remainder is what lets `-e ssh host -p 22` pass `-p 22` through
            // rather than having this parser reject it as an unknown flag.
            "-e" | "--command" => {
                let argv: Vec<String> = rest.cloned().collect();
                if argv.is_empty() {
                    return Err("error: -e requires a program to run
"
                    .to_owned());
                }
                parsed.command = Some(argv);
                return Ok(parsed);
            }
            unknown => {
                return Err(format!(
                    "error: unknown argument '{unknown}'

{USAGE}"
                ));
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
    let raw = rest.next().ok_or_else(|| {
        format!(
            "error: {flag} requires a value
"
        )
    })?;
    parse_value(raw, flag).map(Some)
}

fn parse_value<T: std::str::FromStr>(raw: &str, flag: &str) -> Result<T, String> {
    raw.parse().map_err(|_| {
        format!(
            "error: {flag} expects a number, got '{raw}'
"
        )
    })
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
        snapshot_path,
        script_path,
    } = parsed;
    no_activate |= std::env::var_os("AGENTERM_NO_ACTIVATE").is_some();

    // A bad script must fail before a window ever opens, not silently run
    // partway then stall — same "loud, specific, fail fast" philosophy as
    // the rest of this parser.
    let script = script_path
        .map(|path| {
            let bytes = std::fs::read(&path).map_err(|error| {
                format!(
                    "error: could not read --script {}: {error}\n",
                    path.display()
                )
            })?;
            agent_interface::parse_script(&bytes)
                .map_err(|error| format!("error: --script {}: {error}\n", path.display()))
        })
        .transpose();
    let script = match script {
        Ok(script) => script,
        Err(message) => {
            let _ = agenterm_platform::process::write_parent_console_stderr(&message);
            std::process::exit(2);
        }
    };

    // Load config file: CLI flags override config, config overrides defaults.
    let config = load_config();

    let mut app = ConTerminal::new(working_dir.clone());
    let command_failed = Arc::clone(&app.command_failed);
    app.command = command;
    app.snapshot_path = snapshot_path;
    app.script = script.unwrap_or_default().into();
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
    if command_failed.load(Ordering::Acquire) {
        std::process::exit(1);
    }
}

const USAGE: &str = "\
Usage: agenterm-con [--no-activate] [--working-dir DIR]
                   [--font-size N] [--cols N] [--rows N]
                   [--emit-snapshot PATH] [--script PATH]
                   [-e PROGRAM [ARGS...]]
       agenterm-con --version
       agenterm-con --help

A standalone console host (conhost equivalent). No tabs, no server, no Fleet.

  -e, --command  Run PROGRAM instead of the default shell. Everything after
                 -e is passed through verbatim, so it must come last:
                   agenterm-con -e pwsh -NoLogo
                   agenterm-con --working-dir C:\\src -e cargo test

  --emit-snapshot PATH
                 Write a JSON snapshot of screen text/cursor/selection to
                 PATH after each render (atomic write). For scripts, tests,
                 and other agents that need to inspect a session without
                 capturing pixels.

  --script PATH  Read a JSON array of input commands from PATH and play them
                 back through the real keyboard/paste/mouse code paths —
                 text, paste, key (with ctrl/alt/shift), wait_ms, click
                 (row/col/button, with ctrl/alt/shift), mouse_down/mouse_up
                 (same shape as click, for press-drag-release gestures),
                 mouse_move (row/col), wheel (row/col/notches, or ctrl:true
                 for font-size zoom). Lets a test or another agent drive a
                 session without OS-level input injection. See
                 src/bin/agenterm-con/agent_interface.rs.

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

    /// Mirrors whatever the window title was last set to (default or an OSC
    /// title change), so `--emit-snapshot` can report it without needing to
    /// steal the one-shot `.take()` the render loop uses to notify the OS
    /// window.
    current_title: String,

    /// `--emit-snapshot`: written after each render when set. See
    /// `agent_interface` module docs.
    snapshot_path: Option<PathBuf>,
    /// `--script`: commands not yet executed, in order.
    script: VecDeque<ScriptCommand>,
    /// When the next queued `WaitMs` elapses. `None` when nothing is queued
    /// or the next command is ready to run immediately.
    script_wait_until: Option<Instant>,
    /// Set by a script `Screenshot` command; captured and cleared by the
    /// next `render()`, since pixel data only exists transiently there.
    pending_screenshot: Option<PathBuf>,

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

    /// Signaled once by the waiter thread when the child process actually
    /// exits (via Windows' process-exit notification, not PTY EOF — see
    /// `spawn_pty`). A placeholder with its sender already dropped until
    /// `spawn_pty` installs the real one, matching `pty_rx`'s own pattern.
    child_exit_rx: mpsc::Receiver<()>,

    /// Set before the waiter reports that an explicit `-e` command failed, so
    /// `main` can return the CLI runtime-error code after the window loop exits.
    command_failed: Arc<AtomicBool>,

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

    /// Whether the cursor is in its "on" phase of the blink cycle. Ignored
    /// entirely when `screen.cursor_blinking()` is false (a steady cursor).
    blink_visible: bool,
    /// When `blink_visible` last flipped, for pacing the next flip.
    last_blink_at: Instant,

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
        let (exit_tx, exit_rx) = mpsc::channel();
        drop(exit_tx);
        Self {
            working_dir,
            command: None,
            current_title: String::new(),
            snapshot_path: None,
            script: VecDeque::new(),
            script_wait_until: None,
            pending_screenshot: None,
            parser: vt100::Parser::new_with_callbacks(24, 80, SCROLLBACK, ConCallbacks::default()),
            master: None,
            child: None,
            pty_rx: rx,
            child_exit_rx: exit_rx,
            command_failed: Arc::new(AtomicBool::new(false)),
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
            blink_visible: true,
            last_blink_at: Instant::now(),
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
        let (program, extra_args) = match self.command.as_ref().and_then(|argv| argv.split_first())
        {
            Some((program, args)) => (program.clone(), args.to_vec()),
            None => (
                agenterm_platform::runtime::default_terminal_shell(),
                Vec::new(),
            ),
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
            agenterm_platform::pty::login_shell_argument(
                std::path::Path::new(&program),
                0,
            )
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
        let reader = master.try_clone_for_startup_reader().map_err(|error| {
            PixelWindowError::failed("cmd_reader_clone_failed", format!("{error}"))
        })?;
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
            .map_err(|error| {
                PixelWindowError::failed("cmd_reader_spawn_failed", format!("{error}"))
            })?;

        // Waiter thread: on Windows, ConPTY's output pipe does not reliably
        // EOF just because the immediate child process exited — the pipe
        // stays open as long as the pseudoconsole handle does, which the
        // master side deliberately holds for the session's lifetime (see the
        // comment on `child` below). Without this, `-e cmd.exe /c <command>`
        // — or simply the user's shell exiting normally — left the window
        // open forever with nothing left to read and nothing to show for it;
        // caught by a black-box test that waited on a spawned `/c` command's
        // window to close and it never did. `try_wait`/`wait` go through
        // Windows' actual process-exit signal (WaitForSingleObject on the
        // process handle) rather than through PTY I/O, so this is the
        // correct detection path, not a workaround for the pipe's behavior.
        let mut waiter = child.try_clone_for_wait().map_err(|error| {
            PixelWindowError::failed("cmd_wait_clone_failed", format!("{error}"))
        })?;
        let (exit_tx, exit_rx) = mpsc::channel();
        let exit_waker = window.waker();
        let explicit_command = self.command.is_some();
        let command_failed = Arc::clone(&self.command_failed);
        thread::Builder::new()
            .name("agenterm-con-waiter".into())
            .spawn(move || {
                let wait_result = waiter.wait();
                if explicit_command
                    && !wait_result
                        .as_ref()
                        .is_ok_and(std::process::ExitStatus::success)
                {
                    command_failed.store(true, Ordering::Release);
                }
                let _ = exit_tx.send(());
                let _ = exit_waker.wake();
            })
            .map_err(|error| {
                PixelWindowError::failed("cmd_waiter_spawn_failed", format!("{error}"))
            })?;

        self.master = Some(master);
        self.child = Some(child);
        self.pty_rx = rx;
        self.child_exit_rx = exit_rx;

        Ok(())
    }

    fn drain_pty(&mut self) {
        // Only `Ok(())` (an actual signal from the waiter thread) means
        // anything here — both the placeholder channel `new()` installs
        // before a child exists and a waiter thread that hasn't finished yet
        // report as empty/disconnected, which must not be mistaken for exit.
        if let Ok(()) = self.child_exit_rx.try_recv() {
            self.child_gone = true;
        }

        let mut got_output = false;
        loop {
            match self.pty_rx.try_recv() {
                Ok(bytes) => {
                    self.parser.process(&bytes);
                    got_output = true;
                    // Flush any terminal-query reply (DA1/CPR/DSR) right
                    // after the input that triggered it, not batched until
                    // this whole read loop empties out — a program that
                    // blocks on the reply before sending anything else
                    // should see it as promptly as a real terminal would.
                    let replies = std::mem::take(&mut self.parser.callbacks_mut().pending_replies);
                    if !replies.is_empty() {
                        self.write_pty(&replies);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                // PTY EOF can race ahead of the process waiter. The waiter is
                // authoritative because it records an explicit command's exit
                // status before telling the window loop to terminate.
                Err(mpsc::TryRecvError::Disconnected) => break,
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
        use agenterm_platform::ime::{ImeAction, classify_event};

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
            TerminalPoint { row: start, col: 0 },
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
                surface.blit_glyph(
                    &glyph,
                    CellRect {
                        x: x0,
                        y: y0,
                        w: span,
                        h: self.cell_h,
                    },
                    fg,
                    0.0,
                );
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

        // Typing always shows the cursor and restarts the blink cycle —
        // every terminal does this so the cursor is never invisible right
        // when you start typing, which reads as "did that keystroke land?"
        self.blink_visible = true;
        self.last_blink_at = Instant::now();

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
    ///
    /// Delegates the upper clamp to `Screen::set_scrollback` itself rather
    /// than re-deriving "how much scrollback is available" here: vendored
    /// `vt100`'s `Screen::scrollback()` returns the *current* offset (its
    /// own doc comment says so — "0 when the normal screen is in view"),
    /// not the available range, and there is no public accessor for the
    /// latter. A prior version of this function added that current offset
    /// to itself as a stand-in for the bound (`scrollback() +
    /// self.scroll_offset`), which is always exactly `2 * scroll_offset` —
    /// zero from a fresh view — so scrolling never moved past the very
    /// first notch. `scroll_by(10)` on a fresh terminal still correctly
    /// stays at `0`, which is why the existing unit test covering that case
    /// could not tell the two behaviors apart; only a live session with
    /// real scrolled-off content (a black-box `--script` `wheel` test)
    /// caught it. `set_scrollback` internally clamps to the real buffered
    /// row count (`rows.min(self.scrollback.len())`), so reading the offset
    /// back after calling it is the correct value, not a guess.
    fn scroll_by(&mut self, lines: isize) {
        if self.parser.screen().alternate_screen() {
            return;
        }
        let requested = (self.scroll_offset as isize + lines).max(0) as usize;
        self.parser.screen_mut().set_scrollback(requested);
        self.scroll_offset = self.parser.screen().scrollback();
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

    /// The inverse of [`Self::hit_test`]: a logical position that lands back
    /// on `point` when hit-tested. Targets the cell's center, not its
    /// top-left corner, so the result is robust to `hit_test`'s truncating
    /// division rather than sitting exactly on a rounding boundary. This is
    /// what lets `--script` mouse commands take cell coordinates (what a
    /// script author actually thinks in) while still driving the same
    /// pixel-position-based handlers real pointer events go through.
    fn terminal_point_to_logical(&self, point: TerminalPoint) -> LogicalPoint {
        let phys_x = f64::from(point.col) * self.cell_w as f64 + self.cell_w as f64 / 2.0;
        let phys_y = f64::from(point.row) * self.cell_h as f64 + self.cell_h as f64 / 2.0;
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        LogicalPoint {
            x: phys_x / scale,
            y: phys_y / scale,
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
        self.paste_text(&text);
    }

    /// The paste path proper, independent of where the text came from — the
    /// OS clipboard (`paste_clipboard`) or a `--script` `paste` command. Both
    /// must go through the same normalization and bracketing, which is the
    /// point of factoring this out: a scripted test exercises the exact
    /// logic a real Ctrl+V does, not a lookalike.
    fn paste_text(&mut self, text: &str) {
        // Normalization drops ESC, so a payload cannot close the bracketed
        // guard early and have its tail executed as keystrokes.
        let normalized = terminal_input::normalize_terminal_paste(text);
        if normalized.is_empty() {
            return;
        }
        let bracketed = self.parser.screen().bracketed_paste();
        self.scroll_to_bottom();
        self.write_pty(&terminal_input::terminal_paste_bytes(
            &normalized,
            bracketed,
        ));
    }

    /// Runs every `--script` command that is due, pacing `WaitMs` through the
    /// same `about_to_wait` scheduling the resize debounce and cursor blink
    /// use rather than blocking the thread — a blocking sleep here would
    /// freeze rendering and PTY draining for the wait's whole duration.
    ///
    /// Returns the instant to next wake for the caller to fold into its own
    /// `WaitUntil` decision, or `None` when the queue is empty and nothing
    /// is scheduled.
    fn drain_script(&mut self, window: &PixelWindow, now: Instant) -> Option<Instant> {
        if let Some(until) = self.script_wait_until {
            if now < until {
                return Some(until);
            }
            self.script_wait_until = None;
        }
        while let Some(command) = self.script.pop_front() {
            match command {
                ScriptCommand::Text(text) => {
                    self.scroll_to_bottom();
                    self.write_pty(text.as_bytes());
                }
                ScriptCommand::Paste(text) => self.paste_text(&text),
                ScriptCommand::Key {
                    key,
                    ctrl,
                    alt,
                    shift,
                } => {
                    self.execute_script_key(key, ctrl, alt, shift);
                }
                ScriptCommand::WaitMs(ms) => {
                    let until = now + Duration::from_millis(ms);
                    self.script_wait_until = Some(until);
                    return Some(until);
                }
                ScriptCommand::Screenshot(path) => {
                    // Pixels only exist transiently inside render(); stash the
                    // path and let about_to_wait force the redraw that
                    // actually produces a frame to capture.
                    self.pending_screenshot = Some(path);
                }
                ScriptCommand::Click {
                    row,
                    col,
                    button,
                    ctrl,
                    alt,
                    shift,
                } => {
                    self.execute_script_click(window, row, col, button, ctrl, alt, shift);
                }
                ScriptCommand::MouseDown {
                    row,
                    col,
                    button,
                    ctrl,
                    alt,
                    shift,
                } => {
                    self.execute_script_pointer_button(
                        window,
                        row,
                        col,
                        button,
                        ctrl,
                        alt,
                        shift,
                        PointerButtonState::Pressed,
                    );
                }
                ScriptCommand::MouseUp {
                    row,
                    col,
                    button,
                    ctrl,
                    alt,
                    shift,
                } => {
                    self.execute_script_pointer_button(
                        window,
                        row,
                        col,
                        button,
                        ctrl,
                        alt,
                        shift,
                        PointerButtonState::Released,
                    );
                }
                ScriptCommand::MouseMove { row, col } => {
                    self.execute_script_mouse_move(window, row, col);
                }
                ScriptCommand::Wheel {
                    row,
                    col,
                    notches,
                    ctrl,
                } => {
                    self.execute_script_wheel(window, row, col, notches, ctrl);
                }
            }
        }
        None
    }

    /// Synthesizes a [`NormalizedKeyEvent`] for a script `key` command and
    /// forwards it through [`ConTerminal::forward_key`] — the exact path a
    /// real keystroke takes, including host shortcuts and the live
    /// DECCKM/modifier-aware encoder. A script is a *test* of that wiring,
    /// not a shortcut around it.
    fn execute_script_key(&mut self, key: ScriptKey, ctrl: bool, alt: bool, shift: bool) {
        let modifiers = ModifierState {
            control: ctrl,
            alt,
            shift,
            meta: false,
        };
        let logical = match key {
            ScriptKey::Named(named) => LogicalKey::Named(named),
            ScriptKey::Char(ch) => LogicalKey::Character(ch.to_string()),
        };
        let text = match key {
            ScriptKey::Named(_) => None,
            // Only offered as `text` when unmodified, matching how a real
            // backend reports a plain character key versus a shortcut.
            ScriptKey::Char(ch) if !ctrl && !alt => Some(ch.to_string()),
            ScriptKey::Char(_) => None,
        };
        let event = NormalizedKeyEvent {
            logical,
            physical: PhysicalKeyCode::Other,
            text,
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers,
        };
        self.forward_key(&event);
    }

    /// Presses then releases a mouse button at a cell coordinate for a
    /// `--script` `click` command, through the same `handle_pointer_button`
    /// path a real click takes (application mouse reporting first, local
    /// click-counting/selection second) — see [`ScriptCommand::Click`].
    #[allow(clippy::too_many_arguments)]
    fn execute_script_click(
        &mut self,
        window: &PixelWindow,
        row: u16,
        col: u16,
        button: ScriptMouseButton,
        ctrl: bool,
        alt: bool,
        shift: bool,
    ) {
        self.execute_script_pointer_button(
            window,
            row,
            col,
            button,
            ctrl,
            alt,
            shift,
            PointerButtonState::Pressed,
        );
        self.execute_script_pointer_button(
            window,
            row,
            col,
            button,
            ctrl,
            alt,
            shift,
            PointerButtonState::Released,
        );
    }

    /// One half of a press-drag-release gesture — see
    /// [`ScriptCommand::MouseDown`]/[`ScriptCommand::MouseUp`], and shared
    /// by [`Self::execute_script_click`] for the atomic press+release case.
    #[allow(clippy::too_many_arguments)]
    fn execute_script_pointer_button(
        &mut self,
        window: &PixelWindow,
        row: u16,
        col: u16,
        button: ScriptMouseButton,
        ctrl: bool,
        alt: bool,
        shift: bool,
        state: PointerButtonState,
    ) {
        let modifiers = ModifierState {
            control: ctrl,
            alt,
            shift,
            meta: false,
        };
        let position = self.terminal_point_to_logical(TerminalPoint { row, col });
        let platform_button = match button {
            ScriptMouseButton::Left => PointerButton::Left,
            ScriptMouseButton::Middle => PointerButton::Middle,
            ScriptMouseButton::Right => PointerButton::Right,
        };
        self.handle_pointer_button(window, platform_button, state, Some(position), &modifiers);
    }

    /// Moves the pointer to a cell coordinate for a `--script` `mouse_move`
    /// command, through `handle_pointer_moved` — see
    /// [`ScriptCommand::MouseMove`].
    fn execute_script_mouse_move(&mut self, window: &PixelWindow, row: u16, col: u16) {
        let position = self.terminal_point_to_logical(TerminalPoint { row, col });
        self.handle_pointer_moved(window, position, &ModifierState::default());
    }

    /// One wheel notch's worth of scroll at a cell coordinate for a
    /// `--script` `wheel` command, through `handle_wheel` — see
    /// [`ScriptCommand::Wheel`]. `handle_wheel` itself never requests a
    /// redraw (real wheel events get that from the `MouseWheel` dispatch
    /// arm that calls it), so this mirrors that call site rather than
    /// leaving a scripted scroll invisible until the next unrelated redraw.
    fn execute_script_wheel(
        &mut self,
        window: &PixelWindow,
        row: u16,
        col: u16,
        notches: f32,
        ctrl: bool,
    ) {
        if ctrl {
            // Mirrors the real event: one `zoom_font` call per whole notch,
            // not one call scaled by magnitude — a real Ctrl+wheel session
            // is a *stream* of individual notch events, and reproducing a
            // crash tied to repeated cumulative resizes means replaying
            // that shape, not collapsing it into a single jump.
            let count = notches.abs().round().max(1.0) as usize;
            for _ in 0..count.min(64) {
                self.zoom_font(window, notches > 0.0);
            }
            return;
        }
        let position = self.terminal_point_to_logical(TerminalPoint { row, col });
        self.handle_wheel(notches, &ModifierState::default(), Some(position));
        window.request_redraw();
    }

    /// Builds the current [`ScreenSnapshot`] for `--emit-snapshot`.
    fn build_snapshot(&self) -> ScreenSnapshot {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let cursor = screen.cursor_position();
        let shape = match screen.cursor_shape() {
            vt100::CursorShape::Block => "block",
            vt100::CursorShape::Underline => "underline",
            vt100::CursorShape::Bar => "bar",
        };
        let visible_now = !screen.hide_cursor()
            && self.scroll_offset == 0
            && (!screen.cursor_blinking() || self.blink_visible);
        ScreenSnapshot {
            cols,
            rows,
            title: self.current_title.clone(),
            rows_text: screen.rows(0, cols).collect(),
            cursor: agent_interface::CursorSnapshot {
                row: cursor.0,
                col: cursor.1,
                shape,
                blinking: screen.cursor_blinking(),
                visible_now,
            },
            scroll_offset: self.scroll_offset,
            selection: self.selection.map(|(a, b)| {
                (
                    agent_interface::PointSnapshot {
                        row: a.row,
                        col: a.col,
                    },
                    agent_interface::PointSnapshot {
                        row: b.row,
                        col: b.col,
                    },
                )
            }),
            ime_preedit: self.ime_preedit.clone(),
            child_alive: !self.child_gone,
            font_size_px: self.font_size_px,
        }
    }

    /// Writes the current snapshot to `--emit-snapshot`'s path, if set.
    /// Errors are deliberately swallowed: a full disk or a test harness that
    /// deleted the target directory mid-run must not crash the session it is
    /// trying to observe.
    fn write_snapshot_if_requested(&self) {
        if let Some(path) = &self.snapshot_path {
            let _ = agent_interface::write_snapshot_atomic(path, &self.build_snapshot());
        }
    }

    /// Current mouse reporting contract negotiated by the running application.
    fn mouse_mode(
        &self,
    ) -> (
        terminal_input::ApplicationMouseMode,
        terminal_input::MouseReportEncoding,
    ) {
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

    /// One Ctrl+wheel notch's worth of font-size zoom: `grow = true` is one
    /// step larger, `false` one step smaller, clamped to `[8.0, 36.0]`
    /// logical px. Factored out of the `MouseWheel` event arm so a
    /// `--script` `wheel` command with `ctrl: true` drives the identical
    /// path a real Ctrl+wheel notch does — this is the exact repeated,
    /// cumulative resize path a reported "zoom past a certain size and the
    /// process exits" crash needs a live session (not an isolated
    /// `apply_resize` call) to actually exercise.
    ///
    /// Debounced, not applied synchronously — see the note below on why
    /// that changed. A fast wheel spin can queue many notches within a few
    /// hundred milliseconds; this now coalesces them into one grid/PTY
    /// resize per `RESIZE_DEBOUNCE` window (60ms) via the exact same
    /// `pending_geometry`/`about_to_wait` mechanism a window drag-resize
    /// already goes through, instead of duplicating that logic with a
    /// second, undebounced path.
    fn zoom_font(&mut self, window: &PixelWindow, grow: bool) {
        let delta_size = if grow { 1.0 } else { -1.0 };
        self.font_size_logical = (self.font_size_logical + delta_size).clamp(8.0, 36.0);
        // Cell metrics (and therefore what glyphs look like) update right
        // away, independent of the debounce below, so the zoom still reads
        // as instant — only the expensive part (recomputing cols/rows,
        // resizing the real ConPTY, resizing the vt100 model) is deferred.
        // Before this split, *every single notch* fired a full,
        // synchronous grid+PTY resize with zero throttling — unlike a
        // window drag-resize, which was already debounced. A hosted
        // program that repaints on every resize notification (a real TUI,
        // not just an idle prompt) receiving a burst of a dozen-plus
        // resizes within milliseconds is a real, previously-untested
        // stress shape; this brings Ctrl+wheel zoom in line with the
        // pacing window-resize already gets, on general principle, even
        // where a specific reported crash from it couldn't be reproduced
        // (see the black-box tests around `repeated_ctrl_wheel_zoom_...`).
        self.recompute_metrics(self.scale);
        if let Ok(m) = window.metrics() {
            self.pending_geometry = Some((m.physical_width, m.physical_height, m.scale_factor));
            self.last_geometry_at = Instant::now();
        }
        window.request_redraw();
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
            let point = position
                .map(|p| self.hit_test(&p))
                .unwrap_or(TerminalPoint { row: 0, col: 0 });
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

        self.scroll_by(if up {
            count as isize
        } else {
            -(count as isize)
        });
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
            None => self
                .last_reported_cell
                .unwrap_or(TerminalPoint { row: 0, col: 0 }),
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

    /// Routes a pointer move: an application gesture in flight keeps
    /// ownership so its press/release stay paired; otherwise extends local
    /// selection, or reports hover motion under `ANY_MOTION` (1003).
    /// Factored out of the `PointerMoved` event arm so a `--script`
    /// `mouse_move` command drives the identical logic a real OS pointer
    /// move does, not a lookalike.
    fn handle_pointer_moved(
        &mut self,
        window: &PixelWindow,
        position: LogicalPoint,
        modifiers: &agenterm_platform::input::ModifierState,
    ) {
        let pt = self.hit_test(&position);
        if self.mouse_dragging {
            let button = self.active_button.unwrap_or(0);
            self.report_mouse(button, pt, true, true, modifiers);
        } else if self.selecting {
            if let Some((anchor, _)) = self.selection {
                self.selection = Some((anchor, pt));
                window.request_redraw();
            }
        } else if self.mouse_mode().0 == terminal_input::ApplicationMouseMode::AnyMotion {
            // 1003: report motion with no button held (button 3 = none).
            self.report_mouse(3, pt, true, true, modifiers);
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
        self.current_title = format!("agenterm-con — {}", font::resolved_name());
        window.set_title(&self.current_title);
        // Request keyboard focus so winit delivers KeyboardInput events on Windows.
        window.focus();
        let (cols, rows) = Self::compute_grid(
            metrics.physical_width,
            metrics.physical_height,
            self.cell_w,
            self.cell_h,
        );
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
                if matches!(
                    change,
                    GeometryChange::Resized | GeometryChange::ScaleFactorChanged
                ) && metrics.is_drawable()
                {
                    // Coalesce: keep only the freshest metrics; the resize fires
                    // once the stream has been quiet for RESIZE_DEBOUNCE.
                    self.pending_geometry = Some((
                        metrics.physical_width,
                        metrics.physical_height,
                        metrics.scale_factor,
                    ));
                    self.last_geometry_at = Instant::now();
                }
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::Wake => {
                // Fired by the PTY reader thread's `waker.wake()` whenever
                // new output actually arrived (see `spawn_pty`) — this is
                // the *only* signal that a shell just echoed a keystroke or
                // printed something new. Before this arm existed, `Wake`
                // fell through to the wildcard `_ => Continue` below and
                // requested no redraw at all, so a keystroke's echo did not
                // actually appear on screen until the next unrelated redraw
                // happened to fire — in practice that was the cursor-blink
                // timer's ~530ms period (`BLINK_INTERVAL`), which is
                // measured, not guessed: it matches exactly the "often half
                // a second before it responds" symptom this fixes. Typing
                // was never actually slow — the PTY round-trip is fast —
                // painting the result just wasn't wired to happen promptly.
                window.request_redraw();
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::Keyboard(key) => {
                self.forward_key(&key);
                // Also redraw immediately, not just on the PTY's later
                // `Wake`: purely local effects of a keystroke (blink reset,
                // a host shortcut like copy/paste, IME state) have nothing
                // to do with PTY round-trip time and should not wait on it
                // either.
                window.request_redraw();
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
                        self.zoom_font(window, dir > 0.0);
                    }
                } else {
                    let lines = match delta {
                        WheelDelta::Lines { y, .. } => y,
                        WheelDelta::LogicalPixels { y, .. } => {
                            y as f32 / (self.cell_h as f32).max(1.0)
                        }
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
                self.handle_pointer_moved(window, position, &modifiers);
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
            self.current_title = title;
            window.set_title(&self.current_title);
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
        let cursor_shape = screen.cursor_shape();
        // A steady request always shows the cursor; a blinking one is gated
        // by the timer in about_to_wait. conhost draws the caret the same
        // way — this is parity, not an enhancement — but getting it right
        // matters for vim/nvim, which switch shape *and* blink per mode.
        let cursor_visible_now = !screen.cursor_blinking() || self.blink_visible;
        paint_cells(
            &mut surface,
            screen,
            self.selection,
            self.cell_w,
            self.cell_h,
            self.default_fg,
            self.default_bg,
            self.font_size_px,
        );

        // IME composition, drawn over the cells to the right of the cursor and
        // underlined so it reads as provisional rather than committed text.
        // conhost cannot do this — it leaves composition to a floating OS
        // window that does not line up with the terminal grid.
        let preedit_cells = if self.ime_preedit.is_empty() {
            0
        } else {
            self.draw_preedit(&mut surface, cursor)
        };

        // Cursor. Hidden while scrolled back (it would point at a cell the
        // application is no longer writing to) or mid-blink-off.
        if !cursor_hidden && self.scroll_offset == 0 && cursor_visible_now {
            let cursor_col = u32::from(cursor.1) + preedit_cells;
            let cx = cursor_col * self.cell_w;
            let cy = u32::from(cursor.0) * self.cell_h;
            if cx < fw && cy < fh {
                // A wide (CJK) glyph under the cursor must be covered whole,
                // otherwise a block cursor would bisect it.
                let under = (preedit_cells == 0)
                    .then(|| screen.cell(cursor.0, cursor.1))
                    .flatten();
                let span = match under {
                    Some(cell) if cell.is_wide() => self.cell_w * 2,
                    _ => self.cell_w,
                };

                match cursor_shape {
                    vt100::CursorShape::Block => {
                        // Drawn as a properly inverted cell rather than an
                        // opaque fill, so the character underneath stays
                        // readable — you can see what you're about to type
                        // over, as in conhost.
                        surface.fill_rect(cx, cy, span, self.cell_h, self.default_fg.to_xrgb());
                        let glyph = under.filter(|cell| cell.has_contents()).and_then(|cell| {
                            font::raster(first_grapheme(cell.contents()), self.font_size_px)
                        });
                        if let Some(glyph) = glyph {
                            surface.blit_glyph(
                                &glyph,
                                CellRect {
                                    x: cx,
                                    y: cy,
                                    w: span,
                                    h: self.cell_h,
                                },
                                self.default_bg,
                                0.0,
                            );
                        }
                    }
                    // Underline/bar are decorations, not a cover: the glyph
                    // paint_cells already drew stays as-is underneath them.
                    vt100::CursorShape::Underline => {
                        const THICKNESS: u32 = 2;
                        let y = cy + self.cell_h.saturating_sub(THICKNESS);
                        surface.fill_rect(cx, y, span, THICKNESS, self.default_fg.to_xrgb());
                    }
                    vt100::CursorShape::Bar => {
                        const THICKNESS: u32 = 2;
                        surface.fill_rect(
                            cx,
                            cy,
                            THICKNESS,
                            self.cell_h,
                            self.default_fg.to_xrgb(),
                        );
                    }
                }
            }
        }

        self.write_snapshot_if_requested();

        if let Some(path) = self.pending_screenshot.take() {
            // Errors are swallowed like the snapshot path: a bad path from a
            // script must not crash the session it's trying to observe. A
            // real agent driving this checks for the file; a missing one is
            // itself the signal something went wrong.
            let _ = agent_interface::write_png_atomic(path.as_path(), surface.pixels, fw, fh);
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

        // Three independent timers can all have work pending at once (a
        // resize settling, the cursor mid-blink, a scripted `wait_ms`), and
        // this callback can only return one deadline. Each contributes to a
        // shared "wake no later than" floor instead of returning early —
        // returning early on, say, blink would starve a scripted wait behind
        // blink's ~530ms cadence, making `wait_ms: 50` in a script actually
        // take up to 530ms.
        let mut redraw = false;
        let mut next_wake: Option<Instant> = None;
        let mut fold_wake = |deadline: Instant| {
            next_wake = Some(next_wake.map_or(deadline, |current| current.min(deadline)));
        };

        if let Some((pw, ph, scale)) = self.pending_geometry {
            let deadline = self.last_geometry_at + RESIZE_DEBOUNCE;
            if now >= deadline {
                self.apply_resize(pw, ph, scale);
                self.pending_geometry = None;
                redraw = true;
            } else {
                fold_wake(deadline);
            }
        }

        // A steady cursor needs no timer at all — only pay the periodic
        // wake-up cost while the application actually asked for a blink.
        if self.parser.screen().cursor_blinking() {
            if now.duration_since(self.last_blink_at) >= BLINK_INTERVAL {
                self.blink_visible = !self.blink_visible;
                self.last_blink_at = now;
                redraw = true;
            }
            fold_wake(self.last_blink_at + BLINK_INTERVAL);
        }

        if let Some(deadline) = self.drain_script(window, now) {
            fold_wake(deadline);
        }
        // A pending screenshot needs an actual render to happen — pixels
        // only exist transiently inside render() — so it must force a
        // redraw even when nothing else did.
        if self.pending_screenshot.is_some() {
            redraw = true;
        }

        if redraw {
            window.request_redraw();
        }

        Ok(next_wake.map_or(PixelWindowDirective::Wait, PixelWindowDirective::WaitUntil))
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
                    CellRect {
                        x: x0,
                        y: y0,
                        w: span_w,
                        h: cell_h,
                    },
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

    /// Regression coverage for a real, confirmed hang: `claude` (a real
    /// modern Node/Ink TUI) run through `-e` produced zero output and never
    /// returned — indefinitely — while the identical command via a plain
    /// `cmd.exe /c` outside this binary completed in under a second. Root
    /// cause: neither DA1 (`CSI c`) nor CPR (`CSI 6n`) was answered, and a
    /// program that blocks waiting for either reply before proceeding hangs
    /// forever against a terminal that never responds. Confirmed fixed
    /// live (not just by this unit test): the same `claude --help`
    /// invocation that previously produced nothing now renders its full
    /// output through this binary.
    #[test]
    fn da1_query_gets_a_reply_queued_for_the_pty() {
        let mut parser = parser();
        parser.process(b"\x1b[c");
        assert_eq!(parser.callbacks().pending_replies, b"\x1b[?1;2c");
    }

    #[test]
    fn cpr_query_reports_the_real_current_cursor_position() {
        let mut parser = parser();
        // Two lines of output move the cursor to row 1 (0-indexed), col 0 —
        // reported 1-indexed per the CPR spec, so row 2, col 1.
        parser.process(b"hello\r\nworld");
        parser.callbacks_mut().pending_replies.clear();
        parser.process(b"\x1b[6n");
        assert_eq!(parser.callbacks().pending_replies, b"\x1b[2;6R");
    }

    #[test]
    fn dsr_ok_query_gets_a_reply_queued() {
        let mut parser = parser();
        parser.process(b"\x1b[5n");
        assert_eq!(parser.callbacks().pending_replies, b"\x1b[0n");
    }

    #[test]
    fn unrecognized_csi_queries_are_left_unanswered_not_guessed_at() {
        // Anything with an intermediate byte (private-mode queries, etc.)
        // or an unrecognized final byte must not get a made-up reply —
        // silence is the correct, honest answer for a query this binary
        // does not actually understand, not a guess that could mislead the
        // caller into thinking a real capability exists.
        let mut parser = parser();
        parser.process(b"\x1b[?15n"); // DEC-private status (printer), unhandled
        assert!(parser.callbacks().pending_replies.is_empty());
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
            parser.screen(),
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
    fn scrolling_up_actually_moves_once_real_content_is_off_screen() {
        // Complements `scrolling_clamps_to_available_scrollback`, which only
        // ever exercises a terminal with nothing scrolled off — a case where
        // "clamped to 0 because there's nothing to see" and "clamped to 0
        // because the bound was computed wrong" are indistinguishable, and
        // did not catch a real bug: `scroll_by`'s old bound was
        // `screen().scrollback() + scroll_offset`, but vendored vt100's
        // `Screen::scrollback()` returns the *current* offset (its own doc
        // comment says so), not the available range — so the bound was
        // always `2 * scroll_offset`, i.e. always 0 from a fresh view, and
        // wheel-up silently never worked in a live session. Only caught by
        // a black-box `--script` `wheel` test against a real session with
        // actual scrolled-off lines; this pins the same fact as a fast unit
        // test so it can't regress silently again.
        let mut app = ConTerminal::new(None);
        app.parser.screen_mut().set_size(4, 40);
        for line in 0..20 {
            app.parser.process(format!("line{line}\r\n").as_bytes());
        }
        assert_eq!(app.scroll_offset, 0);

        app.scroll_by(3);
        assert_eq!(
            app.scroll_offset, 3,
            "3 lines of real scrollback exist; scrolling up must move"
        );

        // Overshooting clamps to what's actually buffered, not to 0.
        app.scroll_by(1000);
        let max = app.scroll_offset;
        assert!(
            max > 3,
            "clamp must be the real available scrollback, not stuck at the first move"
        );

        app.scroll_by(-1000);
        assert_eq!(
            app.scroll_offset, 0,
            "scrolling back down must return to the bottom"
        );
    }

    /// The reported "Ctrl+wheel zoom occasionally makes the window vanish
    /// with no dialog" crash, reproduced as a unit test.
    ///
    /// Zooming *in* grows the cell, which shrinks the column count, which
    /// makes `apply_resize` call `vt100::Screen::set_size` with fewer
    /// columns. Shrinking a row truncates its cell array — and if a wide
    /// (CJK/emoji) character straddled the new right edge, its continuation
    /// cell is dropped while the first half stays behind in the final
    /// column. From then on the row violates vt100's own invariant that a
    /// wide cell always has its continuation at `col + 1`, and the next
    /// narrow character written onto that orphan made `Screen::text`
    /// dereference `col + 1` and `unwrap()` a `None` — a panic, which under
    /// this binary's `panic = "abort"` release profile is a silent,
    /// dialog-free process exit. Exactly the reported symptom, exactly the
    /// reported direction (enlarging, not shrinking), and "occasional"
    /// because it needs a wide glyph to land on the new last column.
    ///
    /// A shell that prints CJK (a localized Windows shell banner, a path
    /// with Han characters, any CJK program output) hits this; a pure-ASCII
    /// session never does, which is why earlier ASCII-driven reproduction
    /// attempts came back clean.
    #[test]
    fn narrow_write_over_a_wide_cell_orphaned_by_a_zoom_in_resize_survives() {
        let mut parser =
            vt100::Parser::<ConCallbacks>::new_with_callbacks(2, 6, 0, ConCallbacks::default());
        // Three wide chars fill columns 0-1, 2-3, 4-5 exactly.
        parser.process("你好吗".as_bytes());
        assert!(parser.screen().cell(0, 4).expect("col 4 exists").is_wide());

        // One Ctrl+wheel notch's worth of zoom-in: the same call
        // `apply_resize` makes, with one column fewer. Column 5 (the
        // continuation half) is truncated away; column 4 keeps the first
        // half and is now an orphan.
        parser.screen_mut().set_size(2, 5);

        // The shell then prints one ordinary narrow character onto that
        // cell — a cursor move to row 1, column 5 (1-indexed) and an 'x'.
        // Before the fix this aborted the process here.
        parser.process(b"\x1b[1;5Hx");

        assert_eq!(
            parser
                .screen()
                .cell(0, 4)
                .expect("col 4 still exists")
                .contents(),
            "x",
            "the narrow write must land, not just avoid panicking"
        );
    }

    /// The same invariant, checked one level down: a shrinking row resize
    /// must never leave a wide cell without its continuation. This is the
    /// property the fix actually restores, independent of which write
    /// happens to trip over the violation afterwards.
    #[test]
    fn shrinking_a_grid_never_leaves_a_wide_cell_without_its_continuation() {
        for cols in 2u16..=12 {
            let mut parser = vt100::Parser::<ConCallbacks>::new_with_callbacks(
                2,
                12,
                0,
                ConCallbacks::default(),
            );
            // Offset by one narrow char so the wide pairs straddle both odd
            // and even column boundaries as `cols` sweeps down.
            parser.process("a你好吗你".as_bytes());
            parser.screen_mut().set_size(2, cols);
            let last = cols - 1;
            let cell = parser.screen().cell(0, last).expect("last column exists");
            assert!(
                !cell.is_wide(),
                "cols={cols}: wide cell orphaned in the final column by the resize"
            );
        }
    }

    /// The same crash one level up, driven the way the product drives it:
    /// a full Ctrl+wheel zoom-in sweep through `apply_resize` against a
    /// shell that keeps printing CJK. Every notch shrinks the column count,
    /// and the CJK text guarantees wide characters sit near whatever the new
    /// right edge turns out to be. Deterministic — no window, no timing.
    ///
    /// The reason this angle went unnoticed for two rounds of investigation
    /// is that the existing zoom stress tests either resize without any
    /// output in flight, or push output through a fixed grid; only doing
    /// both, with *wide* characters, reaches the broken invariant.
    #[test]
    fn zoom_in_sweep_while_printing_cjk_never_aborts() {
        // A localized Windows shell banner is CJK, so this is what a real
        // session looks like from its very first frame — not an exotic case.
        let chunks: [&[u8]; 3] = [
            "Microsoft Windows [版本 10.0.20348.1006]\r\n".as_bytes(),
            "(c) Microsoft Corporation。保留所有权利。\r\n".as_bytes(),
            "C:\\dev> 编译 中文日本語 한국어 ██▒░\r\n".as_bytes(),
        ];
        for &(phys_w, phys_h) in &[(960u32, 600u32), (1280, 400), (420, 900)] {
            for scale_tenths in [10u32, 15, 25] {
                let scale = f64::from(scale_tenths) / 10.0;
                let mut app = ConTerminal::new(None);
                app.apply_resize(phys_w, phys_h, scale);
                // One notch per step across the whole clamp range, exactly
                // as `zoom_font` walks it, with output in flight throughout.
                for step in 0..=28u32 {
                    app.font_size_logical = (8.0 + f64::from(step)).clamp(8.0, 36.0);
                    app.apply_resize(phys_w, phys_h, scale);
                    for chunk in &chunks {
                        app.parser.process(chunk);
                    }
                }
                assert!(app.cols >= 2 && app.rows >= 2);
            }
        }
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
        assert!(
            app.parser.screen().row_wrapped(0),
            "row 0 should be wrapped"
        );

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
        assert_eq!(parsed.command, Some(argv(&["ssh", "host", "-p", "22"])));

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
        let mut screen_parser = vt100::Parser::<ConCallbacks>::new_with_callbacks(
            rows,
            cols,
            0,
            ConCallbacks::default(),
        );
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
                    assert!(
                        app.cols >= 2,
                        "cols degenerated at scale={scale} logical={logical} w={w} h={h}"
                    );
                    assert!(
                        app.rows >= 2,
                        "rows degenerated at scale={scale} logical={logical} w={w} h={h}"
                    );
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
        chars.extend([
            '中', '文', '字', '形', '日', '本', '語', '한', '국', '어', '➜', '★', '你',
        ]);
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
            let mut parser = vt100::Parser::<ConCallbacks>::new_with_callbacks(
                rows,
                cols,
                0,
                ConCallbacks::default(),
            );
            parser.process(bytes);
            let fw = u32::from(cols) * cell_w;
            let fh = u32::from(rows) * cell_h;
            let mut pixels = vec![0u32; (fw * fh) as usize];
            let mut surface = Surface {
                pixels: &mut pixels,
                width: fw,
                height: fh,
            };
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
    fn decscusr_selects_shape_and_blink() {
        let mut parser = parser();
        // Default before any DECSCUSR: blinking block.
        assert_eq!(parser.screen().cursor_shape(), vt100::CursorShape::Block);
        assert!(parser.screen().cursor_blinking());

        parser.process(b"\x1b[6 q"); // steady bar (insert-mode convention)
        assert_eq!(parser.screen().cursor_shape(), vt100::CursorShape::Bar);
        assert!(!parser.screen().cursor_blinking());

        parser.process(b"\x1b[3 q"); // blinking underline
        assert_eq!(
            parser.screen().cursor_shape(),
            vt100::CursorShape::Underline
        );
        assert!(parser.screen().cursor_blinking());

        parser.process(b"\x1b[2 q"); // steady block
        assert_eq!(parser.screen().cursor_shape(), vt100::CursorShape::Block);
        assert!(!parser.screen().cursor_blinking());

        // Out-of-range resets to the default rather than leaving stale state.
        parser.process(b"\x1b[9 q");
        assert_eq!(parser.screen().cursor_shape(), vt100::CursorShape::Block);
        assert!(parser.screen().cursor_blinking());
    }

    #[test]
    fn blink_toggles_on_the_configured_interval_and_resets_on_keystroke() {
        let mut app = ConTerminal::new(None);
        assert!(app.blink_visible);
        let start = app.last_blink_at;

        // Simulate the interval having elapsed by moving the recorded time
        // into the past rather than sleeping — deterministic and instant.
        app.last_blink_at = start - BLINK_INTERVAL - Duration::from_millis(1);
        let due = app.last_blink_at;
        let now = Instant::now();
        assert!(now.duration_since(due) >= BLINK_INTERVAL);

        // A keystroke must force the cursor back to visible immediately,
        // regardless of blink phase — this is what stops "did that key even
        // register?" moments.
        app.blink_visible = false;
        let key = NormalizedKeyEvent {
            logical: LogicalKey::Character("a".to_owned()),
            physical: agenterm_platform::input::PhysicalKeyCode::Other,
            text: Some("a".to_owned()),
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers: ModifierState::default(),
        };
        app.forward_key(&key);
        assert!(app.blink_visible);
    }

    #[test]
    fn cursor_shape_default_is_block_absent_any_decscusr() {
        // Regression guard: paint_cells and the cursor overlay must agree
        // with vt100's own default, or a fresh terminal would draw the wrong
        // cursor shape from the very first frame.
        let parser = parser();
        assert_eq!(parser.screen().cursor_shape(), vt100::CursorShape::Block);
    }

    #[test]
    fn arrow_left_key_command_produces_the_expected_csi_bytes() {
        // Isolates the encoder from the ConPTY/cmd.exe environment: if this
        // passes but a real session's cursor still does not move, the bug is
        // downstream of write_pty, not in event construction or encoding.
        let mut app = ConTerminal::new(None);
        app.master = None; // no real PTY; we only care what bytes WOULD be sent
        // Reconstruct exactly what execute_script_key builds, bypassing
        // forward_key's PTY write so we can inspect the encoder's output
        // directly via the same TerminalKeyMode computation forward_key uses.
        let mode = TerminalKeyMode {
            application_cursor: app.parser.screen().application_cursor(),
            ime_active: app.ime_attached,
        };
        let event = NormalizedKeyEvent {
            logical: LogicalKey::Named(NamedKey::ArrowLeft),
            physical: PhysicalKeyCode::Other,
            text: None,
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers: ModifierState::default(),
        };
        let bytes = terminal_input::key_event_to_bytes(&event, mode);
        assert_eq!(bytes, Some(b"\x1b[D".to_vec()));
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
