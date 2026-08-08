//! Machine-readable introspection and control for `agenterm-con`.
//!
//! Every other check in this codebase — is a color right, does a cursor
//! appear where it should — is either a unit test against the pure encoding
//! layer, or a human (or an agent standing in for one) squinting at a
//! screenshot. Neither proves the *running* session behaves correctly: the
//! wiring from a real window event to `ConTerminal`'s internal state has, in
//! this file's history, been exactly where bugs hid (`fill_rect` painted the
//! right width at the wrong x for months before anyone wrote a test that
//! looked at actual pixels instead of trusting the code read correctly).
//!
//! This module is what makes it possible to test that wiring — and, just as
//! importantly, what lets an *agent* (not just a human clicking a mouse)
//! drive and inspect a session at all:
//!
//! - [`ScreenSnapshot`] + [`write_snapshot_atomic`]: a JSON dump of the
//!   session's visible state, written after each render when `--emit-snapshot
//!   <path>` is set. A test (or another agent) polls this file instead of
//!   capturing and OCRing pixels.
//! - [`ScriptCommand`] + [`parse_script`]: a JSON array of input commands
//!   read once from `--script <path>` and played back through the exact
//!   same code paths real keyboard/paste input uses. This is what lets a
//!   test — or an agent — drive a session without OS-level input injection,
//!   which is Windows-specific, timing-sensitive, and steals focus.
//!
//! Both are strictly opt-in: absent the flags, `agenterm-con` behaves exactly
//! as it did before this module existed. Zero cost for the human-interactive
//! path this binary exists for.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use agenterm_platform::input::NamedKey;

// ---------------------------------------------------------------------------
// Snapshot: introspection
// ---------------------------------------------------------------------------

/// One cell's worth of cursor state, flattened for JSON rather than nested,
/// so a consumer can `snapshot.cursor.row` instead of matching an enum.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CursorSnapshot {
    pub row: u16,
    pub col: u16,
    pub shape: &'static str,
    pub blinking: bool,
    /// Whether the cursor would currently be painted — false either because
    /// the application hid it (DECTCEM) or because this frame landed in the
    /// "off" half of a blink cycle. A test polling for "cursor is visible"
    /// needs this, not just `hidden`, or it will flake against the blink.
    pub visible_now: bool,
}

/// A point in terminal cell coordinates, used for selection endpoints.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PointSnapshot {
    pub row: u16,
    pub col: u16,
}

/// The full observable state of one session, at one render.
///
/// Deliberately text-first (`rows_text`) rather than a full per-cell
/// attribute dump: the overwhelming majority of what a test or an agent
/// needs to assert is "did the right text end up on screen," and a plain
/// `Vec<String>` is trivial to `grep`/`contains` from any language. Per-cell
/// color/attribute correctness already has dedicated pixel-level unit tests
/// (`paint_cells`'s own test module) — duplicating that here would test the
/// same fact twice through a slower, flakier path.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScreenSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub title: String,
    /// Plain text per visible row, right-trimmed. Index 0 is the top row
    /// currently in view (which is *not* row 0 of the buffer when scrolled
    /// back — `scroll_offset` tells you how far).
    pub rows_text: Vec<String>,
    pub cursor: CursorSnapshot,
    pub scroll_offset: usize,
    pub selection: Option<(PointSnapshot, PointSnapshot)>,
    /// Non-empty while an IME composition is in progress. A headless/scripted
    /// session never populates this (there is no real IME to drive it), but
    /// a real interactive session does, and a human debugging one benefits
    /// from being able to see it here rather than only on screen.
    pub ime_preedit: String,
    /// False once the child process has exited. A test waiting for a
    /// command to finish should poll this rather than assume a fixed delay.
    pub child_alive: bool,
    pub font_size_px: u16,
}

/// Writes `snapshot` to `path` atomically: serialize to a sibling temp file,
/// then rename over the destination. Without this, a reader polling the file
/// (a test, or another agent) can observe a half-written frame and get a
/// JSON parse error instead of stale-but-valid data — a real flake source
/// for anything polling a file that's rewritten many times a second.
pub fn write_snapshot_atomic(path: &Path, snapshot: &ScreenSnapshot) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| std::io::Error::other(format!("snapshot serialization: {error}")))?;
    let tmp_path = tmp_sibling(path);
    {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)
}

