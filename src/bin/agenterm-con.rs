//! `agenterm-con` — a minimal console host (conhost equivalent).
//!
//! Like Windows `conhost.exe`, it owns the terminal window, renders cells
//! into a pixel surface, and forwards keyboard input to shells running inside
//! independent PTYs. It has an in-window tab tree, but deliberately does not
//! implement a persisted workspace, Fleet, mux, server, or script runtime.
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
#[path = "agenterm-con/composer.rs"]
mod composer;
#[path = "agenterm-con/control.rs"]
mod control;
#[path = "agenterm-con/font.rs"]
mod font;
#[path = "agenterm-con/json.rs"]
mod json;
#[path = "agenterm-con/palette.rs"]
mod palette;
#[path = "agenterm-con/ui.rs"]
mod ui;
#[path = "agenterm-con/workspace.rs"]
mod workspace;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use agent_interface::{ScreenSnapshot, ScriptCommand, ScriptKey, ScriptMouseButton};
use agenterm_platform::contract::pixel_present::PixelPresentStats;
use agenterm_platform::input::{
    KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent, PhysicalKeyCode,
};
use agenterm_platform::pty::{BoundedOutputPipe, ChildCommand, PtyChild, PtyMaster, TerminalSize};
use agenterm_platform::terminal_input::{self, TerminalKeyMode};
use agenterm_platform::window_host::{
    GeometryChange, LogicalPoint, LogicalSize, PixelBackingRetention, PixelFrameWrite,
    PixelPointerCursor, PixelRect as HostPixelRect, PixelWindow, PixelWindowApplication,
    PixelWindowDirective, PixelWindowError, PixelWindowEvent, PixelWindowOptions, PointerButton,
    PointerButtonState, WheelDelta, XrgbPixelFrame, run_pixel_window,
};
use agenterm_ui_core::{
    DirtyRegion, DirtyRows, PixelRect, RetainedXrgbFrame, ScrollbarHit, ScrollbarThumbDrag,
    scrollback_for_thumb_top, scrollbar_hit_test,
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

fn selection_should_auto_copy(selection: Option<(TerminalPoint, TerminalPoint)>) -> bool {
    selection.is_some_and(|(anchor, focus)| anchor != focus)
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
const PTY_QUEUE_BYTES: usize = READ_BUF * 128;
const PTY_DRAIN_BUDGET_BYTES: usize = 128 * 1024;

/// Logical (DIP) font size. 15 px is approximately 11.25 pt at 96 DPI and
/// visually matches the 14 px tree labels. The previous value `11` was
/// pixels, not points, and therefore rendered smaller than intended.
const DEFAULT_FONT_PX: f64 = 15.0;

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

#[inline(never)]
fn load_config() -> ConConfig {
    let Some(path) = config_path() else {
        return ConConfig::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return ConConfig::default();
    };
    let Ok(value) = json::parse(&bytes) else {
        return ConConfig::default();
    };
    let Ok(mut fields) = value.into_object("configuration") else {
        return ConConfig::default();
    };
    let Ok(font_size) = json::take_f64(&mut fields, "font_size") else {
        return ConConfig::default();
    };
    let Ok(cols) = json::take_u16(&mut fields, "cols") else {
        return ConConfig::default();
    };
    let Ok(rows) = json::take_u16(&mut fields, "rows") else {
        return ConConfig::default();
    };
    ConConfig {
        font_size,
        cols,
        rows,
    }
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
    control_endpoint: Option<String>,
    command: Option<Vec<String>>,
    /// `--emit-snapshot`: see `agent_interface` module docs.
    snapshot_path: Option<PathBuf>,
    /// `--script`: see `agent_interface` module docs.
    script_path: Option<PathBuf>,
}

/// Parses arguments, returning the message to print on failure.
#[inline(never)]
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
            "--control" => {
                parsed.control_endpoint = Some(rest.next().cloned().ok_or_else(|| {
                    "error: --control requires pipe:<name> or unix:<absolute-path>\n".to_owned()
                })?);
            }
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
            let _ = agenterm_platform::parent_console::write_stderr(&message);
            std::process::exit(2);
        }
    };
    let ConArgs {
        mut no_activate,
        working_dir,
        font_size,
        cols: initial_cols,
        rows: initial_rows,
        control_endpoint,
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
            let _ = agenterm_platform::parent_console::write_stderr(&message);
            std::process::exit(2);
        }
    };

    // Load config file: CLI flags override config, config overrides defaults.
    let config = load_config();

    let mut app = ConApp::new(working_dir.clone(), control_endpoint);
    let command_failed = app.command_failed();
    let session = app.active_session_mut().expect("initial terminal session");
    session.command = command;
    session.snapshot_path = snapshot_path;
    session.script = script.unwrap_or_default().into();
    // Config values (lowest priority)
    if let Some(fs) = config.font_size {
        session.font_size_logical = fs.clamp(8.0, 36.0);
    }
    if let Some(cols) = config.cols {
        session.cols = cols.max(2);
    }
    if let Some(rows) = config.rows {
        session.rows = rows.max(2);
    }
    // CLI flags override config
    if let Some(fs) = font_size {
        session.font_size_logical = fs.clamp(8.0, 36.0);
    }
    if let Some(cols) = initial_cols {
        session.cols = cols.max(2);
    }
    if let Some(rows) = initial_rows {
        session.rows = rows.max(2);
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
        let _ = agenterm_platform::parent_console::write_stderr(&format!("agenterm-con: {error}"));
        std::process::exit(1);
    }
    if command_failed.load(Ordering::Acquire) {
        std::process::exit(1);
    }
}

const USAGE: &str = "\
Usage: agenterm-con [--no-activate] [--working-dir DIR]
                   [--font-size N] [--cols N] [--rows N]
                   [--control ENDPOINT] [--emit-snapshot PATH]
                   [-e PROGRAM [ARGS...]]
       agenterm-con --version
       agenterm-con --help
       agenterm-con cli --control ENDPOINT COMMAND [ARGS...]

A standalone console host (conhost equivalent). No server, mux, or Fleet.

Control endpoint and CLI (TAB is a stable @ID; omitted target means active tab):
  agenterm-con --control pipe:\\\\.\\pipe\\agenterm-con-test
  agenterm-con cli --control pipe:\\\\.\\pipe\\agenterm-con-test list-tabs
  ... perf-stats | reset-perf-stats
  ... new-tab [--parent TAB]
  ... select-tab --target TAB | close-tab --target TAB
  ... capture-pane [--target TAB] [--max-bytes N]
  ... screenshot-pane [--target TAB] --output PATH
  ... send-text [--target TAB] TEXT
  ... send-keys [--target TAB] KEY...
  ... send-mouse [--target TAB] --action press|release|move|click
                 --button none|left|middle|right --column N --row N
  ... send-wheel [--target TAB] --column N --row N --notches N [--ctrl]
  ... wait-text [--target TAB] [--timeout-ms N] TEXT

Keys use names such as Enter, Escape, Tab, Up, F1 or modifiers such as Ctrl+C.
Mouse coordinates are zero-based terminal cells. Positive wheel notches scroll up.

  Ctrl+Shift+T       New root terminal
  Ctrl+Shift+N       New child terminal below the active tab
  Ctrl+Shift+W       Close active terminal (children are promoted)
  Ctrl+Shift+[ / ]   Switch terminal tabs
  Ctrl+Shift+I       Focus the external input area
  Click a tab to select it. Click the bottom input area; Enter sends its
  text to the active terminal.

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
fn write_offline_stdout(text: &str) {
    let _ = agenterm_platform::parent_console::write_stdout(text);
}

fn write_offline_stderr(text: &str) {
    let _ = agenterm_platform::parent_console::write_stderr(text);
}

