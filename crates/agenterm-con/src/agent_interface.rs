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
//! - The public `agenterm-con cli` control endpoint drives keyboard, paste,
//!   pointer, wheel, wait and screenshot operations through the same product
//!   paths as physical input, without embedding a second script runtime.
//!
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, mpsc};
use std::time::Instant;

use agenterm_platform::{
    filesystem_publish::{write_file_atomic, write_path_atomic},
    screenshot::{XrgbFrame, write_xrgb_png},
};

// ---------------------------------------------------------------------------
// Snapshot: introspection
// ---------------------------------------------------------------------------

/// One cell's worth of cursor state, flattened for JSON rather than nested,
/// so a consumer can `snapshot.cursor.row` instead of matching an enum.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Copy)]
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
#[derive(Debug, Clone)]
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
    pub max_scrollback: usize,
    pub selection: Option<(PointSnapshot, PointSnapshot)>,
    /// Non-empty while an IME composition is in progress. A headless/scripted
    /// session never populates this (there is no real IME to drive it), but
    /// a real interactive session does, and a human debugging one benefits
    /// from being able to see it here rather than only on screen.
    pub ime_preedit: String,
    /// False once the child process has exited. A test waiting for a
    /// command to finish should poll this rather than assume a fixed delay.
    pub child_alive: bool,
    /// Numeric process exit code when the platform supplies one. `None` while
    /// running and for non-numeric termination such as a Unix signal.
    pub child_exit_code: Option<i32>,
    pub font_size_px: u16,
}

/// Writes `snapshot` to `path` atomically: serialize to a sibling temp file,
/// then rename over the destination. Without this, a reader polling the file
/// (a test, or another agent) can observe a half-written frame and get a
/// JSON parse error instead of stale-but-valid data — a real flake source
/// for anything polling a file that's rewritten many times a second.
pub fn write_snapshot_atomic(path: &Path, snapshot: &ScreenSnapshot) -> std::io::Result<()> {
    let json = super::json::to_vec_pretty(&snapshot_json(snapshot));
    write_file_atomic(path, |file| file.write_all(&json)).map_err(std::io::Error::from)
}

fn snapshot_json(snapshot: &ScreenSnapshot) -> super::json::JsonValue {
    use super::json::{JsonValue, nullable, object};
    let point =
        |point: PointSnapshot| object(vec![("row", point.row.into()), ("col", point.col.into())]);
    object(vec![
        ("cols", snapshot.cols.into()),
        ("rows", snapshot.rows.into()),
        ("title", snapshot.title.as_str().into()),
        (
            "rows_text",
            JsonValue::Array(
                snapshot
                    .rows_text
                    .iter()
                    .map(|row| row.as_str().into())
                    .collect(),
            ),
        ),
        (
            "cursor",
            object(vec![
                ("row", snapshot.cursor.row.into()),
                ("col", snapshot.cursor.col.into()),
                ("shape", snapshot.cursor.shape.into()),
                ("blinking", snapshot.cursor.blinking.into()),
                ("visible_now", snapshot.cursor.visible_now.into()),
            ]),
        ),
        ("scroll_offset", snapshot.scroll_offset.into()),
        ("max_scrollback", snapshot.max_scrollback.into()),
        (
            "selection",
            nullable(
                snapshot
                    .selection
                    .map(|(start, end)| JsonValue::Array(vec![point(start), point(end)])),
            ),
        ),
        ("ime_preedit", snapshot.ime_preedit.as_str().into()),
        ("child_alive", snapshot.child_alive.into()),
        (
            "child_exit_code",
            nullable(snapshot.child_exit_code.map(i64::from)),
        ),
        ("font_size_px", snapshot.font_size_px.into()),
    ])
}

/// Encodes an XRGB (`0x00RRGGBB`) pixel buffer as PNG and writes it
/// atomically (temp file + rename), the same guarantee `write_snapshot_atomic`
/// gives text state: a poller must never observe a truncated image.
///
/// Takes raw pixels rather than a richer type because the pixel buffer's
/// real type (`Surface`) lives in the main binary file, not this module —
/// this is the narrow seam between them, not a reason to duplicate PNG
/// encoding at the call site.
pub fn write_png_atomic(
    path: &Path,
    pixels: &[u32],
    width: u32,
    height: u32,
) -> std::io::Result<()> {
    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "PNG dimensions overflow")
        })?;
    if width == 0 || height == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "PNG dimensions must be non-zero",
        ));
    }
    if pixels.len() != pixel_count {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "PNG pixel count does not match dimensions",
        ));
    }
    write_path_atomic(path, |temporary| {
        write_xrgb_png(XrgbFrame::new(temporary, width, height, pixels))
            .map(|_| ())
            .map_err(|error| {
                let kind = match error.code() {
                    "screenshot_invalid_dimensions"
                    | "screenshot_buffer_too_small"
                    | "screenshot_invalid_clip"
                    | "screenshot_too_large" => std::io::ErrorKind::InvalidInput,
                    _ => std::io::ErrorKind::Other,
                };
                std::io::Error::new(kind, error)
            })
    })
    .map_err(std::io::Error::from)
}