/// A temp path in the same directory as `path`, so the later rename is
/// guaranteed atomic (same filesystem/volume) rather than an OS-dependent
/// maybe-atomic cross-directory move.
fn tmp_sibling(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| {
            let mut owned = name.to_os_string();
            owned.push(".tmp");
            owned
        })
        .unwrap_or_else(|| "snapshot.tmp".into());
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(file_name),
        _ => PathBuf::from(file_name),
    }
}

/// Encodes an XRGB (`0x00RRGGBB`) pixel buffer as PNG and writes it
/// atomically (temp file + rename), the same guarantee `write_snapshot_atomic`
/// gives text state: a poller must never observe a truncated image.
///
/// Takes raw pixels rather than a richer type because the pixel buffer's
/// real type (`Surface`) lives in the main binary file, not this module —
/// this is the narrow seam between them, not a reason to duplicate PNG
/// encoding at the call site.
pub fn write_png_atomic(path: &Path, pixels: &[u32], width: u32, height: u32) -> std::io::Result<()> {
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for &pixel in pixels {
        rgb.push(((pixel >> 16) & 0xFF) as u8);
        rgb.push(((pixel >> 8) & 0xFF) as u8);
        rgb.push((pixel & 0xFF) as u8);
    }

    let tmp_path = tmp_sibling(path);
    {
        let file = std::fs::File::create(&tmp_path)?;
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| std::io::Error::other(format!("png header: {error}")))?;
        writer
            .write_image_data(&rgb)
            .map_err(|error| std::io::Error::other(format!("png data: {error}")))?;
    }
    std::fs::rename(&tmp_path, path)
}

// ---------------------------------------------------------------------------
// Script: control
// ---------------------------------------------------------------------------

/// One key a script can send: either a name from the fixed VT-relevant set
/// (arrows, function keys, Enter, ...) or a single character, so
/// `{"key": "c", "ctrl": true}` sends Ctrl+C without needing a name for
/// every letter on the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKey {
    Named(NamedKey),
    Char(char),
}

/// Which physical button a scripted [`ScriptCommand::Click`] presses. A
/// closed set (not a raw code) so a script author gets a loud parse error on
/// a typo instead of an ambiguous number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptMouseButton {
    #[default]
    Left,
    Middle,
    Right,
}