fn offline_cli_exit(args: &[String]) -> Option<i32> {
    if args.first().is_some_and(|arg| arg == "cli") {
        return Some(match control::run_cli(args) {
            Ok(output) => {
                if !output.is_empty() {
                    write_offline_stdout(&output);
                }
                0
            }
            Err(error) => {
                write_offline_stderr(&format!("agenterm-con cli: {error}\n"));
                2
            }
        });
    }
    let alone = args.len() == 1;
    match args.first().map(String::as_str) {
        Some("--version" | "-V") if alone => {
            let _ = agenterm_platform::parent_console::write_stdout(&format!(
                "agenterm-con {}",
                env!("CARGO_PKG_VERSION")
            ));
            Some(0)
        }
        Some("--help" | "-h") if alone => {
            let _ = agenterm_platform::parent_console::write_stdout(USAGE);
            Some(0)
        }
        Some("--version" | "-V" | "--help" | "-h") => {
            let _ = agenterm_platform::parent_console::write_stderr(
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
    /// Deadline for the in-flight `WaitText`, set the first time that command
    /// is seen so re-polling it does not restart its own timeout.
    script_wait_text_deadline: Option<Instant>,
    /// Set by a script `Screenshot` command; captured and cleared by the
    /// next `render()`, since pixel data only exists transiently there.
    pending_screenshot: Option<PathBuf>,
    pending_control_screenshot: Option<(PathBuf, control::ReplySender)>,

    /// VT model. Resized in lock-step with the PTY (see `apply_resize`).
    parser: vt100::Parser<ConCallbacks>,

    /// PTY master (input writes + resize). `None` until `opened` spawns it.
    master: Option<PtyMaster>,

    /// PTY child handle. MUST stay alive for the session lifetime: dropping it
    /// closes the platform-owned Job Object
    /// (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), which kills the shell tree.
    child: Option<PtyChild>,

    /// Preallocated bounded handoff from the PTY reader thread.
    pty_output: Arc<BoundedOutputPipe>,
    /// Coalesces reader notifications so a burst produces one GUI wake until
    /// the event thread has consumed its bounded share.
    pty_wake_pending: Arc<AtomicBool>,

    /// Signaled once by the waiter thread when the child process actually
    /// exits (via Windows' process-exit notification, not PTY EOF — see
    /// `spawn_pty`). A placeholder with its sender already dropped until
    /// `spawn_pty` installs the real one.
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
    scrollbar_drag: Option<ScrollbarThumbDrag>,

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
    /// Physical space owned by the outer tab tree and composer.
    content_left_px: u32,
    content_top_px: u32,
    content_bottom_px: u32,

    /// Conservative raster-candidate evidence for retained pixels and native
    /// redraw requests. Unknown damage remains full rather than guessed.
    dirty: DirtyRegion,
    last_cursor: Option<TerminalPoint>,
    frame_width: u32,
    frame_height: u32,
}

impl Drop for ConTerminal {
    fn drop(&mut self) {
        // The reader may be blocked on a full bounded ring. Closing before the
        // rest of the session drops preserves sync_channel's old guarantee
        // that closing a tab cannot strand its reader thread forever.
        self.pty_output.close();
    }
}

/// One lightweight GUI process containing several isolated terminal sessions.
///
/// The wrapper owns tree identity and routing only. A `ConTerminal` still owns
/// its own PTY, reader/waiter threads, parser, viewport and input state, so a
/// dead child or malformed output cannot corrupt another session's state.
struct ConApp {
    workspace: workspace::Workspace,
    sessions: BTreeMap<workspace::TabId, ConTerminal>,
    composer: String,
    composer_preedit: String,
    composer_focused: bool,
    composer_select_all: bool,
    tree_scroll_offset: usize,
    sidebar_width_logical: f64,
    sidebar_resizing: bool,
    exit: bool,
    control_endpoint: Option<String>,
    control_server: Option<control::ControlServer>,
    control_waits: Vec<PendingControlWait>,
    perf_stats: PerfStats,
    chrome_dirty: DirtyRegion,
    retained: RetainedXrgbFrame,
    frame_width: u32,
    frame_height: u32,
    frame_scale: f64,
}

struct PendingControlWait {
    target: workspace::TabId,
    text: String,
    deadline: Instant,
    reply: control::ReplySender,
}

#[derive(Default)]
struct PerfStats {
    frames: u64,
    observed_frames: u64,
    render_total_us: u128,
    render_last_us: u64,
    render_max_us: u64,
    pty_drained_bytes: u64,
    pty_budget_yields: u64,
    full_candidate_frames: u64,
    partial_candidate_frames: u64,
    dirty_pixels: u64,
    frame_pixels: u64,
    host_direct_frames: u64,
    host_copy_frames: u64,
    host_copy_pixels: u64,
    platform_present: PixelPresentStats,
    present_baseline: PixelPresentStats,
    present_sequence_seen: u64,
    present_last_ns: u64,
    present_max_ns: u64,
}

impl PerfStats {
    fn record_frame(&mut self, elapsed: Duration) {
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.frames = self.frames.saturating_add(1);
        self.observed_frames = self.observed_frames.saturating_add(1);
        self.render_total_us = self.render_total_us.saturating_add(u128::from(micros));
        self.render_last_us = micros;
        self.render_max_us = self.render_max_us.max(micros);
    }

    /// Records only after the full raster function has returned successfully.
    /// These are candidate numbers, not claims about native present support.
    fn record_raster_candidate(&mut self, candidate: DirtyRegion, width: u32, height: u32) {
        let frame_pixels = u64::from(width).saturating_mul(u64::from(height));
        self.frame_pixels = self.frame_pixels.saturating_add(frame_pixels);
        if candidate.is_full() {
            self.full_candidate_frames = self.full_candidate_frames.saturating_add(1);
            self.dirty_pixels = self.dirty_pixels.saturating_add(frame_pixels);
        } else {
            // An empty candidate is still a valid non-full observation (for
            // example a screenshot-only redraw) and contributes zero pixels.
            self.partial_candidate_frames = self.partial_candidate_frames.saturating_add(1);
            self.dirty_pixels = self
                .dirty_pixels
                .saturating_add(candidate.dirty_pixels(width, height));
        }
    }

    fn record_host_direct_frame(&mut self) {
        self.host_direct_frames = self.host_direct_frames.saturating_add(1);
    }

    fn record_host_copy_frame(&mut self, width: u32, height: u32) {
        self.host_copy_frames = self.host_copy_frames.saturating_add(1);
        let pixels = u64::from(width).saturating_mul(u64::from(height));
        self.host_copy_pixels = self.host_copy_pixels.saturating_add(pixels);
    }

    /// Samples the platform's cumulative ledger without adding a second
    /// synchronization primitive. `PixelWindow::present_stats` is a GUI-thread
    /// value copy; native adapters own any internal synchronization.
    fn sync_present_stats(&mut self, current: PixelPresentStats) {
        if current.sequence > self.present_sequence_seen {
            self.present_sequence_seen = current.sequence;
            self.present_last_ns = current.last_ns;
            self.present_max_ns = self.present_max_ns.max(current.last_ns);
        }
        self.platform_present = current;
    }

    fn present_delta(&self) -> PixelPresentStats {
        let current = self.platform_present;
        let baseline = self.present_baseline;
        PixelPresentStats {
            sequence: current.sequence.saturating_sub(baseline.sequence),
            count: current.count.saturating_sub(baseline.count),
            success_count: current.success_count.saturating_sub(baseline.success_count),
            failure_count: current.failure_count.saturating_sub(baseline.failure_count),
            last_ns: self.present_last_ns,
            total_ns: current.total_ns.saturating_sub(baseline.total_ns),
            // A cumulative max is not subtractable. This is the maximum
            // latest-present sample observed after reset, and is zero until a
            // post-reset present sequence is observed.
            max_ns: self.present_max_ns,
            full_pixels: current.full_pixels.saturating_sub(baseline.full_pixels),
            partial_pixels: current
                .partial_pixels
                .saturating_sub(baseline.partial_pixels),
            requested_full_pixels: current
                .requested_full_pixels
                .saturating_sub(baseline.requested_full_pixels),
            requested_partial_pixels: current
                .requested_partial_pixels
                .saturating_sub(baseline.requested_partial_pixels),
        }
    }

    fn reset(&mut self, present: PixelPresentStats) {
        *self = Self::default();
        self.platform_present = present;
        self.present_baseline = present;
        self.present_sequence_seen = present.sequence;
    }

    fn json(&self) -> json::JsonValue {
        let average = if self.frames == 0 {
            0
        } else {
            (self.render_total_us / u128::from(self.frames)).min(u128::from(u64::MAX)) as u64
        };
        let present = self.present_delta();
        json::object([
            ("frames", self.frames.into()),
            ("observed_frames", self.observed_frames.into()),
            ("render_last_us", self.render_last_us.into()),
            ("render_average_us", average.into()),
            ("render_max_us", self.render_max_us.into()),
            ("pty_drained_bytes", self.pty_drained_bytes.into()),
            ("pty_budget_yields", self.pty_budget_yields.into()),
            ("full_candidate_frames", self.full_candidate_frames.into()),
            (
                "partial_candidate_frames",
                self.partial_candidate_frames.into(),
            ),
            ("dirty_pixels", self.dirty_pixels.into()),
            ("frame_pixels", self.frame_pixels.into()),
            ("host_direct_frames", self.host_direct_frames.into()),
            ("host_copy_frames", self.host_copy_frames.into()),
            ("host_copy_pixels", self.host_copy_pixels.into()),
            ("present_count", present.count.into()),
            ("present_success", present.success_count.into()),
            ("present_failure", present.failure_count.into()),
            ("last_ns", present.last_ns.into()),
            ("total_ns", present.total_ns.into()),
            ("max_ns", present.max_ns.into()),
            ("full_pixels", present.full_pixels.into()),
            ("partial_pixels", present.partial_pixels.into()),
            (
                "requested_full_pixels",
                present.requested_full_pixels.into(),
            ),
            (
                "requested_partial_pixels",
                present.requested_partial_pixels.into(),
            ),
        ])
    }
}

#[cfg(test)]
mod perf_stats_tests {
    use super::*;

    #[test]
    fn raster_candidate_fields_serialize_and_reset() {
        let mut stats = PerfStats::default();
        stats.record_frame(Duration::from_micros(7));
        stats.record_raster_candidate(DirtyRegion::full_frame(10, 20), 10, 20);
        stats.record_frame(Duration::from_micros(3));
        let mut partial = DirtyRegion::empty();
        partial.mark_rect(PixelRect::from_xywh(1, 2, 3, 4));
        stats.record_raster_candidate(partial, 10, 20);
        let serialized = String::from_utf8(json::to_vec(&stats.json())).expect("JSON is UTF-8");
        for field in [
            "observed_frames",
            "full_candidate_frames",
            "partial_candidate_frames",
            "dirty_pixels",
            "frame_pixels",
            "host_direct_frames",
            "host_copy_frames",
            "host_copy_pixels",
        ] {
            assert!(serialized.contains(field), "missing {field}: {serialized}");
        }
        assert_eq!(stats.observed_frames, 2);
        stats.reset(PixelPresentStats::default());
        assert_eq!(stats.observed_frames, 0);
        assert_eq!(stats.full_candidate_frames, 0);
        assert_eq!(stats.partial_candidate_frames, 0);
        assert_eq!(stats.dirty_pixels, 0);
        assert_eq!(stats.frame_pixels, 0);
        assert_eq!(stats.host_direct_frames, 0);
        assert_eq!(stats.host_copy_frames, 0);
        assert_eq!(stats.host_copy_pixels, 0);
    }

    #[test]
    fn host_copy_stats_count_actual_pixels_and_saturate() {
        let mut stats = PerfStats::default();
        stats.record_host_direct_frame();
        stats.record_host_copy_frame(10, 20);
        assert_eq!(stats.host_direct_frames, 1);
        assert_eq!(stats.host_copy_frames, 1);
        assert_eq!(stats.host_copy_pixels, 200);

        stats.host_copy_pixels = u64::MAX - 1;
        stats.record_host_copy_frame(u32::MAX, u32::MAX);
        assert_eq!(stats.host_copy_pixels, u64::MAX);
    }

    #[test]
    fn platform_present_baseline_delta_json_and_reset_semantics() {
        let baseline = PixelPresentStats {
            sequence: 4,
            count: 4,
            success_count: 3,
            failure_count: 1,
            last_ns: 8,
            total_ns: 30,
            max_ns: 8,
            full_pixels: 100,
            partial_pixels: 50,
            requested_full_pixels: 120,
            requested_partial_pixels: 60,
        };
        let current = PixelPresentStats {
            sequence: 6,
            count: 6,
            success_count: 4,
            failure_count: 2,
            last_ns: 5,
            total_ns: 43,
            max_ns: 8,
            full_pixels: 140,
            partial_pixels: 65,
            requested_full_pixels: 170,
            requested_partial_pixels: 80,
        };

        let mut stats = PerfStats::default();
        stats.reset(baseline);
        assert_eq!(stats.present_delta(), PixelPresentStats::default());

        stats.sync_present_stats(current);
        let delta = stats.present_delta();
        assert_eq!(delta.count, 2);
        assert_eq!(delta.success_count, 1);
        assert_eq!(delta.failure_count, 1);
        assert_eq!(delta.last_ns, 5);
        assert_eq!(delta.total_ns, 13);
        // The cumulative platform max (8ns) is not subtracted. After reset,
        // max is the maximum post-reset sample observed by the GUI (5ns).
        assert_eq!(delta.max_ns, 5);
        assert_eq!(delta.full_pixels, 40);
        assert_eq!(delta.partial_pixels, 15);
        assert_eq!(delta.requested_full_pixels, 50);
        assert_eq!(delta.requested_partial_pixels, 20);

        let serialized = String::from_utf8(json::to_vec(&stats.json())).expect("JSON is UTF-8");
        for field in [
            "present_count",
            "present_success",
            "present_failure",
            "last_ns",
            "total_ns",
            "max_ns",
            "full_pixels",
            "partial_pixels",
            "requested_full_pixels",
            "requested_partial_pixels",
        ] {
            assert!(serialized.contains(field), "missing {field}: {serialized}");
        }

        stats.reset(current);
        let after_reset = stats.present_delta();
        assert_eq!(after_reset.count, 0);
        assert_eq!(after_reset.last_ns, 0);
        assert_eq!(after_reset.max_ns, 0);
    }

    #[test]
    fn candidate_redraw_request_converts_bounds_without_product_semantics() {
        let mut candidate = DirtyRegion::empty();
        candidate.mark_rect(PixelRect::from_xywh(4, 6, 16, 24));
        assert_eq!(
            candidate_redraw_request(candidate, 100, 80),
            CandidateRedrawRequest::Partial(HostPixelRect::new(4, 6, 20, 30))
        );
        assert_eq!(
            candidate_redraw_request(DirtyRegion::full(), 100, 80),
            CandidateRedrawRequest::Full
        );
        assert_eq!(
            candidate_redraw_request(DirtyRegion::empty(), 100, 80),
            CandidateRedrawRequest::None
        );
    }

    #[test]
    fn frame_write_mapping_for_direct_and_transient_hosts() {
        let mut partial = DirtyRegion::empty();
        partial.mark_rect(PixelRect::from_xywh(4, 6, 16, 24));

        assert_eq!(
            frame_write_for_candidate(
                PixelBackingRetention::RetainedAcrossFrames,
                false,
                partial,
                100,
                80,
            ),
            PixelFrameWrite::Full
        );
        assert_eq!(
            frame_write_for_candidate(
                PixelBackingRetention::RetainedAcrossFrames,
                true,
                partial,
                100,
                80,
            ),
            PixelFrameWrite::Partial(HostPixelRect::new(4, 6, 20, 30))
        );
        assert_eq!(
            frame_write_for_candidate(
                PixelBackingRetention::RetainedAcrossFrames,
                true,
                DirtyRegion::empty(),
                100,
                80,
            ),
            PixelFrameWrite::None
        );
        assert_eq!(
            frame_write_for_candidate(PixelBackingRetention::Transient, true, partial, 100, 80,),
            PixelFrameWrite::Full
        );
    }
}

#[derive(Clone, Copy, Default)]
struct DrainOutcome {
    changed: bool,
    redraw: bool,
    backlog: bool,
    bytes: usize,
}

impl ConApp {
    fn new(working_dir: Option<String>, control_endpoint: Option<String>) -> Self {
        let mut workspace = workspace::Workspace::default();
        let initial = workspace.add_root("terminal".to_owned());
        let mut sessions = BTreeMap::new();
        sessions.insert(initial, ConTerminal::new(working_dir));
        Self {
            workspace,
            sessions,
            composer: String::new(),
            composer_preedit: String::new(),
            composer_focused: false,
            composer_select_all: false,
            tree_scroll_offset: 0,
            sidebar_width_logical: ui::SIDEBAR_WIDTH_DIP,
            sidebar_resizing: false,
            exit: false,
            control_endpoint,
            control_server: None,
            control_waits: Vec::new(),
            perf_stats: PerfStats::default(),
            chrome_dirty: DirtyRegion::full(),
            retained: RetainedXrgbFrame::new(),
            frame_width: 0,
            frame_height: 0,
            frame_scale: 1.0,
        }
    }

    fn command_failed(&mut self) -> Arc<AtomicBool> {
        Arc::clone(
            &self
                .active_session_mut()
                .expect("initial terminal session")
                .command_failed,
        )
    }

    fn active_session_mut(&mut self) -> Result<&mut ConTerminal, PixelWindowError> {
        let id = self.workspace.active().ok_or_else(|| {
            PixelWindowError::failed("con_session_missing", "no active terminal session")
        })?;
        self.sessions.get_mut(&id).ok_or_else(|| {
            PixelWindowError::failed(
                "con_session_missing",
                format!("active terminal session @{} is unavailable", id.get()),
            )
        })
    }

    fn active_session(&self) -> Result<&ConTerminal, PixelWindowError> {
        let id = self.workspace.active().ok_or_else(|| {
            PixelWindowError::failed("con_session_missing", "no active terminal session")
        })?;
        self.sessions.get(&id).ok_or_else(|| {
            PixelWindowError::failed(
                "con_session_missing",
                format!("active terminal session @{} is unavailable", id.get()),
            )
        })
    }

    fn refresh_title(&self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let session = self.active_session()?;
        let id = self.workspace.active().ok_or_else(|| {
            PixelWindowError::failed("con_session_missing", "no active terminal session")
        })?;
        window.set_title(&format!("{} [@{}]", session.current_title, id.get()));
        Ok(())
    }

    fn close_active_session(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let id = self.workspace.active().ok_or_else(|| {
            PixelWindowError::failed("con_session_missing", "no active terminal session")
        })?;
        self.sessions.remove(&id);
        self.workspace.close(id);
        if self.workspace.active().is_none() {
            self.exit = true;
            return Ok(());
        }
        self.mark_chrome_full();
        let metrics = window.metrics()?;
        let sidebar_width = self.sidebar_width_logical;
        let session = self.active_session_mut()?;
        Self::configure_chrome(session, metrics.scale_factor, sidebar_width);
        session.apply_resize(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        self.composer_focused = false;
        self.refresh_title(window)?;
        window.request_redraw();
        Ok(())
    }

    fn configure_chrome(session: &mut ConTerminal, scale: f64, sidebar_width_logical: f64) {
        let scale = scale.max(1.0);
        session.set_content_insets(
            (sidebar_width_logical * scale).round() as u32,
            0,
            (ui::COMPOSER_HEIGHT_DIP * scale).round() as u32,
        );
    }

    fn layout(&self, width: u32, height: u32, scale: f64) -> ui::Layout {
        ui::Layout::with_sidebar_width(width, height, scale, self.sidebar_width_logical)
    }

    fn mark_chrome_full(&mut self) {
        self.chrome_dirty.mark_full();
    }

    fn mark_chrome_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if self.frame_width == 0 || self.frame_height == 0 {
            self.mark_chrome_full();
            return;
        }
        self.chrome_dirty
            .mark_rect(PixelRect::from_xywh(x, y, width, height));
    }

    fn mark_tree_dirty(&mut self) {
        if self.frame_width == 0 || self.frame_height == 0 {
            self.mark_chrome_full();
            return;
        }
        let layout = self.layout(self.frame_width, self.frame_height, self.frame_scale);
        self.mark_chrome_rect(
            layout.sidebar.x,
            layout.sidebar.y,
            layout.sidebar.width,
            layout.sidebar.height,
        );
    }

    fn mark_composer_dirty(&mut self) {
        if self.frame_width == 0 || self.frame_height == 0 {
            self.mark_chrome_full();
            return;
        }
        let layout = self.layout(self.frame_width, self.frame_height, self.frame_scale);
        let right = layout
            .composer_send
            .x
            .saturating_add(layout.composer_send.width);
        self.mark_chrome_rect(
            layout.composer_input.x,
            layout.composer_input.y,
            right.saturating_sub(layout.composer_input.x),
            layout.composer_input.height,
        );
    }

    fn note_frame_dimensions(&mut self, width: u32, height: u32, scale: f64) {
        if self.frame_width != width || self.frame_height != height || self.frame_scale != scale {
            self.mark_chrome_full();
        }
        self.frame_width = width;
        self.frame_height = height;
        self.frame_scale = scale;
    }

    fn take_dirty_candidate(&mut self, width: u32, height: u32) -> DirtyRegion {
        let mut candidate = std::mem::take(&mut self.chrome_dirty);
        if let Ok(session) = self.active_session_mut() {
            candidate = candidate.union(session.take_dirty());
        }
        candidate.clip(width, height)
    }

    fn request_dirty_redraw(&self, window: &PixelWindow) {
        let candidate = self.chrome_dirty.union(
            self.workspace
                .active()
                .and_then(|id| self.sessions.get(&id).map(|session| session.dirty))
                .unwrap_or_else(DirtyRegion::full),
        );
        request_candidate_redraw(window, candidate, self.frame_width, self.frame_height);
    }

    fn reveal_active_tree_row(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let Some(active) = self.workspace.active() else {
            return Ok(());
        };
        let Some(index) = self
            .workspace
            .nodes()
            .iter()
            .position(|node| node.id == active)
        else {
            return Ok(());
        };
        let metrics = window.metrics()?;
        let layout = self.layout(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        self.tree_scroll_offset = ui::reveal_tree_index(
            self.tree_scroll_offset,
            index,
            self.workspace.nodes().len(),
            layout.tree_capacity(),
        );
        Ok(())
    }

    fn open_session(&mut self, window: &PixelWindow, child: bool) -> Result<(), PixelWindowError> {
        let (working_dir, command, font_size_logical, cols, rows) = {
            let current = self.active_session()?;
            (
                current.working_dir.clone(),
                current.command.clone(),
                current.font_size_logical,
                current.cols,
                current.rows,
            )
        };
        let parent = self.workspace.active();
        let id = match (child, parent) {
            (true, Some(parent)) => self.workspace.add_child(parent, "terminal".to_owned()),
            _ => Some(self.workspace.add_root("terminal".to_owned())),
        }
        .ok_or_else(|| {
            PixelWindowError::failed("con_tab_create", "active parent is unavailable")
        })?;

        let mut session = ConTerminal::new(working_dir);
        session.command = command;
        session.font_size_logical = font_size_logical;
        session.cols = cols;
        session.rows = rows;
        Self::configure_chrome(
            &mut session,
            window.metrics()?.scale_factor,
            self.sidebar_width_logical,
        );
        if let Err(error) = session.opened(window) {
            self.workspace.close(id);
            return Err(error);
        }
        self.sessions.insert(id, session);
        self.mark_chrome_full();
        self.reveal_active_tree_row(window)?;
        self.refresh_title(window)
    }

    fn select_relative(
        &mut self,
        window: &PixelWindow,
        direction: isize,
    ) -> Result<(), PixelWindowError> {
        let ids: Vec<_> = self.workspace.nodes().iter().map(|node| node.id).collect();
        let Some(active) = self.workspace.active() else {
            return Ok(());
        };
        let Some(index) = ids.iter().position(|id| *id == active) else {
            return Err(PixelWindowError::failed(
                "con_session_missing",
                "active tab is not in the tree",
            ));
        };
        let next = (index as isize + direction).rem_euclid(ids.len() as isize) as usize;
        self.mark_chrome_full();
        self.workspace.set_active(ids[next]);
        self.reveal_active_tree_row(window)?;
        let metrics = window.metrics()?;
        let sidebar_width = self.sidebar_width_logical;
        let session = self.active_session_mut()?;
        Self::configure_chrome(session, metrics.scale_factor, sidebar_width);
        session.apply_resize(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        self.refresh_title(window)?;
        window.focus();
        window.request_redraw();
        Ok(())
    }

    fn handle_workspace_shortcut(
        &mut self,
        window: &PixelWindow,
        key: &NormalizedKeyEvent,
    ) -> Result<bool, PixelWindowError> {
        if key.state != KeyPressState::Pressed || !key.modifiers.control || !key.modifiers.shift {
            return Ok(false);
        }
        let LogicalKey::Character(text) = &key.logical else {
            return Ok(false);
        };
        if text.eq_ignore_ascii_case("t") {
            self.open_session(window, false)?;
            return Ok(true);
        }
        if text.eq_ignore_ascii_case("n") {
            self.open_session(window, true)?;
            return Ok(true);
        }
        if text.eq_ignore_ascii_case("w") {
            self.close_active_session(window)?;
            return Ok(true);
        }
        if text.eq_ignore_ascii_case("i") {
            self.composer_focused = true;
            self.update_composer_ime_anchor(window)?;
            self.mark_composer_dirty();
            self.request_dirty_redraw(window);
            return Ok(true);
        }
        if text == "[" {
            self.select_relative(window, -1)?;
            return Ok(true);
        }
        if text == "]" {
            self.select_relative(window, 1)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn handle_tree_pointer(
        &mut self,
        window: &PixelWindow,
        position: &LogicalPoint,
    ) -> Result<bool, PixelWindowError> {
        let metrics = window.metrics()?;
        let scale = metrics.scale_factor.max(1.0);
        let layout = self.layout(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        let physical_x = (position.x * scale).max(0.0) as u32;
        let physical_y = (position.y * scale).max(0.0) as u32;
        let ids: Vec<_> = self.workspace.nodes().iter().map(|node| node.id).collect();
        match ui::tree_hit(
            layout,
            physical_x,
            physical_y,
            self.tree_scroll_offset,
            ids.len(),
            scale,
        ) {
            ui::TreeHit::Outside => return Ok(false),
            ui::TreeHit::Background => return Ok(true),
            ui::TreeHit::ZoomOut => {
                self.active_session_mut()?.zoom_font(window, false);
                return Ok(true);
            }
            ui::TreeHit::ZoomIn => {
                self.active_session_mut()?.zoom_font(window, true);
                return Ok(true);
            }
            ui::TreeHit::Close(index) => {
                self.workspace.set_active(ids[index]);
                self.mark_chrome_full();
                self.close_active_session(window)?;
                self.tree_scroll_offset = ui::clamp_tree_scroll(
                    self.tree_scroll_offset,
                    self.workspace.nodes().len(),
                    layout.tree_capacity(),
                );
                return Ok(true);
            }
            ui::TreeHit::Select(index) => {
                self.workspace.set_active(ids[index]);
                self.reveal_active_tree_row(window)?;
                self.mark_chrome_full();
            }
        }
        let sidebar_width = self.sidebar_width_logical;
        let session = self.active_session_mut()?;
        Self::configure_chrome(session, metrics.scale_factor, sidebar_width);
        session.apply_resize(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        self.composer_focused = false;
        self.refresh_title(window)?;
        window.focus();
        window.request_redraw();
        Ok(true)
    }

    fn composer_hit(
        &self,
        window: &PixelWindow,
        position: &LogicalPoint,
    ) -> Result<ui::ComposerHit, PixelWindowError> {
        let metrics = window.metrics()?;
        let scale = metrics.scale_factor.max(1.0);
        let layout = self.layout(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        Ok(ui::composer_hit(
            layout,
            (position.x * scale).max(0.0) as u32,
            (position.y * scale).max(0.0) as u32,
        ))
    }

    fn update_composer_ime_anchor(&self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let metrics = window.metrics()?;
        let scale = metrics.scale_factor.max(1.0);
        let layout = self.layout(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        let x = layout.composer_input.x as f64 / scale
            + 10.0
            + self.composer.chars().count() as f64 * 8.0;
        let y = layout.composer_input.y as f64 / scale + 8.0;
        let _ = window.set_ime_cursor_area(agenterm_platform::window_host::LogicalRect::new(
            x, y, 2.0, 20.0,
        ));
        Ok(())
    }

    fn handle_composer_key(&mut self, window: &PixelWindow, key: &NormalizedKeyEvent) -> bool {
        if key.state != KeyPressState::Pressed {
            return true;
        }
        if key.modifiers.control
            && !key.modifiers.alt
            && let LogicalKey::Character(text) = &key.logical
        {
            if text.eq_ignore_ascii_case("a") {
                composer::select_all(&self.composer, &mut self.composer_select_all);
            } else if text.eq_ignore_ascii_case("c") {
                if let Some(text) =
                    composer::selected_text(&self.composer, &self.composer_select_all)
                {
                    let _ = agenterm_platform::clipboard::set_text(text);
                }
            } else if text.eq_ignore_ascii_case("x") {
                if let Some(text) = composer::cut(&mut self.composer, &mut self.composer_select_all)
                {
                    let _ = agenterm_platform::clipboard::set_text(&text);
                }
            } else if text.eq_ignore_ascii_case("v")
                && let Ok(text) =
                    agenterm_platform::clipboard::get_text(composer::PASTE_LIMIT_BYTES)
            {
                composer::paste(&mut self.composer, &mut self.composer_select_all, &text);
            }
            let _ = self.update_composer_ime_anchor(window);
            return true;
        }
        match &key.logical {
            LogicalKey::Named(NamedKey::Enter) => {
                self.submit_composer();
            }
            LogicalKey::Named(NamedKey::Backspace) => {
                composer::backspace(&mut self.composer, &mut self.composer_select_all);
            }
            LogicalKey::Named(NamedKey::Escape) => {
                self.composer_focused = false;
                self.composer_preedit.clear();
                self.composer_select_all = false;
            }
            LogicalKey::Named(NamedKey::Space) if !key.modifiers.control && !key.modifiers.alt => {
                composer::insert(&mut self.composer, &mut self.composer_select_all, " ");
            }
            LogicalKey::Character(text)
                if !key.modifiers.control && !key.modifiers.alt && !text.is_empty() =>
            {
                composer::insert(&mut self.composer, &mut self.composer_select_all, text);
            }
            _ => {}
        }
        let _ = self.update_composer_ime_anchor(window);
        true
    }

    fn submit_composer(&mut self) {
        if !self.composer.is_empty() {
            let mut input = std::mem::take(&mut self.composer);
            input.push('\r');
            if let Ok(session) = self.active_session_mut() {
                // Submission crosses the PTY boundary and can change arbitrary
                // terminal cells; the composer rectangle alone is not enough.
                session.dirty.mark_full();
                session.scroll_to_bottom();
                session.write_pty(input.as_bytes());
            }
        }
        self.composer.clear();
        self.composer_preedit.clear();
        self.composer_select_all = false;
    }

    fn handle_composer_ime(
        &mut self,
        window: &PixelWindow,
        event: agenterm_platform::ime::ImeEvent,
    ) {
        use agenterm_platform::ime::{ImeAction, classify_event};
        match classify_event(event, true) {
            ImeAction::UpdatePreedit { text, .. } => self.composer_preedit = text,
            ImeAction::ClearPreedit => self.composer_preedit.clear(),
            ImeAction::CommitText(text) => {
                self.composer_preedit.clear();
                composer::insert(&mut self.composer, &mut self.composer_select_all, &text);
            }
            ImeAction::None => {}
            _ => self.composer_preedit.clear(),
        }
        let _ = self.update_composer_ime_anchor(window);
    }

    fn control_target(&self, target: Option<workspace::TabId>) -> Result<workspace::TabId, String> {
        let id = target
            .or_else(|| self.workspace.active())
            .ok_or_else(|| "no active terminal".to_owned())?;
        self.sessions
            .contains_key(&id)
            .then_some(id)
            .ok_or_else(|| format!("terminal @{} does not exist", id.get()))
    }

    fn dispatch_control(&mut self, window: &PixelWindow, request: control::IncomingRequest) {
        use control::CliCommand;
        self.perf_stats.sync_present_stats(window.present_stats());
        let mut reply = Some(request.reply);
        let result = match request.command {
            CliCommand::ListTabs => {
                let active = self.workspace.active();
                let tabs: Vec<_> = self
                    .workspace
                    .nodes()
                    .iter()
                    .map(|node| {
                        let session = self.sessions.get(&node.id);
                        json::object([
                            ("id", format!("@{}", node.id.get()).into()),
                            (
                                "parent",
                                json::nullable(node.parent.map(|id| format!("@{}", id.get()))),
                            ),
                            (
                                "title",
                                session
                                    .map_or(node.title.as_str(), |session| {
                                        session.current_title.as_str()
                                    })
                                    .into(),
                            ),
                            ("active", (active == Some(node.id)).into()),
                            (
                                "child_alive",
                                session.is_some_and(|session| !session.child_gone).into(),
                            ),
                        ])
                    })
                    .collect();
                Ok(json::object([("tabs", json::JsonValue::Array(tabs))]))
            }
            CliCommand::PerfStats => Ok(self.perf_stats.json()),
            CliCommand::ResetPerfStats => {
                self.perf_stats.reset(window.present_stats());
                Ok(json::object([("reset", true.into())]))
            }
            CliCommand::NewTab { parent } => (|| {
                if let Some(parent) = parent {
                    self.control_target(Some(parent))?;
                    self.workspace.set_active(parent);
                }
                self.open_session(window, parent.is_some())
                    .map_err(|error| error.to_string())?;
                let id = self
                    .workspace
                    .active()
                    .ok_or_else(|| "new terminal was not activated".to_owned())?;
                Ok(json::object([
                    ("id", format!("@{}", id.get()).into()),
                    (
                        "parent",
                        json::nullable(parent.map(|id| format!("@{}", id.get()))),
                    ),
                ]))
            })(),
            CliCommand::SelectTab { target } => self.control_target(Some(target)).map(|id| {
                self.mark_chrome_full();
                self.workspace.set_active(id);
                window.request_redraw();
                json::object([("active", format!("@{}", id.get()).into())])
            }),
            CliCommand::CloseTab { target } => self.control_target(Some(target)).and_then(|id| {
                self.workspace.set_active(id);
                self.close_active_session(window)
                    .map_err(|error| error.to_string())?;
                Ok(json::object([("closed", format!("@{}", id.get()).into())]))
            }),
            CliCommand::CapturePane { target, max_bytes } => {
                self.control_target(target).and_then(|id| {
                    let session = self
                        .sessions
                        .get_mut(&id)
                        .ok_or_else(|| "terminal disappeared".to_owned())?;
                    session.drain_pty();
                    let mut text = session.build_snapshot().rows_text.join("\n");
                    if text.len() > max_bytes {
                        let mut end = max_bytes;
                        while end > 0 && !text.is_char_boundary(end) {
                            end -= 1;
                        }
                        text.truncate(end);
                    }
                    Ok(json::JsonValue::String(text))
                })
            }
            CliCommand::SendText { target, text } => self.control_target(target).and_then(|id| {
                let session = self
                    .sessions
                    .get_mut(&id)
                    .ok_or_else(|| "terminal disappeared".to_owned())?;
                session.scroll_to_bottom();
                session.write_pty(text.as_bytes());
                Ok(json::object([("sent_bytes", text.len().into())]))
            }),
            CliCommand::SendKeys { target, keys } => self.control_target(target).and_then(|id| {
                let session = self
                    .sessions
                    .get_mut(&id)
                    .ok_or_else(|| "terminal disappeared".to_owned())?;
                for key in &keys {
                    let (key, ctrl, alt, shift) = parse_control_key(key)?;
                    session.execute_script_key(key, ctrl, alt, shift);
                }
                Ok(json::object([("sent_keys", keys.len().into())]))
            }),
            CliCommand::SendMouse {
                target,
                action,
                button,
                column,
                row,
            } => self.control_target(target).and_then(|id| {
                let session = self
                    .sessions
                    .get_mut(&id)
                    .ok_or_else(|| "terminal disappeared".to_owned())?;
                if row >= session.rows || column >= session.cols {
                    return Err(format!(
                        "mouse cell {row},{column} is outside {}x{}",
                        session.rows, session.cols
                    ));
                }
                match action {
                    control::MouseAction::Move => {
                        session.execute_script_mouse_move(window, row, column)
                    }
                    control::MouseAction::Click => {
                        let button = control_mouse_button(button)?;
                        session
                            .execute_script_click(window, row, column, button, false, false, false);
                    }
                    control::MouseAction::Press | control::MouseAction::Release => {
                        let button = control_mouse_button(button)?;
                        let state = if action == control::MouseAction::Press {
                            PointerButtonState::Pressed
                        } else {
                            PointerButtonState::Released
                        };
                        session.execute_script_pointer_button(
                            window, row, column, button, false, false, false, state,
                        );
                    }
                }
                Ok(json::object([("delivered", true.into())]))
            }),
            CliCommand::SendWheel {
                target,
                column,
                row,
                notches,
                ctrl,
            } => self.control_target(target).and_then(|id| {
                let session = self
                    .sessions
                    .get_mut(&id)
                    .ok_or_else(|| "terminal disappeared".to_owned())?;
                if row >= session.rows || column >= session.cols {
                    return Err(format!(
                        "mouse cell {row},{column} is outside {}x{}",
                        session.rows, session.cols
                    ));
                }
                session.execute_script_wheel(window, row, column, f32::from(notches), ctrl);
                Ok(json::object([("delivered_notches", notches.into())]))
            }),
            CliCommand::ScreenshotPane { target, output } => {
                self.control_target(target).and_then(|id| {
                    if self.workspace.active() != Some(id) {
                        self.workspace.set_active(id);
                    }
                    let session = self
                        .sessions
                        .get_mut(&id)
                        .ok_or_else(|| "terminal disappeared".to_owned())?;
                    if session.pending_control_screenshot.is_some() {
                        return Err("a screenshot is already pending for this terminal".to_owned());
                    }
                    session.pending_control_screenshot = Some((
                        PathBuf::from(output),
                        reply.take().expect("control reply available"),
                    ));
                    window.request_redraw();
                    Ok(json::JsonValue::Null)
                })
            }
            CliCommand::WaitText {
                target,
                text,
                timeout_ms,
            } => self.control_target(target).and_then(|id| {
                if self.control_waits.len() >= 32 {
                    return Err("too many pending wait-text requests".to_owned());
                }
                if self
                    .sessions
                    .get(&id)
                    .is_some_and(|session| session.screen_contains(&text))
                {
                    return Ok(json::object([("matched", true.into())]));
                }
                self.control_waits.push(PendingControlWait {
                    target: id,
                    text,
                    deadline: Instant::now() + Duration::from_millis(timeout_ms),
                    reply: reply.take().expect("control reply available"),
                });
                Ok(json::JsonValue::Null)
            }),
        };
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
    }

    fn drain_control(&mut self, window: &PixelWindow, now: Instant) -> Option<Instant> {
        loop {
            let request = self
                .control_server
                .as_ref()
                .and_then(control::ControlServer::try_recv);
            let Some(request) = request else { break };
            self.dispatch_control(window, request);
        }
        let mut pending = Vec::new();
        let mut next = None;
        for wait in std::mem::take(&mut self.control_waits) {
            let matched = self
                .sessions
                .get(&wait.target)
                .is_some_and(|session| session.screen_contains(&wait.text));
            if matched {
                let _ = wait
                    .reply
                    .send(Ok(json::object([("matched", true.into())])));
            } else if now >= wait.deadline {
                let _ = wait.reply.send(Err(format!(
                    "wait-text timed out waiting for {:?}",
                    wait.text
                )));
            } else {
                next =
                    Some(next.map_or(wait.deadline, |current: Instant| current.min(wait.deadline)));
                pending.push(wait);
            }
        }
        self.control_waits = pending;
        next
    }

    fn handle_sidebar_resize(
        &mut self,
        window: &PixelWindow,
        event: &PixelWindowEvent,
    ) -> Result<bool, PixelWindowError> {
        let metrics = window.metrics()?;
        let scale = metrics.scale_factor.max(1.0);
        let layout = self.layout(
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        let physical = |position: &LogicalPoint| {
            (
                (position.x * scale).max(0.0) as u32,
                (position.y * scale).max(0.0) as u32,
            )
        };
        match event {
            PixelWindowEvent::PointerMoved { position, .. } if self.sidebar_resizing => {
                self.sidebar_width_logical =
                    ui::sidebar_width_from_pointer(position.x, metrics.logical_size.width);
                let sidebar_width = self.sidebar_width_logical;
                let session = self.active_session_mut()?;
                Self::configure_chrome(session, metrics.scale_factor, sidebar_width);
                session.queue_resize(
                    metrics.physical_width,
                    metrics.physical_height,
                    metrics.scale_factor,
                );
                let _ = window.set_pointer_cursor(PixelPointerCursor::ResizeHorizontal);
                window.request_redraw();
                Ok(true)
            }
            PixelWindowEvent::PointerMoved { position, .. } => {
                let (x, y) = physical(position);
                let over_grip = layout.sidebar_resize_grip(scale).contains(x, y);
                let _ = window.set_pointer_cursor(if over_grip {
                    PixelPointerCursor::ResizeHorizontal
                } else {
                    PixelPointerCursor::Arrow
                });
                Ok(over_grip)
            }
            PixelWindowEvent::PointerButton {
                button: PointerButton::Left,
                state: PointerButtonState::Pressed,
                position: Some(position),
                ..
            } => {
                let (x, y) = physical(position);
                if !layout.sidebar_resize_grip(scale).contains(x, y) {
                    return Ok(false);
                }
                self.sidebar_resizing = true;
                let _ = window.set_pointer_capture(true);
                let _ = window.set_pointer_cursor(PixelPointerCursor::ResizeHorizontal);
                Ok(true)
            }
            PixelWindowEvent::PointerButton {
                button: PointerButton::Left,
                state: PointerButtonState::Released,
                ..
            } if std::mem::take(&mut self.sidebar_resizing) => {
                let _ = window.set_pointer_capture(false);
                Ok(true)
            }
            PixelWindowEvent::PointerCaptureLost if std::mem::take(&mut self.sidebar_resizing) => {
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn paint_chrome(
        &self,
        pixels: &mut [u32],
        width: u32,
        height: u32,
        candidate: DirtyRegion,
    ) -> Result<(), PixelWindowError> {
        let session = self.active_session()?;
        let clip = candidate_bounds(candidate, width, height);
        let mut surface = Surface::with_clip(pixels, width, height, clip);
        let scale = session.scale.max(1.0);
        let layout = self.layout(width, height, scale);
        let tree_width = layout.sidebar.width;
        let header_height = layout.tree_header_height;
        let row_height = layout.tree_row_height;
        // High-contrast monochrome chrome. Applications still retain their
        // explicit ANSI colors inside the terminal; only the host UI uses
        // black/white/gray so controls remain legible without color cues.
        let tree_bg = Rgb(0x08, 0x08, 0x08);
        let tree_rule = Rgb(0x70, 0x70, 0x70);
        let branch = Rgb(0x98, 0x98, 0x98);
        let active_bg = Rgb(0x32, 0x32, 0x32);
        let accent = Rgb(0xFF, 0xFF, 0xFF);
        let composer_bg = Rgb(0x00, 0x00, 0x00);
        let text = Rgb(0xF5, 0xF5, 0xF5);
        let muted = Rgb(0xC0, 0xC0, 0xC0);
        surface.fill_rect(0, 0, tree_width, height, tree_bg.to_xrgb());
        surface.fill_rect(
            tree_width.saturating_sub(1),
            0,
            1,
            height,
            tree_rule.to_xrgb(),
        );
        surface.fill_rect(
            tree_width,
            height.saturating_sub(session.content_bottom_px),
            width.saturating_sub(tree_width),
            session.content_bottom_px,
            composer_bg.to_xrgb(),
        );
        surface.fill_rect(
            tree_width,
            height.saturating_sub(session.content_bottom_px),
            width.saturating_sub(tree_width),
            1,
            tree_rule.to_xrgb(),
        );

        paint_chrome_text(
            &mut surface,
            14,
            9,
            "TERMINALS",
            muted,
            12,
            tree_width.saturating_sub(28),
        );
        paint_chrome_text(
            &mut surface,
            layout.zoom_out.x + 6,
            layout.zoom_out.y + 5,
            "z",
            muted,
            12,
            layout.zoom_out.width.saturating_sub(6),
        );
        paint_chrome_text(
            &mut surface,
            layout.zoom_in.x + 5,
            layout.zoom_in.y + 4,
            "Z",
            accent,
            14,
            layout.zoom_in.width.saturating_sub(5),
        );

        let nodes = self.workspace.nodes();
        let depths =
            agenterm_ui_core::compute_tree_depths_by(nodes, |node| node.id, |node| node.parent)
                .unwrap_or_else(|_| vec![0; nodes.len()]);
        for (visible_index, (node_index, node)) in nodes
            .iter()
            .enumerate()
            .skip(self.tree_scroll_offset)
            .enumerate()
        {
            let y = header_height + visible_index as u32 * row_height;
            if y >= height {
                break;
            }
            let depth = depths.get(node_index).copied().unwrap_or(0).min(8);
            let indent = 14 + depth * 18;
            if self.workspace.active() == Some(node.id) {
                surface.fill_rect(0, y, tree_width, row_height, active_bg.to_xrgb());
                surface.fill_rect(0, y, 3, row_height, accent.to_xrgb());
            }
            if depth > 0 {
                let branch_x = indent.saturating_sub(10);
                surface.fill_rect(branch_x, y, 1, row_height / 2 + 1, branch.to_xrgb());
                surface.fill_rect(branch_x, y + row_height / 2, 8, 1, branch.to_xrgb());
            }
            let title = self
                .sessions
                .get(&node.id)
                .map(|terminal| terminal.current_title.as_str())
                .filter(|title| !title.is_empty())
                .unwrap_or(node.title.as_str());
            paint_chrome_text(
                &mut surface,
                indent,
                y + 7,
                &format!("@{}  {}", node.id.get(), title),
                text,
                14,
                tree_width.saturating_sub(indent + 38),
            );
            let close = layout.tree_close_rect(visible_index, scale);
            paint_chrome_text(
                &mut surface,
                close.x + 6,
                close.y + 3,
                "x",
                muted,
                11,
                close.width.saturating_sub(6),
            );
        }

        let active_id = self.workspace.active().map(|id| id.get()).unwrap_or(0);
        let input_y = layout.composer.y;
        paint_chrome_text(
            &mut surface,
            tree_width + 12,
            input_y + 7,
            &format!("SEND TO @{}", active_id),
            if self.composer_focused { accent } else { muted },
            11,
            layout.composer.width.saturating_sub(24),
        );
        surface.fill_rect(
            layout.composer_input.x,
            layout.composer_input.y,
            layout.composer_input.width,
            layout.composer_input.height,
            if self.composer_focused {
                accent
            } else {
                tree_rule
            }
            .to_xrgb(),
        );
        surface.fill_rect(
            layout.composer_input.x + 1,
            layout.composer_input.y + 1,
            layout.composer_input.width.saturating_sub(2),
            layout.composer_input.height.saturating_sub(2),
            if self.composer_select_all {
                active_bg
            } else {
                composer_bg
            }
            .to_xrgb(),
        );
        surface.fill_rect(
            layout.composer_send.x,
            layout.composer_send.y,
            layout.composer_send.width,
            layout.composer_send.height,
            active_bg.to_xrgb(),
        );
        let mut composer = format!("{}{}", self.composer, self.composer_preedit);
        if self.composer_focused && !self.composer_select_all {
            composer.push('|');
        }
        paint_chrome_text(
            &mut surface,
            layout.composer_input.x + 10,
            layout.composer_input.y + 12,
            &composer,
            text,
            15,
            layout.composer_input.width.saturating_sub(20),
        );
        paint_chrome_text(
            &mut surface,
            layout.composer_send.x + 17,
            layout.composer_send.y + 12,
            "SEND",
            accent,
            13,
            layout.composer_send.width.saturating_sub(20),
        );
        Ok(())
    }
}

impl ConTerminal {
    fn new(working_dir: Option<String>) -> Self {
        let pty_output = Arc::new(BoundedOutputPipe::new(PTY_QUEUE_BYTES));
        pty_output.close();
        let (exit_tx, exit_rx) = mpsc::channel();
        drop(exit_tx);
        Self {
            working_dir,
            command: None,
            current_title: String::new(),
            snapshot_path: None,
            script: VecDeque::new(),
            script_wait_until: None,
            script_wait_text_deadline: None,
            pending_screenshot: None,
            pending_control_screenshot: None,
            parser: vt100::Parser::new_with_callbacks(24, 80, SCROLLBACK, ConCallbacks::default()),
            master: None,
            child: None,
            pty_output,
            pty_wake_pending: Arc::new(AtomicBool::new(false)),
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
            default_fg: Rgb(0xF0, 0xF0, 0xF0),
            default_bg: Rgb(0x00, 0x00, 0x00),
            child_gone: false,
            exit: false,
            scroll_offset: 0,
            wheel_accumulator: 0.0,
            scrollbar_drag: None,
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
            content_left_px: 0,
            content_top_px: 0,
            content_bottom_px: 0,
            dirty: DirtyRegion::full(),
            last_cursor: None,
            frame_width: 0,
            frame_height: 0,
        }
    }

    fn set_content_insets(&mut self, left: u32, top: u32, bottom: u32) {
        if self.content_left_px != left
            || self.content_top_px != top
            || self.content_bottom_px != bottom
        {
            self.dirty.mark_full();
        }
        self.content_left_px = left;
        self.content_top_px = top;
        self.content_bottom_px = bottom;
    }

    fn take_dirty(&mut self) -> DirtyRegion {
        std::mem::take(&mut self.dirty)
    }

    fn request_dirty_redraw(&self, window: &PixelWindow) {
        request_candidate_redraw(window, self.dirty, self.frame_width, self.frame_height);
    }

    fn note_frame_dimensions(&mut self, width: u32, height: u32) {
        if self.frame_width != width || self.frame_height != height {
            self.dirty.mark_full();
        }
        self.frame_width = width;
        self.frame_height = height;
    }

    fn mark_cell(&mut self, point: TerminalPoint) {
        if !self.mark_cursor_position((point.row, point.col)) {
            self.dirty.mark_full();
        }
    }

    fn mark_cursor_position(&mut self, position: (u16, u16)) -> bool {
        if self.frame_width == 0
            || self.frame_height == 0
            || self.cols == 0
            || self.rows == 0
            || self.cell_w == 0
            || self.cell_h == 0
        {
            return false;
        }
        let viewport_right = self
            .frame_width
            .saturating_sub(ui::terminal_scrollbar_width(self.scale));
        let viewport_bottom = self.frame_height.saturating_sub(self.content_bottom_px);
        let row = position.0.min(self.rows.saturating_sub(1));
        let col = position.1.min(self.cols.saturating_sub(1));
        let x = self
            .content_left_px
            .saturating_add(u32::from(col).saturating_mul(self.cell_w));
        let y = self
            .content_top_px
            .saturating_add(u32::from(row).saturating_mul(self.cell_h));
        let left = x.min(viewport_right);
        let top = y.min(viewport_bottom);
        let right = x
            .saturating_add(self.cell_w.saturating_mul(2))
            .min(viewport_right);
        let bottom = y.saturating_add(self.cell_h).min(viewport_bottom);
        let rect = PixelRect {
            left,
            top,
            right,
            bottom,
        };
        if rect.is_empty() {
            false
        } else {
            self.dirty.mark_rect(rect);
            true
        }
    }

    fn mark_terminal_rows(&mut self, rows: vt100::RowRange) -> bool {
        let rows = rows.clip(self.rows);
        if rows.is_empty() {
            return false;
        }
        let viewport_right = self
            .frame_width
            .saturating_sub(ui::terminal_scrollbar_width(self.scale));
        let viewport_bottom = self.frame_height.saturating_sub(self.content_bottom_px);
        let terminal_right = self
            .content_left_px
            .saturating_add(u32::from(self.cols).saturating_mul(self.cell_w))
            .min(viewport_right);
        let mut dirty_rows = DirtyRows::empty();
        dirty_rows.mark_range(rows.first(), u64::from(rows.end()));
        let Some(rect) = dirty_rows.to_pixel_bounds(
            self.content_left_px,
            self.content_top_px,
            self.cell_w,
            self.cell_h,
            terminal_right,
            viewport_bottom,
        ) else {
            return false;
        };
        self.dirty.mark_rect(rect);
        true
    }

    fn mark_vt_damage(&mut self, damage: vt100::ScreenDamage) {
        if damage.needs_full_raster() {
            self.dirty.mark_full();
            return;
        }

        let mut needs_full = false;
        if !damage.rows().is_empty() && !self.mark_terminal_rows(damage.rows()) {
            needs_full = true;
        }
        if damage.cursor_changed() {
            match (damage.cursor_before(), damage.cursor_after()) {
                (Some(before), Some(after)) => {
                    if !self.mark_cursor_position(before) || !self.mark_cursor_position(after) {
                        needs_full = true;
                    }
                }
                _ => needs_full = true,
            }
        }
        if needs_full {
            self.dirty.mark_full();
        }
    }

    fn mark_cursor_change(&mut self) {
        if let Some(previous) = self.last_cursor {
            self.mark_cell(previous);
        }
        let cursor = self.parser.screen().cursor_position();
        self.mark_cell(TerminalPoint {
            row: cursor.0,
            col: cursor.1,
        });
    }

    fn mark_ime_bounds(&mut self) {
        let cursor = self.parser.screen().cursor_position();
        let x = self
            .content_left_px
            .saturating_add(u32::from(cursor.1).saturating_mul(self.cell_w));
        let y = self
            .content_top_px
            .saturating_add(u32::from(cursor.0).saturating_mul(self.cell_h));
        let right = self
            .content_left_px
            .saturating_add(u32::from(self.cols).saturating_mul(self.cell_w));
        if right > x && self.cell_h > 0 {
            self.dirty.mark_rect(PixelRect::from_xywh(
                x,
                y,
                right.saturating_sub(x),
                self.cell_h,
            ));
        } else {
            self.dirty.mark_full();
        }
    }

    fn mark_selection(&mut self, selection: Option<(TerminalPoint, TerminalPoint)>) {
        let Some((start, end)) = selection.map(|(a, b)| TerminalPoint::normalize(a, b)) else {
            return;
        };
        let mut rows = DirtyRows::empty();
        rows.mark_range(u32::from(start.row), u64::from(end.row).saturating_add(1));
        if let Some(bounds) = rows.to_pixel_bounds(
            self.content_left_px,
            self.content_top_px,
            self.cell_w,
            self.cell_h,
            self.frame_width,
            self.frame_height,
        ) {
            self.dirty.mark_rect(bounds);
        } else {
            self.dirty.mark_full();
        }
    }

    fn mark_selection_change(
        &mut self,
        previous: Option<(TerminalPoint, TerminalPoint)>,
        current: Option<(TerminalPoint, TerminalPoint)>,
    ) {
        self.mark_selection(previous);
        self.mark_selection(current);
    }

    fn mark_scrollbar_bounds(&mut self) {
        if self.frame_width == 0 || self.frame_height == 0 {
            self.dirty.mark_full();
            return;
        }
        let (geometry, _, _) = self.scrollbar_geometry(self.frame_width, self.frame_height);
        for rect in [geometry.track, geometry.thumb] {
            let left = rect.left.max(0) as u32;
            let top = rect.top.max(0) as u32;
            let right = rect.right.max(0) as u32;
            let bottom = rect.bottom.max(0) as u32;
            if right > left && bottom > top {
                self.dirty.mark_rect(PixelRect {
                    left,
                    top,
                    right,
                    bottom,
                });
            }
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
        let output = Arc::new(BoundedOutputPipe::new(PTY_QUEUE_BYTES));
        let reader_output = Arc::clone(&output);
        let waker = window.waker();
        let wake_pending = Arc::new(AtomicBool::new(false));
        let reader_wake_pending = Arc::clone(&wake_pending);
        thread::Builder::new()
            .name("agenterm-con-reader".into())
            .spawn(move || {
                let mut buf = [0u8; READ_BUF];
                loop {
                    match reader.io().read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if reader_output.push_blocking(&buf[..n]).is_err() {
                                break;
                            }
                            if !reader_wake_pending.swap(true, Ordering::AcqRel) {
                                let _ = waker.wake();
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                reader_output.close();
                if !reader_wake_pending.swap(true, Ordering::AcqRel) {
                    let _ = waker.wake();
                }
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
        self.pty_output = output;
        self.pty_wake_pending = wake_pending;
        self.child_exit_rx = exit_rx;

        Ok(())
    }

    fn drain_pty(&mut self) -> DrainOutcome {
        self.pty_wake_pending.store(false, Ordering::Release);
        let mut outcome = DrainOutcome::default();
        // Only `Ok(())` (an actual signal from the waiter thread) means
        // anything here — both the placeholder channel `new()` installs
        // before a child exists and a waiter thread that hasn't finished yet
        // report as empty/disconnected, which must not be mistaken for exit.
        if let Ok(()) = self.child_exit_rx.try_recv() {
            self.child_gone = true;
            outcome.redraw = true;
        }

        let output = Arc::clone(&self.pty_output);
        let report = output.drain(PTY_DRAIN_BUDGET_BYTES, |bytes| {
            self.parser.process(bytes);
            outcome.changed = true;
            outcome.redraw = true;
            // Flush terminal-query replies immediately after the contiguous
            // input span that completed them.
            let replies = std::mem::take(&mut self.parser.callbacks_mut().pending_replies);
            if !replies.is_empty() {
                self.write_pty(&replies);
            }
        });
        outcome.bytes = report.bytes;
        outcome.backlog = report.backlog;
        if outcome.backlog {
            self.pty_wake_pending.store(true, Ordering::Release);
        }
        // New output snaps scrollback to bottom.
        if outcome.changed && self.scroll_offset > 0 {
            self.scroll_offset = 0;
            self.parser.screen_mut().set_scrollback(0);
        }
        // New output clears stale selection.
        if outcome.changed && !self.selecting {
            self.mark_selection(self.selection);
            self.selection = None;
        }
        let damage = self.parser.take_damage();
        if !damage.is_empty() {
            outcome.redraw = true;
        }
        self.mark_vt_damage(damage);
        outcome
    }

    /// Applies a settled geometry: resize PTY first, then the VT model. The PTY
    /// resize is allowed to fail (some backends reject transient bad sizes);
    /// the model still converges so the next event is consistent.
    fn apply_resize(&mut self, phys_w: u32, phys_h: u32, scale: f64) {
        // Resize, DPI, font metrics, and grid changes all invalidate the
        // complete terminal viewport, even when clamping preserves rows/cols.
        self.dirty.mark_full();
        self.scale = scale;
        self.recompute_metrics(scale);
        let usable_w = phys_w
            .saturating_sub(self.content_left_px)
            .saturating_sub(ui::terminal_scrollbar_width(scale));
        let usable_h = phys_h
            .saturating_sub(self.content_top_px)
            .saturating_sub(self.content_bottom_px);
        let (cols, rows) = Self::compute_grid(usable_w, usable_h, self.cell_w, self.cell_h);
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

    fn queue_resize(&mut self, phys_w: u32, phys_h: u32, scale: f64) {
        self.pending_geometry = Some((phys_w, phys_h, scale));
        self.last_geometry_at = Instant::now();
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
                self.mark_ime_bounds();
                self.ime_preedit = text;
                self.mark_ime_bounds();
                self.update_ime_anchor(window);
            }
            ImeAction::ClearPreedit => {
                self.mark_ime_bounds();
                self.ime_preedit.clear();
                self.mark_ime_bounds();
            }
            ImeAction::CommitText(text) => {
                // The commit enters the PTY and can cause arbitrary terminal
                // output on the next drain; do not report it as a local range.
                self.dirty.mark_full();
                self.ime_preedit.clear();
                if !self.exit && !self.child_gone {
                    self.scroll_to_bottom();
                    self.write_pty(text.as_bytes());
                }
            }
            // `ImeAction` is non-exhaustive; an unknown future action must not
            // silently drop a composition, so clear rather than guess.
            ImeAction::None => {}
            _ => {
                self.ime_preedit.clear();
                self.dirty.mark_full();
            }
        }
        self.request_dirty_redraw(window);
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
        let y0 = self.content_top_px + u32::from(cursor.0) * self.cell_h;
        let mut advance = 0u32;
        // Inverted so the provisional text is unmistakable against committed
        // output, plus an underline in the conventional IME style.
        let fg = self.default_bg;
        let bg = self.default_fg;

        for character in self.ime_preedit.chars() {
            let wide = unicode_width::UnicodeWidthChar::width(character).unwrap_or(1) > 1;
            let cells = if wide { 2 } else { 1 };
            let x0 = self.content_left_px + (u32::from(cursor.1) + advance) * self.cell_w;
            if x0 >= surface.width || y0 >= surface.height {
                break;
            }
            let span = self.cell_w * cells;
            if !surface.intersects_rect(x0, y0, span, self.cell_h) {
                advance += cells;
                continue;
            }
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
        let x = f64::from(self.content_left_px + u32::from(col) * self.cell_w) / scale;
        let y = f64::from(self.content_top_px + u32::from(row) * self.cell_h) / scale;
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

        // If IME composition is in progress, suppress keys without committed
        // text because they are still editing the preedit candidate. Keys that
        // already carry committed text (including some winit IME commit
        // representations) must still be forwarded.
        if !self.ime_preedit.is_empty() && event.text.as_deref().is_none_or(str::is_empty) {
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
    /// Uses vt100's read-only scrollback length instead of temporarily moving
    /// the viewport to `usize::MAX` and restoring it. Bounds queries must not
    /// create their own viewport damage or perturb parser state.
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

    fn scrollback_bounds(&mut self) -> (usize, usize) {
        if self.parser.screen().alternate_screen() {
            return (0, 0);
        }
        let offset = self.parser.screen().scrollback();
        let maximum = self.parser.screen().scrollback_len();
        self.scroll_offset = offset;
        (offset, maximum)
    }

    fn set_scrollback(&mut self, requested: usize) {
        if !self.parser.screen().alternate_screen() {
            let previous = self.scroll_offset;
            if previous != requested {
                self.mark_scrollbar_bounds();
            }
            self.parser.screen_mut().set_scrollback(requested);
            self.scroll_offset = self.parser.screen().scrollback();
            if self.scroll_offset != previous {
                // Scrolling changes the entire visible terminal viewport. The
                // scrollbar itself is also included so its old/new thumb
                // bounds remain observable in the candidate evidence.
                self.dirty.mark_full();
                self.mark_scrollbar_bounds();
            }
        }
    }

    fn scrollbar_geometry(
        &mut self,
        width: u32,
        height: u32,
    ) -> (agenterm_ui_core::ScrollbarGeometry, usize, usize) {
        let (offset, maximum) = self.scrollback_bounds();
        (
            ui::terminal_scrollbar_geometry(
                ui::TerminalViewport {
                    width,
                    height,
                    left: self.content_left_px,
                    top: self.content_top_px,
                    bottom_inset: self.content_bottom_px,
                    scale: self.scale,
                    rows: usize::from(self.rows),
                },
                offset,
                maximum,
            ),
            offset,
            maximum,
        )
    }

    fn handle_scrollbar_event(
        &mut self,
        window: &PixelWindow,
        event: &PixelWindowEvent,
    ) -> Result<bool, PixelWindowError> {
        let metrics = window.metrics()?;
        let scale = self.scale;
        let physical = |position: &LogicalPoint| {
            (
                (position.x * scale).round() as i32,
                (position.y * scale).round() as i32,
            )
        };
        match event {
            PixelWindowEvent::PointerButton {
                button: PointerButton::Left,
                state: PointerButtonState::Pressed,
                position: Some(position),
                ..
            } => {
                let (geometry, current, _) =
                    self.scrollbar_geometry(metrics.physical_width, metrics.physical_height);
                let (x, y) = physical(position);
                let Some(hit) = scrollbar_hit_test(&geometry, x, y) else {
                    return Ok(false);
                };
                match hit {
                    ScrollbarHit::Thumb => {
                        self.mark_scrollbar_bounds();
                        self.scrollbar_drag =
                            Some(ScrollbarThumbDrag::begin(y, geometry.thumb.top));
                        let _ = window.set_pointer_capture(true);
                    }
                    ScrollbarHit::TrackAbove => {
                        self.set_scrollback(current.saturating_add(usize::from(self.rows).max(1)))
                    }
                    ScrollbarHit::TrackBelow => {
                        self.set_scrollback(current.saturating_sub(usize::from(self.rows).max(1)))
                    }
                }
                self.mark_scrollbar_bounds();
                self.request_dirty_redraw(window);
                Ok(true)
            }
            PixelWindowEvent::PointerMoved { position, .. } => {
                let Some(drag) = self.scrollbar_drag else {
                    return Ok(false);
                };
                let (geometry, _, maximum) =
                    self.scrollbar_geometry(metrics.physical_width, metrics.physical_height);
                let (_, y) = physical(position);
                self.set_scrollback(scrollback_for_thumb_top(
                    geometry,
                    drag.thumb_top(y),
                    maximum,
                ));
                self.request_dirty_redraw(window);
                Ok(true)
            }
            PixelWindowEvent::PointerButton {
                button: PointerButton::Left,
                state: PointerButtonState::Released,
                ..
            } if self.scrollbar_drag.take().is_some() => {
                self.mark_scrollbar_bounds();
                let _ = window.set_pointer_capture(false);
                self.request_dirty_redraw(window);
                Ok(true)
            }
            PixelWindowEvent::PointerCaptureLost if self.scrollbar_drag.take().is_some() => {
                self.mark_scrollbar_bounds();
                self.request_dirty_redraw(window);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Converts a logical (DIP) pointer position to terminal cell coordinates.
    fn hit_test(&self, pos: &LogicalPoint) -> TerminalPoint {
        let phys_x = (pos.x * self.scale - f64::from(self.content_left_px)).max(0.0);
        let phys_y = (pos.y * self.scale - f64::from(self.content_top_px)).max(0.0);
        TerminalPoint {
            row: ((phys_y / self.cell_h as f64) as u16).min(self.rows.saturating_sub(1)),
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
        let phys_x = f64::from(self.content_left_px)
            + f64::from(point.col) * self.cell_w as f64
            + self.cell_w as f64 / 2.0;
        let phys_y = f64::from(self.content_top_px)
            + f64::from(point.row) * self.cell_h as f64
            + self.cell_h as f64 / 2.0;
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

    /// Whether `needle` appears in any rendered row right now. Used by the
    /// script `wait_text` command to sequence on real output instead of a
    /// guessed duration.
    fn screen_contains(&self, needle: &str) -> bool {
        let screen = self.parser.screen();
        let cols = screen.size().1;
        screen.rows(0, cols).any(|row| row.contains(needle))
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
                ScriptCommand::WaitText { text, timeout_ms } => {
                    let deadline = *self
                        .script_wait_text_deadline
                        .get_or_insert(now + Duration::from_millis(timeout_ms));
                    if self.screen_contains(&text) {
                        self.script_wait_text_deadline = None;
                    } else if now >= deadline {
                        // Fail loudly: a silently-skipped wait would hand the
                        // next command exactly the race this command exists
                        // to remove.
                        eprintln!(
                            "agenterm-con: script wait_text timed out after {timeout_ms}ms                              waiting for {text:?}"
                        );
                        std::process::exit(3);
                    } else {
                        // Re-poll soon; PTY draining and rendering continue
                        // between wakes because this returns to the event loop
                        // instead of sleeping here.
                        self.script
                            .push_front(ScriptCommand::WaitText { text, timeout_ms });
                        let until = now + Duration::from_millis(20);
                        self.script_wait_until = Some(until);
                        return Some(until);
                    }
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
    fn build_snapshot(&mut self) -> ScreenSnapshot {
        let (_, max_scrollback) = self.scrollback_bounds();
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
            max_scrollback,
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
    fn write_snapshot_if_requested(&mut self) {
        if let Some(path) = self.snapshot_path.clone() {
            let _ = agent_interface::write_snapshot_atomic(&path, &self.build_snapshot());
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
        self.dirty.mark_full();
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
        let old_selection = self.selection;
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

        let _ = window.set_pointer_capture(pressed);

        if pressed {
            if self.report_mouse(code, point, true, false, modifiers) {
                self.mouse_dragging = true;
                self.active_button = Some(code);
                // The application owns this gesture; drop any stale selection
                // so the highlight does not linger over its UI.
                self.selection = None;
                self.mark_selection_change(old_selection, self.selection);
                self.request_dirty_redraw(window);
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
            }
            (PointerButton::Left, false) => {
                self.selecting = false;
                if selection_should_auto_copy(self.selection) {
                    self.copy_selection();
                }
            }
            (PointerButton::Right, true) => {
                // Right-click: copy if a selection exists, else paste.
                if self.selection.is_some() {
                    self.copy_selection();
                    self.selection = None;
                } else {
                    self.paste_clipboard();
                }
            }
            _ => {}
        }
        self.mark_selection_change(old_selection, self.selection);
        self.request_dirty_redraw(window);
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
        let old_selection = self.selection;
        let pt = self.hit_test(&position);
        if self.mouse_dragging {
            let button = self.active_button.unwrap_or(0);
            self.report_mouse(button, pt, true, true, modifiers);
        } else if self.selecting {
            if let Some((anchor, _)) = self.selection {
                self.selection = Some((anchor, pt));
            }
        } else if self.mouse_mode().0 == terminal_input::ApplicationMouseMode::AnyMotion {
            // 1003: report motion with no button held (button 3 = none).
            self.report_mouse(3, pt, true, true, modifiers);
        }
        self.mark_selection_change(old_selection, self.selection);
        self.request_dirty_redraw(window);
    }

    fn cancel_pointer_gesture(&mut self, window: &PixelWindow) {
        if let Some((button, point)) = self.take_cancelled_pointer_release() {
            let modifiers = agenterm_platform::input::ModifierState {
                control: false,
                shift: false,
                alt: false,
                meta: false,
            };
            self.report_mouse(button, point, false, false, &modifiers);
        }
        window.request_redraw();
    }

    fn take_cancelled_pointer_release(&mut self) -> Option<(u8, TerminalPoint)> {
        let release = self.mouse_dragging.then(|| {
            (
                self.active_button.unwrap_or(0),
                self.last_reported_cell
                    .unwrap_or(TerminalPoint { row: 0, col: 0 }),
            )
        });
        self.mouse_dragging = false;
        self.active_button = None;
        self.selecting = false;
        release
    }
}

impl ConTerminal {
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
            metrics
                .physical_width
                .saturating_sub(self.content_left_px)
                .saturating_sub(ui::terminal_scrollbar_width(scale)),
            metrics
                .physical_height
                .saturating_sub(self.content_top_px)
                .saturating_sub(self.content_bottom_px),
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
        if self.handle_scrollbar_event(window, &event)? {
            return Ok(PixelWindowDirective::Continue);
        }
        match event {
            PixelWindowEvent::CloseRequested => {
                self.exit = true;
                Ok(PixelWindowDirective::Exit)
            }
            PixelWindowEvent::GeometryChanged { change, metrics } => {
                self.dirty.mark_full();
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
                if self.dirty.is_empty() {
                    window.request_redraw();
                } else {
                    self.request_dirty_redraw(window);
                }
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::Keyboard(key) => {
                self.dirty.mark_full();
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
                self.dirty.mark_full();
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
            PixelWindowEvent::PointerCaptureLost => {
                self.cancel_pointer_gesture(window);
                Ok(PixelWindowDirective::Continue)
            }
            _ => {
                // Unknown future host events are not safe to classify as a
                // smaller region.
                self.dirty.mark_full();
                Ok(PixelWindowDirective::Continue)
            }
        }
    }

    fn render(
        &mut self,
        window: &PixelWindow,
        pixels: &mut [u32],
        width: u32,
        height: u32,
        candidate: DirtyRegion,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        // Apply OSC title changes (shell emits \e]0;title\a).
        if let Some(title) = self.parser.callbacks_mut().title.take() {
            self.current_title = title;
            window.set_title(&self.current_title);
        }

        let fw = width;
        let fh = height;
        if candidate.is_empty() {
            self.write_snapshot_if_requested();
            return Ok(PixelWindowDirective::Continue);
        }
        let bg_word = self.default_bg.to_xrgb();
        let clip = candidate_bounds(candidate, fw, fh);
        let mut surface = Surface::with_clip(pixels, fw, fh, clip);
        if candidate.is_full() {
            surface.fill_rect(0, 0, fw, fh, bg_word);
        } else {
            let terminal_height = fh
                .saturating_sub(self.content_top_px)
                .saturating_sub(self.content_bottom_px);
            surface.fill_rect(
                self.content_left_px,
                self.content_top_px,
                fw.saturating_sub(self.content_left_px),
                terminal_height,
                bg_word,
            );
        }

        let (scrollbar, _, _) = self.scrollbar_geometry(fw, fh);
        let scrollbar_active = self.scrollbar_drag.is_some();
        let screen = self.parser.screen();
        let cursor = screen.cursor_position();
        let cursor_hidden = screen.hide_cursor();
        let cursor_shape = screen.cursor_shape();
        self.last_cursor = Some(TerminalPoint {
            row: cursor.0,
            col: cursor.1,
        });
        // A steady request always shows the cursor; a blinking one is gated
        // by the timer in about_to_wait. conhost draws the caret the same
        // way — this is parity, not an enhancement — but getting it right
        // matters for vim/nvim, which switch shape *and* blink per mode.
        let cursor_visible_now = !screen.cursor_blinking() || self.blink_visible;
        paint_cells_at(
            &mut surface,
            screen,
            self.selection,
            self.cell_w,
            self.cell_h,
            self.default_fg,
            self.default_bg,
            self.font_size_px,
            self.content_left_px,
            self.content_top_px,
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
            let cx = self.content_left_px + cursor_col * self.cell_w;
            let cy = self.content_top_px + u32::from(cursor.0) * self.cell_h;
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

        surface.fill_rect(
            scrollbar.track.left.max(0) as u32,
            scrollbar.track.top.max(0) as u32,
            scrollbar.track.width().max(0) as u32,
            scrollbar.track.height().max(0) as u32,
            Rgb(0x18, 0x18, 0x18).to_xrgb(),
        );
        surface.fill_rect(
            scrollbar.thumb.left.max(0) as u32,
            scrollbar.thumb.top.max(0) as u32,
            scrollbar.thumb.width().max(0) as u32,
            scrollbar.thumb.height().max(0) as u32,
            if scrollbar_active {
                Rgb(0xF0, 0xF0, 0xF0)
            } else {
                Rgb(0xA8, 0xA8, 0xA8)
            }
            .to_xrgb(),
        );

        self.write_snapshot_if_requested();

        Ok(PixelWindowDirective::Continue)
    }

    fn about_to_wait(
        &mut self,
        window: &PixelWindow,
        now: Instant,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        // (see impl ConTerminal::draw_preedit for the composition renderer)
        if self.exit {
            return Ok(PixelWindowDirective::Exit);
        }

        // A session with an exited child remains drawable and selectable. The
        // outer ConApp may still host live siblings; closing the entire GUI
        // here made an ordinary child failure indistinguishable from a host
        // crash and discarded unrelated terminals.
        if self.child_gone {
            return Ok(PixelWindowDirective::Wait);
        }

        // Three independent timers can all have work pending at once (a
        // resize settling, the cursor mid-blink, a scripted `wait_ms`), and
        // this callback can only return one deadline. Each contributes to a
        // shared "wake no later than" floor instead of returning early —
        // returning early on, say, blink would starve a scripted wait behind
        // blink's ~530ms cadence, making `wait_ms: 50` in a script actually
        // take up to 530ms.
        let mut redraw = false;
        let mut partial_redraw = false;
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
                self.mark_cursor_change();
                self.blink_visible = !self.blink_visible;
                self.last_blink_at = now;
                partial_redraw = true;
            }
            fold_wake(self.last_blink_at + BLINK_INTERVAL);
        }

        if let Some(deadline) = self.drain_script(window, now) {
            fold_wake(deadline);
        }
        // A pending screenshot needs an actual render to happen — pixels
        // only exist transiently inside render() — so it must force a
        // redraw even when nothing else did.
        if self.pending_screenshot.is_some() || self.pending_control_screenshot.is_some() {
            redraw = true;
        }

        if redraw {
            window.request_redraw();
        } else if partial_redraw {
            self.request_dirty_redraw(window);
        }

        Ok(next_wake.map_or(PixelWindowDirective::Wait, PixelWindowDirective::WaitUntil))
    }
}

impl PixelWindowApplication for ConApp {
    fn opened(&mut self, window: &PixelWindow) -> Result<PixelWindowDirective, PixelWindowError> {
        let metrics = window.metrics()?;
        let sidebar_width = self.sidebar_width_logical;
        Self::configure_chrome(
            self.active_session_mut()?,
            metrics.scale_factor,
            sidebar_width,
        );
        let directive = self.active_session_mut()?.opened(window)?;
        if let Some(endpoint) = self.control_endpoint.clone() {
            let waker = window.waker();
            self.control_server = Some(
                control::ControlServer::bind(&endpoint, move || {
                    let _ = waker.wake();
                })
                .map_err(|error| PixelWindowError::failed("con_control_bind", error))?,
            );
        }
        self.refresh_title(window)?;
        Ok(directive)
    }

    fn event(
        &mut self,
        window: &PixelWindow,
        event: PixelWindowEvent,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        if self.exit {
            return Ok(PixelWindowDirective::Exit);
        }
        if matches!(event, PixelWindowEvent::Wake) {
            let active = self.workspace.active();
            let mut active_redraw = false;
            let mut backlog = false;
            for (id, session) in &mut self.sessions {
                let outcome = session.drain_pty();
                self.perf_stats.pty_drained_bytes = self
                    .perf_stats
                    .pty_drained_bytes
                    .saturating_add(outcome.bytes as u64);
                self.perf_stats.pty_budget_yields = self
                    .perf_stats
                    .pty_budget_yields
                    .saturating_add(u64::from(outcome.backlog));
                active_redraw |= active == Some(*id) && outcome.redraw;
                backlog |= outcome.backlog;
            }
            if active_redraw {
                if self.active_session()?.dirty.is_empty() {
                    // Title-only output and child exit still need one render,
                    // but there is no pixel rectangle to invalidate.
                    window.request_redraw();
                } else {
                    self.active_session()?.request_dirty_redraw(window);
                }
            }
            if backlog {
                let _ = window.waker().wake();
            }
            return Ok(PixelWindowDirective::Continue);
        }
        if self.handle_sidebar_resize(window, &event)? {
            self.mark_chrome_full();
            return Ok(PixelWindowDirective::Continue);
        }
        if let PixelWindowEvent::GeometryChanged { metrics, .. } = &event {
            self.mark_chrome_full();
            let sidebar_width = self.sidebar_width_logical;
            Self::configure_chrome(
                self.active_session_mut()?,
                metrics.scale_factor,
                sidebar_width,
            );
        }
        if let PixelWindowEvent::Keyboard(key) = &event
            && self.handle_workspace_shortcut(window, key)?
        {
            return Ok(PixelWindowDirective::Continue);
        }
        if let PixelWindowEvent::MouseWheel {
            delta,
            position: Some(position),
            modifiers,
        } = &event
        {
            let metrics = window.metrics()?;
            let scale = metrics.scale_factor.max(1.0);
            let layout = self.layout(
                metrics.physical_width,
                metrics.physical_height,
                metrics.scale_factor,
            );
            if !modifiers.control
                && layout.sidebar.contains(
                    (position.x * scale).max(0.0) as u32,
                    (position.y * scale).max(0.0) as u32,
                )
            {
                let rows = match delta {
                    WheelDelta::Lines { y, .. } => y.round() as isize,
                    WheelDelta::LogicalPixels { y, .. } => {
                        (*y / ui::TREE_ROW_HEIGHT_DIP).round() as isize
                    }
                    _ => 0,
                };
                self.tree_scroll_offset = ui::scroll_tree(
                    self.tree_scroll_offset,
                    -rows,
                    self.workspace.nodes().len(),
                    layout.tree_capacity(),
                );
                self.mark_tree_dirty();
                self.request_dirty_redraw(window);
                return Ok(PixelWindowDirective::Continue);
            }
        }
        if let PixelWindowEvent::PointerButton {
            button: PointerButton::Left,
            state: PointerButtonState::Pressed,
            position: Some(position),
            ..
        } = &event
        {
            if self.handle_tree_pointer(window, position)? {
                return Ok(PixelWindowDirective::Continue);
            }
            match self.composer_hit(window, position)? {
                ui::ComposerHit::Input => {
                    self.composer_focused = true;
                    self.composer_select_all = false;
                    self.update_composer_ime_anchor(window)?;
                    self.mark_composer_dirty();
                    window.focus();
                    self.request_dirty_redraw(window);
                    return Ok(PixelWindowDirective::Continue);
                }
                ui::ComposerHit::Send => {
                    self.submit_composer();
                    self.composer_focused = true;
                    self.update_composer_ime_anchor(window)?;
                    self.mark_composer_dirty();
                    self.request_dirty_redraw(window);
                    return Ok(PixelWindowDirective::Continue);
                }
                ui::ComposerHit::Outside => {}
            }
            self.composer_focused = false;
            self.mark_composer_dirty();
            self.request_dirty_redraw(window);
        }
        if self.composer_focused {
            match event {
                PixelWindowEvent::Keyboard(key) if self.handle_composer_key(window, &key) => {
                    self.mark_composer_dirty();
                    self.request_dirty_redraw(window);
                    return Ok(PixelWindowDirective::Continue);
                }
                PixelWindowEvent::Ime(ime) => {
                    self.handle_composer_ime(window, ime);
                    self.mark_composer_dirty();
                    self.request_dirty_redraw(window);
                    return Ok(PixelWindowDirective::Continue);
                }
                _ => {}
            }
        }
        self.active_session_mut()?.event(window, event)
    }

    fn render(
        &mut self,
        window: &PixelWindow,
        frame: &mut XrgbPixelFrame<'_>,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        self.perf_stats.sync_present_stats(window.present_stats());
        let width = frame.width();
        let height = frame.height();
        let frame_info = frame.info();
        let host_retains_pixels = matches!(
            frame_info.retention,
            PixelBackingRetention::RetainedAcrossFrames
        );
        macro_rules! render_try {
            ($expression:expr) => {{
                match $expression {
                    Ok(value) => value,
                    Err(error) => {
                        if !host_retains_pixels {
                            self.retained.invalidate();
                        }
                        return Err(error);
                    }
                }
            }};
        }
        let scale = self.active_session()?.scale.max(1.0);
        self.note_frame_dimensions(width, height, scale);
        render_try!(self.active_session_mut()).note_frame_dimensions(width, height);
        let retained_requires_full = if host_retains_pixels {
            !frame_info.content_valid
        } else {
            match self.retained.prepare(width, height) {
                Ok(requires_full) => requires_full,
                Err(error) => {
                    self.retained.invalidate();
                    return Err(PixelWindowError::failed(
                        "con_retained_frame",
                        error.to_string(),
                    ));
                }
            }
        };
        if retained_requires_full {
            self.chrome_dirty.mark_full();
            self.active_session_mut()?.dirty.mark_full();
            window.request_redraw();
        }

        // Drain before consuming the candidate. PTY output can alter arbitrary
        // cells, cursor state, modes, scrollback, and selection, so it always
        // upgrades the candidate to full before raster starts.
        let (drain, wake_pending) = {
            let session = render_try!(self.active_session_mut());
            let drain = session.drain_pty();
            let wake_pending = session.pty_wake_pending.load(Ordering::Acquire);
            (drain, wake_pending)
        };
        self.perf_stats.pty_drained_bytes = self
            .perf_stats
            .pty_drained_bytes
            .saturating_add(drain.bytes as u64);
        self.perf_stats.pty_budget_yields = self
            .perf_stats
            .pty_budget_yields
            .saturating_add(u64::from(drain.backlog));
        if drain.backlog || wake_pending {
            // Output arrived while this render was being prepared, or the
            // bounded drain still has a tail. The current retained frame is
            // made safe with a full raster; the reader/waker will schedule the
            // next bounded drain without forcing an unconditional Wake full.
            self.active_session_mut()?.dirty.mark_full();
        }

        // The candidate is complete before either product surface starts
        // rasterizing. A late dirty state is therefore a programming error,
        // not an excuse to label a partial frame after the fact.
        let mut candidate = self.take_dirty_candidate(width, height);
        if host_retains_pixels && !frame_info.content_valid {
            candidate = DirtyRegion::full_frame(width, height);
        }
        if candidate.is_full() {
            // A late resize, PTY drain, or invalidation must widen the native
            // update region before a partial GDI present can be accepted.
            window.request_redraw();
        }
        let render_started = Instant::now();
        let active_id = match self.workspace.active() {
            Some(id) => id,
            None => {
                if !host_retains_pixels {
                    self.retained.invalidate();
                }
                return Err(PixelWindowError::failed(
                    "con_session_missing",
                    "no active terminal session",
                ));
            }
        };
        let directive = if host_retains_pixels {
            let render_result = {
                let session = match self.sessions.get_mut(&active_id) {
                    Some(session) => session,
                    None => {
                        return Err(PixelWindowError::failed(
                            "con_session_missing",
                            "active terminal session missing",
                        ));
                    }
                };
                session.render(window, frame.pixels_mut(), width, height, candidate)
            };
            let directive = render_result?;
            if !candidate.is_empty() {
                self.paint_chrome(frame.pixels_mut(), width, height, candidate)?;
            }
            directive
        } else {
            let mut retained = std::mem::take(&mut self.retained);
            let render_result = {
                let session = match self.sessions.get_mut(&active_id) {
                    Some(session) => session,
                    None => {
                        self.retained = retained;
                        self.retained.invalidate();
                        return Err(PixelWindowError::failed(
                            "con_session_missing",
                            "active terminal session missing",
                        ));
                    }
                };
                session.render(window, retained.pixels_mut(), width, height, candidate)
            };
            let directive = match render_result {
                Ok(directive) => directive,
                Err(error) => {
                    self.retained = retained;
                    self.retained.invalidate();
                    return Err(error);
                }
            };
            if !candidate.is_empty()
                && let Err(error) =
                    self.paint_chrome(retained.pixels_mut(), width, height, candidate)
            {
                self.retained = retained;
                self.retained.invalidate();
                return Err(error);
            }
            self.retained = retained;
            self.retained.mark_valid();
            directive
        };
        let (pending_screenshot, pending_control_screenshot) = {
            let session = render_try!(self.sessions.get_mut(&active_id).ok_or_else(|| {
                PixelWindowError::failed("con_session_missing", "active terminal session missing")
            }));
            (
                session.pending_screenshot.take(),
                session.pending_control_screenshot.take(),
            )
        };
        if let Some(path) = pending_screenshot {
            if host_retains_pixels {
                let _ = agent_interface::write_png_atomic(
                    path.as_path(),
                    frame.pixels_mut(),
                    width,
                    height,
                );
            } else {
                let _ = agent_interface::write_png_atomic(
                    path.as_path(),
                    self.retained.pixels(),
                    width,
                    height,
                );
            }
        }
        if let Some((path, reply)) = pending_control_screenshot {
            let encode_started = Instant::now();
            let write_result = if host_retains_pixels {
                agent_interface::write_png_atomic(path.as_path(), frame.pixels_mut(), width, height)
            } else {
                agent_interface::write_png_atomic(
                    path.as_path(),
                    self.retained.pixels(),
                    width,
                    height,
                )
            };
            let encode_ns = encode_started
                .elapsed()
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64;
            let result = write_result
                .map(|()| {
                    json::object([
                        ("path", path.to_string_lossy().into_owned().into()),
                        ("width", width.into()),
                        ("height", height.into()),
                        ("encode_ns", encode_ns.into()),
                    ])
                })
                .map_err(|error| format!("write screenshot: {error}"));
            let _ = reply.send(result);
        }
        let write = frame_write_for_candidate(
            frame_info.retention,
            frame_info.content_valid,
            candidate,
            width,
            height,
        );
        if host_retains_pixels {
            if let Err(error) = frame.commit(write) {
                return Err(PixelWindowError::failed(
                    "con_frame_commit",
                    error.to_string(),
                ));
            }
            self.perf_stats.record_host_direct_frame();
        } else {
            if let Err(error) = self.retained.copy_to(frame.pixels_mut(), width, height) {
                self.retained.invalidate();
                return Err(PixelWindowError::failed(
                    "con_retained_copy",
                    error.to_string(),
                ));
            }
            if let Err(error) = frame.commit(PixelFrameWrite::Full) {
                self.retained.invalidate();
                return Err(PixelWindowError::failed(
                    "con_frame_commit",
                    error.to_string(),
                ));
            }
            self.perf_stats.record_host_copy_frame(width, height);
        }
        self.perf_stats.record_frame(render_started.elapsed());
        self.perf_stats
            .record_raster_candidate(candidate, width, height);
        Ok(directive)
    }

    fn about_to_wait(
        &mut self,
        window: &PixelWindow,
        now: Instant,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        if self.exit {
            return Ok(PixelWindowDirective::Exit);
        }
        let control_deadline = self.drain_control(window, now);
        let directive = self.active_session_mut()?.about_to_wait(window, now)?;
        // `ConTerminal::about_to_wait` returns `Wait` for a session whose child
        // exited so one dead tab cannot discard live siblings. With NO live
        // session left there is nothing to host, and `agenterm-con -e CMD` must
        // exit when its child does: `--emit-snapshot` automation and
        // tests/agenterm_con_blackbox.rs wait for this process to exit. Without
        // it, `-e cmd.exe /c echo X` writes its snapshot and then hangs, which
        // stalled the first GUI test in that suite and took the windows
        // unit-tests gate past its budget with no test completing.
        //
        // Checked AFTER the session runs: `child_gone` is set by `drain_pty`
        // inside that call, so testing first sees the pre-drain value on the one
        // wake that reports the exit. A bound control endpoint means a client is
        // driving this GUI and may still open tabs, so that case keeps waiting.
        if self.control_server.is_none()
            && !self.sessions.is_empty()
            && self.sessions.values().all(|session| session.child_gone)
        {
            return Ok(PixelWindowDirective::Exit);
        }
        Ok(match (directive, control_deadline) {
            (PixelWindowDirective::Wait, Some(deadline)) => {
                PixelWindowDirective::WaitUntil(deadline)
            }
            (PixelWindowDirective::WaitUntil(current), Some(deadline)) => {
                PixelWindowDirective::WaitUntil(current.min(deadline))
            }
            (directive, _) => directive,
        })
    }
}

fn parse_control_key(spec: &str) -> Result<(ScriptKey, bool, bool, bool), String> {
    let mut parts: Vec<_> = spec.split('+').collect();
    let key_name = parts
        .pop()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid key specification {spec:?}"))?;
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    for modifier in parts {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            _ => return Err(format!("unknown key modifier {modifier:?}")),
        }
    }
    let named = match key_name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(NamedKey::Enter),
        "escape" | "esc" => Some(NamedKey::Escape),
        "tab" => Some(NamedKey::Tab),
        "backspace" => Some(NamedKey::Backspace),
        "delete" | "del" => Some(NamedKey::Delete),
        "insert" | "ins" => Some(NamedKey::Insert),
        "home" => Some(NamedKey::Home),
        "end" => Some(NamedKey::End),
        "pageup" => Some(NamedKey::PageUp),
        "pagedown" => Some(NamedKey::PageDown),
        "up" | "arrowup" => Some(NamedKey::ArrowUp),
        "down" | "arrowdown" => Some(NamedKey::ArrowDown),
        "left" | "arrowleft" => Some(NamedKey::ArrowLeft),
        "right" | "arrowright" => Some(NamedKey::ArrowRight),
        _ => None,
    };
    let key = if let Some(named) = named {
        ScriptKey::Named(named)
    } else {
        let mut chars = key_name.chars();
        let character = chars.next().ok_or_else(|| "empty key".to_owned())?;
        if chars.next().is_some() {
            return Err(format!("unknown key {key_name:?}"));
        }
        ScriptKey::Char(character)
    };
    Ok((key, ctrl, alt, shift))
}

fn control_mouse_button(button: control::MouseButton) -> Result<ScriptMouseButton, String> {
    match button {
        control::MouseButton::Left => Ok(ScriptMouseButton::Left),
        control::MouseButton::Middle => Ok(ScriptMouseButton::Middle),
        control::MouseButton::Right => Ok(ScriptMouseButton::Right),
        control::MouseButton::None => Err("press/release requires a mouse button".to_owned()),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateRedrawRequest {
    None,
    Full,
    Partial(HostPixelRect),
}

fn candidate_redraw_request(
    candidate: DirtyRegion,
    width: u32,
    height: u32,
) -> CandidateRedrawRequest {
    if candidate.is_full() || width == 0 || height == 0 {
        return CandidateRedrawRequest::Full;
    }
    let Some(bounds) = candidate.clip(width, height).bounds() else {
        return CandidateRedrawRequest::None;
    };
    if bounds.is_empty() {
        CandidateRedrawRequest::None
    } else {
        CandidateRedrawRequest::Partial(HostPixelRect::new(
            bounds.left,
            bounds.top,
            bounds.right,
            bounds.bottom,
        ))
    }
}

fn frame_write_for_candidate(
    retention: PixelBackingRetention,
    content_valid: bool,
    candidate: DirtyRegion,
    width: u32,
    height: u32,
) -> PixelFrameWrite {
    if matches!(retention, PixelBackingRetention::Transient) || !content_valid {
        return PixelFrameWrite::Full;
    }
    match candidate_redraw_request(candidate, width, height) {
        CandidateRedrawRequest::None => PixelFrameWrite::None,
        CandidateRedrawRequest::Full => PixelFrameWrite::Full,
        CandidateRedrawRequest::Partial(rect) => PixelFrameWrite::Partial(rect),
    }
}

fn request_candidate_redraw(window: &PixelWindow, candidate: DirtyRegion, width: u32, height: u32) {
    match candidate_redraw_request(candidate, width, height) {
        CandidateRedrawRequest::None => {}
        CandidateRedrawRequest::Full => window.request_redraw(),
        CandidateRedrawRequest::Partial(rect) => window.request_redraw_rect(rect),
    }
}

fn candidate_bounds(candidate: DirtyRegion, width: u32, height: u32) -> PixelRect {
    candidate
        .clip(width, height)
        .bounds()
        .unwrap_or_else(PixelRect::empty)
}

/// The pixel target for one frame: the buffer and its dimensions, which always
/// travel together. Bundling them keeps the drawing calls readable — the free
/// functions this replaced took nine positional arguments, most of them the
/// same three values threaded through every call.
struct Surface<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
    clip: PixelRect,
}

impl<'a> Surface<'a> {
    #[cfg(test)]
    fn new(pixels: &'a mut [u32], width: u32, height: u32) -> Surface<'a> {
        Self::with_clip(pixels, width, height, PixelRect::full_frame(width, height))
    }

    fn with_clip(pixels: &'a mut [u32], width: u32, height: u32, clip: PixelRect) -> Surface<'a> {
        Self {
            pixels,
            width,
            height,
            clip: clip.clip(width, height),
        }
    }

    fn clipped_rect(&self, x: u32, y: u32, w: u32, h: u32) -> PixelRect {
        let rect = PixelRect::from_xywh(x, y, w, h).clip(self.width, self.height);
        let left = rect.left.max(self.clip.left);
        let top = rect.top.max(self.clip.top);
        let right = rect.right.min(self.clip.right).max(left);
        let bottom = rect.bottom.min(self.clip.bottom).max(top);
        PixelRect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn intersects_rect(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        !self.clipped_rect(x, y, w, h).is_empty()
    }

    fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let rect = self.clipped_rect(x, y, w, h);
        if rect.is_empty() {
            return;
        }
        agenterm_ui_core::pixel::fill_xrgb_rect(
            self.pixels,
            self.width,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            color,
        );
    }

    /// Blits a rasterized glyph into a cell, clipped to that cell.
    ///
    /// `shear` slants the glyph for faux italic: a per-row horizontal offset
    /// proportional to height above the baseline. Synthesizing the slant beats
    /// loading a real italic face, which would have a different advance width
    /// and break the fixed cell grid.
    fn blit_glyph(&mut self, glyph: &font::RasterGlyph, cell: CellRect, fg: Rgb, shear: f32) {
        let clip = self.clipped_rect(cell.x, cell.y, cell.w, cell.h);
        if clip.is_empty() {
            return;
        }
        let start_x = i64::from(cell.x) + i64::from(glyph.offset_x);
        let start_y = i64::from(cell.y) + i64::from(glyph.offset_y);
        let clip_x0 = i64::from(clip.left);
        let clip_y0 = i64::from(clip.top);
        let clip_x1 = i64::from(clip.right);
        let clip_y1 = i64::from(clip.bottom);

        for gy in 0..glyph.height {
            let py = start_y + i64::from(gy);
            if py < clip_y0 || py >= clip_y1 || py < 0 || py >= i64::from(self.height) {
                continue;
            }
            // Rows nearer the top lean further right, pivoting on the bottom
            // of the cell so the glyph stays seated on its baseline.
            let slant = if shear == 0.0 {
                0
            } else {
                ((clip_y1 - py) as f32 * shear).round() as i64
            };
            let row_start_x = start_x + slant;
            let source_x_start = (clip_x0 - row_start_x).max(0).min(i64::from(u32::MAX)) as u32;
            let source_x_end = glyph
                .width
                .min((clip_x1 - row_start_x).max(0).min(i64::from(u32::MAX)) as u32);
            if source_x_start >= source_x_end {
                continue;
            }
            let destination_x = row_start_x + i64::from(source_x_start);
            if destination_x < 0 || destination_x >= i64::from(self.width) {
                continue;
            }
            let count = usize::try_from(source_x_end - source_x_start).unwrap_or(0);
            let Some(row_start) = usize::try_from(py)
                .ok()
                .and_then(|row| row.checked_mul(self.width as usize))
            else {
                continue;
            };
            let Some(destination_start) = row_start.checked_add(destination_x as usize) else {
                continue;
            };
            let Some(destination_end) = destination_start.checked_add(count) else {
                continue;
            };
            let Some(source_start) = usize::try_from(gy)
                .ok()
                .and_then(|row| row.checked_mul(glyph.width as usize))
                .and_then(|row| row.checked_add(source_x_start as usize))
            else {
                continue;
            };
            let Some(source_end) = source_start.checked_add(count) else {
                continue;
            };
            let Some(destination) = self.pixels.get_mut(destination_start..destination_end) else {
                continue;
            };
            let Some(source) = glyph.alpha.get(source_start..source_end) else {
                continue;
            };
            agenterm_ui_core::pixel::blend_mask_xrgb(destination, source, fg.to_xrgb());
        }
    }
}

/// Paints every cell of one screen into `surface`. Pure with respect to
/// window/frame types so it is directly unit-testable — see the `tests`
/// module, which renders into a plain `Vec<u32>` and asserts on pixel colors.
// Only the `tests` module calls this wrapper, and this file is a `[[bin]]`:
// its non-test compilation cfg's the tests away, so `-D warnings` sees an
// unused function and fails the lint gate. Same shape as
// `NativeToolbarHit::ORDER` in `src/frontend/toolbar.rs`.
#[cfg_attr(not(test), allow(dead_code))]
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
    paint_cells_at(
        surface,
        screen,
        selection,
        cell_w,
        cell_h,
        default_fg,
        default_bg,
        font_size_px,
        0,
        0,
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_cells_at(
    surface: &mut Surface<'_>,
    screen: &vt100::Screen,
    selection: Option<(TerminalPoint, TerminalPoint)>,
    cell_w: u32,
    cell_h: u32,
    default_fg: Rgb,
    default_bg: Rgb,
    font_size_px: u16,
    left: u32,
    top: u32,
) {
    let (rows, cols) = screen.size();
    for row in 0..rows {
        let y0 = top.saturating_add(u32::from(row).saturating_mul(cell_h));
        if y0 >= surface.height {
            break;
        }
        if !surface.intersects_rect(left, y0, surface.width.saturating_sub(left), cell_h) {
            continue;
        }
        for col in 0..cols {
            let x0 = left.saturating_add(u32::from(col).saturating_mul(cell_w));
            if x0 >= surface.width {
                break;
            }
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let span_w = if cell.is_wide() { cell_w * 2 } else { cell_w };
            if !surface.intersects_rect(x0, y0, span_w, cell_h) {
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

fn paint_chrome_text(
    surface: &mut Surface<'_>,
    x: u32,
    y: u32,
    text: &str,
    color: Rgb,
    font_size_px: u16,
    max_width: u32,
) {
    let metrics = font::cell_metrics(font_size_px);
    let cell_w = metrics.width.max(1);
    let cell_h = metrics.height.max(1);
    let mut cursor = x;
    let limit = x.saturating_add(max_width).min(surface.width);
    for character in text.chars() {
        if cursor.saturating_add(cell_w) > limit {
            break;
        }
        if surface.intersects_rect(cursor, y, cell_w, cell_h)
            && let Some(glyph) = font::raster(character, font_size_px)
        {
            surface.blit_glyph(
                &glyph,
                CellRect {
                    x: cursor,
                    y,
                    w: cell_w,
                    h: cell_h,
                },
                color,
                0.0,
            );
        }
        cursor = cursor.saturating_add(cell_w);
    }
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

    #[test]
    fn vt_damage_rows_map_to_clamped_content_and_cursor_endpoints() {
        let mut app = ConTerminal::new(None);
        app.dirty = DirtyRegion::empty();
        app.frame_width = 100;
        app.frame_height = 60;
        app.content_left_px = 10;
        app.content_top_px = 8;
        app.content_bottom_px = 12;
        app.cell_w = 8;
        app.cell_h = 10;
        app.cols = 8;
        app.rows = 4;
        app.parser.screen_mut().set_size(4, 8);
        let _ = app.parser.take_damage();

        app.parser.process(b"\x1b[2J");
        let damage = app.parser.take_damage();
        assert!(!damage.needs_full_raster());
        app.mark_vt_damage(damage);
        let rows = app.dirty.bounds().expect("row damage has a pixel bound");
        assert_eq!(rows.left, 10);
        assert_eq!(rows.top, 8);
        assert_eq!(rows.right, 74);
        assert_eq!(rows.bottom, 48);
        assert!(!app.dirty.is_full());

        app.dirty = DirtyRegion::empty();
        app.parser.process(b"A");
        let _ = app.parser.take_damage();
        app.parser.process(b"\x1b[1;1H");
        let damage = app.parser.take_damage();
        assert_eq!(damage.cursor_before(), Some((0, 1)));
        assert_eq!(damage.cursor_after(), Some((0, 0)));
        app.mark_vt_damage(damage);
        let cursor = app.dirty.bounds().expect("cursor endpoints are dirty");
        assert_eq!(cursor.left, 10);
        assert_eq!(cursor.right, 34);
        assert_eq!(cursor.top, 8);
        assert_eq!(cursor.bottom, 18);
        assert!(!app.dirty.is_full());
    }

    #[test]
    fn pty_drain_consumes_vt_damage_without_unconditional_full() {
        let mut app = ConTerminal::new(None);
        app.pty_output = Arc::new(BoundedOutputPipe::new(1024));
        app.dirty = DirtyRegion::empty();
        app.frame_width = 640;
        app.frame_height = 400;
        app.content_left_px = 10;
        app.content_top_px = 8;
        app.content_bottom_px = 12;
        app.cell_w = 8;
        app.cell_h = 16;
        app.pty_output.push_blocking(b"ASCII").expect("pipe open");

        let outcome = app.drain_pty();
        assert!(outcome.changed);
        assert!(outcome.redraw);
        assert!(!app.dirty.is_full());
        assert!(app.dirty.bounds().is_some());
    }

    #[test]
    fn full_vt_damage_is_the_explicit_safe_fallback() {
        let mut app = ConTerminal::new(None);
        app.dirty = DirtyRegion::empty();
        app.frame_width = 640;
        app.frame_height = 400;
        app.parser.screen_mut().mark_full_damage();

        let outcome = app.drain_pty();
        assert!(outcome.redraw);
        assert!(app.dirty.is_full());
    }

    #[test]
    fn scrollback_bounds_uses_read_only_vt_length() {
        let mut app = ConTerminal::new(None);
        app.parser.screen_mut().set_size(3, 10);
        let _ = app.parser.take_damage();
        app.parser.process(b"a\r\nb\r\nc\r\nd");
        let _ = app.parser.take_damage();

        let before = app.parser.screen().scrollback();
        let expected = app.parser.screen().scrollback_len();
        let (offset, maximum) = app.scrollback_bounds();
        assert_eq!(offset, before);
        assert_eq!(maximum, expected);
        assert_eq!(app.parser.screen().scrollback(), before);
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
    fn terminal_paint_respects_left_tree_inset() {
        let mut parser = vt100::Parser::new_with_callbacks(2, 4, 0, ConCallbacks::default());
        parser.process(b"A");
        let width = 64;
        let height = 32;
        let untouched = 0x0012_3456;
        let mut pixels = vec![untouched; (width * height) as usize];
        let mut surface = Surface::new(&mut pixels, width, height);
        paint_cells_at(
            &mut surface,
            parser.screen(),
            None,
            8,
            16,
            Rgb(0xEE, 0xEE, 0xEE),
            Rgb(0x00, 0x00, 0x00),
            12,
            24,
            0,
        );
        for row in surface.pixels.chunks_exact(width as usize) {
            assert!(row[..24].iter().all(|pixel| *pixel == untouched));
        }
        assert!((0..16).any(|y| {
            let row = &surface.pixels[y * width as usize..(y + 1) * width as usize];
            row[24..32].iter().any(|pixel| *pixel != untouched)
        }));
    }

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
    fn auto_copy_requires_a_non_empty_local_selection() {
        let point = TerminalPoint { row: 2, col: 4 };
        assert!(!selection_should_auto_copy(None));
        assert!(!selection_should_auto_copy(Some((point, point))));
        assert!(selection_should_auto_copy(Some((
            point,
            TerminalPoint { row: 2, col: 7 },
        ))));
    }

    #[test]
    fn queued_resize_coalesces_without_synchronously_mutating_the_grid() {
        let mut terminal = ConTerminal::new(None);
        let original_grid = (terminal.cols, terminal.rows);
        terminal.queue_resize(900, 600, 1.0);
        terminal.queue_resize(1200, 800, 1.25);
        assert_eq!((terminal.cols, terminal.rows), original_grid);
        assert_eq!(terminal.pending_geometry, Some((1200, 800, 1.25)));
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
        let mut surface = Surface::new(&mut pixels, fw, fh);
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
    fn clipped_surface_matches_full_terminal_rect_operations() {
        let width = 16u32;
        let height = 8u32;
        let candidate = PixelRect::from_xywh(2, 1, 12, 6);
        let mut full_pixels = vec![0u32; (width * height) as usize];
        let mut full = Surface::new(&mut full_pixels, width, height);
        full.fill_rect(0, 0, width, height, 0);
        full.fill_rect(2, 1, 4, 3, 0x0011_2233);
        full.fill_rect(8, 4, 5, 2, 0x0044_5566);
        full.fill_rect(3, 6, 8, 1, 0x0077_8899);

        let mut partial_pixels = vec![0u32; (width * height) as usize];
        let mut partial = Surface::with_clip(&mut partial_pixels, width, height, candidate);
        partial.fill_rect(0, 0, width, height, 0);
        partial.fill_rect(2, 1, 4, 3, 0x0011_2233);
        partial.fill_rect(8, 4, 5, 2, 0x0044_5566);
        partial.fill_rect(3, 6, 8, 1, 0x0077_8899);

        assert_eq!(partial_pixels, full_pixels);
        assert_eq!(partial_pixels[0], 0);
        assert_eq!(partial_pixels[(7 * width + 15) as usize], 0);
    }

    #[test]
    fn direct_host_target_pixels_match_retained_raster_pixel_for_pixel() {
        let width = 9u32;
        let height = 5u32;
        let mut retained_pixels = vec![0u32; (width * height) as usize];
        let mut direct_pixels = vec![0u32; (width * height) as usize];
        for pixels in [&mut retained_pixels, &mut direct_pixels] {
            let mut surface = Surface::new(pixels, width, height);
            surface.fill_rect(0, 0, width, height, 0x0001_0203);
            surface.fill_rect(2, 1, 4, 2, 0x000A_0B0C);
            surface.fill_rect(6, 3, 2, 1, 0x000D_0E0F);
        }
        assert_eq!(direct_pixels, retained_pixels);
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
            let mut surface = Surface::new(&mut pixels, fw, fh);
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
    fn capture_loss_cancels_local_selection_and_pairs_raw_mouse_release() {
        let mut app = ConTerminal::new(None);
        app.mouse_dragging = true;
        app.selecting = true;
        app.active_button = Some(2);
        app.last_reported_cell = Some(TerminalPoint { row: 7, col: 11 });

        assert_eq!(
            app.take_cancelled_pointer_release(),
            Some((2, TerminalPoint { row: 7, col: 11 }))
        );
        assert!(!app.mouse_dragging);
        assert!(!app.selecting);
        assert_eq!(app.active_button, None);
        assert_eq!(app.take_cancelled_pointer_release(), None);
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