type PngCompletion = Box<dyn FnOnce(std::io::Result<u64>) + Send + 'static>;

fn complete_png(completion: PngCompletion, result: std::io::Result<u64>) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(result)));
}

struct PngJob {
    path: PathBuf,
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    completion: PngCompletion,
}

type PngWorkerInit = Result<mpsc::SyncSender<PngJob>, (std::io::ErrorKind, String)>;

fn png_worker() -> std::io::Result<&'static mpsc::SyncSender<PngJob>> {
    static WORKER: OnceLock<PngWorkerInit> = OnceLock::new();
    match WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<PngJob>(1);
        agenterm_platform::threading::spawn_named_detached(
            "agenterm-con-png",
            Box::new(move || {
                while let Ok(job) = receiver.recv() {
                    let PngJob {
                        path,
                        pixels,
                        width,
                        height,
                        completion,
                    } = job;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let started = Instant::now();
                        write_png_atomic(&path, &pixels, width, height)
                            .map(|()| started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64)
                    }))
                    .unwrap_or_else(|_| Err(std::io::Error::other("PNG worker panicked")));
                    complete_png(completion, result);
                }
            }),
        )
        .map_err(|error| (error.kind(), error.to_string()))?;
        Ok(sender)
    }) {
        Ok(sender) => Ok(sender),
        Err((kind, message)) => Err(std::io::Error::new(*kind, message.clone())),
    }
}

pub fn initialize_png_worker() -> std::io::Result<()> {
    png_worker().map(|_| ())
}

pub fn submit_png_atomic(
    path: PathBuf,
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    completion: PngCompletion,
) {
    let job = PngJob {
        path,
        pixels,
        width,
        height,
        completion,
    };
    let worker = match png_worker() {
        Ok(worker) => worker,
        Err(error) => {
            complete_png(job.completion, Err(error));
            return;
        }
    };
    if let Err(error) = worker.try_send(job) {
        let (kind, message, job) = match error {
            mpsc::TrySendError::Full(job) => (
                std::io::ErrorKind::WouldBlock,
                "PNG worker queue is full",
                job,
            ),
            mpsc::TrySendError::Disconnected(job) => (
                std::io::ErrorKind::BrokenPipe,
                "PNG worker is unavailable",
                job,
            ),
        };
        complete_png(job.completion, Err(std::io::Error::new(kind, message)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "agenterm-con-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn snapshot_round_trips_with_stable_fields() {
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
            max_scrollback: 120,
            selection: Some((
                PointSnapshot { row: 0, col: 0 },
                PointSnapshot { row: 0, col: 4 },
            )),
            ime_preedit: String::new(),
            child_alive: true,
            child_exit_code: None,
            font_size_px: 16,
        };
        let value: serde_json::Value =
            serde_json::from_slice(&super::super::json::to_vec(&snapshot_json(&snapshot))).unwrap();
        assert_eq!(value["cols"], 80);
        assert_eq!(value["rows_text"][0], "hello");
        assert_eq!(value["cursor"]["shape"], "block");
        assert_eq!(value["selection"][1]["col"], 4);
        assert_eq!(value["child_alive"], true);
        assert!(value["child_exit_code"].is_null());
    }

    #[test]
    fn write_png_atomic_produces_a_readable_png() {
        let dir = scratch("png-test");
        let path = dir.join("shot.png");
        let pixels = [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0x00FF_FFFF];
        write_png_atomic(&path, &pixels, 2, 2).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut reader = png::Decoder::new(bytes.as_slice()).read_info().unwrap();
        let mut decoded = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(&decoded[..3], &[0xFF, 0x00, 0x00]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn native_png_encoder_handles_a_large_frame() {
        let dir = scratch("large-png-test");
        let path = dir.join("shot.png");
        let width = 256;
        let height = 100;
        let pixels = vec![0x0012_3456; width * height];
        write_png_atomic(&path, &pixels, width as u32, height as u32).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut reader = png::Decoder::new(bytes.as_slice()).read_info().unwrap();
        let mut decoded = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut decoded).unwrap();
        assert_eq!(&decoded[..3], &[0x12, 0x34, 0x56]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_png_rejects_invalid_dimensions_before_creating_a_file() {
        let dir = scratch("invalid-png-test");
        let path = dir.join("shot.png");
        assert_eq!(
            write_png_atomic(&path, &[], 0, 1).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            write_png_atomic(&path, &[0], 2, 1).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn panicking_png_completion_is_contained() {
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = called.clone();
        complete_png(
            Box::new(move |_| {
                observed.store(true, std::sync::atomic::Ordering::Release);
                panic!("completion panic must not escape");
            }),
            Ok(1),
        );
        assert!(called.load(std::sync::atomic::Ordering::Acquire));
    }
}