/// One command in a `--script` file, already validated — by the time this
/// type exists, `key` names have been resolved and mutually-exclusive fields
/// have been checked, so the executor never has to fail mid-script.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptCommand {
    /// Raw text written directly to the PTY, as if the application read it
    /// from a paste with no bracketing — this is the bulk-typing command,
    /// not a substitute for `Key` when the point is exercising the real
    /// keyboard encoder (mode-awareness, modifiers, Alt-as-ESC, ...).
    Text(String),
    /// Goes through the same `forward_key` path real keyboard input does,
    /// including host shortcuts (Ctrl+Shift+C copies, etc.) and the live
    /// DECCKM/modifier-aware encoder — this is what closes the gap between
    /// "the encoder is unit-tested" and "a real keystroke in a real session
    /// produces the right bytes."
    Key {
        key: ScriptKey,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    /// Goes through the same bracketed-paste-aware path Ctrl+V uses.
    Paste(String),
    /// Pause before the next command. Does not block the render loop —
    /// the executor schedules it via the same `about_to_wait` `WaitUntil`
    /// mechanism the resize debounce and cursor blink use.
    WaitMs(u64),
    /// Captures the next rendered frame as a PNG at the given path.
    ///
    /// `--emit-snapshot` proves what *text* is on screen; it cannot prove
    /// the pixels are right — that needs an actual image, which is exactly
    /// what caught the `fill_rect` bug earlier this session (every
    /// text-level check would have passed while backgrounds painted at the
    /// wrong column). Before this, getting a picture of a running session
    /// meant an ad hoc PrintWindow script; this is the same capability as a
    /// first-class, scriptable, in-sequence command.
    Screenshot(PathBuf),
    /// Presses then releases a mouse button at a cell coordinate, through
    /// the same `handle_pointer_button` path a real click takes — so an
    /// application that has grabbed mouse reporting (DECSET 1000/1002/1003
    /// + 1006) sees a real press/release pair, and one that hasn't gets the
    /// same local click-counting (select/word-select/line-select) a human
    /// click does. Closes the gap this binary's docs flagged: `--script`
    /// had text/key/paste but nothing that reaches `report_mouse` at all.
    Click {
        row: u16,
        col: u16,
        button: ScriptMouseButton,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    /// Moves the pointer to a cell coordinate without a button transition —
    /// drives `handle_pointer_moved`, so a dragging selection extends, an
    /// application-owned drag reports motion, and `ANY_MOTION` (1003)
    /// hover reporting fires exactly as it would for a real mouse move.
    MouseMove { row: u16, col: u16 },
    /// One wheel notch's worth of scroll at a cell coordinate, through
    /// `handle_wheel` — so the same three-tier precedence a real wheel
    /// gesture gets (application report → alternate-screen cursor keys →
    /// local scrollback) applies to a scripted one. Positive `notches`
    /// scrolls up (back through history), negative scrolls down.
    ///
    /// `ctrl: true` instead drives the Ctrl+wheel font-size zoom path —
    /// same sign convention (positive grows the font). This is a distinct
    /// code path from plain scroll (it lives in the window-event handler,
    /// not `handle_wheel`), included because a real user report ("zoom past
    /// a certain size and the process exits") could not be reproduced by
    /// single-shot unit-test sweeps over `apply_resize`/`font::raster` in
    /// isolation — only a live session driving *repeated, cumulative*
    /// zoom steps through the real ConPTY resize path can tell whether
    /// something about that sequence, not any one fixed size, is at fault.
    Wheel { row: u16, col: u16, notches: f32, ctrl: bool },
}

/// One entry of the raw JSON array — every field optional so serde can parse
/// it uniformly, then [`validate`](RawCommand::validate) enforces "exactly
/// one action" instead of silently picking one when a script author sets two
/// by mistake.
/// A cell coordinate as it appears in script JSON — shared shape for
/// `mouse_move` and (nested in [`RawClick`]/[`RawWheel`]) `click`/`wheel`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoint {
    row: u16,
    col: u16,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClick {
    row: u16,
    col: u16,
    #[serde(default)]
    button: ScriptMouseButton,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWheel {
    row: u16,
    col: u16,
    notches: f32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommand {
    text: Option<String>,
    paste: Option<String>,
    key: Option<String>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    shift: bool,
    wait_ms: Option<u64>,
    screenshot: Option<PathBuf>,
    click: Option<RawClick>,
    mouse_move: Option<RawPoint>,
    wheel: Option<RawWheel>,
}

impl RawCommand {
    fn validate(self, index: usize) -> Result<ScriptCommand, String> {
        let present = [
            self.text.is_some(),
            self.paste.is_some(),
            self.key.is_some(),
            self.wait_ms.is_some(),
            self.screenshot.is_some(),
            self.click.is_some(),
            self.mouse_move.is_some(),
            self.wheel.is_some(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count();
        if present != 1 {
            return Err(format!(
                "script command {index}: exactly one of text/paste/key/wait_ms/screenshot/click/mouse_move/wheel is required, found {present}"
            ));
        }
        if let Some(text) = self.text {
            return Ok(ScriptCommand::Text(text));
        }
        if let Some(text) = self.paste {
            return Ok(ScriptCommand::Paste(text));
        }
        if let Some(name) = self.key {
            let key = parse_key_name(&name)
                .ok_or_else(|| format!("script command {index}: unknown key name '{name}'"))?;
            return Ok(ScriptCommand::Key {
                key,
                ctrl: self.ctrl,
                alt: self.alt,
                shift: self.shift,
            });
        }
        if let Some(ms) = self.wait_ms {
            return Ok(ScriptCommand::WaitMs(ms));
        }
        if let Some(path) = self.screenshot {
            return Ok(ScriptCommand::Screenshot(path));
        }
        if let Some(click) = self.click {
            return Ok(ScriptCommand::Click {
                row: click.row,
                col: click.col,
                button: click.button,
                ctrl: self.ctrl,
                alt: self.alt,
                shift: self.shift,
            });
        }
        if let Some(point) = self.mouse_move {
            return Ok(ScriptCommand::MouseMove { row: point.row, col: point.col });
        }
        if let Some(wheel) = self.wheel {
            return Ok(ScriptCommand::Wheel {
                row: wheel.row,
                col: wheel.col,
                notches: wheel.notches,
                ctrl: self.ctrl,
            });
        }
        unreachable!("validated exactly one field is present above");
    }
}

/// Resolves a script's `key` string to a [`ScriptKey`]. Named keys match
/// case-insensitively so a script author does not need to remember exact
/// casing; a single character (any Unicode scalar, not just ASCII) is taken
/// literally so `{"key": "文"}` is a meaningful thing to write even though it
/// will rarely be useful.
fn parse_key_name(name: &str) -> Option<ScriptKey> {
    let named = match name.to_ascii_lowercase().as_str() {
        "enter" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "backspace" => NamedKey::Backspace,
        "escape" | "esc" => NamedKey::Escape,
        "space" => NamedKey::Space,
        "arrowup" | "up" => NamedKey::ArrowUp,
        "arrowdown" | "down" => NamedKey::ArrowDown,
        "arrowleft" | "left" => NamedKey::ArrowLeft,
        "arrowright" | "right" => NamedKey::ArrowRight,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "delete" | "del" => NamedKey::Delete,
        "insert" => NamedKey::Insert,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        _ => {
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                (Some(ch), None) => Some(ScriptKey::Char(ch)),
                _ => None,
            };
        }
    };
    Some(ScriptKey::Named(named))
}

/// Parses a `--script` file's contents: a JSON array of command objects.
///
/// Fails loudly and specifically (which command, what was wrong) rather than
/// skipping bad entries — a script silently missing half its steps is a
/// worse debugging experience than the process refusing to start.
pub fn parse_script(bytes: &[u8]) -> Result<Vec<ScriptCommand>, String> {
    let raw: Vec<RawCommand> = serde_json::from_slice(bytes)
        .map_err(|error| format!("script is not a JSON array of command objects: {error}"))?;
    raw.into_iter()
        .enumerate()
        .map(|(index, command)| command.validate(index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_command_kind() {
        let script = br#"[
            {"text": "echo hi\r"},
            {"paste": "pasted\ntext"},
            {"key": "ArrowUp"},
            {"key": "c", "ctrl": true},
            {"wait_ms": 250},
            {"screenshot": "out.png"}
        ]"#;
        let commands = parse_script(script).expect("valid script");
        assert_eq!(
            commands,
            vec![
                ScriptCommand::Text("echo hi\r".to_owned()),
                ScriptCommand::Paste("pasted\ntext".to_owned()),
                ScriptCommand::Key {
                    key: ScriptKey::Named(NamedKey::ArrowUp),
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                ScriptCommand::Key {
                    key: ScriptKey::Char('c'),
                    ctrl: true,
                    alt: false,
                    shift: false,
                },
                ScriptCommand::WaitMs(250),
                ScriptCommand::Screenshot(PathBuf::from("out.png")),
            ]
        );
    }

    #[test]
    fn write_png_atomic_produces_a_readable_png_of_the_right_size() {
        let dir = std::env::temp_dir().join(format!(
            "agenterm-con-png-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        // 2x2, XRGB: red, green, blue, white.
        let pixels = [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0x00FF_FFFF];
        write_png_atomic(&path, &pixels, 2, 2).expect("write png");

        let bytes = std::fs::read(&path).expect("read png back");
        let decoder = png::Decoder::new(bytes.as_slice());
        let mut reader = decoder.read_info().expect("valid PNG");
        assert_eq!(reader.info().width, 2);
        assert_eq!(reader.info().height, 2);
        let mut buffer = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut buffer).expect("decode frame");
        // First pixel must be red (0xFF, 0x00, 0x00), proving channel order
        // survived the XRGB -> RGB8 conversion, not just that *a* PNG came out.
        assert_eq!(&buffer[0..3], &[0xFF, 0x00, 0x00]);
    }

    #[test]
    fn key_names_are_case_insensitive_and_have_shorthands() {
        assert_eq!(parse_key_name("ArrowUp"), Some(ScriptKey::Named(NamedKey::ArrowUp)));
        assert_eq!(parse_key_name("arrowup"), Some(ScriptKey::Named(NamedKey::ArrowUp)));
        assert_eq!(parse_key_name("UP"), Some(ScriptKey::Named(NamedKey::ArrowUp)));
        assert_eq!(parse_key_name("Esc"), Some(ScriptKey::Named(NamedKey::Escape)));
        assert_eq!(parse_key_name("a"), Some(ScriptKey::Char('a')));
        assert_eq!(parse_key_name("中"), Some(ScriptKey::Char('中')));
        assert_eq!(parse_key_name("notakey"), None);
        assert_eq!(parse_key_name(""), None);
        assert_eq!(parse_key_name("ab"), None, "two ASCII chars is not a single key");
    }

    #[test]
    fn parses_mouse_commands() {
        let script = br#"[
            {"click": {"row": 3, "col": 10}},
            {"click": {"row": 3, "col": 10, "button": "right"}},
            {"click": {"row": 0, "col": 0, "button": "middle"}, "shift": true},
            {"mouse_move": {"row": 5, "col": 20}},
            {"wheel": {"row": 5, "col": 20, "notches": 3}},
            {"wheel": {"row": 5, "col": 20, "notches": -1.5}},
            {"wheel": {"row": 5, "col": 20, "notches": 1}, "ctrl": true}
        ]"#;
        let commands = parse_script(script).expect("valid script");
        assert_eq!(
            commands,
            vec![
                ScriptCommand::Click {
                    row: 3,
                    col: 10,
                    button: ScriptMouseButton::Left,
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                ScriptCommand::Click {
                    row: 3,
                    col: 10,
                    button: ScriptMouseButton::Right,
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                ScriptCommand::Click {
                    row: 0,
                    col: 0,
                    button: ScriptMouseButton::Middle,
                    ctrl: false,
                    alt: false,
                    shift: true,
                },
                ScriptCommand::MouseMove { row: 5, col: 20 },
                ScriptCommand::Wheel { row: 5, col: 20, notches: 3.0, ctrl: false },
                ScriptCommand::Wheel { row: 5, col: 20, notches: -1.5, ctrl: false },
                ScriptCommand::Wheel { row: 5, col: 20, notches: 1.0, ctrl: true },
            ]
        );
    }

    #[test]
    fn rejects_a_command_with_two_actions_rather_than_silently_picking_one() {
        let script = br#"[{"text": "a", "paste": "b"}]"#;
        let error = parse_script(script).expect_err("must reject ambiguous command");
        assert!(error.contains("command 0"), "{error}");
        assert!(error.contains("exactly one"), "{error}");
    }

    #[test]
    fn rejects_a_click_with_an_unknown_button_name() {
        let script = br#"[{"click": {"row": 0, "col": 0, "button": "super"}}]"#;
        assert!(parse_script(script).is_err());
    }

    #[test]
    fn rejects_a_command_with_no_action() {
        let script = br#"[{"ctrl": true}]"#;
        let error = parse_script(script).expect_err("must reject empty command");
        assert!(error.contains("exactly one"), "{error}");
    }

    #[test]
    fn rejects_an_unknown_key_name_with_the_offending_name_in_the_message() {
        let script = br#"[{"key": "SuperDelete"}]"#;
        let error = parse_script(script).expect_err("must reject unknown key");
        assert!(error.contains("SuperDelete"), "{error}");
    }

    #[test]
    fn unknown_fields_are_rejected_rather_than_silently_ignored() {
        // A typo like "txet" must not be silently treated as "no action" —
        // deny_unknown_fields turns it into a loud parse error instead.
        let script = br#"[{"txet": "typo"}]"#;
        assert!(parse_script(script).is_err());
    }

    #[test]
    fn snapshot_round_trips_through_json_with_stable_field_names() {
        // Not a schema test in the strict sense — there is no consumer to
        // pin against yet — but it does prove serialization succeeds and
        // that a consumer parsing generic JSON (jq, Python, another agent)
        // finds the fields this module's docs promise.
        let snapshot = ScreenSnapshot {
            cols: 80,
            rows: 24,
            title: "agenterm-con".to_owned(),
            rows_text: vec!["hello".to_owned(), String::new()],
            cursor: CursorSnapshot {
                row: 0,
                col: 5,
                shape: "block",
                blinking: true,
                visible_now: true,
            },
            scroll_offset: 0,
            selection: Some((PointSnapshot { row: 0, col: 0 }, PointSnapshot { row: 0, col: 4 })),
            ime_preedit: String::new(),
            child_alive: true,
            font_size_px: 16,
        };
        let value: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert_eq!(value["cols"], 80);
        assert_eq!(value["rows_text"][0], "hello");
        assert_eq!(value["cursor"]["shape"], "block");
        assert_eq!(value["cursor"]["visible_now"], true);
        assert_eq!(value["selection"][0]["col"], 0);
        assert_eq!(value["selection"][1]["col"], 4);
        assert_eq!(value["child_alive"], true);
    }

    #[test]
    fn tmp_sibling_stays_in_the_same_directory_for_an_atomic_rename() {
        let path = Path::new("/some/dir/snapshot.json");
        let tmp = tmp_sibling(path);
        assert_eq!(tmp.parent(), Some(Path::new("/some/dir")));
        assert_eq!(tmp.file_name().unwrap(), "snapshot.json.tmp");
    }
}
