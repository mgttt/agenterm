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

mod a11y;
mod agent_interface;
mod composer;
mod control;
mod control_pending;
mod font;
mod json;
mod palette;
mod perf;
mod raster_surface;
mod session_store;
#[cfg(windows)]
mod startup;
mod terminal_paint;
mod ui;
mod workspace;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use agent_interface::ScreenSnapshot;
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

use control_pending::{PendingControl, WaitKind, WaitProbe};
use palette::Rgb;
use perf::PerfStats;
use raster_surface::{CellRect, Surface};
use session_store::SessionStore;
#[cfg(test)]
use terminal_paint::paint_cells;
use terminal_paint::{CursorPaintSpec, cursor_visible, paint_cells_at, paint_cursor};

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

use agenterm_ui_core::terminal_selection::{
    TerminalPoint, normalize_endpoints, terminal_selection_text, visible_row_selection,
    word_selection,
};

fn selection_should_auto_copy(selection: Option<(TerminalPoint, TerminalPoint)>) -> bool {
    selection.is_some_and(|(anchor, focus)| anchor != focus)
}

/// Extracts text from the VT screen between two points (inclusive).
/// Produces Windows CRLF line joins, trims trailing whitespace per row.
fn selection_text(screen: &vt100::Screen, a: TerminalPoint, b: TerminalPoint) -> String {
    terminal_selection_text(screen, a, b)
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

/// Scrollback retained by the vt100 model.
const SCROLLBACK: usize = 4000;
const PTY_QUEUE_BYTES: usize = READ_BUF * 128;
const PTY_DRAIN_BUDGET_BYTES: usize = 128 * 1024;
const CONTROL_DRAIN_BUDGET_REQUESTS: usize = 2;

fn pty_drain_budget_per_session(session_count: usize) -> usize {
    if session_count == 0 {
        return 0;
    }
    (PTY_DRAIN_BUDGET_BYTES / session_count).max(1)
}

#[cfg(test)]
mod pty_budget_tests {
    use super::*;

    #[test]
    fn wake_budget_is_shared_without_multiplying_by_tab_count() {
        assert_eq!(pty_drain_budget_per_session(0), 0);
        assert_eq!(pty_drain_budget_per_session(1), PTY_DRAIN_BUDGET_BYTES);
        assert_eq!(pty_drain_budget_per_session(2), PTY_DRAIN_BUDGET_BYTES / 2);
        for count in 1..=256 {
            assert!(pty_drain_budget_per_session(count) * count <= PTY_DRAIN_BUDGET_BYTES);
        }
    }
}

/// Logical (DIP) font size. 15 px is approximately 11.25 pt at 96 DPI and
/// visually matches the 14 px tree labels. The previous value `11` was
/// pixels, not points, and therefore rendered smaller than intended.
const DEFAULT_FONT_PX: f64 = 15.0;

#[allow(clippy::manual_clamp)] // f64::clamp retains the large float-format panic path.
fn clamp_font_size(value: f64) -> f64 {
    if value < 8.0 {
        8.0
    } else if value > 36.0 {
        36.0
    } else {
        value
    }
}

/// Configuration loaded from `agenterm-con.json` (analogous to conhost
/// "Defaults" — persist font size, window geometry, etc. without a GUI dialog).
///
/// Location: platform user-config directory + `agenterm-con.json`
/// (Windows Roaming AppData, Unix `~/.config`, see `runtime::user_config_directory`).
#[derive(Default)]
struct ConConfig {
    font_size: Option<f64>,
    cols: Option<u16>,
    rows: Option<u16>,
}

fn config_path() -> Option<std::path::PathBuf> {
    agenterm_platform::runtime::user_config_directory()
        .ok()
        .map(|directory| directory.join("agenterm-con.json"))
}

#[inline(never)]
fn load_config() -> ConConfig {
    let Some(path) = config_path() else {
        return ConConfig::default();
    };
    let Ok(bytes) = agenterm_platform::filesystem_read::read_bounded(&path, json::MAX_INPUT_BYTES)
    else {
        return ConConfig::default();
    };
    let Ok(config) = json::parse_config(&bytes) else {
        return ConConfig::default();
    };
    ConConfig {
        font_size: config.font_size,
        cols: config.cols,
        rows: config.rows,
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
            "--font-size" => parsed.font_size = next_decimal(&mut rest, "--font-size")?,
            other if other.starts_with("--font-size=") => {
                parsed.font_size = Some(parse_decimal(
                    &other["--font-size=".len()..],
                    "--font-size",
                )?);
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

{}",
                    usage_text()
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

fn next_decimal<'a>(
    rest: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<Option<f64>, String> {
    let raw = rest
        .next()
        .ok_or_else(|| format!("error: {flag} requires a value\n"))?;
    parse_decimal(raw, flag).map(Some)
}

fn parse_decimal(raw: &str, flag: &str) -> Result<f64, String> {
    json::parse_finite_decimal(raw)
        .ok_or_else(|| format!("error: {flag} expects a finite number, got '{raw}'\n"))
}

fn main() {
    let args = match agenterm_platform::runtime::application_arguments() {
        Ok(args) => args,
        Err(error) => {
            let message = format!("error: cannot read process arguments: {error}\n");
            let _ = agenterm_platform::parent_console::write_stderr(&message);
            std::process::exit(2);
        }
    };

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
    } = parsed;
    no_activate |=
        agenterm_platform::runtime::ascii_environment_variable_present("AGENTERM_NO_ACTIVATE");

    // Load config file: CLI flags override config, config overrides defaults.
    let config = load_config();

    let mut app = ConApp::new(working_dir.clone(), control_endpoint);
    let session = app.active_session_mut().expect("initial terminal session");
    session.command = command;
    session.snapshot_path = snapshot_path;
    // Config values (lowest priority)
    if let Some(fs) = config.font_size {
        session.font_size_logical = clamp_font_size(fs);
    }
    if let Some(cols) = config.cols {
        session.cols = cols.max(2);
    }
    if let Some(rows) = config.rows {
        session.rows = rows.max(2);
    }
    // CLI flags override config
    if let Some(fs) = font_size {
        session.font_size_logical = clamp_font_size(fs);
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
}

#[cfg(windows)]
const USAGE_CONTROL_EXAMPLES: &str = "\
  agenterm-con --control pipe:\\\\.\\pipe\\agenterm-con-test
  agenterm-con cli --control pipe:\\\\.\\pipe\\agenterm-con-test list-tabs";

#[cfg(unix)]
const USAGE_CONTROL_EXAMPLES: &str = "\
  agenterm-con --control unix:$TMPDIR/agenterm-con-test/control.sock
  agenterm-con cli --control unix:$TMPDIR/agenterm-con-test/control.sock list-tabs
  (Unix socket parents may be under /tmp; the host resolves symlink roots such
   as macOS /tmp → /private/tmp so control works like a Windows named pipe.)";

#[cfg(windows)]
const USAGE_SHELL_EXAMPLES: &str = "\
                   agenterm-con -e pwsh -NoLogo
                   agenterm-con --working-dir C:\\src -e cargo test";

#[cfg(unix)]
const USAGE_SHELL_EXAMPLES: &str = "\
                   agenterm-con -e /bin/zsh -l
                   agenterm-con --working-dir ~/src -e cargo test";

#[cfg(windows)]
const USAGE_CONFIG_LOCATION: &str = "Configuration: create agenterm-con.json under the user config directory\n\
     (Windows: %APPDATA%\\agenterm-con.json via runtime::user_config_directory).";

#[cfg(unix)]
const USAGE_CONFIG_LOCATION: &str = "Configuration: create agenterm-con.json under the user config directory\n\
     (Unix: ~/.config/agenterm-con.json via runtime::user_config_directory).";

fn usage_text() -> String {
    format!(
        "\
Usage: agenterm-con [--no-activate] [--working-dir DIR]
                   [--font-size N] [--cols N] [--rows N]
                   [--control ENDPOINT] [--emit-snapshot PATH]
                   [-e PROGRAM [ARGS...]]
       agenterm-con --version
       agenterm-con --help
       agenterm-con cli --control ENDPOINT COMMAND [ARGS...]

A standalone console host (conhost equivalent). No server, mux, or Fleet.

Control endpoint and CLI (TAB is a stable @ID; omitted target means active tab):
  agenterm-con cli list-commands
{control_examples}
  ... ui-snapshot | perf-stats | reset-perf-stats | close-window
  ... resize-window --width N --height N
  ... new-tab [--parent TAB]
  ... select-tab --target TAB | close-tab --target TAB
  ... capture-pane [--target TAB] [--max-bytes N]
  ... screenshot-pane [--target TAB] --output PATH
  ... send-text [--target TAB] TEXT
  ... send-paste [--target TAB] TEXT
  ... send-keys [--target TAB] KEY...
  ... send-ui-keys KEY...
  ... send-mouse [--target TAB] --action press|release|move|click
                 --button none|left|middle|right --column N --row N
  ... send-wheel [--target TAB] --column N --row N --notches N [--ctrl]
  ... wait-text [--target TAB] [--timeout-ms N] TEXT
  ... wait-tab-exit --target TAB [--timeout-ms N]

Keys use names such as Enter, Escape, Tab, Up, F1 or modifiers such as Ctrl+C.
send-ui-keys follows current UI focus; send-keys always targets a terminal.
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
{shell_examples}

  --emit-snapshot PATH
                 Write a JSON snapshot of screen text/cursor/selection to
                 PATH after each render (atomic write). For CLI clients, tests,
                 and other agents that need to inspect a session without
                 capturing pixels.

{config_location}
Keys: font_size, cols, rows (all optional).
CLI flags override config; config overrides defaults.
Ctrl+wheel adjusts font size at runtime.",
        control_examples = USAGE_CONTROL_EXAMPLES,
        shell_examples = USAGE_SHELL_EXAMPLES,
        config_location = USAGE_CONFIG_LOCATION,
    )
}

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
            let _ = agenterm_platform::parent_console::write_stdout(&usage_text());
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

    /// Set once by the waiter thread when the child process actually exits
    /// (via Windows' process-exit notification, not PTY EOF — see `spawn_pty`).
    /// The existing window wake transports notification; this atomic owns only
    /// the completion state, so no general-purpose channel is required.
    child_exit_pending: Arc<AtomicBool>,
    /// Encoded optional `ExitStatus::code`, published before
    /// `child_exit_pending` with release ordering.
    child_exit_code_encoded: Arc<AtomicU64>,
    child_exit_code: Option<i32>,

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
        self.shutdown_pty();
    }
}

/// One lightweight GUI process containing several isolated terminal sessions.
///
/// The wrapper owns tree identity and routing only. A `ConTerminal` still owns
/// its own PTY, reader/waiter threads, parser, viewport and input state, so a
/// dead child or malformed output cannot corrupt another session's state.
struct ConApp {
    workspace: workspace::Workspace,
    sessions: SessionStore<ConTerminal>,
    composer: composer::ComposerState,
    tree_scroll_offset: usize,
    sidebar_width_logical: f64,
    sidebar_resizing: bool,
    exit: bool,
    control_endpoint: Option<String>,
    control_server: Option<control::ControlServer>,
    pending_control: PendingControl,
    perf_stats: PerfStats,
    chrome_dirty: DirtyRegion,
    retained: RetainedXrgbFrame,
    frame_width: u32,
    frame_height: u32,
    frame_scale: f64,
    // `None` on hosts whose accessibility backend discards snapshots, so the
    // publish path costs nothing there. The platform crate owns that choice.
    a11y: Option<agenterm_platform::accessibility_publish::AccessibilityPublisher>,
    a11y_inbox: Arc<std::sync::Mutex<Vec<a11y::Request>>>,
    a11y_dirty: bool,
    current_window_title: String,
}

impl Drop for ConApp {
    fn drop(&mut self) {
        self.pending_control
            .cancel_all("terminal window closed while control request was pending");
    }
}

#[derive(Clone, Copy, Default)]
struct DrainOutcome {
    changed: bool,
    redraw: bool,
    backlog: bool,
    bytes: usize,
}

#[inline(never)]
fn single_field_json(name: &'static str, value: json::JsonValue) -> json::JsonValue {
    json::object(vec![(name, value)])
}

fn tab_exit_json(id: workspace::TabId, exit_code: Option<i32>) -> json::JsonValue {
    json::object(vec![
        ("id", json::JsonValue::TabId(id.get())),
        ("child_alive", false.into()),
        (
            "child_exit_code",
            exit_code.map_or(json::JsonValue::Null, |code| i64::from(code).into()),
        ),
    ])
}

impl ConApp {
    fn new(working_dir: Option<String>, control_endpoint: Option<String>) -> Self {
        let mut workspace = workspace::Workspace::default();
        let initial = workspace
            .add_root("terminal".to_owned())
            .expect("an empty workspace accepts its initial tab");
        let mut sessions = SessionStore::default();
        assert!(
            sessions
                .insert(initial, ConTerminal::new(working_dir))
                .is_ok(),
            "an empty session store accepts its initial tab"
        );
        Self {
            workspace,
            sessions,
            composer: composer::ComposerState::default(),
            tree_scroll_offset: 0,
            sidebar_width_logical: ui::SIDEBAR_WIDTH_DIP,
            sidebar_resizing: false,
            exit: false,
            control_endpoint,
            control_server: None,
            pending_control: PendingControl::default(),
            perf_stats: PerfStats::default(),
            chrome_dirty: DirtyRegion::full(),
            retained: RetainedXrgbFrame::new(),
            frame_width: 0,
            frame_height: 0,
            frame_scale: 1.0,
            a11y: None,
            a11y_inbox: Arc::new(std::sync::Mutex::new(Vec::new())),
            a11y_dirty: false,
            current_window_title: String::from("agenterm-con"),
        }
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

    fn refresh_title(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let session = self.active_session()?;
        let id = self.workspace.active().ok_or_else(|| {
            PixelWindowError::failed("con_session_missing", "no active terminal session")
        })?;
        let title = format!("{} [@{}]", session.current_title, id.get());
        window.set_title(&title);
        self.current_window_title = title;
        self.mark_a11y_dirty();
        Ok(())
    }

    fn close_active_session(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let id = self.workspace.active().ok_or_else(|| {
            PixelWindowError::failed("con_session_missing", "no active terminal session")
        })?;
        self.cancel_control_for_tab(
            id,
            &format!(
                "terminal @{} closed while control request was pending",
                id.get()
            ),
        );
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
        self.composer.focused = false;
        self.refresh_title(window)?;
        window.request_redraw();
        Ok(())
    }

    fn configure_chrome(session: &mut ConTerminal, scale: f64, sidebar_width_logical: f64) {
        let scale = scale.max(1.0);
        session.set_content_insets(
            agenterm_platform::numeric::round_f64(sidebar_width_logical * scale) as u32,
            0,
            agenterm_platform::numeric::round_f64(ui::COMPOSER_HEIGHT_DIP * scale) as u32,
        );
    }

    fn layout(&self, width: u32, height: u32, scale: f64) -> ui::Layout {
        ui::Layout::with_sidebar_width(width, height, scale, self.sidebar_width_logical)
    }

    fn mark_a11y_dirty(&mut self) {
        self.a11y_dirty = true;
    }

    fn publish_a11y(&mut self, window: &PixelWindow) {
        let Some(publisher) = self.a11y.as_ref() else {
            return;
        };
        publisher.set_window_handle(window.native_identity());
        if let Ok(metrics) = window.metrics()
            && metrics.is_drawable()
        {
            self.frame_width = metrics.physical_width;
            self.frame_height = metrics.physical_height;
            self.frame_scale = metrics.scale_factor;
        }
        let width = self.frame_width.max(1);
        let height = self.frame_height.max(1);
        let layout = self.layout(width, height, self.frame_scale.max(1.0));
        publisher.publish(a11y::tree(
            "agenterm-con",
            &self.current_window_title,
            layout,
            width,
            height,
            self.composer.focused,
            &self.composer.text,
        ));
        self.a11y_dirty = false;
    }

    fn drain_a11y_actions(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        let requests = {
            let Ok(mut inbox) = self.a11y_inbox.lock() else {
                return Ok(());
            };
            std::mem::take(&mut *inbox)
        };
        for request in requests {
            match (request.node, request.action) {
                (
                    agenterm_platform::accessibility_publish::NODE_COMMAND,
                    agenterm_platform::accessibility_publish::PublishedAction::SetText(text),
                ) => {
                    self.composer.focused = true;
                    composer::replace_text(
                        &mut self.composer.text,
                        &mut self.composer.select_all,
                        &text,
                    );
                    self.composer.preedit.clear();
                    let _ = self.update_composer_ime_anchor(window);
                    self.mark_composer_dirty();
                }
                (
                    agenterm_platform::accessibility_publish::NODE_COMMAND,
                    agenterm_platform::accessibility_publish::PublishedAction::Key(key),
                ) => {
                    self.composer.focused = true;
                    match agenterm_platform::accessibility_publish::published_key_effect(&key) {
                        agenterm_platform::accessibility_publish::KeyEffect::Insert(text) => {
                            composer::insert(
                                &mut self.composer.text,
                                &mut self.composer.select_all,
                                &text,
                            );
                        }
                        agenterm_platform::accessibility_publish::KeyEffect::Backspace => {
                            composer::backspace(
                                &mut self.composer.text,
                                &mut self.composer.select_all,
                            );
                        }
                        agenterm_platform::accessibility_publish::KeyEffect::SelectAll => {
                            composer::select_all(
                                &self.composer.text,
                                &mut self.composer.select_all,
                            );
                        }
                        agenterm_platform::accessibility_publish::KeyEffect::Submit => {
                            self.submit_composer();
                        }
                        agenterm_platform::accessibility_publish::KeyEffect::Cancel => {
                            self.composer.cancel_focus();
                        }
                        agenterm_platform::accessibility_publish::KeyEffect::Ignore => {}
                    }
                    self.composer.preedit.clear();
                    let _ = self.update_composer_ime_anchor(window);
                    self.mark_composer_dirty();
                }
                (agenterm_platform::accessibility_publish::NODE_COMMAND, _) => {
                    self.composer.focused = true;
                    let _ = self.update_composer_ime_anchor(window);
                    self.mark_composer_dirty();
                }
                (
                    agenterm_platform::accessibility_publish::NODE_SEND,
                    agenterm_platform::accessibility_publish::PublishedAction::Click,
                ) => {
                    self.submit_composer();
                }
                (
                    agenterm_platform::accessibility_publish::NODE_SEND,
                    agenterm_platform::accessibility_publish::PublishedAction::Focus,
                )
                | (agenterm_platform::accessibility_publish::NODE_SESSION, _) => {
                    self.composer.focused = false;
                    self.mark_composer_dirty();
                }
                (agenterm_platform::accessibility_publish::NODE_FRAME, _) => {
                    window.focus();
                }
                _ => {}
            }
            self.mark_a11y_dirty();
        }
        Ok(())
    }

    fn mark_chrome_full(&mut self) {
        self.chrome_dirty.mark_full();
        self.mark_a11y_dirty();
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
        self.mark_a11y_dirty();
        if self.frame_width == 0 || self.frame_height == 0 {
            self.mark_chrome_full();
            return;
        }
        let layout = self.layout(self.frame_width, self.frame_height, self.frame_scale);
        // The whole composer band, not just the control row. The "SEND TO @N"
        // label is painted at `composer.y + 7` -- above `composer_input.y` --
        // and its color tracks `composer_focused`, so marking only the controls
        // left the label showing the previous focus state until some unrelated
        // damage happened to cover it. The band is the provable bound: every
        // pixel `paint_chrome` derives from composer state lives inside it.
        self.mark_chrome_rect(
            layout.composer.x,
            layout.composer.y,
            self.frame_width.saturating_sub(layout.composer.x),
            self.frame_height.saturating_sub(layout.composer.y),
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
            _ => self.workspace.add_root("terminal".to_owned()),
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
        if self.sessions.insert(id, session).is_err() {
            self.workspace.close(id);
            return Err(PixelWindowError::failed(
                "con_tab_create",
                "stable tab id is already in use",
            ));
        }
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
            self.composer.focused = true;
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
        self.composer.focused = false;
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
            + self.composer.text.chars().count() as f64 * 8.0;
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
                composer::select_all(&self.composer.text, &mut self.composer.select_all);
            } else if text.eq_ignore_ascii_case("c") {
                if let Some(text) =
                    composer::selected_text(&self.composer.text, &self.composer.select_all)
                {
                    let _ = agenterm_platform::clipboard::set_text(text);
                }
            } else if text.eq_ignore_ascii_case("x") {
                if let Some(text) =
                    composer::cut(&mut self.composer.text, &mut self.composer.select_all)
                {
                    let _ = agenterm_platform::clipboard::set_text(&text);
                }
            } else if text.eq_ignore_ascii_case("v")
                && let Ok(text) =
                    agenterm_platform::clipboard::get_text(composer::PASTE_LIMIT_BYTES)
            {
                composer::paste(
                    &mut self.composer.text,
                    &mut self.composer.select_all,
                    &text,
                );
            }
            let _ = self.update_composer_ime_anchor(window);
            return true;
        }
        match &key.logical {
            LogicalKey::Named(NamedKey::Enter) => {
                self.submit_composer();
            }
            LogicalKey::Named(NamedKey::Backspace) => {
                composer::backspace(&mut self.composer.text, &mut self.composer.select_all);
            }
            LogicalKey::Named(NamedKey::Escape) => {
                self.composer.cancel_focus();
            }
            LogicalKey::Named(NamedKey::Space) if !key.modifiers.control && !key.modifiers.alt => {
                composer::insert(&mut self.composer.text, &mut self.composer.select_all, " ");
            }
            LogicalKey::Character(text)
                if !key.modifiers.control && !key.modifiers.alt && !text.is_empty() =>
            {
                composer::insert(&mut self.composer.text, &mut self.composer.select_all, text);
            }
            _ => {}
        }
        let _ = self.update_composer_ime_anchor(window);
        true
    }

    fn submit_composer(&mut self) {
        if let Some(input) = self.composer.take_submission()
            && let Ok(session) = self.active_session_mut()
        {
            // Submission crosses the PTY boundary and can change arbitrary
            // terminal cells; the composer rectangle alone is not enough.
            session.dirty.mark_full();
            session.scroll_to_bottom();
            session.write_pty(input.as_bytes());
        }
    }

    fn handle_composer_ime(
        &mut self,
        window: &PixelWindow,
        event: agenterm_platform::ime::ImeEvent,
    ) {
        use agenterm_platform::ime::{ImeAction, classify_event};
        match classify_event(event, true) {
            ImeAction::UpdatePreedit { text, .. } => self.composer.preedit = text,
            ImeAction::ClearPreedit => self.composer.preedit.clear(),
            ImeAction::CommitText(text) => {
                self.composer.preedit.clear();
                composer::insert(
                    &mut self.composer.text,
                    &mut self.composer.select_all,
                    &text,
                );
            }
            ImeAction::None => {}
            _ => self.composer.preedit.clear(),
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

    fn control_session_mut(
        &mut self,
        target: Option<workspace::TabId>,
    ) -> Result<&mut ConTerminal, String> {
        let id = self.control_target(target)?;
        self.sessions
            .get_mut(&id)
            .ok_or_else(|| "terminal disappeared".to_owned())
    }

    fn cancel_control_for_tab(&mut self, id: workspace::TabId, reason: &str) {
        self.pending_control.cancel_for_tab(id, reason);
    }

    fn cancel_all_control_requests(&mut self, reason: &str) {
        self.pending_control.cancel_all(reason);
    }

    fn reap_finished_control_screenshot(&mut self) {
        self.pending_control.reap_finished_screenshot();
    }

    fn validate_control_cell(session: &ConTerminal, row: u16, column: u16) -> Result<(), String> {
        if row >= session.rows || column >= session.cols {
            return Err(format!(
                "mouse cell {row},{column} is outside {}x{}",
                session.rows, session.cols
            ));
        }
        Ok(())
    }

    fn dispatch_control(&mut self, window: &PixelWindow, request: control::IncomingRequest) {
        #[inline(never)]
        fn tab_id_json(id: Option<workspace::TabId>) -> json::JsonValue {
            match id {
                Some(id) => json::JsonValue::TabId(id.get()),
                None => json::JsonValue::Null,
            }
        }

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
                        json::object(vec![
                            ("id", tab_id_json(Some(node.id))),
                            ("parent", tab_id_json(node.parent)),
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
                            (
                                "child_exit_code",
                                session
                                    .and_then(|session| session.child_exit_code)
                                    .map_or(json::JsonValue::Null, |code| i64::from(code).into()),
                            ),
                        ])
                    })
                    .collect();
                Ok(single_field_json("tabs", json::JsonValue::Array(tabs)))
            }
            CliCommand::UiSnapshot => {
                window
                    .metrics()
                    .map_err(|error| error.to_string())
                    .map(|metrics| {
                        let layout = self.layout(
                            metrics.physical_width,
                            metrics.physical_height,
                            metrics.scale_factor,
                        );
                        json::object(vec![
                            ("active", tab_id_json(self.workspace.active())),
                            ("composer_focused", self.composer.focused.into()),
                            ("composer_text", self.composer.text.as_str().into()),
                            ("composer_preedit", self.composer.preedit.as_str().into()),
                            (
                                "pending_control_waits",
                                self.pending_control.wait_count().into(),
                            ),
                            (
                                "pending_control_screenshots",
                                self.pending_control.screenshot_count().into(),
                            ),
                            (
                                "composer_input",
                                json::object(vec![
                                    ("x", layout.composer_input.x.into()),
                                    ("y", layout.composer_input.y.into()),
                                    ("width", layout.composer_input.width.into()),
                                    ("height", layout.composer_input.height.into()),
                                ]),
                            ),
                        ])
                    })
            }
            CliCommand::PerfStats => Ok(self.perf_stats.json()),
            CliCommand::ResetPerfStats => {
                self.perf_stats.reset(window.present_stats());
                Ok(single_field_json("reset", true.into()))
            }
            CliCommand::CloseWindow => {
                self.cancel_all_control_requests(
                    "terminal window closed while control request was pending",
                );
                self.exit = true;
                Ok(single_field_json("closing", true.into()))
            }
            CliCommand::ResizeWindow { width, height } => window
                .request_logical_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
                .map_err(|error| error.to_string())
                .map(|()| json::object(vec![("width", width.into()), ("height", height.into())])),
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
                Ok(json::object(vec![
                    ("id", tab_id_json(Some(id))),
                    ("parent", tab_id_json(parent)),
                ]))
            })(),
            CliCommand::SelectTab { target } => self.control_target(Some(target)).map(|id| {
                self.mark_chrome_full();
                self.workspace.set_active(id);
                window.request_redraw();
                single_field_json("active", tab_id_json(Some(id)))
            }),
            CliCommand::CloseTab { target } => self.control_target(Some(target)).and_then(|id| {
                self.workspace.set_active(id);
                self.close_active_session(window)
                    .map_err(|error| error.to_string())?;
                Ok(single_field_json("closed", tab_id_json(Some(id))))
            }),
            CliCommand::CapturePane { target, max_bytes } => {
                self.control_session_mut(target).map(|session| {
                    session.drain_pty();
                    let mut text = session.build_snapshot().rows_text.join("\n");
                    if text.len() > max_bytes {
                        let mut end = max_bytes;
                        while end > 0 && !text.is_char_boundary(end) {
                            end -= 1;
                        }
                        text.truncate(end);
                    }
                    json::JsonValue::String(text)
                })
            }
            CliCommand::SendText { target, text } => {
                self.control_session_mut(target).map(|session| {
                    session.scroll_to_bottom();
                    session.write_pty(text.as_bytes());
                    single_field_json("sent_bytes", text.len().into())
                })
            }
            CliCommand::SendPaste { target, text } => {
                self.control_session_mut(target).map(|session| {
                    session.paste_text(&text);
                    single_field_json("sent_bytes", text.len().into())
                })
            }
            CliCommand::SendKeys { target, keys } => {
                self.control_session_mut(target).and_then(|session| {
                    for key in &keys {
                        let (key, ctrl, alt, shift) = parse_control_key(key)?;
                        session.inject_key(key, ctrl, alt, shift);
                    }
                    Ok(single_field_json("sent_keys", keys.len().into()))
                })
            }
            CliCommand::SendUiKeys { keys } => (|| {
                for key in &keys {
                    let (key, ctrl, alt, shift) = parse_control_key(key)?;
                    let event = injected_key_event(key, ctrl, alt, shift);
                    if self
                        .handle_workspace_shortcut(window, &event)
                        .map_err(|error| error.to_string())?
                    {
                        continue;
                    }
                    if self.composer.focused {
                        self.handle_composer_key(window, &event);
                        self.mark_composer_dirty();
                    } else {
                        self.active_session_mut()
                            .map_err(|error| error.to_string())?
                            .forward_key(&event);
                    }
                }
                self.request_dirty_redraw(window);
                Ok(single_field_json("sent_keys", keys.len().into()))
            })(),
            CliCommand::SendMouse {
                target,
                action,
                button,
                column,
                row,
            } => self.control_session_mut(target).and_then(|session| {
                Self::validate_control_cell(session, row, column)?;
                match action {
                    control::MouseAction::Move => session.inject_mouse_move(window, row, column),
                    control::MouseAction::Click => {
                        let button = control_mouse_button(button)?;
                        session.inject_click(window, row, column, button);
                    }
                    control::MouseAction::Press | control::MouseAction::Release => {
                        let button = control_mouse_button(button)?;
                        let state = if action == control::MouseAction::Press {
                            PointerButtonState::Pressed
                        } else {
                            PointerButtonState::Released
                        };
                        session.inject_pointer_button(window, row, column, button, state);
                    }
                }
                Ok(single_field_json("delivered", true.into()))
            }),
            CliCommand::SendWheel {
                target,
                column,
                row,
                notches,
                ctrl,
            } => self.control_session_mut(target).and_then(|session| {
                Self::validate_control_cell(session, row, column)?;
                session.inject_wheel(window, row, column, f32::from(notches), ctrl);
                Ok(single_field_json("delivered_notches", notches.into()))
            }),
            CliCommand::ScreenshotPane { target, output } => {
                self.control_target(target).and_then(|id| {
                    agent_interface::initialize_png_worker()
                        .map_err(|error| format!("initialize PNG worker: {error}"))?;
                    self.pending_control.enqueue_screenshot(
                        id,
                        PathBuf::from(output),
                        &mut reply,
                    )?;
                    window.request_redraw();
                    Ok(json::JsonValue::Null)
                })
            }
            CliCommand::WaitText {
                target,
                text,
                timeout_ms,
            } => self.control_target(target).and_then(|id| {
                if self
                    .sessions
                    .get(&id)
                    .is_some_and(|session| session.screen_contains(&text))
                {
                    return Ok(single_field_json("matched", true.into()));
                }
                self.pending_control.enqueue_wait(
                    id,
                    WaitKind::Text(text),
                    timeout_ms,
                    &mut reply,
                    "too many pending wait-text requests",
                )?;
                Ok(json::JsonValue::Null)
            }),
            CliCommand::WaitTabExit { target, timeout_ms } => {
                self.control_target(Some(target)).and_then(|id| {
                    if let Some(session) = self.sessions.get(&id)
                        && session.child_gone
                    {
                        return Ok(tab_exit_json(id, session.child_exit_code));
                    }
                    self.pending_control.enqueue_wait(
                        id,
                        WaitKind::TabExit,
                        timeout_ms,
                        &mut reply,
                        "too many pending control wait requests",
                    )?;
                    Ok(json::JsonValue::Null)
                })
            }
        };
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
    }

    fn drain_control(&mut self, window: &PixelWindow, now: Instant) -> Option<Instant> {
        self.reap_finished_control_screenshot();
        let (requests, backlog) = self
            .control_server
            .as_ref()
            .map(|server| server.recv_batch(CONTROL_DRAIN_BUDGET_REQUESTS))
            .unwrap_or_else(|| (Vec::new(), false));
        for request in requests {
            self.perf_stats.control_requests = self.perf_stats.control_requests.saturating_add(1);
            self.dispatch_control(window, request);
        }
        if backlog {
            self.perf_stats.control_budget_yields =
                self.perf_stats.control_budget_yields.saturating_add(1);
            let _ = window.waker().wake();
        }
        let sessions = &self.sessions;
        self.pending_control.poll_waits(now, |target, kind| {
            let Some(session) = sessions.get(&target) else {
                return WaitProbe::Missing(format!(
                    "terminal @{} disappeared while control request was pending",
                    target.get()
                ));
            };
            match kind {
                WaitKind::Text(text) if session.screen_contains(text) => {
                    WaitProbe::Completed(single_field_json("matched", true.into()))
                }
                WaitKind::TabExit if session.child_gone => {
                    WaitProbe::Completed(tab_exit_json(target, session.child_exit_code))
                }
                _ => WaitProbe::Pending,
            }
        })
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
        let depths = self.workspace.depths();
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
            let mut id = itoa::Buffer::new();
            paint_chrome_text_parts(
                &mut surface,
                indent,
                y + 7,
                &["@", id.format(node.id.get()), "  ", title],
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
        let mut active_id_text = itoa::Buffer::new();
        paint_chrome_text_parts(
            &mut surface,
            tree_width + 12,
            input_y + 7,
            &["SEND TO @", active_id_text.format(active_id)],
            if self.composer.focused { accent } else { muted },
            11,
            layout.composer.width.saturating_sub(24),
        );
        surface.fill_rect(
            layout.composer_input.x,
            layout.composer_input.y,
            layout.composer_input.width,
            layout.composer_input.height,
            if self.composer.focused {
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
            if self.composer.select_all {
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
        let composer_cursor = if self.composer.focused && !self.composer.select_all {
            "|"
        } else {
            ""
        };
        paint_chrome_text_parts(
            &mut surface,
            layout.composer_input.x + 10,
            layout.composer_input.y + 12,
            &[
                self.composer.text.as_str(),
                self.composer.preedit.as_str(),
                composer_cursor,
            ],
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
    fn shutdown_pty(&mut self) {
        // First release product backpressure, then transfer both ownership
        // halves. ClosePseudoConsole may block while a flooded client drains,
        // so native teardown must never run on the GUI event thread.
        self.pty_output.close();
        let master = self.master.take();
        let child = self.child.take();
        let _ = agenterm_platform::pty::shutdown_session_detached(master, child);
    }

    fn new(working_dir: Option<String>) -> Self {
        let pty_output = Arc::new(BoundedOutputPipe::new(PTY_QUEUE_BYTES));
        pty_output.close();
        Self {
            working_dir,
            command: None,
            current_title: String::new(),
            snapshot_path: None,
            parser: vt100::Parser::new_with_callbacks(24, 80, SCROLLBACK, ConCallbacks::default()),
            master: None,
            child: None,
            pty_output,
            pty_wake_pending: Arc::new(AtomicBool::new(false)),
            child_exit_pending: Arc::new(AtomicBool::new(false)),
            child_exit_code_encoded: Arc::new(AtomicU64::new(0)),
            child_exit_code: None,
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
        let Some((start, end)) = selection.map(|(a, b)| normalize_endpoints(a, b)) else {
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
        self.font_size_px =
            agenterm_platform::numeric::round_f64(self.font_size_logical * scale).max(8.0) as u16;
        let m = font::cell_metrics(self.font_size_px);
        self.cell_w = m.width.max(1);
        self.cell_h = m.height.max(1);
    }

    /// Spawns the shell PTY and the reader thread. Called once from `opened`.
    fn spawn_pty(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        agenterm_platform::pty::initialize_shutdown_reaper().map_err(|error| {
            PixelWindowError::failed("pty_reaper_init_failed", format!("{error}"))
        })?;
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
        agenterm_platform::threading::spawn_named_detached(
            "agenterm-con-reader",
            Box::new(move || {
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
            }),
        )
        .map_err(|error| PixelWindowError::failed("cmd_reader_spawn_failed", format!("{error}")))?;

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
        let child_exit_pending = Arc::new(AtomicBool::new(false));
        let waiter_exit_pending = Arc::clone(&child_exit_pending);
        let child_exit_code_encoded = Arc::new(AtomicU64::new(0));
        let waiter_exit_code = Arc::clone(&child_exit_code_encoded);
        let exit_waker = window.waker();
        agenterm_platform::threading::spawn_named_detached(
            "agenterm-con-waiter",
            Box::new(move || {
                let wait_result = waiter.wait();
                waiter_exit_code.store(
                    encode_child_exit_code(
                        wait_result
                            .as_ref()
                            .ok()
                            .and_then(std::process::ExitStatus::code),
                    ),
                    Ordering::Release,
                );
                waiter_exit_pending.store(true, Ordering::Release);
                let _ = exit_waker.wake();
            }),
        )
        .map_err(|error| PixelWindowError::failed("cmd_waiter_spawn_failed", format!("{error}")))?;

        self.master = Some(master);
        self.child = Some(child);
        self.pty_output = output;
        self.pty_wake_pending = wake_pending;
        self.child_exit_pending = child_exit_pending;
        self.child_exit_code_encoded = child_exit_code_encoded;

        Ok(())
    }

    fn drain_pty(&mut self) -> DrainOutcome {
        self.drain_pty_with_budget(PTY_DRAIN_BUDGET_BYTES)
    }

    fn drain_pty_with_budget(&mut self, budget: usize) -> DrainOutcome {
        self.pty_wake_pending.store(false, Ordering::Release);
        let mut outcome = DrainOutcome::default();
        if self.child_exit_pending.swap(false, Ordering::AcqRel) {
            self.child_gone = true;
            self.child_exit_code =
                decode_child_exit_code(self.child_exit_code_encoded.load(Ordering::Acquire));
            outcome.redraw = true;
        }

        let output = Arc::clone(&self.pty_output);
        let report = output.drain(budget, |bytes| {
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
        word_selection(self.parser.screen(), point)
    }

    /// Triple-click owns one visible terminal row; soft-wrapped neighbors are
    /// separate selectable rows, matching the professional-selection contract.
    fn line_at(&self, point: TerminalPoint) -> Option<(TerminalPoint, TerminalPoint)> {
        visible_row_selection(self.parser.screen(), point.row)
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
                && self.active_selection().is_some()
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
                agenterm_platform::numeric::round_f64(position.x * scale) as i32,
                agenterm_platform::numeric::round_f64(position.y * scale) as i32,
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
    /// what lets control commands take cell coordinates (what a CLI caller
    /// actually thinks in) while still driving the same
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

    /// The selection as far as anything outside the drag gesture is concerned.
    ///
    /// A left press seeds `selection` with `(point, point)` so a drag has an
    /// anchor to extend from, but a degenerate range covers no text. The
    /// product already encoded that in `selection_should_auto_copy`, which
    /// refuses to copy it -- every other consumer treated it as a real
    /// one-cell selection. That single omission produced three separate
    /// symptoms from one plain click: the clicked cell stayed inverted for the
    /// rest of the session, a following right-click copied nothing instead of
    /// pasting, and bare Ctrl+C took the copy branch and returned, so the
    /// child process never received SIGINT.
    fn active_selection(&self) -> Option<(TerminalPoint, TerminalPoint)> {
        self.selection.filter(|(anchor, focus)| anchor != focus)
    }

    fn copy_selection(&self) {
        let Some((start, end)) = self.active_selection() else {
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
    /// OS clipboard (`paste_clipboard`) or a control `send-paste` command. Both
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
    /// control `wait-text` command to sequence on real output instead of a
    /// guessed duration.
    fn screen_contains(&self, needle: &str) -> bool {
        let screen = self.parser.screen();
        let cols = screen.size().1;
        screen
            .rows(0, cols)
            .any(|row| control::contains_utf8(&row, needle))
    }

    /// Synthesizes a [`NormalizedKeyEvent`] for a control key command and
    /// forwards it through [`ConTerminal::forward_key`] — the exact path a
    /// real keystroke takes, including host shortcuts and the live
    /// DECCKM/modifier-aware encoder.
    fn inject_key(&mut self, key: InjectedKey, ctrl: bool, alt: bool, shift: bool) {
        self.forward_key(&injected_key_event(key, ctrl, alt, shift));
    }

    /// Presses then releases a mouse button at a control cell coordinate
    /// through the same path used by a physical click.
    fn inject_click(
        &mut self,
        window: &PixelWindow,
        row: u16,
        col: u16,
        button: InjectedMouseButton,
    ) {
        self.inject_pointer_button(window, row, col, button, PointerButtonState::Pressed);
        self.inject_pointer_button(window, row, col, button, PointerButtonState::Released);
    }

    /// One half of a control press-drag-release gesture, shared by
    /// [`Self::inject_click`] for the atomic press+release case.
    fn inject_pointer_button(
        &mut self,
        window: &PixelWindow,
        row: u16,
        col: u16,
        button: InjectedMouseButton,
        state: PointerButtonState,
    ) {
        let position = self.terminal_point_to_logical(TerminalPoint { row, col });
        let platform_button = match button {
            InjectedMouseButton::Left => PointerButton::Left,
            InjectedMouseButton::Middle => PointerButton::Middle,
            InjectedMouseButton::Right => PointerButton::Right,
        };
        self.handle_pointer_button(
            window,
            platform_button,
            state,
            Some(position),
            &ModifierState::default(),
        );
    }

    /// Moves the pointer to a control cell coordinate through the physical
    /// pointer-motion path.
    fn inject_mouse_move(&mut self, window: &PixelWindow, row: u16, col: u16) {
        let position = self.terminal_point_to_logical(TerminalPoint { row, col });
        self.handle_pointer_moved(window, position, &ModifierState::default());
    }

    /// Sends wheel input at a control cell coordinate through `handle_wheel`.
    /// `handle_wheel` itself never requests a
    /// redraw (real wheel events get that from the `MouseWheel` dispatch
    /// arm that calls it), so this mirrors that call site rather than
    /// leaving a scripted scroll invisible until the next unrelated redraw.
    fn inject_wheel(&mut self, window: &PixelWindow, row: u16, col: u16, notches: f32, ctrl: bool) {
        if ctrl {
            // Mirrors the real event: one `zoom_font` call per whole notch,
            // not one call scaled by magnitude — a real Ctrl+wheel session
            // is a *stream* of individual notch events, and reproducing a
            // crash tied to repeated cumulative resizes means replaying
            // that shape, not collapsing it into a single jump.
            let count = agenterm_platform::numeric::round_f32(notches.abs()).max(1.0) as usize;
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
        let visible_now = cursor_visible(screen, self.scroll_offset, self.blink_visible);
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
            selection: self.active_selection().map(|(a, b)| {
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
            child_exit_code: self.child_exit_code,
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
    /// control `send-wheel --ctrl` command drives the identical
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
        self.font_size_logical = clamp_font_size(self.font_size_logical + delta_size);
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
        let count = (agenterm_platform::numeric::round_f32(notches.abs()) as usize).clamp(1, 32);

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
                        self.selection = self.line_at(point);
                        self.selecting = false;
                    }
                }
            }
            (PointerButton::Left, false) => {
                self.selecting = false;
                if selection_should_auto_copy(self.selection) {
                    self.copy_selection();
                } else {
                    // The drag never left its anchor cell, so no selection was
                    // ever made. Dropping the seed here keeps the stored state
                    // equal to what every consumer already sees through
                    // `active_selection`.
                    self.selection = None;
                }
            }
            (PointerButton::Right, true) => {
                // Right-click: copy if a selection exists, else paste.
                if self.active_selection().is_some() {
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
    /// Factored out of the `PointerMoved` event arm so a control command
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
                    let whole = agenterm_platform::numeric::trunc_f32(self.wheel_accumulator);
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
        self.last_cursor = Some(TerminalPoint {
            row: cursor.0,
            col: cursor.1,
        });
        // A steady request always shows the cursor; a blinking one is gated
        // by the timer in about_to_wait. conhost draws the caret the same
        // way — this is parity, not an enhancement — but getting it right
        // matters for vim/nvim, which switch shape *and* blink per mode.
        paint_cells_at(
            &mut surface,
            screen,
            self.active_selection(),
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

        paint_cursor(
            &mut surface,
            screen,
            CursorPaintSpec {
                cell_w: self.cell_w,
                cell_h: self.cell_h,
                default_fg: self.default_fg,
                default_bg: self.default_bg,
                font_size_px: self.font_size_px,
                left: self.content_left_px,
                top: self.content_top_px,
                scroll_offset: self.scroll_offset,
                preedit_cells,
                blink_visible: self.blink_visible,
            },
        );

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
        match agenterm_platform::accessibility_publish::start(
            "agenterm-con",
            window.native_identity(),
        ) {
            // A backend that discards snapshots is dropped here, so the chrome
            // tree is never built on hosts that cannot serve it.
            Ok(publisher) if publisher.is_publishing() => {
                let inbox = Arc::clone(&self.a11y_inbox);
                let waker = window.waker();
                publisher.set_handler(Arc::new(move |node, action| {
                    if let Ok(mut queue) = inbox.lock() {
                        queue.push(a11y::Request { node, action });
                    }
                    let _ = waker.wake();
                }));
                self.a11y = Some(publisher);
                self.a11y_dirty = true;
                self.publish_a11y(window);
            }
            Ok(_) => {}
            Err(error) => {
                let _ = agenterm_platform::parent_console::write_stderr(&format!(
                    "agenterm-con a11y: {error}\n"
                ));
            }
        }
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
            self.drain_a11y_actions(window)?;
            // PTY readers and the control server share the native wake path.
            // A flooded PTY continuously reposts Wake while bounded output
            // remains, so waiting until about_to_wait would starve control
            // requests (including the request that closes that PTY).
            let _ = self.drain_control(window, Instant::now());
            if self.exit {
                return Ok(PixelWindowDirective::Exit);
            }
            let active = self.workspace.active();
            let session_budget = pty_drain_budget_per_session(self.workspace.nodes().len());
            let mut active_redraw = false;
            let mut backlog = false;
            for (id, session) in self.sessions.entries_mut() {
                let outcome = session.drain_pty_with_budget(session_budget);
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
                    WheelDelta::Lines { y, .. } => {
                        agenterm_platform::numeric::round_f32(*y) as isize
                    }
                    WheelDelta::LogicalPixels { y, .. } => {
                        agenterm_platform::numeric::round_f64(*y / ui::TREE_ROW_HEIGHT_DIP) as isize
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
                    self.composer.focused = true;
                    self.composer.select_all = false;
                    self.update_composer_ime_anchor(window)?;
                    self.mark_composer_dirty();
                    // A physical client click has already activated and
                    // focused this top-level window. Re-entering native focus
                    // here synchronously emits another focus-message chain
                    // from inside pointer dispatch and can disrupt painting.
                    self.request_dirty_redraw(window);
                    return Ok(PixelWindowDirective::Continue);
                }
                ui::ComposerHit::Send => {
                    self.submit_composer();
                    self.composer.focused = true;
                    self.update_composer_ime_anchor(window)?;
                    self.mark_composer_dirty();
                    self.request_dirty_redraw(window);
                    return Ok(PixelWindowDirective::Continue);
                }
                ui::ComposerHit::Outside => {}
            }
            self.composer.focused = false;
            self.mark_composer_dirty();
            self.request_dirty_redraw(window);
        }
        if self.composer.focused {
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
        // Closing the last tab sets `exit` from control drain/event paths.
        // Portable (macOS/Linux) hosts may still deliver a pending redraw after
        // that; do not touch active session or report con_session_missing —
        // match Windows close-window: clean Exit directive.
        if self.exit {
            return Ok(PixelWindowDirective::Exit);
        }
        self.perf_stats.sync_present_stats(window.present_stats());
        if let Some(target) = self
            .pending_control
            .prepare_screenshot(self.workspace.active())
        {
            self.workspace.set_active(target);
        }
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
        #[cfg(windows)]
        if !candidate.is_empty() && !candidate.is_full() {
            // The optimized clipped raster path can currently erase retained
            // pixels outside its dirty bounds after pointer/composer updates.
            // Correctness wins until that path is proven by native before/after
            // screenshots; Windows con redraws a complete frame meanwhile.
            candidate = DirtyRegion::full_frame(width, height);
        }
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
        let mut discard_capture_frame = false;
        if let Some(screenshot) = self.pending_control.take_screenshot() {
            let control_pending::ScreenshotWork {
                target,
                path,
                reply,
                restore_active,
            } = screenshot;
            let pixels = if host_retains_pixels {
                frame.pixels_mut().to_vec()
            } else {
                self.retained.pixels().to_vec()
            };
            let response_path = path.to_string_lossy().into_owned();
            let shared_reply = Arc::new(std::sync::Mutex::new(Some(reply)));
            let done = Arc::new(AtomicBool::new(false));
            self.pending_control.start_screenshot(
                target,
                Arc::clone(&shared_reply),
                Arc::clone(&done),
            );
            let waker = window.waker();
            agent_interface::submit_png_atomic(
                path,
                pixels,
                width,
                height,
                Box::new(move |write_result| {
                    let result = write_result
                        .map(|encode_ns| {
                            json::object(vec![
                                ("path", response_path.into()),
                                ("width", width.into()),
                                ("height", height.into()),
                                ("encode_ns", encode_ns.into()),
                            ])
                        })
                        .map_err(|error| format!("write screenshot: {error}"));
                    if let Some(reply) = shared_reply
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        let _ = reply.send(result);
                    }
                    done.store(true, Ordering::Release);
                    let _ = waker.wake();
                }),
            );
            if let Some(restore_active) = restore_active
                && self.sessions.contains_key(&restore_active)
            {
                self.workspace.set_active(restore_active);
                self.mark_chrome_full();
                render_try!(self.active_session_mut()).dirty.mark_full();
                render_try!(self.refresh_title(window));
                window.request_redraw();
                discard_capture_frame = true;
            }
        }
        if discard_capture_frame {
            if let Err(error) = frame.commit(PixelFrameWrite::Discard) {
                self.retained.invalidate();
                return Err(PixelWindowError::failed(
                    "con_capture_frame_discard",
                    error.to_string(),
                ));
            }
            self.retained.invalidate();
            self.perf_stats.discarded_capture_frames =
                self.perf_stats.discarded_capture_frames.saturating_add(1);
            self.perf_stats.record_frame(render_started.elapsed());
            self.perf_stats
                .record_raster_candidate(candidate, width, height);
            return Ok(directive);
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
        // A control request can close the final tab while draining. Re-check
        // before asking for an active session so explicit close is a normal
        // host exit rather than a synthetic `con_session_missing` failure.
        if self.exit {
            return Ok(PixelWindowDirective::Exit);
        }
        if self.pending_control.has_pending_screenshot() {
            window.request_redraw();
        }
        if self.a11y_dirty {
            self.publish_a11y(window);
        }
        let directive = self.active_session_mut()?.about_to_wait(window, now)?;
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

#[derive(Clone, Copy)]
enum InjectedKey {
    Named(NamedKey),
    Char(char),
}

fn injected_key_event(key: InjectedKey, ctrl: bool, alt: bool, shift: bool) -> NormalizedKeyEvent {
    let modifiers = ModifierState {
        control: ctrl,
        alt,
        shift,
        meta: false,
    };
    let logical = match key {
        InjectedKey::Named(named) => LogicalKey::Named(named),
        InjectedKey::Char(ch) => LogicalKey::Character(ch.to_string()),
    };
    let text = match key {
        InjectedKey::Named(_) => None,
        InjectedKey::Char(ch) if !ctrl && !alt => Some(ch.to_string()),
        InjectedKey::Char(_) => None,
    };
    NormalizedKeyEvent {
        logical,
        physical: PhysicalKeyCode::Other,
        text,
        state: KeyPressState::Pressed,
        repeat: false,
        modifiers,
    }
}

fn encode_child_exit_code(code: Option<i32>) -> u64 {
    code.map_or(0, |code| u64::from(code as u32) + 1)
}

fn decode_child_exit_code(encoded: u64) -> Option<i32> {
    encoded
        .checked_sub(1)
        .and_then(|bits| u32::try_from(bits).ok())
        .map(|bits| bits as i32)
}

#[derive(Clone, Copy)]
enum InjectedMouseButton {
    Left,
    Middle,
    Right,
}

fn parse_control_key(spec: &str) -> Result<(InjectedKey, bool, bool, bool), String> {
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
    let named = NamedKey::from_name(key_name);
    let key = if let Some(named) = named {
        InjectedKey::Named(named)
    } else {
        let mut chars = key_name.chars();
        let character = chars.next().ok_or_else(|| "empty key".to_owned())?;
        if chars.next().is_some() {
            return Err(format!("unknown key {key_name:?}"));
        }
        InjectedKey::Char(character)
    };
    Ok((key, ctrl, alt, shift))
}

fn control_mouse_button(button: control::MouseButton) -> Result<InjectedMouseButton, String> {
    match button {
        control::MouseButton::Left => Ok(InjectedMouseButton::Left),
        control::MouseButton::Middle => Ok(InjectedMouseButton::Middle),
        control::MouseButton::Right => Ok(InjectedMouseButton::Right),
        control::MouseButton::None => Err("press/release requires a mouse button".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// Pixel helpers
// ---------------------------------------------------------------------------

/// A cell's pixel rectangle. The four values are always derived together from
/// the grid position, so passing them separately only invited transposition.

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

fn paint_chrome_text(
    surface: &mut Surface<'_>,
    x: u32,
    y: u32,
    text: &str,
    color: Rgb,
    font_size_px: u16,
    max_width: u32,
) {
    paint_chrome_text_parts(surface, x, y, &[text], color, font_size_px, max_width);
}

fn paint_chrome_text_parts(
    surface: &mut Surface<'_>,
    x: u32,
    y: u32,
    parts: &[&str],
    color: Rgb,
    font_size_px: u16,
    max_width: u32,
) {
    let metrics = font::cell_metrics(font_size_px);
    let cell_w = metrics.width.max(1);
    let cell_h = metrics.height.max(1);
    let mut cursor = x;
    let limit = x.saturating_add(max_width).min(surface.width);
    for part in parts {
        for character in part.chars() {
            // The same width rule `paint_cells` and the terminal IME preedit
            // already apply: a double-width character owns two cells. Advancing
            // one narrow cell per character truncated every wide glyph to half
            // its width via the CellRect clip in `blit_glyph`, then started the
            // next character on top of the remains -- which is exactly why CJK
            // typed into the composer rendered as overlapping garbage while
            // ASCII stayed crisp.
            let cells = if unicode_width::UnicodeWidthChar::width(character).unwrap_or(1) > 1 {
                2
            } else {
                1
            };
            let span_w = cell_w.saturating_mul(cells);
            if cursor.saturating_add(span_w) > limit {
                return;
            }
            if surface.intersects_rect(cursor, y, span_w, cell_h)
                && let Some(glyph) = font::raster(character, font_size_px)
            {
                surface.blit_glyph(
                    &glyph,
                    CellRect {
                        x: cursor,
                        y,
                        w: span_w,
                        h: cell_h,
                    },
                    color,
                    0.0,
                );
            }
            cursor = cursor.saturating_add(span_w);
        }
    }
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

    #[test]
    fn chrome_text_parts_match_joined_text_with_clipping() {
        let width = 160;
        let height = 32;
        let mut joined_pixels = vec![0; width * height];
        let mut parts_pixels = vec![0; width * height];
        paint_chrome_text(
            &mut Surface::new(&mut joined_pixels, width as u32, height as u32),
            3,
            2,
            "@12  中abc|",
            Rgb(240, 240, 240),
            14,
            73,
        );
        paint_chrome_text_parts(
            &mut Surface::new(&mut parts_pixels, width as u32, height as u32),
            3,
            2,
            &["@", "12", "  ", "中abc", "|"],
            Rgb(240, 240, 240),
            14,
            73,
        );
        assert_eq!(parts_pixels, joined_pixels);
    }

    /// A double-width character must consume two cells in chrome text exactly
    /// as it does in the terminal grid. Asserted by composition rather than by
    /// glyph appearance: painting "中A" in one call must equal painting "中"
    /// then "A" two cells along. Under the one-cell-per-character advance this
    /// replaced, the "A" landed one narrow cell in -- on top of the truncated
    /// right half of the wide glyph -- which is what made CJK typed into the
    /// composer render as overlapping garbage.
    #[test]
    fn wide_chrome_characters_occupy_two_cells() {
        let width = 160u32;
        let height = 32u32;
        let size = 15u16;
        let cell_w = font::cell_metrics(size).width.max(1);
        let color = Rgb(240, 240, 240);

        let mut together = vec![0u32; (width * height) as usize];
        paint_chrome_text(
            &mut Surface::new(&mut together, width, height),
            3,
            2,
            "中A",
            color,
            size,
            width,
        );

        let mut apart = vec![0u32; (width * height) as usize];
        {
            let mut surface = Surface::new(&mut apart, width, height);
            paint_chrome_text(&mut surface, 3, 2, "中", color, size, width);
            paint_chrome_text(&mut surface, 3 + 2 * cell_w, 2, "A", color, size, width);
        }

        assert_eq!(
            together, apart,
            "a wide character must advance two cells, leaving the next glyph clear of it"
        );
    }

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
        // a black-box control `send-wheel` test against a real session with
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
    fn double_click_uses_shared_terminal_word_classes() {
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

        // Whitespace is its own terminal word class rather than being folded
        // into either adjacent command token.
        let (start, end) = app
            .word_at(TerminalPoint { row: 0, col: 2 })
            .expect("blank run is selectable");
        assert_eq!((start.col, end.col), (2, 2));
    }

    #[test]
    fn triple_click_selects_only_the_visible_row() {
        let mut app = ConTerminal::new(None);
        app.parser.screen_mut().set_size(4, 10);
        // 15 characters over a 10-column grid soft-wraps onto row 1.
        app.parser.process(b"abcdefghijklmno");
        assert!(
            app.parser.screen().row_wrapped(0),
            "row 0 should be wrapped"
        );

        let (start, end) = app
            .line_at(TerminalPoint { row: 1, col: 2 })
            .expect("visible row");
        assert_eq!((start.row, start.col), (1, 0));
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

    /// A plain click seeds a drag anchor, not a selection. Rendering, copying,
    /// and the Ctrl+C / right-click branches must all agree that a degenerate
    /// range is nothing — otherwise one click leaves its cell inverted forever,
    /// bare Ctrl+C copies an empty string instead of interrupting the child,
    /// and right-click stops pasting.
    #[test]
    fn a_click_without_a_drag_is_not_a_selection() {
        let mut app = ConTerminal::new(None);
        let point = TerminalPoint { row: 2, col: 4 };

        app.selection = Some((point, point));
        assert_eq!(
            app.active_selection(),
            None,
            "an anchor-only range covers no cells and must not render or copy"
        );

        let dragged = TerminalPoint { row: 2, col: 7 };
        app.selection = Some((point, dragged));
        assert_eq!(
            app.active_selection(),
            Some((point, dragged)),
            "a real drag stays a selection"
        );

        // The stored state agrees with what consumers see, so the next
        // right-click pastes rather than copying nothing.
        app.selection = Some((point, point));
        app.selecting = true;
        if !selection_should_auto_copy(app.selection) {
            app.selection = None;
        }
        assert_eq!(app.selection, None);
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
        // Reconstruct exactly what inject_key builds, bypassing
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

    #[test]
    fn child_exit_code_encoding_preserves_complete_signed_domain() {
        for code in [
            None,
            Some(i32::MIN),
            Some(-1),
            Some(0),
            Some(1),
            Some(i32::MAX),
        ] {
            assert_eq!(decode_child_exit_code(encode_child_exit_code(code)), code);
        }
    }
}
